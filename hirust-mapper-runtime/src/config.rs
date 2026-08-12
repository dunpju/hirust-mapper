//! 配置管理模块
//!
//! 解析 `hirust-mapper.toml` 配置文件，定义数据库连接、mapper 路径、类型别名等。
//! 同时提供基于 glob 的 XML mapper 文件发现能力。
//!
//! # 配置优先级（高 → 低）
//!
//! 1. **环境变量**（`HIRUST_MAPPER_*`，部署/CI 覆盖用）
//! 2. **编程式 builder**（`with_*` 方法）
//! 3. **TOML 文件默认值**（`load_file`）
//!
//! env 层只在变量**存在时**覆盖对应字段（缺失则保留编程/TOML 值）。通过应用顺序编码优先级——
//! 把 `with_env_overrides()` 放在链式调用最后：
//!
//! ```no_run
//! # use hirust_mapper_runtime::HirustMapperConfig;
//! let config = HirustMapperConfig::load_file("hirust-mapper.toml")?  // 3. TOML
//!     .with_url("programmatic-url")                                  // 2. 编程（非 Result）
//!     .with_env_overrides()?;                                        // 1. env（最高）
//! # Ok::<(), hirust_mapper_runtime::MapperRuntimeError>(())
//! ```
//!
//! ## 支持的环境变量
//!
//! | 变量 | 覆盖字段 | 解析 |
//! |------|----------|------|
//! | `HIRUST_MAPPER_DRIVER` | `environment.driver` | 字符串 |
//! | `HIRUST_MAPPER_URL`（或 `DATABASE_URL`） | `environment.url` | 字符串（前者优先） |
//! | `HIRUST_MAPPER_POOL_MAX` | `environment.pool_max_connections` | u32 |
//! | `HIRUST_MAPPER_POOL_MIN` | `environment.pool_min_connections` | u32 |
//! | `HIRUST_MAPPER_PATHS` | `settings.mapper_paths` | 逗号分隔列表 |
//! | `HIRUST_MAPPER_REFRESH_MS` | `settings.mapper_refresh_interval_ms` | u64 |
//! | `HIRUST_MAPPER_SQL_LOG` | `settings.sql_log` | 布尔（true/1/yes/false/0/no） |
//! | `HIRUST_MAPPER_SQL_LOG_SLOW_MS` | `settings.sql_log_slow_threshold_ms` | u64 |
//! | `HIRUST_MAPPER_TYPE_ALIASES` | `type_aliases` | 逗号分隔 `name=type`（合并） |

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
    /// 是否开启 SQL 执行日志（耗时 + 可读 SQL，经 `log` facade 输出，默认关闭）。
    /// 需应用初始化日志后端（如 env_logger / tracing_subscriber）方能见到输出。
    #[serde(default)]
    pub sql_log: bool,
    /// 慢查询阈值（毫秒）；仅记录耗时 ≥ 此值的 SQL。`0` 表示记录全部执行的 SQL。
    #[serde(default)]
    pub sql_log_slow_threshold_ms: u64,
}

fn default_mapper_paths() -> Vec<String> {
    vec!["mappers/**/*.xml".to_string()]
}

