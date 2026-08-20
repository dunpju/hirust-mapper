//! SqlSession 工厂模块
//!
//! `SqlSessionFactory` 是应用级的长生命周期对象（对应 MyBatis 的 `SqlSessionFactory`），
//! 持有 Mapper 注册表、数据库环境、类型别名与类型处理器注册表。
//! 通过 `build()` 从配置构建，通过 `open_session()` 创建请求级的 [`SqlSession`]。

use std::path::Path;
use std::sync::Arc;

use crate::config::HirustMapperConfig;
use crate::environment::Environment;
use crate::error::Result;
use crate::event::EventBus;
use crate::hot_reload::{extract_watch_dirs, MapperWatcher};
use crate::registry::{MapperRegistry, TypeAliasRegistry};
use crate::sql_log::SqlLogConfig;
use crate::type_handler::TypeHandlerRegistry;

pub use crate::session::{MapperProxy, SqlSession};

/// SqlSession 工厂（应用级，线程安全）
///
/// 生命周期与整个应用相同。持有一个连接池、一个线程安全的 Mapper 注册表，
/// 以及类型别名 / 类型处理器注册表。每个请求通过 `open_session()` 获取独立的 `SqlSession`。
///
/// # 示例
///
/// ```ignore
/// let config = HirustMapperConfig::load_file("hirust-mapper.toml")?;
/// let factory = SqlSessionFactory::build(config, ".").await?;
/// let mut session = factory.open_session();
/// ```
pub struct SqlSessionFactory {
    environment: Environment,
    /// Mapper 注册表（内部自带 RwLock，无需外层再包一层锁）
    mapper_registry: Arc<MapperRegistry>,
    type_alias_registry: Arc<TypeAliasRegistry>,
    type_handler_registry: Arc<TypeHandlerRegistry>,
    sql_log: Arc<SqlLogConfig>,
    event_bus: Arc<EventBus>,
    config: HirustMapperConfig,
    base_dir: std::path::PathBuf,
    /// 热重载监视器（None 表示未启用热重载）
    watcher: Option<MapperWatcher>,
}

impl std::fmt::Debug for SqlSessionFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlSessionFactory")
            .field("driver", &self.environment.driver())
            .field("url", &self.environment.url())
            .field("mapper_count", &self.mapper_count())
            .field("hot_reload", &self.watcher.as_ref().map(|w| w.is_running()).unwrap_or(false))
            .finish()
    }
}

impl SqlSessionFactory {
    /// 从配置构建 SqlSessionFactory
    ///
    /// 完整流程：
    /// 1. 创建数据库连接池（`Environment`）
    /// 2. 初始化 Mapper 注册表并加载所有 XML mapper 文件
    /// 3. 初始化类型别名 / 类型处理器注册表
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
        let mapper_registry = Arc::new(mapper_registry);

        // 3. 初始化类型别名 / 类型处理器注册表
        let type_alias_registry = Arc::new(TypeAliasRegistry::from_map(config.type_aliases.clone()));
        let type_handler_registry = Arc::new(TypeHandlerRegistry::with_defaults());

        // SQL 执行日志配置（从 settings 解析，默认关闭）
        let sql_log = Arc::new(SqlLogConfig {
            enabled: config.settings.sql_log,
            slow_threshold_ms: config.settings.sql_log_slow_threshold_ms,
        });

        // 事件总线（SQL 执行前/后生命周期事件；无监听器时派发零开销）
        let event_bus = Arc::new(EventBus::new());

