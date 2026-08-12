//! 事件系统（事件监听与订阅）
//!
//! 灵感来自 ThinkPHP 的「模型事件 / 事件订阅」，按 Rust 生态最佳实践实现：
//! 类型擦除的 [`EventBus`] + [`Event`] / [`Listener`] trait + [`Subscriber`] 批量订阅。
//!
//! # 设计要点
//!
//! - **类型化事件**：每种事件是一个实现 [`Event`] 的结构体；监听器按事件类型路由（`TypeId`）。
//! - **观察者语义**：监听器收到 `&E`（不可变），用于日志/审计/指标/缓存失效等副作用，
//!   不支持在事件中修改数据或取消操作（保持分发简单、无返回值串联）。
//! - **同步回调**：监听器是同步的（`fn handle(&self, &E)`），在派发点内联调用。
//!   耗时或异步工作请在监听器内部 `tokio::spawn`。
//! - **线程安全**：监听器表用 `RwLock<HashMap>` 保护；派发时先克隆出监听器 `Arc` 列表、
//!   **释放锁后再回调**，从而监听器内部可安全地再次订阅/派发（避免重入死锁）。
//! - **零开销快路径**：无任何监听器时，[`EventBus::dispatch`] / [`dispatch_if`](EventBus::dispatch_if)
//!   经一个 `AtomicUsize` 原子读即返回，不取锁、不构造事件。
//!
//! # 示例
//!
//! ```ignore
//! use hirust_mapper::runtime::{EventBus, Event, Subscriber};
//! use hirust_mapper::runtime::AfterSqlEvent;
//!
//! #[derive(Debug)]
//! struct LoginEvent { user: String }
//! impl Event for LoginEvent {}
//!
//! let bus = EventBus::new();
//! // 1) 闭包订阅单个事件
//! bus.on(|e: &LoginEvent| println!("{} 登录", e.user));
//! // 2) 订阅器批量订阅
//! bus.add_subscriber(&AuditSubscriber);
//! // 3) 派发
//! bus.dispatch(&LoginEvent { user: "张三".into() });
//! # struct AuditSubscriber;
//! # impl Subscriber for AuditSubscriber {
//! #     fn subscribe(&self, bus: &EventBus) {
//! #         bus.on(|e: &LoginEvent| ());
//! #     }
//! # }
//! ```

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

pub mod lifecycle;

/// 事件标记 trait。事件类型须 `Send + Sync + 'static`（用于类型擦除与 `TypeId`）。
pub trait Event: Send + Sync + 'static {}

/// 事件监听器：处理特定事件类型 `E`（观察者，收到不可变引用）。
pub trait Listener<E: Event>: Send + Sync {
    /// 处理事件。应快速返回；耗时/异步工作请在内部 spawn。
    fn handle(&self, event: &E);
}

// 闭包自动实现 Listener —— 让 `bus.on(|e: &E| {...})` 可用。
impl<E: Event, F> Listener<E> for F
where
    F: Fn(&E) + Send + Sync,
{
    fn handle(&self, event: &E) {
        self(event)
    }
}

/// 事件订阅器：批量注册多个事件监听器（对应 ThinkPHP 的「事件订阅」）。
///
/// 实现者在 [`subscribe`](Subscriber::subscribe) 中将（自身的）多个处理逻辑绑定到不同事件。
pub trait Subscriber: Send + Sync {
    /// 将本订阅器关心的所有事件监听器注册到 `bus`。
    fn subscribe(&self, bus: &EventBus);
}

// ─── 内部：类型擦除的监听器适配器 ───────────────────────────────────

/// 类型擦除后的监听器：把 `&dyn Any` 内部 downcast 回具体事件类型再交给真实监听器。
trait ErasedListener: Send + Sync {
    fn handle_any(&self, event: &dyn Any);
}

struct Erased<E: Event>(Arc<dyn Listener<E>>);

impl<E: Event> ErasedListener for Erased<E> {
    fn handle_any(&self, event: &dyn Any) {
        if let Some(e) = event.downcast_ref::<E>() {
            // 仅当事件类型匹配本监听器的 E 时才回调（同一 TypeId 槽下类型恒匹配，此处为保险）
            (self.0).handle(e);
        }
    }
}

/// 事件分发器（事件总线）：线程安全，按事件类型路由到监听器。
///
/// 一个 `EventBus` 可被多处共享（`Arc<EventBus>`），监听器在其生命周期内常驻。
pub struct EventBus {
    /// 事件类型(TypeId) → 该类型的监听器列表（不可变切片，`Arc` 共享）
    /// 存 `Arc<[...]>` 而非 `Vec`：派发时只需克隆 `Arc`（1 次原子自增、零分配），
    /// 订阅时重建切片（罕见路径）。派发读锁释放后再回调，监听器内可安全重入。
    listeners: RwLock<HashMap<TypeId, Arc<[Arc<dyn ErasedListener>]>>>,
    /// 所有事件类型的监听器总数；用于无监听器时的锁原子快路径
    total: AtomicUsize,
}

