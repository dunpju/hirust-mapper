//! P9 `#[hirust_mapper(xml)]` 宏集成测试：编译时生成 DAO + 运行时 CRUD。

use std::collections::HashMap;

use hirust_mapper_macros::hirust_mapper;
use hirust_mapper_runtime::{EnvironmentConfig, HirustMapperConfig, SqlSessionFactory};
use serde::{Deserialize, Serialize};

// 编译时加载 + 解析 XML，生成 UserDao 的 typed 方法
#[hirust_mapper(xml = "tests/mappers/UserDao.xml")]
struct UserDao;

#[derive(Debug, Deserialize, PartialEq)]
struct User {
    id: i64,
    name: String,
    age: i64,
}

#[derive(Serialize)]
struct UpdateAge {
    id: i64,
    age: i64,
}

async fn setup() -> UserDao {
    let base = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let config = HirustMapperConfig::new()
        .with_environment(EnvironmentConfig {
            driver: "sqlite".into(),
            url: "sqlite::memory:".into(),
            pool_max_connections: 1, // 单连接：内存库 schema 跨 session 持久
            pool_min_connections: 1,
        })
        .with_mapper_paths(vec!["tests/mappers/UserDao.xml".to_string()]);

    let factory = SqlSessionFactory::build(config, &base).await.unwrap();
    sqlx::query(
        "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, age INTEGER)",
    )
    .execute(factory.environment().pool())
    .await
    .unwrap();

    UserDao::new(factory)
}

fn params(pairs: &[(&str, serde_json::Value)]) -> HashMap<String, serde_json::Value> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

#[tokio::test]
async fn dao_insert_and_find_by_id() {
    let dao = setup().await;

    let id = dao
        .insertUser(&serde_json::json!({ "name": "张三", "age": 30 }))
        .await
        .unwrap();
    assert!(id.is_some(), "应返回自增主键");

    let users: Vec<User> = dao
        .findById(&params(&[("id", serde_json::json!(id.unwrap()))]))
        .await
        .unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].name, "张三");
    assert_eq!(users[0].age, 30);
}

#[tokio::test]
async fn dao_find_all_and_update_and_delete() {
    let dao = setup().await;

    dao.insertUser(&serde_json::json!({ "name": "a", "age": 1 })).await.unwrap();
    dao.insertUser(&serde_json::json!({ "name": "b", "age": 2 })).await.unwrap();

    let all: Vec<User> = dao.findAll(&HashMap::new()).await.unwrap();
    assert_eq!(all.len(), 2);

    // update
    let affected = dao
        .updateAge(&UpdateAge { id: 1, age: 99 })
        .await
        .unwrap();
    assert_eq!(affected, 1);

    let u: Vec<User> = dao.findById(&params(&[("id", serde_json::json!(1))])).await.unwrap();
    assert_eq!(u[0].age, 99);

    // delete
    let deleted = dao.deleteById(&params(&[("id", serde_json::json!(1))])).await.unwrap();
    assert_eq!(deleted, 1);

    let remaining: Vec<User> = dao.findAll(&HashMap::new()).await.unwrap();
    assert_eq!(remaining.len(), 1);
}

#[tokio::test]
async fn dao_factory_accessor() {
    let dao = setup().await;
    // 验证生成的 factory 访问器
    assert_eq!(dao.factory().environment().driver(), "sqlite");
}
