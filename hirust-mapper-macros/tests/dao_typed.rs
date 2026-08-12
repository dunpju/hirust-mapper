//! `#[dao]` + `#[mapper_query]` 类型化 DAO 集成测试。
//!
//! 验证：方法名→statement_id、形参名→SQL 参数、返回类型分派（Option/Vec/insert/update/delete）、
//! 编译期 XML statement 校验（`xml=`）、foreach 集合参数。

use hirust_mapper_macros::dao;
use hirust_mapper_runtime::{EnvironmentConfig, HirustMapperConfig, Result, SqlSessionFactory};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct User {
    id: i64,
    name: String,
    status: i64,
}

// struct 侧：#[dao] 加 factory 字段 + new/factory
#[dao]
pub struct PrivilegeDao;

// impl 侧：namespace 显式；xml= 启用编译期 statement_id 校验
#[dao(namespace = "test.privilege", xml = "tests/mappers/PrivilegeDao.xml")]
impl PrivilegeDao {
    #[mapper_query]
    pub async fn find_by_id(&self, id: i64) -> Result<Option<User>> {}

    #[mapper_query]
    pub async fn list_by_status(&self, status: i64) -> Result<Vec<User>> {}

    #[mapper_query(kind = "insert")]
    pub async fn create(&self, name: String, status: i64) -> Result<i64> {}

    #[mapper_query(kind = "update")]
    pub async fn set_status(&self, id: i64, status: i64) -> Result<u64> {}

    #[mapper_query(kind = "delete")]
    pub async fn remove_by_id(&self, id: i64) -> Result<u64> {}

    // foreach 集合参数：形参名 project_ids ↔ collection="project_ids"
    #[mapper_query]
    pub async fn get_by_privilege_project_ids(&self, project_ids: Vec<i64>) -> Result<Vec<User>> {}
}

async fn setup() -> PrivilegeDao {
    let base = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let config = HirustMapperConfig::new()
        .with_environment(EnvironmentConfig {
            driver: "sqlite".into(),
            url: "sqlite::memory:".into(),
            pool_max_connections: 1,
            pool_min_connections: 1,
        })
        .with_mapper_paths(vec!["tests/mappers/PrivilegeDao.xml".to_string()]);
    let factory = SqlSessionFactory::build(config, &base).await.unwrap();
    sqlx::query(
        "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, status INTEGER)",
    )
    .execute(factory.environment().pool())
    .await
    .unwrap();
    PrivilegeDao::new(factory)
}

#[tokio::test]
async fn dao_select_one() {
    let dao = setup().await;
    dao.create("张三".into(), 1).await.unwrap();
    let u = dao.find_by_id(1).await.unwrap().expect("应查到");
    assert_eq!(u.name, "张三");
    assert_eq!(u.status, 1);

    // 无行 → None
    let none = dao.find_by_id(999).await.unwrap();
    assert!(none.is_none());
}

#[tokio::test]
async fn dao_select_list_and_foreach() {
    let dao = setup().await;
    dao.create("a".into(), 5).await.unwrap();
    dao.create("b".into(), 5).await.unwrap();
    dao.create("c".into(), 9).await.unwrap();

    let list: Vec<User> = dao.list_by_status(5).await.unwrap();
    assert_eq!(list.len(), 2);

    // foreach 集合参数
    let ids = dao.get_by_privilege_project_ids(vec![1, 2, 3]).await.unwrap();
    assert_eq!(ids.len(), 3);
}

#[tokio::test]
async fn dao_insert_update_delete() {
    let dao = setup().await;
    let id = dao.create("王五".into(), 2).await.unwrap();
    assert!(id > 0, "insert 返回主键");

    let n = dao.set_status(id, 7).await.unwrap();
    assert_eq!(n, 1);
    assert_eq!(dao.find_by_id(id).await.unwrap().unwrap().status, 7);

    let d = dao.remove_by_id(id).await.unwrap();
    assert_eq!(d, 1);
    assert!(dao.find_by_id(id).await.unwrap().is_none());
}

#[tokio::test]
async fn dao_multiple_params_map_by_name() {
    // create(name, status)：两个形参按名映射到 #{name} #{status}
    let dao = setup().await;
    let id = dao.create("多参".into(), 42).await.unwrap();
    let u = dao.find_by_id(id).await.unwrap().unwrap();
    assert_eq!(u.name, "多参");
    assert_eq!(u.status, 42);
}

#[tokio::test]
async fn dao_factory_accessor() {
    let dao = setup().await;
    assert_eq!(dao.factory().environment().driver(), "sqlite");
}
