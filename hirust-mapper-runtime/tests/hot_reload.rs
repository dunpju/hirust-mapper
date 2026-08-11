//! P7 热重载集成测试：修改 XML 文件后，注册表自动更新，查询结果随之变化。

use std::collections::HashMap;

use hirust_mapper_runtime::{EnvironmentConfig, HirustMapperConfig, SqlSessionFactory};

/// 写入 mapper 文件并返回（工厂, temp_dir）
async fn setup_with_hot_reload(
    suffix: &str,
    initial_xml: &str,
) -> (SqlSessionFactory, std::path::PathBuf) {
    let temp = std::env::temp_dir().join(format!("hirust_p7_hot_{}", suffix));
    std::fs::remove_dir_all(&temp).ok();
    let mappers_dir = temp.join("mappers");
    std::fs::create_dir_all(&mappers_dir).unwrap();
    std::fs::write(mappers_dir.join("UserMapper.xml"), initial_xml).unwrap();

    let config = HirustMapperConfig::new()
        .with_environment(EnvironmentConfig {
            driver: "sqlite".into(),
            url: "sqlite::memory:".into(),
            pool_max_connections: 1,
            pool_min_connections: 1,
        })
        .with_mapper_paths(vec!["mappers/**/*.xml".to_string()])
        .with_hot_reload(200); // 200ms 去抖

    let factory = SqlSessionFactory::build(config, &temp).await.unwrap();
    assert!(factory.hot_reload_enabled(), "热重载应已启用");
    (factory, temp)
}

/// 轮询等待热重载生效（最多 3 秒）
async fn await_reload<F: Fn() -> bool>(check: F) -> bool {
    for _ in 0..60 {
        if check() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn test_hot_reload_detects_sql_change() {
    let initial = r#"<mapper namespace="com.test.UserDao">
        <select id="findById">SELECT 'before' AS v</select>
    </mapper>"#;
    let (factory, temp) = setup_with_hot_reload("sql_change", initial).await;

    let session = factory.open_session();
    let params = HashMap::new();

    // 初始 SQL
    let before = session
        .build_bound_sql("com.test.UserDao", "findById", &params)
        .unwrap();
    assert!(
        before.sql.contains("before"),
        "初始 SQL 应含 'before': {}",
        before.sql
    );

    // 修改 XML 文件（更改 SQL 内容）
    let updated = r#"<mapper namespace="com.test.UserDao">
        <select id="findById">SELECT 'after' AS v</select>
    </mapper>"#;
    std::fs::write(temp.join("mappers").join("UserMapper.xml"), updated).unwrap();

    // 等待热重载生效（去抖 + 重解析）
    let factory_ref = &factory;
    let ok = await_reload(|| {
        let s = factory_ref.open_session();
        let b = s
            .build_bound_sql("com.test.UserDao", "findById", &params)
            .unwrap();
        b.sql.contains("after")
    })
    .await;
    assert!(ok, "热重载应在修改后反映新 SQL");

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_hot_reload_adds_new_statement() {
    // 初始只有一个 statement，热重载后增加第二个
    let initial = r#"<mapper namespace="com.test.UserDao">
        <select id="findById">SELECT 1</select>
    </mapper>"#;
    let (factory, temp) = setup_with_hot_reload("add_stmt", initial).await;

    let session = factory.open_session();
    let params = HashMap::new();
    assert!(session
        .build_bound_sql("com.test.UserDao", "findById", &params)
        .is_ok());
    assert!(session
        .build_bound_sql("com.test.UserDao", "findByName", &params)
        .is_err()); // 尚不存在

    // 增加新 statement
    let updated = r#"<mapper namespace="com.test.UserDao">
        <select id="findById">SELECT 1</select>
        <select id="findByName">SELECT 2 WHERE name = #{name}</select>
    </mapper>"#;
    std::fs::write(temp.join("mappers").join("UserMapper.xml"), updated).unwrap();

    let factory_ref = &factory;
    let ok = await_reload(|| {
        let s = factory_ref.open_session();
        s.build_bound_sql("com.test.UserDao", "findByName", &params).is_ok()
    })
    .await;
    assert!(ok, "热重载应使新增 statement 可用");

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}

#[tokio::test]
async fn test_hot_reload_disabled_by_default() {
    // refresh_interval = 0 → 不启用热重载
    let temp = std::env::temp_dir().join("hirust_p7_no_reload");
    std::fs::remove_dir_all(&temp).ok();
    let mappers_dir = temp.join("mappers");
    std::fs::create_dir_all(&mappers_dir).unwrap();
    std::fs::write(
        mappers_dir.join("UserMapper.xml"),
        r#"<mapper namespace="com.test.UserDao">
            <select id="findById">SELECT 1</select>
        </mapper>"#,
    )
    .unwrap();

    let config = HirustMapperConfig::new()
        .with_environment(EnvironmentConfig {
            driver: "sqlite".into(),
            url: "sqlite::memory:".into(),
            pool_max_connections: 1,
            pool_min_connections: 1,
        })
        .with_mapper_paths(vec!["mappers/**/*.xml".to_string()]);
    // 默认 refresh_interval = 0

    let factory = SqlSessionFactory::build(config, &temp).await.unwrap();
    assert!(!factory.hot_reload_enabled(), "默认应禁用热重载");

    factory.close().await;
    std::fs::remove_dir_all(temp).ok();
}
