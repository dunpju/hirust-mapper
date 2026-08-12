//! 流式查询（streaming fetch）集成测试
//!
//! 验证 `select_for_each`（session 级回调式）与 `SimpleExecutor::query_stream`
//!（executor 级 Stream）按行拉取、不一次性物化整表，且映射正确。

use std::collections::HashMap;

use futures_util::StreamExt;
use hirust_mapper_runtime::{
    EnvironmentConfig, HirustMapperConfig, SqlSessionFactory,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct User {
    id: i64,
    name: String,
    age: i64,
}

const USER_MAPPER_XML: &str = r#"<mapper namespace="com.test.UserDao">
    <select id="findAll">SELECT id, name, age FROM users ORDER BY id</select>
    <insert id="insert">INSERT INTO users (name, age) VALUES (#{name}, #{age})</insert>
</mapper>"#;

async fn setup(suffix: &str) -> (SqlSessionFactory, std::path::PathBuf) {
    let temp = std::env::temp_dir().join(format!("hirust_stream_{}", suffix));
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
        .with_mapper_paths(vec!["mappers/**/*.xml".to_string()]);

    let factory = SqlSessionFactory::build(config, &temp).await.unwrap();

    sqlx::query("CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, age INTEGER)")
        .execute(factory.environment().pool())
        .await
        .unwrap();

    (factory, temp)
}

async fn insert_users(session: &mut hirust_mapper_runtime::SqlSession, names: &[&str]) {
    for name in names {
        session
            .insert("com.test.UserDao", "insert", &User { id: 0, name: (*name).into(), age: 20 })
            .await
            .unwrap();
    }
}

/// session 级回调式流式：逐行回调，验证行数与内容
#[tokio::test]
async fn test_select_for_each() {
    let (factory, temp) = setup("foreach").await;
    let mut session = factory.open_session();
    insert_users(&mut session, &["张三", "李四", "王五"]).await;

    let mut collected: Vec<User> = Vec::new();
    session
        .select_for_each(
            "com.test.UserDao",
            "findAll",
            &HashMap::new(),
            |u: &User| {
                collected.push(u.clone());
                Ok(())
            },
        )
        .await
        .unwrap();

    assert_eq!(collected.len(), 3);
    assert_eq!(collected[0].name, "张三");
    assert_eq!(collected[1].name, "李四");
    assert_eq!(collected[2].name, "王五");
    // id 自增连续
    assert_eq!(collected[0].id + 1, collected[1].id);

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

/// 回调返回 Err 可提前终止流
#[tokio::test]
async fn test_select_for_each_early_exit() {
    let (factory, temp) = setup("early_exit").await;
    let mut session = factory.open_session();
    insert_users(&mut session, &["甲", "乙", "丙"]).await;

    let mut seen = 0;
    let res = session
        .select_for_each(
            "com.test.UserDao",
            "findAll",
            &HashMap::new(),
            |_u: &User| {
                seen += 1;
                if seen == 2 {
                    Err(hirust_mapper_runtime::MapperRuntimeError::Transaction("stop".into()))
                } else {
                    Ok(())
                }
            },
        )
        .await;

    assert!(res.is_err(), "回调 Err 应向上传递");
    assert_eq!(seen, 2, "应在第 2 行终止");

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

/// executor 级 Stream：配合 StreamExt 消费
#[tokio::test]
async fn test_executor_query_stream() {
    let (factory, temp) = setup("exec_stream").await;
    let mut session = factory.open_session();
    insert_users(&mut session, &["甲", "乙"]).await;

    let bound = session
        .build_bound_sql("com.test.UserDao", "findAll", &HashMap::new())
        .unwrap();
    let mut stream = session.executor().query_stream::<_, User>(&bound, session.pool());

    let mut names = Vec::new();
    while let Some(item) = stream.next().await {
        names.push(item.unwrap().name);
    }
    assert_eq!(names, vec!["甲".to_string(), "乙".to_string()]);

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

/// 空结果集：流立即结束，回调不触发
#[tokio::test]
async fn test_select_for_each_empty() {
    let (factory, temp) = setup("empty").await;
    let mut session = factory.open_session();

    let mut count = 0;
    session
        .select_for_each(
            "com.test.UserDao",
            "findAll",
            &HashMap::new(),
            |_u: &User| {
                count += 1;
                Ok(())
            },
        )
        .await
        .unwrap();
    assert_eq!(count, 0);

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}
