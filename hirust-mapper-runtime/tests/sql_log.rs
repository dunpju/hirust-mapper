//! SQL 执行日志集成测试
//!
//! 用自定义 `log::Log` 捕获器验证：开启 `sql_log` 后执行点经 `log` facade 真正发射日志，
//! 日志含「耗时 + 参数内联的可读 SQL」；关闭时不发射。

use std::collections::HashMap;
use std::sync::{Mutex, Once};

use hirust_mapper_runtime::{
    executor::execute_rows_affected, BoundSql, EnvironmentConfig, EventBus, HirustMapperConfig,
    SqlLogConfig, SqlSessionFactory,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct User {
    id: i64,
    name: String,
    age: i64,
}

/// 捕获本二进制内所有 hirust_mapper target 的日志。
static LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// 日志后端只能安装一次（跨测试共享）。
static INIT: Once = Once::new();
/// 串行化本文件中的测试，避免并发污染 LOGS 的前后快照。
static TEST_LOCK: Mutex<()> = Mutex::new(());

struct CapturingLogger;
impl log::Log for CapturingLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.target().starts_with("hirust_mapper")
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            LOGS.lock().unwrap().push(format!("{}", record.args()));
        }
    }
    fn flush(&self) {}
}

fn install_logger() {
    INIT.call_once(|| {
        log::set_boxed_logger(Box::new(CapturingLogger)).unwrap();
        log::set_max_level(log::LevelFilter::Info);
    });
}

const USER_MAPPER_XML: &str = r#"<mapper namespace="app.UserDao">
    <select id="findAll">SELECT id, name, age FROM users ORDER BY id</select>
    <insert id="insert">INSERT INTO users (name, age) VALUES (#{name}, #{age})</insert>
</mapper>"#;

async fn setup(suffix: &str, sql_log: bool) -> (SqlSessionFactory, std::path::PathBuf) {
    let temp = std::env::temp_dir().join(format!("hirust_sqllog_{}", suffix));
    std::fs::remove_dir_all(&temp).ok();
    let mappers_dir = temp.join("mappers");
    std::fs::create_dir_all(&mappers_dir).unwrap();
    std::fs::write(mappers_dir.join("UserDao.xml"), USER_MAPPER_XML).unwrap();

    let config = HirustMapperConfig::new()
        .with_environment(EnvironmentConfig {
            driver: "sqlite".into(),
            url: "sqlite::memory:".into(),
            pool_max_connections: 1,
            pool_min_connections: 1,
        })
        .with_mapper_paths(vec!["mappers/**/*.xml".to_string()])
        .with_sql_log(sql_log);

    let factory = SqlSessionFactory::build(config, &temp).await.unwrap();
    sqlx::query("CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, age INTEGER)")
        .execute(factory.environment().pool())
        .await
        .unwrap();
    (factory, temp)
}

#[tokio::test]
async fn test_sql_log_emits_on_query_and_insert() {
    let _guard = TEST_LOCK.lock().unwrap();
    install_logger();
    let (factory, temp) = setup("emits", true).await;
    let mut session = factory.open_session();

    let before = LOGS.lock().unwrap().len();
    session
        .insert("app.UserDao", "insert", &User { id: 0, name: "张三".into(), age: 30 })
        .await
        .unwrap();
    let _: Vec<User> = session
        .select_list("app.UserDao", "findAll", &HashMap::new())
        .await
        .unwrap();
    let after = LOGS.lock().unwrap().len();

    assert!(after > before, "开启 sql_log 后应发射日志");

    let logs = LOGS.lock().unwrap();
    let recent = logs[before..after].join("\n");
    assert!(recent.contains("Consume Time"), "应包含耗时字段\n{recent}");
    assert!(recent.contains("Execute SQL"), "应包含 Execute SQL\n{recent}");
    assert!(recent.contains("INSERT INTO users"), "应记录 insert SQL\n{recent}");
    assert!(recent.contains("SELECT id, name, age"), "应记录 select SQL\n{recent}");
    assert!(recent.contains("'张三'"), "参数应内联进 SQL\n{recent}");

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_sql_log_disabled_emits_nothing() {
    let _guard = TEST_LOCK.lock().unwrap();
    install_logger();
    let (factory, temp) = setup("disabled", false).await;
    let mut session = factory.open_session();

    let before = LOGS.lock().unwrap().len();
    session
        .insert("app.UserDao", "insert", &User { id: 0, name: "x".into(), age: 1 })
        .await
        .unwrap();
    let _: Vec<User> = session
        .select_list("app.UserDao", "findAll", &HashMap::new())
        .await
        .unwrap();
    let after = LOGS.lock().unwrap().len();

    assert_eq!(after, before, "关闭 sql_log 时不应发射任何日志");

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_execute_rows_affected_respects_config() {
    let _guard = TEST_LOCK.lock().unwrap();
    install_logger();
    let (factory, temp) = setup("rows_affected", false).await; // 全局关闭
    let session = factory.open_session();
    sqlx::query("DELETE FROM users").execute(session.pool()).await.unwrap();

    // 显式传入开启的配置 → execute_rows_affected 应发射日志
    let bound = BoundSql {
        sql: "INSERT INTO users (name, age) VALUES (?, ?)".to_string(),
        parameters: vec![json!("甲"), json!(1)],
    };
    let cfg_on = SqlLogConfig { enabled: true, slow_threshold_ms: 0 };
    let bus = EventBus::new();
    let before = LOGS.lock().unwrap().len();
    let n = execute_rows_affected(&bound, session.pool(), &cfg_on, &bus).await.unwrap();
    let after = LOGS.lock().unwrap().len();
    assert_eq!(n, 1);
    assert!(after > before, "传入开启配置时应发射日志");
    assert!(LOGS.lock().unwrap()[before..after].join("\n").contains("INSERT INTO users"));

    // 传入关闭的配置 → 不发射
    let cfg_off = SqlLogConfig::default();
    let before2 = LOGS.lock().unwrap().len();
    let _ = execute_rows_affected(&bound, session.pool(), &cfg_off, &bus).await.unwrap();
    assert_eq!(LOGS.lock().unwrap().len(), before2, "传入关闭配置时不应发射");

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}
