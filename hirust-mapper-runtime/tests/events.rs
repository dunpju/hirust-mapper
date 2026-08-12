//! 事件系统集成测试：ORM 执行自动触发 BeforeSqlEvent / AfterSqlEvent，
//! 以及 Subscriber 批量订阅模式。

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use hirust_mapper_runtime::{
    AfterSqlEvent, BeforeSqlEvent, EnvironmentConfig, EventBus, HirustMapperConfig, SqlKind,
    SqlOutcome, SqlSessionFactory, Subscriber,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct User {
    id: i64,
    name: String,
    age: i64,
}

const XML: &str = r#"<mapper namespace="app.UserDao">
    <select id="findAll">SELECT id, name, age FROM users ORDER BY id</select>
    <insert id="insert">INSERT INTO users (name, age) VALUES (#{name}, #{age})</insert>
    <delete id="deleteAll">DELETE FROM users</delete>
</mapper>"#;

async fn setup(suffix: &str) -> (SqlSessionFactory, std::path::PathBuf) {
    let temp = std::env::temp_dir().join(format!("hirust_event_{}", suffix));
    std::fs::remove_dir_all(&temp).ok();
    let mappers = temp.join("mappers");
    std::fs::create_dir_all(&mappers).unwrap();
    std::fs::write(mappers.join("UserDao.xml"), XML).unwrap();

    let config = HirustMapperConfig::new()
        .with_environment(EnvironmentConfig {
            driver: "sqlite".into(),
            url: "sqlite::memory:".into(),
            pool_max_connections: 1,
            pool_min_connections: 1,
        })
        .with_mapper_paths(vec!["mappers/**/*.xml".to_string()]);

    let factory = SqlSessionFactory::build(config, &temp).await.unwrap();
    sqlx::query("CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, age INTEGER)")
        .execute(factory.environment().pool())
        .await
        .unwrap();
    (factory, temp)
}

/// 监听器记录的「种类 → 数量」结果摘要
fn outcome_count(e: &AfterSqlEvent) -> usize {
    match &e.outcome {
        SqlOutcome::Fetched(n) => *n,
        SqlOutcome::Affected(n) => *n as usize,
        SqlOutcome::Failed(_) => 0,
    }
}

#[tokio::test]
async fn test_lifecycle_events_fire_with_correct_data() {
    let (factory, temp) = setup("fire").await;

    let before_kinds = Arc::new(Mutex::new(Vec::<SqlKind>::new()));
    let after_summaries = Arc::new(Mutex::new(Vec::<(SqlKind, usize)>::new()));

    let b = Arc::clone(&before_kinds);
    factory.event_bus().on(move |e: &BeforeSqlEvent| {
        b.lock().unwrap().push(e.kind);
    });
    let a = Arc::clone(&after_summaries);
    factory.event_bus().on(move |e: &AfterSqlEvent| {
        a.lock().unwrap().push((e.kind, outcome_count(e)));
    });

    let mut session = factory.open_session();
    session
        .insert("app.UserDao", "insert", &User { id: 0, name: "张三".into(), age: 30 })
        .await
        .unwrap();
    let _: Vec<User> = session
        .select_list("app.UserDao", "findAll", &HashMap::new())
        .await
        .unwrap();

    let b = before_kinds.lock().unwrap();
    let a = after_summaries.lock().unwrap();

    // before 事件按执行顺序：Insert 然后 Select
    assert_eq!(*b, vec![SqlKind::Insert, SqlKind::Select]);

    // after 事件含正确结果摘要
    assert!(a.iter().any(|(k, n)| *k == SqlKind::Insert && *n == 1), "insert affected 1: {:?}", *a);
    assert!(a.iter().any(|(k, n)| *k == SqlKind::Select && *n == 1), "select fetched 1: {:?}", *a);

    // factory 与 session 共享同一事件总线
    assert!(factory.event_bus().listener_count::<AfterSqlEvent>() >= 1);

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_subscriber_pattern_registers_multiple() {
    let (factory, temp) = setup("subscriber").await;

    struct CountSubscriber {
        before: Arc<AtomicUsize>,
        after: Arc<AtomicUsize>,
    }
    impl Subscriber for CountSubscriber {
        fn subscribe(&self, bus: &EventBus) {
            let before = Arc::clone(&self.before);
            bus.on(move |_: &BeforeSqlEvent| {
                before.fetch_add(1, Ordering::Relaxed);
            });
            let after = Arc::clone(&self.after);
            bus.on(move |_: &AfterSqlEvent| {
                after.fetch_add(1, Ordering::Relaxed);
            });
        }
    }

    let before = Arc::new(AtomicUsize::new(0));
    let after = Arc::new(AtomicUsize::new(0));
    factory.event_bus().add_subscriber(&CountSubscriber {
        before: Arc::clone(&before),
        after: Arc::clone(&after),
    });

    assert_eq!(factory.event_bus().total_listeners(), 2);

    let mut session = factory.open_session();
    session
        .insert("app.UserDao", "insert", &User { id: 0, name: "x".into(), age: 1 })
        .await
        .unwrap();
    let _n = session
        .delete("app.UserDao", "deleteAll", &User { id: 0, name: "x".into(), age: 1 })
        .await
        .unwrap();

    // 2 次写操作 → 各 1 个 before + 1 个 after
    assert_eq!(before.load(Ordering::Relaxed), 2);
    assert_eq!(after.load(Ordering::Relaxed), 2);

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_no_listeners_zero_overhead() {
    // 不注册任何监听器：执行仍正常，不 panic
    let (factory, temp) = setup("zero").await;
    let mut session = factory.open_session();
    assert_eq!(factory.event_bus().total_listeners(), 0);

    session
        .insert("app.UserDao", "insert", &User { id: 0, name: "y".into(), age: 2 })
        .await
        .unwrap();
    let _: Vec<User> = session
        .select_list("app.UserDao", "findAll", &HashMap::new())
        .await
        .unwrap();

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}