impl Default for EventBus {
    fn default() -> Self {
        Self {
            listeners: RwLock::new(HashMap::new()),
            total: AtomicUsize::new(0),
        }
    }
}

impl EventBus {
    /// 创建空的事件总线
    pub fn new() -> Self {
        Self::default()
    }

    /// 订阅：注册一个 `Listener<E>`（trait 对象形式）。
    pub fn subscribe<E: Event>(&self, listener: Arc<dyn Listener<E>>) {
        let key = TypeId::of::<E>();
        let erased: Arc<dyn ErasedListener> = Arc::new(Erased(listener));
        {
            let mut map = self.listeners.write().expect("EventBus 锁中毒");
            // 重建切片（订阅是罕见路径，重建成本可接受；换取派发的零分配）
            let new: Arc<[Arc<dyn ErasedListener>]> = match map.get(&key) {
                Some(existing) => {
                    let mut v = Vec::with_capacity(existing.len() + 1);
                    v.extend(existing.iter().cloned());
                    v.push(erased);
                    Arc::from(v)
                }
                None => Arc::from(vec![erased]),
            };
            map.insert(key, new);
        }
        self.total.fetch_add(1, Ordering::Relaxed);
    }

    /// 便捷订阅：注册一个闭包监听器 `Fn(&E)`。
    pub fn on<E: Event, F>(&self, handler: F)
    where
        F: Fn(&E) + Send + Sync + 'static,
    {
        let listener: Arc<dyn Listener<E>> = Arc::new(handler);
        self.subscribe(listener);
    }

    /// 批量订阅：通过 [`Subscriber`] 注册其全部监听器。
    pub fn add_subscriber<S: Subscriber>(&self, subscriber: &S) {
        subscriber.subscribe(self);
    }

    /// 派发事件：按注册顺序同步调用 `E` 的所有监听器。无监听器时零开销（一次原子读）。
    pub fn dispatch<E: Event>(&self, event: &E) {
        let Some(listeners) = self.snapshot::<E>() else {
            return;
        };
        for l in listeners.iter() {
            l.handle_any(event);
        }
    }

    /// 惰性派发：仅当存在 `E` 的监听器时才调用 `build` 构造事件并派发。
    /// 适用于构造事件代价较高（如克隆大参数）的场景。
    pub fn dispatch_if<E: Event, F>(&self, build: F)
    where
        F: FnOnce() -> E,
    {
        let Some(listeners) = self.snapshot::<E>() else {
            return;
        };
        let event = build();
        for l in listeners.iter() {
            l.handle_any(&event);
        }
    }

    /// 是否存在 `E` 的监听器
    pub fn has_listeners<E: Event>(&self) -> bool {
        if self.total.load(Ordering::Relaxed) == 0 {
            return false;
        }
        let map = self.listeners.read().expect("EventBus 锁中毒");
        map.get(&TypeId::of::<E>())
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }

    /// `E` 的监听器数量
    pub fn listener_count<E: Event>(&self) -> usize {
        let map = self.listeners.read().expect("EventBus 锁中毒");
        map.get(&TypeId::of::<E>()).map(|v| v.len()).unwrap_or(0)
    }

    /// 所有事件类型的监听器总数
    pub fn total_listeners(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }

    /// 取 `E` 监听器切片的 `Arc` 快照（读锁内一次 `Arc::clone`，无分配；锁随后释放）。
    /// 无监听器返回 `None`（不创建任何 `Arc`，快路径零分配）。
    fn snapshot<E: Event>(&self) -> Option<Arc<[Arc<dyn ErasedListener>]>> {
        if self.total.load(Ordering::Relaxed) == 0 {
            return None;
        }
        let map = self.listeners.read().expect("EventBus 锁中毒");
        match map.get(&TypeId::of::<E>()) {
            Some(v) if !v.is_empty() => Some(Arc::clone(v)),
            _ => None,
        }
    }
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus")
            .field("total_listeners", &self.total.load(Ordering::Relaxed))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[derive(Debug)]
    struct LoginEvent {
        user: String,
    }
    impl Event for LoginEvent {}

    #[derive(Debug)]
    struct LogoutEvent;
    impl Event for LogoutEvent {}