        // 4. 热重载（当 refresh_interval > 0 时启动）
        let watcher = if config.settings.mapper_refresh_interval_ms > 0 {
            let watch_dirs = extract_watch_dirs(&config.settings.mapper_paths, &base_dir);
            // 克隆 MapperRegistry（内部共享同一 Arc<RwLock<..>>，热重载替换对工厂可见）
            let registry_clone: MapperRegistry = (*mapper_registry).clone();
            match MapperWatcher::start(
                registry_clone,
                watch_dirs,
                config.settings.mapper_refresh_interval_ms,
            ) {
                Ok(w) => {
                    eprintln!(
                        "[hirust-mapper] 热重载已启用: 间隔 {}ms",
                        config.settings.mapper_refresh_interval_ms
                    );
                    Some(w)
                }
                Err(e) => {
                    // 热重载失败不阻断工厂构建（ORM 仍可用，仅失去热重载能力）
                    eprintln!("[hirust-mapper][WARN] 热重载启动失败（已禁用）: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            environment,
            mapper_registry,
            type_alias_registry,
            type_handler_registry,
            sql_log,
            event_bus,
            config,
            base_dir,
            watcher,
        })
    }

    /// 从已有组件构造（供高级用法 / 测试使用；不启动热重载）
    pub fn from_parts(
        environment: Environment,
        mapper_registry: MapperRegistry,
        type_alias_registry: TypeAliasRegistry,
        type_handler_registry: TypeHandlerRegistry,
        config: HirustMapperConfig,
        base_dir: std::path::PathBuf,
    ) -> Self {
        let sql_log = Arc::new(SqlLogConfig {
            enabled: config.settings.sql_log,
            slow_threshold_ms: config.settings.sql_log_slow_threshold_ms,
        });
        Self {
            environment,
            mapper_registry: Arc::new(mapper_registry),
            type_alias_registry: Arc::new(type_alias_registry),
            type_handler_registry: Arc::new(type_handler_registry),
            sql_log,
            event_bus: Arc::new(EventBus::new()),
            config,
            base_dir,
            watcher: None,
        }
    }

    /// 事件总线引用（注册 SQL 执行前/后事件监听器，所有 session 共享）
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// 数据库环境引用
    pub fn environment(&self) -> &Environment {
        &self.environment
    }

    /// Mapper 注册表引用（内部自带线程安全，写操作如 insert_mapper 也经 &self）
    pub fn mapper_registry(&self) -> &MapperRegistry {
        &self.mapper_registry
    }

    /// 类型别名注册表引用
    pub fn type_alias_registry(&self) -> &TypeAliasRegistry {
        &self.type_alias_registry
    }

    /// 类型处理器注册表引用
    pub fn type_handler_registry(&self) -> &TypeHandlerRegistry {
        &self.type_handler_registry
    }

    /// 配置引用
    pub fn config(&self) -> &HirustMapperConfig {
        &self.config
    }

    /// 基准目录
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// 已注册的 Mapper 数量
    pub fn mapper_count(&self) -> usize {
        self.mapper_registry().len()
    }

    /// 所有已注册的 namespace
    pub fn namespaces(&self) -> Vec<String> {
        self.mapper_registry().namespaces()
    }

    /// 热重载是否启用
    pub fn hot_reload_enabled(&self) -> bool {
        self.watcher.as_ref().map(|w| w.is_running()).unwrap_or(false)
    }

    /// 打开一个新的 SqlSession（请求级，共享工厂的连接池和注册表）
    pub fn open_session(&self) -> SqlSession {
        SqlSession::new(
            self.environment.clone(),
            Arc::clone(&self.mapper_registry),
            Arc::clone(&self.type_alias_registry),
            Arc::clone(&self.type_handler_registry),
            Arc::clone(&self.sql_log),
            Arc::clone(&self.event_bus),
        )
    }

    /// 关闭工厂，释放连接池资源
    pub async fn close(self) {
        self.environment.close().await;
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

        assert_eq!(factory.environment().driver(), "sqlite");
        assert_eq!(factory.mapper_count(), 1);

        let namespaces = factory.namespaces();
        assert_eq!(namespaces.len(), 1);
        assert_eq!(namespaces[0], "com.test.UserMapper");

        let mut session = factory.open_session();
        assert!(!session.is_closed());

        let mapper = session.get_mapper("com.test.UserMapper").unwrap();
        assert!(mapper.statements.contains_key("findById"));
        assert!(mapper.statements.contains_key("findByName"));

        assert!(session.get_mapper("not.exist").is_err());

        session.close().await.unwrap();
        assert!(session.is_closed());

        factory.close().await;
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

        assert_eq!(factory.mapper_count(), 0);

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
