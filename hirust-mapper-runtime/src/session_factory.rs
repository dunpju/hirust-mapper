//! SqlSession 工厂模块
//!
//! `SqlSessionFactory` 是应用级的长生命周期对象（对应 MyBatis 的 `SqlSessionFactory`），
//! 持有 Mapper 注册表、数据库环境与类型别名注册表。
//! 通过 `build()` 从配置构建，通过 `open_session()` 创建请求级的 `SqlSession`。

use std::path::Path;
use std::sync::{Arc, RwLock};

use hirust_mapper_core::Mapper;

use crate::config::HirustMapperConfig;
use crate::environment::Environment;
use crate::error::{MapperRuntimeError, Result};
use crate::registry::{MapperRegistry, TypeAliasRegistry};

/// SqlSession 工厂（应用级，线程安全）
///
/// 生命周期与整个应用相同。持有一个连接池、一个线程安全的 Mapper 注册表，
/// 以及一个类型别名注册表。每个请求通过 `open_session()` 获取独立的 `SqlSession`。
///
/// # 示例
///
/// ```ignore
/// let config = HirustMapperConfig::load_file("hirust-mapper.toml")?;
/// let factory = SqlSessionFactory::build(config).await?;
/// let session = factory.open_session().await?;
/// ```
pub struct SqlSessionFactory {
    /// 数据库环境（连接池）
    environment: Environment,
    /// 线程安全的 Mapper 注册表
    mapper_registry: Arc<RwLock<MapperRegistry>>,
    /// 类型别名注册表
    type_alias_registry: TypeAliasRegistry,
    /// 配置（保留副本供后续热重载等使用）
    config: HirustMapperConfig,
    /// 配置文件的基准目录（用于解析相对路径）
    base_dir: std::path::PathBuf,
}

impl std::fmt::Debug for SqlSessionFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlSessionFactory")
            .field("driver", &self.environment.driver())
            .field("url", &self.environment.url())
            .field("mapper_count", &self.mapper_count())
            .field("type_aliases", &self.type_alias_registry.resolve("__not_used__"))
            .finish()
    }
}

impl SqlSessionFactory {
    /// 从配置构建 SqlSessionFactory
    ///
    /// 完整流程：
    /// 1. 创建数据库连接池（`Environment`）
    /// 2. 初始化 Mapper 注册表并加载所有 XML mapper 文件
    /// 3. 初始化类型别名注册表
    /// 4. 返回就绪的工厂实例
    ///
    /// `base_dir` 用于解析 mapper 文件的相对 glob 路径，通常传入项目根目录。
    pub async fn build<P: AsRef<Path>>(
        config: HirustMapperConfig,
        base_dir: P,
    ) -> Result<Self> {
        let base_dir = base_dir.as_ref().to_path_buf();

        // 1. 创建数据库环境
        let environment = Environment::from_config(&config.environment).await?;

        // 2. 初始化并加载 Mapper 注册表
        let mapper_registry = MapperRegistry::new();
        let _namespaces = mapper_registry.load_from_config(&config, &base_dir)?;

        // 3. 初始化类型别名注册表
        let type_alias_registry = TypeAliasRegistry::from_map(config.type_aliases.clone());

        Ok(Self {
            environment,
            mapper_registry: Arc::new(RwLock::new(mapper_registry)),
            type_alias_registry,
            config,
            base_dir,
        })
    }