impl Default for SettingsConfig {
    fn default() -> Self {
        Self {
            mapper_paths: default_mapper_paths(),
            mapper_refresh_interval_ms: 0,
            sql_log: false,
            sql_log_slow_threshold_ms: 0,
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

    /// 单独覆盖 driver（不影响 url / 连接池等其他环境字段）
    pub fn with_driver(mut self, driver: impl Into<String>) -> Self {
        self.environment.driver = driver.into();
        self
    }

    /// 单独覆盖数据库连接 URL
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.environment.url = url.into();
        self
    }

    /// 单独覆盖连接池最大连接数
    pub fn with_pool_max_connections(mut self, max: u32) -> Self {
        self.environment.pool_max_connections = max;
        self
    }

    /// 单独覆盖连接池最小连接数
    pub fn with_pool_min_connections(mut self, min: u32) -> Self {
        self.environment.pool_min_connections = min;
        self
    }

    /// 注册单个类型别名（合并入现有 map）
    pub fn with_type_alias(mut self, alias: impl Into<String>, full_path: impl Into<String>) -> Self {
        self.type_aliases.insert(alias.into(), full_path.into());
        self
    }

    /// 整体替换类型别名表
    pub fn with_type_aliases(mut self, aliases: HashMap<String, String>) -> Self {
        self.type_aliases = aliases;
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

    /// 开启 SQL 执行日志（耗时 + 可读 SQL）。
    ///
    /// 等价于 toml `[settings] sql_log = true`。阈值可用
    /// [`with_sql_log_slow_threshold_ms`](Self::with_sql_log_slow_threshold_ms) 设置。
    pub fn with_sql_log(mut self, enabled: bool) -> Self {
        self.settings.sql_log = enabled;
        self
    }

    /// 设置慢查询日志阈值（毫秒）：仅记录耗时 ≥ 此值的 SQL。`0` 表示记录全部。
    pub fn with_sql_log_slow_threshold_ms(mut self, threshold_ms: u64) -> Self {
        self.settings.sql_log_slow_threshold_ms = threshold_ms;
        self
    }

    // ─── 环境变量覆盖层 ─────────────────────────────────────────────

    /// 从指定 env 源应用环境变量覆盖（仅覆盖已设置的变量）。
    ///
    /// 优先级最高：在 TOML / 编程设置之后调用。变量缺失则保留原值；解析失败返回
    /// `Config` 错误（不静默吞错）。生产用 [`StdEnv`](crate::config::StdEnv)。
    pub fn apply_env_overrides_from(&mut self, src: &dyn EnvSource) -> Result<()> {
        if let Some(v) = src.get(ENV_DRIVER) {
            self.environment.driver = v;
        }
        // HIRUST_MAPPER_URL 优先于 DATABASE_URL 别名
        if let Some(v) = src.get(ENV_URL).or_else(|| src.get(ENV_DATABASE_URL)) {
            self.environment.url = v;
        }
        if let Some(v) = src.get(ENV_POOL_MAX) {
            self.environment.pool_max_connections =
                parse_u32(&v, ENV_POOL_MAX)?;
        }
        if let Some(v) = src.get(ENV_POOL_MIN) {
            self.environment.pool_min_connections =
                parse_u32(&v, ENV_POOL_MIN)?;
        }
        if let Some(v) = src.get(ENV_PATHS) {
            self.settings.mapper_paths = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Some(v) = src.get(ENV_REFRESH_MS) {
            self.settings.mapper_refresh_interval_ms = parse_u64(&v, ENV_REFRESH_MS)?;
        }
        if let Some(v) = src.get(ENV_SQL_LOG) {
            self.settings.sql_log = parse_bool(&v, ENV_SQL_LOG)?;
        }
        if let Some(v) = src.get(ENV_SQL_LOG_SLOW_MS) {
            self.settings.sql_log_slow_threshold_ms = parse_u64(&v, ENV_SQL_LOG_SLOW_MS)?;
        }
        if let Some(v) = src.get(ENV_TYPE_ALIASES) {
            for (k, t) in parse_aliases(&v)? {
                self.type_aliases.insert(k, t);
            }
        }
        Ok(())
    }

    /// 从进程环境变量（`std::env`）应用覆盖。
    pub fn apply_env_overrides(&mut self) -> Result<()> {
        self.apply_env_overrides_from(&StdEnv)
    }

    /// 消费式：应用 env 覆盖后返回 self（链式，来自指定源）。
    pub fn with_env_overrides_from(mut self, src: &dyn EnvSource) -> Result<Self> {
        self.apply_env_overrides_from(src)?;
        Ok(self)
    }

    /// 消费式：应用进程环境变量覆盖后返回 self（链式）。
    pub fn with_env_overrides(mut self) -> Result<Self> {
        self.apply_env_overrides()?;
        Ok(self)
    }

    /// 一站式分层加载：TOML 文件 → 进程环境变量覆盖。
    pub fn load_layered<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::load_file(path)?.with_env_overrides()
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

// ─── 环境变量层：源抽象 + 解析辅助 ──────────────────────────────────

/// 环境变量来源抽象。
///
/// 生产用 [`StdEnv`]（读 `std::env`）；测试可传入自定义实现以避免并行测试竞态。
pub trait EnvSource {
    /// 读取指定变量；不存在返回 `None`。
    fn get(&self, key: &str) -> Option<String>;
}

/// 进程环境变量源（生产用）。
#[derive(Debug, Clone, Copy, Default)]
pub struct StdEnv;

impl EnvSource for StdEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

// 环境变量名常量（集中定义，便于维护与文档）
const ENV_DRIVER: &str = "HIRUST_MAPPER_DRIVER";
const ENV_URL: &str = "HIRUST_MAPPER_URL";
const ENV_DATABASE_URL: &str = "DATABASE_URL";
const ENV_POOL_MAX: &str = "HIRUST_MAPPER_POOL_MAX";
const ENV_POOL_MIN: &str = "HIRUST_MAPPER_POOL_MIN";
const ENV_PATHS: &str = "HIRUST_MAPPER_PATHS";
const ENV_REFRESH_MS: &str = "HIRUST_MAPPER_REFRESH_MS";
const ENV_SQL_LOG: &str = "HIRUST_MAPPER_SQL_LOG";
const ENV_SQL_LOG_SLOW_MS: &str = "HIRUST_MAPPER_SQL_LOG_SLOW_MS";
const ENV_TYPE_ALIASES: &str = "HIRUST_MAPPER_TYPE_ALIASES";

fn config_err(msg: impl Into<String>) -> MapperRuntimeError {
    MapperRuntimeError::Config(msg.into())
}

fn parse_u32(v: &str, var: &str) -> Result<u32> {
    v.trim().parse::<u32>().map_err(|_| {
        config_err(format!("环境变量 {} 期望 u32，实际 '{}'", var, v))
    })
}

fn parse_u64(v: &str, var: &str) -> Result<u64> {
    v.trim().parse::<u64>().map_err(|_| {
        config_err(format!("环境变量 {} 期望 u64，实际 '{}'", var, v))
    })
}

/// 解析布尔：true/1/yes/on → true；false/0/no/off → false（大小写不敏感）
fn parse_bool(v: &str, var: &str) -> Result<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(config_err(format!(
            "环境变量 {} 期望布尔（true/false/1/0/yes/no），实际 '{}'",
            var, v
        ))),
    }
}

/// 解析类型别名：`int=i32,long=i64` → [(int, i32), (long, i64)]
fn parse_aliases(v: &str) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for pair in v.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let (k, t) = pair.split_once('=').ok_or_else(|| {
            config_err(format!(
                "环境变量 {} 期望 'name=type' 形式（逗号分隔），无效项 '{}'",
                ENV_TYPE_ALIASES, pair
            ))
        })?;
        out.push((k.trim().to_string(), t.trim().to_string()));
    }
    Ok(out)
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

