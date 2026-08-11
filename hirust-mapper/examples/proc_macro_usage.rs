//! Proc Macro 类型安全示例。
//!
//! 演示：`#[hirust_mapper(xml)]` 编译时生成 DAO + `#[derive(MapperModel)]` 列映射。
//!
//! 运行：
//! ```sh
//! cargo run --example proc_macro_usage --features full
//! ```

use std::collections::HashMap;

use hirust_mapper::{
    hirust_mapper, EnvironmentConfig, HirustMapperConfig, MapperModel, SqlSessionFactory,
};
use serde::{Deserialize, Serialize};

// 编译时加载 + 解析 examples/mappers/UserDao.xml，校验并生成 UserDao 方法。
// 路径相对 CARGO_MANIFEST_DIR（即 facade crate 根目录）。
#[hirust_mapper(xml = "examples/mappers/UserDao.xml")]
struct UserDao;

// 列映射内省（column_mappings / type_handlers）
#[derive(MapperModel, Deserialize, Serialize, Debug, PartialEq)]
#[allow(dead_code)]
struct User {
    id: i64,
    name: String,
    age: i64,
}

fn params(pairs: &[(&str, serde_json::Value)]) -> HashMap<String, serde_json::Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("User::column_mappings() = {:?}", User::column_mappings());

    // 配置 + 工厂（运行时加载同一 XML 注册 namespace）
    let base = std::env::var("CARGO_MANIFEST_DIR")?;
    let config = HirustMapperConfig::new()
        .with_environment(EnvironmentConfig {
            driver: "sqlite".into(),
            url: "sqlite::memory:".into(),
            pool_max_connections: 1,
            pool_min_connections: 1,
        })
        .with_mapper_paths(vec!["examples/mappers/UserDao.xml".into()]);
    let factory = SqlSessionFactory::build(config, &base).await?;

    sqlx::query("CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, age INTEGER)")
        .execute(factory.environment().pool())
        .await?;

    // 编译时生成的 DAO 方法（类型安全，方法名即 statement id）
    let dao = UserDao::new(factory);

    // insert（返回主键）
    let id = dao
        .insertUser(&User { id: 0, name: "张三".into(), age: 30 })
        .await?
        .expect("主键");
    println!("✓ insertUser -> id = {}", id);

    dao.insertUser(&User { id: 0, name: "李四".into(), age: 25 }).await?;

    // select（返回类型由调用方指定）
    let users: Vec<User> = dao.findAll(&HashMap::new()).await?;
    println!("✓ findAll -> {} 行: {:?}", users.len(), users);

    let one: Vec<User> = dao.findById(&params(&[("id", serde_json::json!(id))])).await?;
    println!("✓ findById -> {:?}", one);

    // update
    let n = dao.updateAge(&params(&[("id", serde_json::json!(id)), ("age", serde_json::json!(99))])).await?;
    println!("✓ updateAge -> 受影响 {} 行", n);

    // delete
    let d = dao.deleteById(&params(&[("id", serde_json::json!(id))])).await?;
    println!("✓ deleteById -> 删除 {} 行", d);

    let remaining: Vec<User> = dao.findAll(&HashMap::new()).await?;
    println!("✓ 剩余 {} 行", remaining.len());

    // factory 在 Arc 中，程序结束时连接池自动释放
    println!("\n全部完成 ✨");
    Ok(())
}