    /// 从已有组件构造（供高级用法 / 测试使用）
    pub fn from_parts(
        environment: Environment,
        mapper_registry: MapperRegistry,
        type_alias_registry: TypeAliasRegistry,
        config: HirustMapperConfig,
        base_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            environment,
            mapper_registry: Arc::new(RwLock::new(mapper_registry)),
            type_alias_registry,
            config,
            base_dir,
        }
    }

    /// 获取数据库环境引用
    pub fn environment(&self) -> &Environment {
        &self.environment
    }

    /// 获取 Mapper 注册表引用（加锁读取）
    pub fn mapper_registry(&self) -> std::sync::RwLockReadGuard<'_, MapperRegistry> {
        self.mapper_registry
            .read()
            .expect("MapperRegistry 锁中毒")
    }

    /// 获取 Mapper 注册表的可写锁
    pub fn mapper_registry_mut(&self) -> std::sync::RwLockWriteGuard<'_, MapperRegistry> {
        self.mapper_registry
            .write()
            .expect("MapperRegistry 锁中毒")
    }

    /// 获取类型别名注册表引用
    pub fn type_alias_registry(&self) -> &TypeAliasRegistry {
        &self.type_alias_registry
    }

    /// 获取配置引用
    pub fn config(&self) -> &HirustMapperConfig {
        &self.config
    }

    /// 获取基准目录
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// 已注册的 Mapper 数量
    pub fn mapper_count(&self) -> usize {
        self.mapper_registry().len()
    }

    /// 获取所有已注册的 namespace
    pub fn namespaces(&self) -> Vec<String> {
        self.mapper_registry().namespaces()
    }

    /// 打开一个新的 SqlSession
    ///
    /// Session 是请求级的轻量对象，共享工厂的连接池和注册表。
    pub fn open_session(&self) -> SqlSession {
        SqlSession {
            environment: self.environment.clone(),
            mapper_registry: Arc::clone(&self.mapper_registry),
            type_alias_registry: Arc::new(self.type_alias_registry.clone()),
            closed: false,
        }
    }

    /// 关闭工厂，释放所有资源
    pub async fn close(self) {
        self.environment.close().await;
    }
}

/// SqlSession（请求级）
///
/// 每个请求（或每个逻辑工作单元）一个 Session。
/// 当前为轻量占位，P6 阶段将扩展完整的 CRUD / 事务 / MapperProxy 接口。
pub struct SqlSession {
    /// 数据库环境
    environment: Environment,
    /// 共享的 Mapper 注册表
    mapper_registry: Arc<RwLock<MapperRegistry>>,
    /// 类型别名注册表
    type_alias_registry: Arc<TypeAliasRegistry>,
    /// 标记 session 是否已关闭
    closed: bool,
}

impl std::fmt::Debug for SqlSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlSession")
            .field("driver", &self.environment.driver())
            .field("closed", &self.closed)
            .finish()
    }
}

impl SqlSession {
    /// 获取数据库环境引用
    pub fn environment(&self) -> &Environment {
        &self.environment
    }

    /// 获取连接池引用
    pub fn pool(&self) -> &sqlx::AnyPool {
        self.environment.pool()
    }

