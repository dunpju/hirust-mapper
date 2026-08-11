//! 数据库环境模块
//!
//! `Environment` 封装 `sqlx::Pool`，提供基于配置的连接池创建与生命周期管理。
//! 支持通过 `EnvironmentConfig` 构造，按 driver 字段自动选择数据库后端。

use crate::config::EnvironmentConfig;
use crate::error::{MapperRuntimeError, Result};
use sqlx::AnyPool;
use std::collections::HashMap;

/// 数据库环境
///
/// 持有一个 sqlx 连接池，代表一个已配置的数据库环境。
/// 通常由 `SqlSessionFactory::build()` 创建，生命周期与应用相同。
#[derive(Debug, Clone)]
pub struct Environment {
    /// 连接池
    pool: AnyPool,
    /// 驱动标识（"mysql" | "postgres" | "sqlite"）
    driver: String,
    /// 连接 URL
    url: String,
}

impl Environment {
    /// 从配置创建数据库环境并建立连接池
    ///
    /// 根据 `EnvironmentConfig.driver` 自动选择数据库后端。
    pub async fn from_config(config: &EnvironmentConfig) -> Result<Self> {
        let driver = config.driver.trim().to_lowercase();
        let url = config.url.as_str();

        // 注册已编译的数据库驱动（sqlite/mysql/postgres）
        sqlx::any::install_default_drivers();

        // 校验 driver 合法性
        if !matches!(driver.as_str(), "mysql" | "postgres" | "sqlite") {
            return Err(MapperRuntimeError::Config(format!(
                "不支持的数据库驱动: '{}'，支持的选项: mysql, postgres, sqlite",
                driver
            )));
        }

        if url.is_empty() {
            return Err(MapperRuntimeError::Config(
                "数据库连接 URL 不能为空".to_string(),
            ));
        }

        // 构建 sqlx 连接池选项
        let mut pool_opts = sqlx::any::AnyPoolOptions::new()
            .max_connections(config.pool_max_connections)
            .min_connections(config.pool_min_connections);

        // SQLite 内存库需要特殊处理：启用共享缓存以允许同一进程内多连接
        if driver == "sqlite" && url.contains(":memory:") {
            pool_opts = pool_opts.after_connect(|conn, _| {
                Box::pin(async move {
                    // 对内存 SQLite 启用 WAL 模式以支持并发读写
                    sqlx::query("PRAGMA journal_mode=WAL")
                        .execute(&mut *conn)
                        .await?;
                    Ok(())
                })
            });
        }

        let pool = pool_opts.connect(url).await.map_err(|e| {
            MapperRuntimeError::Connection(format!(
                "无法连接到 {} 数据库 '{}': {}",
                driver, url, e
            ))
        })?;

        Ok(Self {
            pool,
            driver,
            url: url.to_string(),
        })
    }

    /// 获取连接池引用
    pub fn pool(&self) -> &AnyPool {
        &self.pool
    }

    /// 获取驱动标识
    pub fn driver(&self) -> &str {
        &self.driver
    }

    /// 获取连接 URL
    pub fn url(&self) -> &str {
        &self.url
    }

    /// 关闭连接池（等待所有活跃连接归还后关闭）
    pub async fn close(self) {
        self.pool.close().await;
    }
}

/// 支持多个命名环境的容器
///
/// 用于多数据库场景，通过名称索引不同的数据库连接池。
#[derive(Debug, Default)]
pub struct EnvironmentRegistry {
    environments: HashMap<String, Environment>,
}

impl EnvironmentRegistry {
    /// 创建空注册表
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个命名环境
    pub fn insert(&mut self, name: String, env: Environment) {
        self.environments.insert(name, env);
    }

    /// 获取指定名称的环境
    pub fn get(&self, name: &str) -> Option<&Environment> {
        self.environments.get(name)
    }

    /// 获取所有已注册的环境名
    pub fn names(&self) -> Vec<&str> {
        self.environments.keys().map(|s| s.as_str()).collect()
    }

    /// 关闭所有连接池
    pub async fn close_all(self) {
        for (_name, env) in self.environments {
            env.close().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EnvironmentConfig;

    #[tokio::test]
    async fn test_environment_from_sqlite_memory() {
        let config = EnvironmentConfig {
            driver: "sqlite".into(),
            url: "sqlite::memory:".into(),
            pool_max_connections: 5,
            pool_min_connections: 1,
        };

        let env = Environment::from_config(&config).await.unwrap();
        assert_eq!(env.driver(), "sqlite");
        assert!(env.pool().size() >= 1);

        // 验证连接池可用
        let result: Vec<(i64,)> = sqlx::query_as("SELECT 1")
            .fetch_all(env.pool())
            .await
            .unwrap();
        assert_eq!(result[0].0, 1);

        env.close().await;
    }

    #[test]
    fn test_environment_invalid_driver() {
        let config = EnvironmentConfig {
            driver: "oracle".into(),
            url: "oracle://localhost/db".into(),
            pool_max_connections: 5,
            pool_min_connections: 1,
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(Environment::from_config(&config));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("不支持的数据库驱动"));
        assert!(err.contains("oracle"));
    }

    #[test]
    fn test_environment_empty_url() {
        let config = EnvironmentConfig {
            driver: "sqlite".into(),
            url: String::new(),
            pool_max_connections: 5,
            pool_min_connections: 1,
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(Environment::from_config(&config));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("连接 URL 不能为空"));
    }

    #[tokio::test]
    async fn test_environment_registry() {
        let config1 = EnvironmentConfig {
            driver: "sqlite".into(),
            url: "sqlite::memory:".into(),
            pool_max_connections: 2,
            pool_min_connections: 1,
        };
        let config2 = EnvironmentConfig {
            driver: "sqlite".into(),
            url: "sqlite::memory:".into(),
            pool_max_connections: 2,
            pool_min_connections: 1,
        };

        let env1 = Environment::from_config(&config1).await.unwrap();
        let env2 = Environment::from_config(&config2).await.unwrap();

        let mut registry = EnvironmentRegistry::new();
        registry.insert("primary".into(), env1);
        registry.insert("secondary".into(), env2);

        assert!(registry.get("primary").is_some());
        assert!(registry.get("secondary").is_some());
        assert!(registry.get("unknown").is_none());

        let names = registry.names();
        assert!(names.contains(&"primary"));
        assert!(names.contains(&"secondary"));

        registry.close_all().await;
    }
}