    #[test]
    fn test_parse_sql_log_settings() {
        let toml_str = r#"
[environment]
driver = "sqlite"
url = "sqlite::memory:"

[settings]
sql_log = true
sql_log_slow_threshold_ms = 100
"#;
        let config = HirustMapperConfig::parse_toml(toml_str).unwrap();
        assert!(config.settings.sql_log);
        assert_eq!(config.settings.sql_log_slow_threshold_ms, 100);

        // 默认（缺省字段）应为关闭、阈值 0
        let minimal = HirustMapperConfig::parse_toml(
            r#"[environment]
driver = "sqlite"
url = "sqlite::memory:""#,
        )
        .unwrap();
        assert!(!minimal.settings.sql_log);
        assert_eq!(minimal.settings.sql_log_slow_threshold_ms, 0);
    }

    #[test]
    fn test_sql_log_builder() {
        let config = HirustMapperConfig::new()
            .with_environment(EnvironmentConfig {
                driver: "sqlite".into(),
                url: "sqlite::memory:".into(),
                pool_max_connections: 5,
                pool_min_connections: 1,
            })
            .with_sql_log(true)
            .with_sql_log_slow_threshold_ms(50);
        assert!(config.settings.sql_log);
        assert_eq!(config.settings.sql_log_slow_threshold_ms, 50);
    }

