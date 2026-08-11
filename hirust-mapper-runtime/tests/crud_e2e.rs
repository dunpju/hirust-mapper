//! P6 端到端集成测试：加载 XML → SqlSession CRUD → 结果映射 → 事务
//!
//! 使用 SQLite 内存库（单连接池）执行真实 SQL。

use std::collections::HashMap;

use hirust_mapper_runtime::{
    EnvironmentConfig, HirustMapperConfig, MapperProxy, SqlSessionFactory,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct User {
    id: i64,
    name: String,
    age: i64,
}

const USER_MAPPER_XML: &str = r#"<mapper namespace="com.test.UserDao">
    <select id="findById">SELECT id, name, age FROM users WHERE id = #{id}</select>
    <select id="findAll">SELECT id, name, age FROM users ORDER BY id</select>
    <insert id="insert">INSERT INTO users (name, age) VALUES (#{name}, #{age})</insert>
    <update id="updateAge">UPDATE users SET age = #{age} WHERE id = #{id}</update>
    <delete id="deleteById">DELETE FROM users WHERE id = #{id}</delete>
    <select id="count">SELECT count(*) AS cnt FROM users</select>
</mapper>"#;

/// 准备：创建工厂 + 初始化 schema + 注册 mapper。
/// `suffix` 隔离并行测试的临时目录。返回 (factory, temp_dir)。
async fn setup(suffix: &str) -> (SqlSessionFactory, std::path::PathBuf) {
    let temp = std::env::temp_dir().join(format!("hirust_p6_e2e_{}", suffix));
    std::fs::remove_dir_all(&temp).ok();
    let mappers_dir = temp.join("mappers");
    std::fs::create_dir_all(&mappers_dir).unwrap();
    std::fs::write(mappers_dir.join("UserDao.xml"), USER_MAPPER_XML).unwrap();

    let config = HirustMapperConfig::new()
        .with_environment(EnvironmentConfig {
            driver: "sqlite".into(),
            url: "sqlite::memory:".into(),
            pool_max_connections: 1, // 单连接：保证内存库 schema 持久
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

fn params(pairs: &[(&str, Value)]) -> HashMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

#[tokio::test]
async fn test_insert_and_select_one() {
    let (factory, temp) = setup("insert_select").await;
    let mut session = factory.open_session();

    let id = session
        .insert("com.test.UserDao", "insert", &User { id: 0, name: "张三".into(), age: 30 })
        .await
        .unwrap();
    assert!(id.is_some(), "应返回自增主键");

    let found: Option<User> = session
        .select_one("com.test.UserDao", "findById", &params(&[("id", json!(id.unwrap()))]))
        .await
        .unwrap();
    let user = found.expect("应查到刚插入的用户");
    assert_eq!(user.name, "张三");
    assert_eq!(user.age, 30);

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_select_list() {
    let (factory, temp) = setup("select_list").await;
    let mut session = factory.open_session();

    for (i, name) in ["张三", "李四", "王五"].iter().enumerate() {
        session
            .insert("com.test.UserDao", "insert", &User { id: 0, name: (*name).into(), age: 20 + i as i64 })
            .await
            .unwrap();
    }

    let users: Vec<User> = session
        .select_list("com.test.UserDao", "findAll", &HashMap::new())
        .await
        .unwrap();
    assert_eq!(users.len(), 3);
    assert_eq!(users[0].name, "张三");
    assert_eq!(users[2].name, "王五");

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_update_and_delete() {
    let (factory, temp) = setup("update_delete").await;
    let mut session = factory.open_session();

    let id = session
        .insert("com.test.UserDao", "insert", &User { id: 0, name: "赵六".into(), age: 25 })
        .await
        .unwrap()
        .unwrap();

    let affected = session
        .update("com.test.UserDao", "updateAge", &params(&[("id", json!(id)), ("age", json!(99))]))
        .await
        .unwrap();
    assert_eq!(affected, 1);

    let user: User = session
        .select_one("com.test.UserDao", "findById", &params(&[("id", json!(id))]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(user.age, 99);

    let deleted = session
        .delete("com.test.UserDao", "deleteById", &params(&[("id", json!(id))]))
        .await
        .unwrap();
    assert_eq!(deleted, 1);

    let gone: Option<User> = session
        .select_one("com.test.UserDao", "findById", &params(&[("id", json!(id))]))
        .await
        .unwrap();
    assert!(gone.is_none(), "删除后应查不到");

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_transaction_commit() {
    let (factory, temp) = setup("tx_commit").await;
    let mut session = factory.open_session();

    session.begin().await.unwrap();
    assert!(session.in_transaction());

    session
        .insert("com.test.UserDao", "insert", &User { id: 0, name: "事务用户".into(), age: 40 })
        .await
        .unwrap();

    session.commit().await.unwrap();

    let mut session2 = factory.open_session();
    let users: Vec<User> = session2
        .select_list("com.test.UserDao", "findAll", &HashMap::new())
        .await
        .unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].name, "事务用户");

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_transaction_rollback() {
    let (factory, temp) = setup("tx_rollback").await;
    let mut session = factory.open_session();

    session.begin().await.unwrap();
    session
        .insert("com.test.UserDao", "insert", &User { id: 0, name: "应回滚".into(), age: 1 })
        .await
        .unwrap();
    session.rollback().await.unwrap();

    let mut session2 = factory.open_session();
    let users: Vec<User> = session2
        .select_list("com.test.UserDao", "findAll", &HashMap::new())
        .await
        .unwrap();
    assert!(users.is_empty(), "回滚后应无数据");

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_transaction_rollback_on_close() {
    let (factory, temp) = setup("tx_close").await;
    let mut session = factory.open_session();

    session.begin().await.unwrap();
    session
        .insert("com.test.UserDao", "insert", &User { id: 0, name: "未提交".into(), age: 2 })
        .await
        .unwrap();
    session.close().await.unwrap(); // 关闭 → 隐式回滚

    let mut session2 = factory.open_session();
    let users: Vec<User> = session2
        .select_list("com.test.UserDao", "findAll", &HashMap::new())
        .await
        .unwrap();
    assert!(users.is_empty(), "close 隐式回滚后应无数据");

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_mapper_proxy() {
    let (factory, temp) = setup("proxy").await;
    let mut session = factory.open_session();

    {
        let mut dao: MapperProxy = session.mapper("com.test.UserDao").unwrap();
        assert_eq!(dao.namespace(), "com.test.UserDao");
        dao.insert("insert", &User { id: 0, name: "代理用户".into(), age: 18 })
            .await
            .unwrap();
        let users: Vec<User> = dao.select_list("findAll", &HashMap::new()).await.unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].name, "代理用户");
    }

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_select_one_too_many_rows() {
    let (factory, temp) = setup("too_many").await;
    let mut session = factory.open_session();

    for name in ["a", "b"] {
        session
            .insert("com.test.UserDao", "insert", &User { id: 0, name: name.into(), age: 1 })
            .await
            .unwrap();
    }

    let result: Result<Option<User>, _> = session
        .select_one("com.test.UserDao", "findAll", &HashMap::new())
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("行数过多"));

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_scalar_count_query() {
    let (factory, temp) = setup("count").await;
    let mut session = factory.open_session();

    session
        .insert("com.test.UserDao", "insert", &User { id: 0, name: "x".into(), age: 1 })
        .await
        .unwrap();
    session
        .insert("com.test.UserDao", "insert", &User { id: 0, name: "y".into(), age: 2 })
        .await
        .unwrap();

    #[derive(Deserialize)]
    struct Count { cnt: i64 }
    let result: Count = session
        .select_one("com.test.UserDao", "count", &HashMap::new())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.cnt, 2);

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_mapper_not_found_error() {
    let (factory, temp) = setup("not_found").await;
    let mut session = factory.open_session();

    let result: Result<Option<User>, _> = session
        .select_one("nonexistent.Namespace", "x", &HashMap::new())
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("不存在"));

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}
