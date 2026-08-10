//! 配置管理模块
//!
//! 解析 `hirust-mapper.toml` 配置文件，定义数据库连接、mapper 路径、类型别名等。
//! 同时提供基于 glob 的 XML mapper 文件发现能力。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use crate::error::{MapperRuntimeError, Result};

/// 根配置结构，对应 `hirust-mapper.toml`
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct HirustMapperConfig {
    /// 主数据库环境
    #[serde(default)]
    pub environment: EnvironmentConfig,
    /// 可选的命名环境（多数据库支持）
    #[serde(default)]
    pub environments: HashMap<String, EnvironmentConfig>,
    /// 全局设置
    #[serde(default)]
    pub settings: SettingsConfig,
    /// 类型别名：XML 短名 → Rust 全限定类型名
    #[serde(default)]
    pub type_aliases: HashMap<String, String>,
    /// 自定义类型处理器注册项
    #[serde(default)]
    pub type_handlers: Vec<TypeHandlerEntry>,
}

/// 数据库环境配置
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EnvironmentConfig {
    /// 驱动类型："mysql" | "postgres" | "sqlite"
    pub driver: String,
    /// 数据库连接 URL
    pub url: String,
    /// 连接池最大连接数
    #[serde(default = "default_pool_max")]
    pub pool_max_connections: u32,
    /// 连接池最小连接数
    #[serde(default = "default_pool_min")]
    pub pool_min_connections: u32,
}

impl Default for EnvironmentConfig {
    fn default() -> Self {
        Self {
            driver: String::new(),
            url: String::new(),
            pool_max_connections: default_pool_max(),
            pool_min_connections: default_pool_min(),
        }
    }
}

fn default_pool_max() -> u32 { 10 }
fn default_pool_min() -> u32 { 2 }

/// 全局设置
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SettingsConfig {
    /// XML mapper 文件的 glob 模式列表
    #[serde(default = "default_mapper_paths")]
    pub mapper_paths: Vec<String>,
    /// 热重载轮询间隔（毫秒），0 表示禁用
    #[serde(default)]
    pub mapper_refresh_interval_ms: u64,
}

fn default_mapper_paths() -> Vec<String> {
    vec!["mappers/**/*.xml".to_string()]
}

impl Default for SettingsConfig {
    fn default() -> Self {
        Self {
            mapper_paths: default_mapper_paths(),
            mapper_refresh_interval_ms: 0,
        }
    }
}

/// 自定义类型处理器注册项
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TypeHandlerEntry {
    /// Rust 类型全限定路径
    #[serde(rename = "type")]
    pub type_path: String,
    /// 处理器全限定路径
    #[serde(rename = "handler")]
    pub handler_path: String,
}

impl HirustMapperConfig {
    /// 创建空配置，使用默认值
    pub fn new() -> Self {
        Self::default()
    }

    /// 从 TOML 字符串解析配置
    pub fn parse_toml(toml_str: &str) -> Result<Self> {
        toml::from_str(toml_str)
            .map_err(|e| MapperRuntimeError::Config(format!("TOML 解析失败: {}", e)))
    }