    // ─── 粒度 setter 测试 ──────────────────────────────────────────

    #[test]
    fn test_granular_environment_setters() {
        let config = HirustMapperConfig::new()
            .with_driver("postgres")
            .with_url("postgres://localhost/db")
            .with_pool_max_connections(42)
            .with_pool_min_connections(7);
        assert_eq!(config.environment.driver, "postgres");
        assert_eq!(config.environment.url, "postgres://localhost/db");
        assert_eq!(config.environment.pool_max_connections, 42);
        assert_eq!(config.environment.pool_min_connections, 7);
    }

    #[test]
    fn test_granular_setter_does_not_reset_others() {
        // with_driver 只改 driver，保留其他字段
        let config = HirustMapperConfig::new()
            .with_environment(EnvironmentConfig {
                driver: "mysql".into(),
                url: "mysql://x".into(),
                pool_max_connections: 30,
                pool_min_connections: 5,
            })
            .with_driver("postgres");
        assert_eq!(config.environment.driver, "postgres");
        assert_eq!(config.environment.url, "mysql://x"); // 未被重置
        assert_eq!(config.environment.pool_max_connections, 30);
    }

    #[test]
    fn test_type_alias_setters() {
        let config = HirustMapperConfig::new()
            .with_type_alias("int", "i32")
            .with_type_alias("long", "i64");
        assert_eq!(config.type_aliases.get("int"), Some(&"i32".to_string()));
        assert_eq!(config.type_aliases.get("long"), Some(&"i64".to_string()));

        // 整体替换
        let mut map = HashMap::new();
        map.insert("dec".to_string(), "f64".to_string());
        let config = config.with_type_aliases(map);
        assert_eq!(config.type_aliases.len(), 1);
        assert!(config.type_aliases.get("int").is_none());
        assert_eq!(config.type_aliases.get("dec"), Some(&"f64".to_string()));
    }

    // ─── env 覆盖测试（用 TestEnv，避免 std::env 并行竞态）─────────

