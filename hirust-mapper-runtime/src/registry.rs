//! Mapper 注册表与类型别名注册表
//!
//! 线程安全地持有所有已解析的 `Mapper` 实例，支持热重载时的并发读写。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use hirust_mapper_core::{Mapper, MapperError, MyBatisXmlParser};
use crate::error::MapperRuntimeError;

/// 线程安全的 Mapper 注册表
///
/// 使用 `RwLock` 允许查询路径的并发读，以及热重载时的独占写。
#[derive(Debug, Default, Clone)]
pub struct MapperRegistry {
    inner: Arc<RwLock<HashMap<String, Mapper>>>,
}

impl MapperRegistry {
    /// 创建空注册表
    pub fn new() -> Self {
        Self::default()
    }

    /// 从 XML 内容解析并注册一个 Mapper
    pub fn register_from_xml(&self, xml_content: &str) -> Result<String, MapperError> {
        let mut parser = MyBatisXmlParser::new(xml_content);
        let mapper = parser.parse_mapper()?;
        let namespace = mapper.namespace.clone();
        self.insert_mapper(mapper);
        Ok(namespace)
    }

    /// 从文件路径加载并注册一个 Mapper
    pub fn register_from_file<P: AsRef<std::path::Path>>(&self, path: P) -> Result<String, crate::error::MapperRuntimeError> {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| MapperRuntimeError::Config(
                format!("无法读取 mapper 文件 {}: {}", path.as_ref().display(), e)
            ))?;
        self.register_from_xml(&content)
            .map_err(MapperRuntimeError::from)
    }

    /// 按配置批量加载所有 mapper 文件
    ///
    /// 返回成功注册的 namespace 列表。跳过（并记录）解析失败的文件。
    pub fn load_from_config(
        &self,
        config: &crate::config::HirustMapperConfig,
        base_dir: &std::path::Path,
    ) -> crate::error::Result<Vec<String>> {
        let files = config.discover_mapper_files(base_dir)?;
        let mut namespaces = Vec::with_capacity(files.len());

        for file in files {
            match self.register_from_file(&file) {
                Ok(ns) => namespaces.push(ns),
                Err(e) => {
                    return Err(MapperRuntimeError::Config(format!(
                        "加载 mapper 文件 {} 失败: {}",
                        file.display(), e
                    )));
                }
            }
        }

        Ok(namespaces)
    }

    /// 插入（或替换）一个已解析的 Mapper，返回旧的 Mapper（若存在）
    pub fn insert_mapper(&self, mapper: Mapper) -> Option<Mapper> {
        let mut guard = self.inner.write().expect("MapperRegistry 锁中毒");
        guard.insert(mapper.namespace.clone(), mapper)
    }

    /// 按 namespace 查找 Mapper 的克隆（避免长时间持锁）
    pub fn get_mapper(&self, namespace: &str) -> Option<Mapper> {
        let guard = self.inner.read().expect("MapperRegistry 锁中毒");
        guard.get(namespace).cloned()
    }

    /// 当前已注册的所有 namespace
    pub fn namespaces(&self) -> Vec<String> {
        let guard = self.inner.read().expect("MapperRegistry 锁中毒");
        guard.keys().cloned().collect()
    }

    /// 已注册的 Mapper 数量
    pub fn len(&self) -> usize {
        let guard = self.inner.read().expect("MapperRegistry 锁中毒");
        guard.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 类型别名注册表：XML 短名 → Rust 全限定类型名
#[derive(Debug, Default, Clone)]
pub struct TypeAliasRegistry {
    aliases: HashMap<String, String>,
}

impl TypeAliasRegistry {
    /// 创建空注册表
    pub fn new() -> Self {
        Self::default()
    }

    /// 从配置 map 构造
    pub fn from_map(aliases: HashMap<String, String>) -> Self {
        Self { aliases }
    }

    /// 注册一个别名
    pub fn register(&mut self, alias: impl Into<String>, full_path: impl Into<String>) {
        self.aliases.insert(alias.into(), full_path.into());
    }

    /// 解析类型名：若是已注册别名则返回全限定名，否则原样返回
    pub fn resolve(&self, type_name: &str) -> String {
        self.aliases
            .get(type_name)
            .cloned()
            .unwrap_or_else(|| type_name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_register_and_get() {
        let reg = MapperRegistry::new();
        let xml = r#"<mapper namespace="com.test.UserMapper">
            <select id="findById">SELECT * FROM users WHERE id = #{id}</select>
        </mapper>"#;

        let ns = reg.register_from_xml(xml).unwrap();
        assert_eq!(ns, "com.test.UserMapper");
        assert_eq!(reg.len(), 1);

        let mapper = reg.get_mapper("com.test.UserMapper").unwrap();
        assert!(mapper.statements.contains_key("findById"));
    }

    #[test]
    fn test_type_alias_resolve() {
        let mut reg = TypeAliasRegistry::new();
        reg.register("int", "i32");
        assert_eq!(reg.resolve("int"), "i32");
        assert_eq!(reg.resolve("unknown"), "unknown");
    }

    #[test]
    fn test_load_from_config_e2e() {
        // 创建临时目录，写入两个 mapper XML 文件
        let temp = std::env::temp_dir().join("hirust_test_load_config");
        let mappers_dir = temp.join("mappers");
        std::fs::create_dir_all(&mappers_dir).unwrap();

        std::fs::write(mappers_dir.join("UserMapper.xml"), r#"<mapper namespace="com.test.UserMapper">
            <select id="findById">SELECT * FROM users WHERE id = #{id}</select>
        </mapper>"#).unwrap();

        std::fs::write(mappers_dir.join("OrderMapper.xml"), r#"<mapper namespace="com.test.OrderMapper">
            <select id="findAll">SELECT * FROM orders</select>
        </mapper>"#).unwrap();

        let config = crate::config::HirustMapperConfig::new()
            .with_mapper_paths(vec!["mappers/**/*.xml".to_string()]);

        let registry = MapperRegistry::new();
        let namespaces = registry.load_from_config(&config, &temp).unwrap();

        assert_eq!(namespaces.len(), 2);
        assert!(namespaces.contains(&"com.test.UserMapper".to_string()));
        assert!(namespaces.contains(&"com.test.OrderMapper".to_string()));

        // 验证可以查询
        let user_mapper = registry.get_mapper("com.test.UserMapper").unwrap();
        assert!(user_mapper.build_sql("findById", &HashMap::new()).is_ok());

        // 清理
        std::fs::remove_dir_all(&temp).ok();
    }
}
