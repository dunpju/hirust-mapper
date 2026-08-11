//! 运行时 API 基础示例。
//!
//! 演示：配置加载 → SqlSessionFactory → SqlSession → CRUD → 事务。
//!
//! 运行：
//! ```sh
//! cargo run --example runtime_basic --features runtime
//! ```

use std::collections::HashMap;

use hirust_mapper::{EnvironmentConfig, HirustMapperConfig, SqlSessionFactory};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct User {
    id: i64,
    name: String,
    age: i64,
}

const USER_DAO_XML: &str = r#"<mapper namespace="ex.UserDao">
    <select id="findById">SELECT id, name, age FROM users WHERE id = #{id}</select>
    <select id="findAll">SELECT id, name, age FROM users ORDER BY id</select>
    <insert id="insert">INSERT INTO users (name, age) VALUES (#{name}, #{age})</insert>
    <update id="updateAge">UPDATE users SET age = #{age} WHERE id = #{id}</update>
</mapper>"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 准备 mapper XML（写入临时目录，供运行时 glob 发现）
    let temp = std::env::temp_dir().join("hirust_example_runtime");
    std::fs::remove_dir_all(&temp).ok();
    let mappers_dir = temp.join("mappers");
    std::fs::create_dir_all(&mappers_dir)?;
    std::fs::write(mappers_dir.join("UserDao.xml"), USER_DAO_XML)?;

    // 2. 配置 + 构建 SessionFactory（应用级）
    let config = HirustMapperConfig::new()
        .with_environment(EnvironmentConfig {
            driver: "sqlite".into(),
            url: "sqlite::memory:".into(),
            pool_max_connections: 1, // 单连接：保证内存库 schema 持久
            pool_min_connections: 1,
        })
        .with_mapper_paths(vec!["mappers/**/*.xml".into()]);
    let factory = SqlSessionFactory::build(config, &temp).await?;
    println!("✓ SessionFactory 构建完成，加载 mapper: {:?}", factory.namespaces());

    // 3. 建表（DDL 走连接池）
    sqlx::query(
        "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, age INTEGER)",
    )
    .execute(factory.environment().pool())
    .await?;

    // 4. 插入（返回自增主键）
    let mut session = factory.open_session();
    let id = session
        .insert("ex.UserDao", "insert", &User { id: 0, name: "张三".into(), age: 30 })
        .await?
        .expect("应返回主键");
    println!("✓ 插入用户，生成主键 id = {}", id);

    // 5. 查询单行
    let mut params = HashMap::new();
    params.insert("id".to_string(), serde_json::json!(id));
    let user: User = session
        .select_one("ex.UserDao", "findById", &params)
        .await?
        .unwrap();
    println!("✓ 查询: {:?}", user);

    // 6. 查询多行
    session
        .insert("ex.UserDao", "insert", &User { id: 0, name: "李四".into(), age: 25 })
        .await?;
    let all: Vec<User> = session
        .select_list("ex.UserDao", "findAll", &HashMap::new())
        .await?;
    println!("✓ 全表查询 ({} 行): {:?}", all.len(), all);

    // 7. 更新
    let mut upd = HashMap::new();
    upd.insert("id".to_string(), serde_json::json!(id));
    upd.insert("age".to_string(), serde_json::json!(31));
    let affected = session.update("ex.UserDao", "updateAge", &upd).await?;
    println!("✓ 更新受影响行数: {}", affected);

    // 8. 事务（提交）
    drop(session); // 释放上一个 session
    let mut tx = factory.open_session();
    tx.begin().await?;
    tx.insert("ex.UserDao", "insert", &User { id: 0, name: "事务用户".into(), age: 40 }).await?;
    tx.commit().await?;
    println!("✓ 事务提交成功");

    // 9. 验证事务结果
    let mut s2 = factory.open_session();
    let after: Vec<User> = s2.select_list("ex.UserDao", "findAll", &HashMap::new()).await?;
    println!("✓ 事务后共 {} 行", after.len());

    factory.close().await;
    std::fs::remove_dir_all(&temp).ok();
    println!("\n全部完成 ✨");
    Ok(())
}