    /// 测试用 env 源：基于 HashMap
    #[derive(Default)]
    struct TestEnv(HashMap<&'static str, String>);
    impl TestEnv {
        fn set(mut self, k: &'static str, v: impl Into<String>) -> Self {
            self.0.insert(k, v.into());
            self
        }
    }
    impl EnvSource for TestEnv {
        fn get(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
    }

    #[test]
    fn test_env_override_all_fields() {
        let env = TestEnv::default()
            .set(ENV_DRIVER, "postgres")
            .set(ENV_URL, "postgres://h/db")
            .set(ENV_POOL_MAX, "88")
            .set(ENV_POOL_MIN, "8")
            .set(ENV_PATHS, "a/*.xml, b/**/*.xml")
            .set(ENV_REFRESH_MS, "1234")
            .set(ENV_SQL_LOG, "true")
            .set(ENV_SQL_LOG_SLOW_MS, "200")
            .set(ENV_TYPE_ALIASES, "int=i32, long=i64");

        let mut config = HirustMapperConfig::new();
        config.apply_env_overrides_from(&env).unwrap();

        assert_eq!(config.environment.driver, "postgres");
        assert_eq!(config.environment.url, "postgres://h/db");
        assert_eq!(config.environment.pool_max_connections, 88);
        assert_eq!(config.environment.pool_min_connections, 8);
        assert_eq!(config.settings.mapper_paths, vec!["a/*.xml", "b/**/*.xml"]);
        assert_eq!(config.settings.mapper_refresh_interval_ms, 1234);
        assert!(config.settings.sql_log);
        assert_eq!(config.settings.sql_log_slow_threshold_ms, 200);
        assert_eq!(config.type_aliases.get("int"), Some(&"i32".to_string()));
        assert_eq!(config.type_aliases.get("long"), Some(&"i64".to_string()));
    }

    #[test]
    fn test_env_override_priority_env_wins() {
        // 编程/TOML 设的值被 env 覆盖
        let env = TestEnv::default().set(ENV_URL, "env-url");
        let config = HirustMapperConfig::new()
            .with_url("programmatic-url")
            .with_env_overrides_from(&env)
            .unwrap();
        assert_eq!(config.environment.url, "env-url"); // env 胜
    }

    #[test]
    fn test_env_override_absent_keeps_programmatic() {
        // env 未设 → 保留编程/TOML 值
        let env = TestEnv::default(); // 空
        let config = HirustMapperConfig::new()
            .with_driver("mysql")
            .with_url("kept-url")
            .with_pool_max_connections(15)
            .with_env_overrides_from(&env)
            .unwrap();
        assert_eq!(config.environment.driver, "mysql");
        assert_eq!(config.environment.url, "kept-url");
        assert_eq!(config.environment.pool_max_connections, 15);
    }

    #[test]
    fn test_database_url_alias() {
        // HIRUST_MAPPER_URL 优先于 DATABASE_URL
        let env = TestEnv::default()
            .set(ENV_DATABASE_URL, "db-url-alias")
            .set(ENV_URL, "explicit-url");
        let mut config = HirustMapperConfig::new();
        config.apply_env_overrides_from(&env).unwrap();
        assert_eq!(config.environment.url, "explicit-url");

        // 仅 DATABASE_URL 时生效
        let env = TestEnv::default().set(ENV_DATABASE_URL, "db-url-alias");
        let mut config = HirustMapperConfig::new().with_url("orig");
        config.apply_env_overrides_from(&env).unwrap();
        assert_eq!(config.environment.url, "db-url-alias");
    }

    #[test]
    fn test_env_bool_variants() {
        for (raw, expected) in [
            ("true", true), ("1", true), ("YES", true), ("on", true),
            ("false", false), ("0", false), ("No", false), ("off", false),
        ] {
            let env = TestEnv::default().set(ENV_SQL_LOG, raw);
            let mut config = HirustMapperConfig::new();
            config.apply_env_overrides_from(&env).unwrap();
            assert_eq!(config.settings.sql_log, expected, "raw={}", raw);
        }
    }

    #[test]
    fn test_env_invalid_bool_errors() {
        let env = TestEnv::default().set(ENV_SQL_LOG, "maybe");
        let mut config = HirustMapperConfig::new();
        let err = config.apply_env_overrides_from(&env).unwrap_err();
        assert!(err.to_string().contains("HIRUST_MAPPER_SQL_LOG"));
    }

    #[test]
    fn test_env_invalid_number_errors() {
        let env = TestEnv::default().set(ENV_POOL_MAX, "not-a-number");
        let mut config = HirustMapperConfig::new();
        let err = config.apply_env_overrides_from(&env).unwrap_err();
        assert!(err.to_string().contains("HIRUST_MAPPER_POOL_MAX"));
    }

    #[test]
    fn test_env_type_aliases_merges() {
        // env 别名合并入已存在的 map
        let env = TestEnv::default().set(ENV_TYPE_ALIASES, "new=NEW");
        let config = HirustMapperConfig::new()
            .with_type_alias("old", "OLD")
            .with_env_overrides_from(&env)
            .unwrap();
        assert_eq!(config.type_aliases.get("old"), Some(&"OLD".to_string()));
        assert_eq!(config.type_aliases.get("new"), Some(&"NEW".to_string()));
    }

    #[test]
    fn test_env_paths_trims_and_filters_empty() {
        let env = TestEnv::default().set(ENV_PATHS, " a.xml , , b.xml ,");
        let mut config = HirustMapperConfig::new();
        config.apply_env_overrides_from(&env).unwrap();
        assert_eq!(config.settings.mapper_paths, vec!["a.xml", "b.xml"]);
    }

    #[test]
    fn test_env_invalid_alias_format_errors() {
        let env = TestEnv::default().set(ENV_TYPE_ALIASES, "noequals");
        let mut config = HirustMapperConfig::new();
        assert!(config.apply_env_overrides_from(&env).is_err());
    }
}