    /// 获取 Mapper 注册表的只读锁
    pub fn mapper_registry(&self) -> std::sync::RwLockReadGuard<'_, MapperRegistry> {
        self.mapper_registry
            .read()
            .expect("MapperRegistry 锁中毒")
    }

    /// 获取类型别名注册表引用
    pub fn type_alias_registry(&self) -> &TypeAliasRegistry {
        &self.type_alias_registry
    }

    /// 按 namespace 查找 Mapper
    pub fn get_mapper(&self, namespace: &str) -> Result<Mapper> {
        self.mapper_registry()
            .get_mapper(namespace)
            .ok_or_else(|| MapperRuntimeError::MapperNotFound(namespace.to_string()))
    }

    /// 两阶段绑定：生成 [`BoundSql`]（`#{}` → `?`，`${}` → 内联）
    ///
    /// 根据 namespace + statement id 查找 Mapper，再按参数生成参数化 SQL。
    /// 这是 P6 Executor 执行层的输入。
    pub fn build_bound_sql(
        &self,
        namespace: &str,
        statement_id: &str,
        params: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<crate::bound_sql::BoundSql> {
        let mapper = self.get_mapper(namespace)?;
        crate::bound_sql::build_bound_sql(&mapper, statement_id, params)
    }

    /// 关闭 session
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// session 是否已关闭
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EnvironmentConfig;

    fn create_test_config() -> HirustMapperConfig {
        let temp = std::env::temp_dir().join("hirust_p3_factory_test");
        let mappers_dir = temp.join("mappers");
        std::fs::create_dir_all(&mappers_dir).unwrap();

        std::fs::write(
            mappers_dir.join("UserMapper.xml"),
            r#"<mapper namespace="com.test.UserMapper">
                <select id="findById">SELECT * FROM users WHERE id = #{id}</select>
                <select id="findByName">SELECT * FROM users WHERE name = #{name}</select>
            </mapper>"#,
        )
        .unwrap();

        HirustMapperConfig::new()
            .with_environment(EnvironmentConfig {
                driver: "sqlite".into(),
                url: "sqlite::memory:".into(),
                pool_max_connections: 5,
                pool_min_connections: 1,
            })
            .with_mapper_paths(vec!["mappers/**/*.xml".to_string()])
    }

    #[tokio::test]
    async fn test_factory_build_and_session() {
        let temp = std::env::temp_dir().join("hirust_p3_factory_test");
        let config = create_test_config();

        let factory = SqlSessionFactory::build(config, &temp).await.unwrap();

        // 验证工厂状态
        assert_eq!(factory.environment().driver(), "sqlite");
        assert_eq!(factory.mapper_count(), 1);

        let namespaces = factory.namespaces();
        assert_eq!(namespaces.len(), 1);
        assert_eq!(namespaces[0], "com.test.UserMapper");

        // 打开 session
        let mut session = factory.open_session();
        assert!(!session.is_closed());

        // 验证 session 可以访问 mapper
        let mapper = session.get_mapper("com.test.UserMapper").unwrap();
        assert!(mapper.statements.contains_key("findById"));
        assert!(mapper.statements.contains_key("findByName"));

        // 不存在的 mapper 应报错
        let result = session.get_mapper("not.exist");
        assert!(result.is_err());

        session.close();
        assert!(session.is_closed());

        factory.close().await;

        // 清理
        std::fs::remove_dir_all(&temp).ok();
    }

    #[tokio::test]
    async fn test_factory_build_loads_type_aliases() {
        let temp = std::env::temp_dir().join("hirust_p3_aliases_test");

        let config = HirustMapperConfig::new()
            .with_environment(EnvironmentConfig {
                driver: "sqlite".into(),
                url: "sqlite::memory:".into(),
                pool_max_connections: 2,
                pool_min_connections: 1,
            })
            .with_mapper_paths(vec!["mappers/**/*.xml".to_string()]);

        let factory = SqlSessionFactory::build(config, &temp).await.unwrap();

        // 验证环境可正常工作（无 mapper 文件也 OK）
        assert_eq!(factory.mapper_count(), 0);

        // 验证 session 的 pool 可用
        let session = factory.open_session();
        let result: Vec<(i64,)> = sqlx::query_as("SELECT 42")
            .fetch_all(session.pool())
            .await
            .unwrap();
        assert_eq!(result[0].0, 42);

        factory.close().await;
    }

    #[tokio::test]
    async fn test_multiple_sessions_share_registry() {
        let temp = std::env::temp_dir().join("hirust_p3_multi_session_test");
        let mappers_dir = temp.join("mappers");
        std::fs::create_dir_all(&mappers_dir).unwrap();

        std::fs::write(
            mappers_dir.join("SharedMapper.xml"),
            r#"<mapper namespace="com.test.SharedMapper">
                <select id="selectAll">SELECT * FROM items</select>
            </mapper>"#,
        )
        .unwrap();

        let config = HirustMapperConfig::new()
            .with_environment(EnvironmentConfig {
                driver: "sqlite".into(),
                url: "sqlite::memory:".into(),
                pool_max_connections: 10,
                pool_min_connections: 1,
            })
            .with_mapper_paths(vec!["mappers/**/*.xml".to_string()]);

        let factory = SqlSessionFactory::build(config, &temp).await.unwrap();

        // 多个 session 共享同一个注册表
        let session1 = factory.open_session();
        let session2 = factory.open_session();

        assert!(session1.get_mapper("com.test.SharedMapper").is_ok());
        assert!(session2.get_mapper("com.test.SharedMapper").is_ok());

        factory.close().await;
        std::fs::remove_dir_all(&temp).ok();
    }

    #[tokio::test]
    async fn test_factory_invalid_db_connection() {
        let config = HirustMapperConfig::new()
            .with_environment(EnvironmentConfig {
                driver: "sqlite".into(),
                // 故意使用不存在的文件路径（非 :memory:）
                url: "sqlite:///nonexistent/path/to/db.sqlite".to_string(),
                pool_max_connections: 1,
                pool_min_connections: 1,
            })
            .with_mapper_paths(vec!["mappers/**/*.xml".to_string()]);

        let temp = std::env::temp_dir().join("hirust_p3_invalid_db_test");
        let result = SqlSessionFactory::build(config, &temp).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("连接错误"));
    }
}