    /// 从文件加载配置
    pub fn load_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| MapperRuntimeError::Config(
                format!("无法读取配置文件 {}: {}", path.as_ref().display(), e)
            ))?;
        Self::parse_toml(&content)
    }

    /// 设置主环境
    pub fn with_environment(mut self, env: EnvironmentConfig) -> Self {
        self.environment = env;
        self
    }

    /// 设置 mapper 路径
    pub fn with_mapper_paths(mut self, paths: Vec<String>) -> Self {
        self.settings.mapper_paths = paths;
        self
    }

    /// 启用热重载
    pub fn with_hot_reload(mut self, interval_ms: u64) -> Self {
        self.settings.mapper_refresh_interval_ms = interval_ms;
        self
    }

    /// 按 glob 模式发现所有 XML mapper 文件
    ///
    /// 返回 (文件路径, 文件内容) 的列表。相对路径基于 `base_dir` 解析。
    pub fn discover_mapper_files(&self, base_dir: &Path) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        for pattern in &self.settings.mapper_paths {
            // 处理绝对/相对 glob 模式
            let full_pattern = if Path::new(pattern).is_absolute() {
                pattern.clone()
            } else {
                base_dir.join(pattern).to_string_lossy().to_string()
            };

            for entry in glob::glob(&full_pattern)
                .map_err(|e| MapperRuntimeError::Config(format!("无效的 glob 模式 '{}': {}", pattern, e)))?
            {
                let path = entry.map_err(|e| MapperRuntimeError::Config(
                    format!("glob 遍历错误: {}", e)
                ))?;
                if path.is_file() {
                    files.push(path);
                }
            }
        }
        files.sort();
        files.dedup();
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_toml_basic() {
        let toml_str = r#"
[environment]
driver = "mysql"
url = "mysql://user:pass@localhost:3306/mydb"
pool_max_connections = 20

[settings]
mapper_paths = ["mappers/**/*.xml"]
mapper_refresh_interval_ms = 5000

[type_aliases]
"int" = "i32"
"long" = "i64"
"#;

        let config = HirustMapperConfig::parse_toml(toml_str).unwrap();
        assert_eq!(config.environment.driver, "mysql");
        assert_eq!(config.environment.url, "mysql://user:pass@localhost:3306/mydb");
        assert_eq!(config.environment.pool_max_connections, 20);
        assert_eq!(config.environment.pool_min_connections, 2); // 默认值
        assert_eq!(config.settings.mapper_paths, vec!["mappers/**/*.xml"]);
        assert_eq!(config.settings.mapper_refresh_interval_ms, 5000);
        assert_eq!(config.type_aliases.get("int"), Some(&"i32".to_string()));
    }

    #[test]
    fn test_parse_toml_minimal() {
        // 仅必需字段
        let toml_str = r#"
[environment]
driver = "sqlite"
url = "sqlite::memory:"
"#;
        let config = HirustMapperConfig::parse_toml(toml_str).unwrap();
        assert_eq!(config.environment.driver, "sqlite");
        assert_eq!(config.environment.pool_max_connections, 10); // 默认
        assert!(!config.settings.mapper_paths.is_empty()); // 默认
    }

    #[test]
    fn test_parse_toml_invalid() {
        let toml_str = "this is not valid toml {{{";
        let result = HirustMapperConfig::parse_toml(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_type_handlers_deserialize() {
        let toml_str = r#"
[environment]
driver = "mysql"
url = "mysql://localhost/db"

[[type_handlers]]
type = "myapp::MyEnum"
handler = "myapp::MyEnumHandler"

[[type_handlers]]
type = "myapp::Money"
handler = "myapp::MoneyHandler"
"#;
        let config = HirustMapperConfig::parse_toml(toml_str).unwrap();
        assert_eq!(config.type_handlers.len(), 2);
        assert_eq!(config.type_handlers[0].type_path, "myapp::MyEnum");
        assert_eq!(config.type_handlers[1].handler_path, "myapp::MoneyHandler");
    }

    #[test]
    fn test_discover_mapper_files() {
        // 创建临时目录结构测试 glob 发现
        let temp = std::env::temp_dir().join("hirust_test_glob");
        let mappers_dir = temp.join("mappers");
        std::fs::create_dir_all(&mappers_dir).unwrap();
        std::fs::create_dir_all(mappers_dir.join("sub")).unwrap();

        std::fs::write(mappers_dir.join("a.xml"), "<mapper/>").unwrap();
        std::fs::write(mappers_dir.join("b.xml"), "<mapper/>").unwrap();
        std::fs::write(mappers_dir.join("sub").join("c.xml"), "<mapper/>").unwrap();
        std::fs::write(mappers_dir.join("d.txt"), "not xml").unwrap();

        let config = HirustMapperConfig::new()
            .with_mapper_paths(vec!["mappers/**/*.xml".to_string()]);

        let files = config.discover_mapper_files(&temp).unwrap();
        let names: Vec<String> = files.iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert!(names.contains(&"a.xml".to_string()));
        assert!(names.contains(&"b.xml".to_string()));
        assert!(names.contains(&"c.xml".to_string()));
        assert!(!names.contains(&"d.txt".to_string()));

        // 清理
        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn test_builder_pattern() {
        let config = HirustMapperConfig::new()
            .with_environment(EnvironmentConfig {
                driver: "sqlite".into(),
                url: "sqlite::memory:".into(),
                pool_max_connections: 5,
                pool_min_connections: 1,
            })
            .with_mapper_paths(vec!["sql/**/*.xml".into()])
            .with_hot_reload(1000);

        assert_eq!(config.environment.driver, "sqlite");
        assert_eq!(config.settings.mapper_paths, vec!["sql/**/*.xml"]);
        assert_eq!(config.settings.mapper_refresh_interval_ms, 1000);
    }
}
