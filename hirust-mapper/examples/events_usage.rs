//! 事件监听与订阅示例。
//!
//! 演示：
//! 1. 自定义事件类型（实现 [`hirust_mapper::Event`]）+ 闭包监听器
//! 2. [`hirust_mapper::Subscriber`] 订阅器批量注册多个 SQL 生命周期事件
//! 3. ORM 执行自动触发 `BeforeSqlEvent` / `AfterSqlEvent`
//!
//! 运行：
//! ```sh
//! cargo run --example events_usage --features runtime
//! ```

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use hirust_mapper::{
    AfterSqlEvent, BeforeSqlEvent, EnvironmentConfig, Event, EventBus, HirustMapperConfig,
    SqlOutcome, SqlSessionFactory, Subscriber,
};
use serde::{Deserialize, Serialize};

/// 1) 自定义事件：只需实现 `Event` 标记 trait 即可被 EventBus 分发
#[derive(Debug)]
struct LoginEvent {
    user: String,
}
impl Event for LoginEvent {}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct User {
    id: i64,
    name: String,
    age: i64,
}

/// 2) 订阅器：在一个 `subscribe` 实现里批量注册多个事件监听器
///    （对应 ThinkPHP 的「事件订阅」概念）
struct AuditSubscriber {
    sql_count: Arc<AtomicUsize>,
}

impl Subscriber for AuditSubscriber {
    fn subscribe(&self, bus: &EventBus) {
        // 执行前：观察即将运行的 SQL
        bus.on(|e: &BeforeSqlEvent| {
            println!("[BEFORE] kind={:?} params={}", e.kind, e.params.len());
        });
        // 执行后：记录耗时与结果摘要
        let counter = Arc::clone(&self.sql_count);
        bus.on(move |e: &AfterSqlEvent| {
            counter.fetch_add(1, Ordering::Relaxed);
            match &e.outcome {
                SqlOutcome::Fetched(n) => println!(
                    "[AFTER ] kind={:?} | {:>3} ms | fetched {}",
                    e.kind,
                    e.elapsed.as_millis(),
                    n
                ),
                SqlOutcome::Affected(n) => println!(
                    "[AFTER ] kind={:?} | {:>3} ms | affected {}",
                    e.kind,
                    e.elapsed.as_millis(),
                    n
                ),
                SqlOutcome::Failed(err) => println!("[AFTER ] kind={:?} | FAILED: {}", e.kind, err),
            }
        });
    }
}

const USER_DAO_XML: &str = r#"<mapper namespace="ex.UserDao">
    <select id="findAll">SELECT id, name, age FROM users ORDER BY id</select>
    <insert id="insert">INSERT INTO users (name, age) VALUES (#{name}, #{age})</insert>
</mapper>"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 准备 mapper XML 到临时目录（供运行时 glob 发现）
    let temp = std::env::temp_dir().join("hirust_example_events");
    std::fs::remove_dir_all(&temp).ok();
    let mappers_dir = temp.join("mappers");
    std::fs::create_dir_all(&mappers_dir)?;
    std::fs::write(mappers_dir.join("UserDao.xml"), USER_DAO_XML)?;

    let config = HirustMapperConfig::new()
        .with_environment(EnvironmentConfig {
            driver: "sqlite".into(),
            url: "sqlite::memory:".into(),
            pool_max_connections: 1,
            pool_min_connections: 1,
        })
        .with_mapper_paths(vec!["mappers/**/*.xml".to_string()]);

    let factory = SqlSessionFactory::build(config, &temp).await?;
    sqlx::query("CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, age INTEGER)")
        .execute(factory.environment().pool())
        .await?;

    // 3a) 用订阅器批量注册 SQL 生命周期监听器（所有 session 共享工厂的事件总线）
    let sql_count = Arc::new(AtomicUsize::new(0));
    factory
        .event_bus()
        .add_subscriber(&AuditSubscriber { sql_count: Arc::clone(&sql_count) });

    // 3b) 同一总线也可派发自定义业务事件
    factory
        .event_bus()
        .on(|e: &LoginEvent| println!("[LOGIN] {} 登录了", e.user));
    factory.event_bus().dispatch(&LoginEvent { user: "张三".into() });

    println!("--- ORM 操作开始 ---");
    let mut session = factory.open_session();
    session
        .insert("ex.UserDao", "insert", &User { id: 0, name: "张三".into(), age: 30 })
        .await?;
    let users: Vec<User> = session.select_list("ex.UserDao", "findAll", &Default::default()).await?;
    println!("--- ORM 操作结束，查到 {} 行 ---", users.len());

    println!("共触发 {} 次 SQL 执行后事件", sql_count.load(Ordering::Relaxed));

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
    Ok(())
}
