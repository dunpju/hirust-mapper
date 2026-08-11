//! P8 ResultMap 嵌套映射端到端测试：association（一对一）+ collection（一对多分组）。

use std::collections::HashMap;

use hirust_mapper_runtime::{EnvironmentConfig, HirustMapperConfig, SqlSessionFactory};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Department {
    id: i64,
    name: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct UserWithDept {
    id: i64,
    name: String,
    #[serde(default)]
    department: Option<Department>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Role {
    id: i64,
    name: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct UserWithRoles {
    id: i64,
    name: String,
    #[serde(default)]
    roles: Vec<Role>,
}

async fn setup(suffix: &str, mapper_xml: &str, schema: &str) -> (SqlSessionFactory, std::path::PathBuf) {
    let temp = std::env::temp_dir().join(format!("hirust_p8_{}", suffix));
    std::fs::remove_dir_all(&temp).ok();
    let mappers_dir = temp.join("mappers");
    std::fs::create_dir_all(&mappers_dir).unwrap();
    std::fs::write(mappers_dir.join("M.xml"), mapper_xml).unwrap();

    let config = HirustMapperConfig::new()
        .with_environment(EnvironmentConfig {
            driver: "sqlite".into(),
            url: "sqlite::memory:".into(),
            pool_max_connections: 1,
            pool_min_connections: 1,
        })
        .with_mapper_paths(vec!["mappers/**/*.xml".to_string()]);

    let factory = SqlSessionFactory::build(config, &temp).await.unwrap();
    sqlx::query(schema).execute(factory.environment().pool()).await.unwrap();
    (factory, temp)
}

#[tokio::test]
async fn test_association_one_to_one() {
    // 用户 + 部门（扁平 join 行 → 嵌套 department）
    let mapper_xml = r#"<mapper namespace="u">
        <resultMap id="userDeptMap" type="UserWithDept">
            <id property="id" column="id"/>
            <result property="name" column="name"/>
            <association property="department" javaType="Department">
                <id property="id" column="dept_id"/>
                <result property="name" column="dept_name"/>
            </association>
        </resultMap>
        <select id="findAll" resultMap="userDeptMap">
            SELECT u.id AS id, u.name AS name, d.id AS dept_id, d.name AS dept_name
            FROM users u LEFT JOIN depts d ON u.dept_id = d.id ORDER BY u.id
        </select>
    </mapper>"#;
    let schema = "CREATE TABLE users (id INTEGER, name TEXT, dept_id INTEGER);
                  CREATE TABLE depts (id INTEGER, name TEXT)";
    let (factory, temp) = setup("assoc", mapper_xml, schema).await;

    // 插入数据
    sqlx::query("INSERT INTO depts VALUES (10, '工程部')").execute(factory.environment().pool()).await.unwrap();
    sqlx::query("INSERT INTO users VALUES (1, '张三', 10)").execute(factory.environment().pool()).await.unwrap();
    sqlx::query("INSERT INTO users VALUES (2, '李四', NULL)").execute(factory.environment().pool()).await.unwrap();

    let mut session = factory.open_session();
    let users: Vec<UserWithDept> = session
        .select_list("u", "findAll", &HashMap::new())
        .await
        .unwrap();

    assert_eq!(users.len(), 2);
    // 张三有部门
    assert_eq!(users[0].name, "张三");
    let dept = users[0].department.as_ref().expect("张三应有部门");
    assert_eq!(dept.id, 10);
    assert_eq!(dept.name, "工程部");
    // 李四无部门（LEFT JOIN，dept_id NULL → department null）
    assert_eq!(users[1].name, "李四");
    assert!(users[1].department.is_none(), "李四部门应为 None");

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_collection_one_to_many() {
    // 用户 + 多角色（join 行按 user.id 分组）
    let mapper_xml = r#"<mapper namespace="u">
        <resultMap id="userRolesMap" type="UserWithRoles">
            <id property="id" column="id"/>
            <result property="name" column="name"/>
            <collection property="roles" ofType="Role">
                <id property="id" column="role_id"/>
                <result property="name" column="role_name"/>
            </collection>
        </resultMap>
        <select id="findAll" resultMap="userRolesMap">
            SELECT u.id AS id, u.name AS name, r.id AS role_id, r.name AS role_name
            FROM users u LEFT JOIN roles r ON r.user_id = u.id ORDER BY u.id, r.id
        </select>
    </mapper>"#;
    let schema = "CREATE TABLE users (id INTEGER, name TEXT);
                  CREATE TABLE roles (id INTEGER, name TEXT, user_id INTEGER)";
    let (factory, temp) = setup("coll", mapper_xml, schema).await;

    sqlx::query("INSERT INTO users VALUES (1, '张三')").execute(factory.environment().pool()).await.unwrap();
    sqlx::query("INSERT INTO users VALUES (2, '李四')").execute(factory.environment().pool()).await.unwrap();
    sqlx::query("INSERT INTO roles VALUES (1, 'admin', 1)").execute(factory.environment().pool()).await.unwrap();
    sqlx::query("INSERT INTO roles VALUES (2, 'editor', 1)").execute(factory.environment().pool()).await.unwrap();
    // 用户 2 无角色

    let mut session = factory.open_session();
    let users: Vec<UserWithRoles> = session
        .select_list("u", "findAll", &HashMap::new())
        .await
        .unwrap();

    assert_eq!(users.len(), 2, "应分组成 2 个用户");
    // 张三：2 个角色
    assert_eq!(users[0].name, "张三");
    assert_eq!(users[0].roles.len(), 2, "张三应有 2 个角色");
    assert_eq!(users[0].roles[0].name, "admin");
    assert_eq!(users[0].roles[1].name, "editor");
    // 李四：0 个角色
    assert_eq!(users[1].name, "李四");
    assert!(users[1].roles.is_empty(), "李四应无角色");

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_select_one_with_result_map() {
    let mapper_xml = r#"<mapper namespace="u">
        <resultMap id="userDeptMap" type="UserWithDept">
            <id property="id" column="id"/>
            <result property="name" column="name"/>
            <association property="department" javaType="Department">
                <id property="id" column="dept_id"/>
                <result property="name" column="dept_name"/>
            </association>
        </resultMap>
        <select id="findById" resultMap="userDeptMap">
            SELECT u.id AS id, u.name AS name, d.id AS dept_id, d.name AS dept_name
            FROM users u LEFT JOIN depts d ON u.dept_id = d.id WHERE u.id = #{id}
        </select>
    </mapper>"#;
    let schema = "CREATE TABLE users (id INTEGER, name TEXT, dept_id INTEGER);
                  CREATE TABLE depts (id INTEGER, name TEXT)";
    let (factory, temp) = setup("one", mapper_xml, schema).await;

    sqlx::query("INSERT INTO depts VALUES (20, '市场部')").execute(factory.environment().pool()).await.unwrap();
    sqlx::query("INSERT INTO users VALUES (5, '王五', 20)").execute(factory.environment().pool()).await.unwrap();

    let mut session = factory.open_session();
    let mut p = HashMap::new();
    p.insert("id".to_string(), serde_json::json!(5));
    let user: Option<UserWithDept> = session.select_one("u", "findById", &p).await.unwrap();

    let user = user.expect("应查到王五");
    assert_eq!(user.name, "王五");
    assert_eq!(user.department.as_ref().unwrap().name, "市场部");

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}