    #[test]
    fn test_dispatch_calls_matching_listener() {
        let bus = EventBus::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&counter);
        bus.on(move |e: &LoginEvent| {
            assert_eq!(e.user, "张三");
            c.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(bus.listener_count::<LoginEvent>(), 1);
        bus.dispatch(&LoginEvent { user: "张三".into() });
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_type_isolation() {
        // Login 的监听器不应被 Logout 事件触发
        let bus = EventBus::new();
        let seen = Arc::new(AtomicUsize::new(0));
        let s = Arc::clone(&seen);
        bus.on(move |_: &LoginEvent| {
            s.fetch_add(1, Ordering::Relaxed);
        });
        bus.dispatch(&LogoutEvent);
        bus.dispatch(&LoginEvent { user: "x".into() });
        assert_eq!(seen.load(Ordering::Relaxed), 1, "只应被 LoginEvent 触发一次");
    }

    #[test]
    fn test_multiple_listeners_in_registration_order() {
        let bus = EventBus::new();
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let o1 = Arc::clone(&order);
        bus.on(move |_: &LoginEvent| o1.lock().unwrap().push(1));
        let o2 = Arc::clone(&order);
        bus.on(move |_: &LoginEvent| o2.lock().unwrap().push(2));
        let o3 = Arc::clone(&order);
        bus.on(move |_: &LoginEvent| o3.lock().unwrap().push(3));

        bus.dispatch(&LoginEvent { user: "a".into() });
        assert_eq!(*order.lock().unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn test_dispatch_if_lazy_and_skips_when_empty() {
        let bus = EventBus::new();
        let built = Arc::new(AtomicUsize::new(0));
        let b = Arc::clone(&built);
        // 无监听器：build 不应被调用
        bus.dispatch_if(move || {
            b.fetch_add(1, Ordering::Relaxed);
            LoginEvent { user: "x".into() }
        });
        assert_eq!(built.load(Ordering::Relaxed), 0, "无监听器时不应构造事件");

        let b2 = Arc::clone(&built);
        bus.on(move |_: &LoginEvent| {
            b2.fetch_add(10, Ordering::Relaxed);
        });
        bus.dispatch_if(|| {
            built.fetch_add(1, Ordering::Relaxed);
            LoginEvent { user: "y".into() }
        });
        assert_eq!(built.load(Ordering::Relaxed), 11, "有监听器时构造一次 + 触发一次");
    }

    #[test]
    fn test_subscriber_registers_multiple() {
        struct MySubscriber {
            logins: Arc<AtomicUsize>,
            logouts: Arc<AtomicUsize>,
        }
        impl Subscriber for MySubscriber {
            fn subscribe(&self, bus: &EventBus) {
                let l = Arc::clone(&self.logins);
                bus.on(move |_: &LoginEvent| {
                    l.fetch_add(1, Ordering::Relaxed);
                });
                let lo = Arc::clone(&self.logouts);
                bus.on(move |_: &LogoutEvent| {
                    lo.fetch_add(1, Ordering::Relaxed);
                });
            }
        }

        let bus = EventBus::new();
        let logins = Arc::new(AtomicUsize::new(0));
        let logouts = Arc::new(AtomicUsize::new(0));
        bus.add_subscriber(&MySubscriber {
            logins: Arc::clone(&logins),
            logouts: Arc::clone(&logouts),
        });

        assert_eq!(bus.total_listeners(), 2);
        bus.dispatch(&LoginEvent { user: "a".into() });
        bus.dispatch(&LoginEvent { user: "b".into() });
        bus.dispatch(&LogoutEvent);
        assert_eq!(logins.load(Ordering::Relaxed), 2);
        assert_eq!(logouts.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_reentrant_subscribe_during_dispatch() {
        // 派发期间监听器再次订阅 —— 不应死锁（锁已在回调前释放，且派发用的是切片快照）
        let bus = Arc::new(EventBus::new());
        let bus2 = Arc::clone(&bus);
        let added = Arc::new(AtomicUsize::new(0));
        let a = Arc::clone(&added);
        bus.on(move |_: &LoginEvent| {
            a.fetch_add(1, Ordering::Relaxed);
            // 在回调中新增一个监听器：重建切片，不影响当前派发（用的是旧快照）
            bus2.on(|_: &LoginEvent| {});
        });
        bus.dispatch(&LoginEvent { user: "x".into() });
        assert_eq!(added.load(Ordering::Relaxed), 1);
        assert_eq!(bus.listener_count::<LoginEvent>(), 2, "回调中应已新增一个监听器");
        // 再次派发应触发 2 个监听器
        let second = Arc::new(AtomicUsize::new(0));
        let s = Arc::clone(&second);
        bus.on(move |_: &LoginEvent| {
            s.fetch_add(1, Ordering::Relaxed);
        });
        // 现有 3 个；dispatch 一次
        bus.dispatch(&LoginEvent { user: "y".into() });
        assert_eq!(second.load(Ordering::Relaxed), 1, "新监听器应被后续派发触发");
    }

    #[test]
    fn test_zero_cost_when_empty() {
        let bus = EventBus::new();
        assert_eq!(bus.total_listeners(), 0);
        assert!(!bus.has_listeners::<LoginEvent>());
        // 无监听器派发不应 panic、不分配
        bus.dispatch(&LoginEvent { user: "x".into() });
        bus.dispatch_if(|| LoginEvent { user: "y".into() });
    }
}
