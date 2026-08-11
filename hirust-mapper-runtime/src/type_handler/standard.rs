//! 内置类型处理器与注册表
//!
//! 提供 i32 / i64 / String / bool / f64 五种基础类型的标准处理器，
//! 以及 feature-gated 的 `chrono`（日期时间）与 `uuid` 处理器。
//!
//! [`TypeHandlerRegistry`] 持有这些处理器，供 ResultSetHandler 按类型名查找。

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{MapperRuntimeError, Result};
use crate::type_handler::trait_def::TypeHandler;
use serde_json::{Number, Value};
use sqlx::any::{AnyArguments, AnyRow, AnyTypeInfoKind};
use sqlx::Arguments;
use sqlx::Column;
use sqlx::Row;

// ─── 辅助：读取列的类型种类 ─────────────────────────────────────────

/// 取列的类型种类；若列不存在返回 None
fn column_kind(row: &AnyRow, column: &str) -> Result<AnyTypeInfoKind> {
    for col in row.columns() {
        if col.name() == column {
            return Ok(col.type_info().kind());
        }
    }
    Err(MapperRuntimeError::TypeConversion(format!(
        "列 '{}' 不存在", column
    )))
}

/// 将 add 的 BoxDynError 结果转为 MapperRuntimeError
fn check_add(
    r: std::result::Result<(), sqlx::error::BoxDynError>,
    type_name: &str,
) -> Result<()> {
    r.map_err(|e| {
        MapperRuntimeError::TypeConversion(format!("{} 处理器绑定参数失败: {}", type_name, e))
    })
}

// ─── i64 处理器 ────────────────────────────────────────────────────

/// i64 类型处理器（覆盖 SmallInt / Integer / BigInt）
pub struct I64Handler;

impl TypeHandler for I64Handler {
    fn type_name(&self) -> &'static str {
        "i64"
    }

    fn get_result(&self, row: &AnyRow, column: &str) -> Result<Value> {
        let v: Option<i64> = row
            .try_get(column)
            .map_err(|e| MapperRuntimeError::TypeConversion(format!("读取 i64 列 '{}': {}", column, e)))?;
        Ok(v.map(|x| Value::Number(x.into())).unwrap_or(Value::Null))
    }

    fn set_parameter(&self, value: &Value, arguments: &mut AnyArguments<'_>) -> Result<()> {
        match value {
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    check_add(arguments.add(i), "i64")?;
                } else if let Some(f) = n.as_f64() {
                    // 超出 i64 范围的数降级为 f64
                    check_add(arguments.add(f), "i64")?;
                } else {
                    check_add(arguments.add(0i64), "i64")?;
                }
            }
            Value::Null => {
                check_add(arguments.add(Option::<i64>::None), "i64")?;
            }
            other => {
                return Err(MapperRuntimeError::TypeConversion(format!(
                    "i64 处理器无法绑定 {:?}（仅接受数字或 null）", other
                )));
            }
        }
        Ok(())
    }
}

// ─── i32 处理器 ────────────────────────────────────────────────────

/// i32 类型处理器
pub struct I32Handler;

impl TypeHandler for I32Handler {
    fn type_name(&self) -> &'static str {
        "i32"
    }

    fn get_result(&self, row: &AnyRow, column: &str) -> Result<Value> {
        let v: Option<i32> = row
            .try_get(column)
            .map_err(|e| MapperRuntimeError::TypeConversion(format!("读取 i32 列 '{}': {}", column, e)))?;
        Ok(v.map(|x| Value::Number(x.into())).unwrap_or(Value::Null))
    }

    fn set_parameter(&self, value: &Value, arguments: &mut AnyArguments<'_>) -> Result<()> {
        match value {
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    check_add(arguments.add(i as i32), "i32")?;
                } else {
                    check_add(arguments.add(0i32), "i32")?;
                }
            }
            Value::Null => {
                check_add(arguments.add(Option::<i32>::None), "i32")?;
            }
            other => {
                return Err(MapperRuntimeError::TypeConversion(format!(
                    "i32 处理器无法绑定 {:?}", other
                )));
            }
        }
        Ok(())
    }
}

// ─── f64 处理器 ────────────────────────────────────────────────────

/// f64 类型处理器（覆盖 Real / Double）
pub struct F64Handler;

impl TypeHandler for F64Handler {
    fn type_name(&self) -> &'static str {
        "f64"
    }

    fn get_result(&self, row: &AnyRow, column: &str) -> Result<Value> {
        let kind = column_kind(row, column)?;
        let val = match kind {
            AnyTypeInfoKind::Real => {
                let v: Option<f32> = row.try_get(column).map_err(|e| {
                    MapperRuntimeError::TypeConversion(format!("读取 f32 列 '{}': {}", column, e))
                })?;
                v.map(|x| x as f64)
            }
            _ => {
                let v: Option<f64> = row.try_get(column).map_err(|e| {
                    MapperRuntimeError::TypeConversion(format!("读取 f64 列 '{}': {}", column, e))
                })?;
                v
            }
        };
        Ok(val
            .and_then(Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::Null))
    }

    fn set_parameter(&self, value: &Value, arguments: &mut AnyArguments<'_>) -> Result<()> {
        match value {
            Value::Number(n) => {
                let f = n.as_f64().unwrap_or(0.0);
                check_add(arguments.add(f), "f64")?;
            }
            Value::Null => {
                check_add(arguments.add(Option::<f64>::None), "f64")?;
            }
            other => {
                return Err(MapperRuntimeError::TypeConversion(format!(
                    "f64 处理器无法绑定 {:?}", other
                )));
            }
        }
        Ok(())
    }
}

// ─── bool 处理器 ───────────────────────────────────────────────────

/// bool 类型处理器
pub struct BoolHandler;

impl TypeHandler for BoolHandler {
    fn type_name(&self) -> &'static str {
        "bool"
    }

    fn get_result(&self, row: &AnyRow, column: &str) -> Result<Value> {
        let v: Option<bool> = row
            .try_get(column)
            .map_err(|e| MapperRuntimeError::TypeConversion(format!("读取 bool 列 '{}': {}", column, e)))?;
        Ok(v.map(Value::Bool).unwrap_or(Value::Null))
    }

    fn set_parameter(&self, value: &Value, arguments: &mut AnyArguments<'_>) -> Result<()> {
        match value {
            Value::Bool(b) => {
                check_add(arguments.add(*b), "bool")?;
            }
            Value::Null => {
                check_add(arguments.add(Option::<bool>::None), "bool")?;
            }
            other => {
                return Err(MapperRuntimeError::TypeConversion(format!(
                    "bool 处理器无法绑定 {:?}", other
                )));
            }
        }
        Ok(())
    }
}

// ─── String 处理器 ─────────────────────────────────────────────────

/// String 类型处理器（Text 列）
pub struct StringHandler;

impl TypeHandler for StringHandler {
    fn type_name(&self) -> &'static str {
        "string"
    }

    fn get_result(&self, row: &AnyRow, column: &str) -> Result<Value> {
        let v: Option<String> = row
            .try_get(column)
            .map_err(|e| MapperRuntimeError::TypeConversion(format!("读取 string 列 '{}': {}", column, e)))?;
        Ok(v.map(Value::String).unwrap_or(Value::Null))
    }

    fn set_parameter(&self, value: &Value, arguments: &mut AnyArguments<'_>) -> Result<()> {
        match value {
            Value::String(s) => {
                check_add(arguments.add(s.clone()), "string")?;
            }
            Value::Null => {
                check_add(arguments.add(Option::<String>::None), "string")?;
            }
            // 非 String 值：序列化为 JSON 字符串（保持信息）
            other => {
                check_add(arguments.add(other.to_string()), "string")?;
            }
        }
        Ok(())
    }
}

// ─── feature-gated: chrono 处理器 ──────────────────────────────────

#[cfg(feature = "chrono")]
mod chrono_handler {
    use super::*;
    use chrono::{DateTime, Utc};

    /// chrono::DateTime<Utc> 处理器（以 RFC3339 字符串存储）
    pub struct ChronoHandler;

    impl TypeHandler for ChronoHandler {
        fn type_name(&self) -> &'static str {
            "chrono"
        }

        fn get_result(&self, row: &AnyRow, column: &str) -> Result<Value> {
            let s: Option<String> = row
                .try_get(column)
                .map_err(|e| MapperRuntimeError::TypeConversion(format!("读取 chrono 列 '{}': {}", column, e)))?;
            match s {
                Some(text) => {
                    let dt: DateTime<Utc> = text.parse().map_err(|e| {
                        MapperRuntimeError::TypeConversion(format!("解析日期时间 '{}' 失败: {}", text, e))
                    })?;
                    Ok(Value::String(dt.to_rfc3339()))
                }
                None => Ok(Value::Null),
            }
        }

        fn set_parameter(&self, value: &Value, arguments: &mut AnyArguments<'_>) -> Result<()> {
            match value {
                Value::String(s) => {
                    // 验证可解析
                    let dt: DateTime<Utc> = s.parse().map_err(|e| {
                        MapperRuntimeError::TypeConversion(format!("无效的日期时间 '{}': {}", s, e))
                    })?;
                    check_add(arguments.add(dt.to_rfc3339()), "chrono")?;
                }
                Value::Null => {
                    check_add(arguments.add(Option::<String>::None), "chrono")?;
                }
                other => {
                    return Err(MapperRuntimeError::TypeConversion(format!(
                        "chrono 处理器无法绑定 {:?}（期望 RFC3339 字符串）", other
                    )));
                }
            }
            Ok(())
        }
    }
}

#[cfg(feature = "chrono")]
pub use chrono_handler::ChronoHandler;

// ─── feature-gated: uuid 处理器 ────────────────────────────────────

#[cfg(feature = "uuid")]
mod uuid_handler {
    use super::*;

    /// uuid::Uuid 处理器（以字符串存储）
    pub struct UuidHandler;

    impl TypeHandler for UuidHandler {
        fn type_name(&self) -> &'static str {
            "uuid"
        }

        fn get_result(&self, row: &AnyRow, column: &str) -> Result<Value> {
            let s: Option<String> = row
                .try_get(column)
                .map_err(|e| MapperRuntimeError::TypeConversion(format!("读取 uuid 列 '{}': {}", column, e)))?;
            match s {
                Some(text) => {
                    let _u: uuid::Uuid = text.parse().map_err(|e| {
                        MapperRuntimeError::TypeConversion(format!("无效的 UUID '{}': {}", text, e))
                    })?;
                    Ok(Value::String(text))
                }
                None => Ok(Value::Null),
            }
        }

        fn set_parameter(&self, value: &Value, arguments: &mut AnyArguments<'_>) -> Result<()> {
            match value {
                Value::String(s) => {
                    let u: uuid::Uuid = s.parse().map_err(|e| {
                        MapperRuntimeError::TypeConversion(format!("无效的 UUID '{}': {}", s, e))
                    })?;
                    check_add(arguments.add(u.to_string()), "uuid")?;
                }
                Value::Null => {
                    check_add(arguments.add(Option::<String>::None), "uuid")?;
                }
                other => {
                    return Err(MapperRuntimeError::TypeConversion(format!(
                        "uuid 处理器无法绑定 {:?}", other
                    )));
                }
            }
            Ok(())
        }
    }
}

#[cfg(feature = "uuid")]
pub use uuid_handler::UuidHandler;

// ─── 类型处理器注册表 ──────────────────────────────────────────────

/// 类型处理器注册表
///
/// 按类型名（"i64"、"string" 等）查找 [`TypeHandler`]。
/// 默认注册 5 种基础类型；chrono/uuid 在对应 feature 启用时注册。
#[derive(Clone, Default)]
pub struct TypeHandlerRegistry {
    handlers: HashMap<String, Arc<dyn TypeHandler>>,
}

impl std::fmt::Debug for TypeHandlerRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypeHandlerRegistry")
            .field("registered", &self.handlers.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl TypeHandlerRegistry {
    /// 创建空注册表
    pub fn new() -> Self {
        Self::default()
    }

    /// 创建带默认内置处理器的注册表
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        reg.register_defaults();
        reg
    }

    /// 注册所有内置默认处理器
    pub fn register_defaults(&mut self) {
        self.register(Arc::new(I64Handler));
        self.register(Arc::new(I32Handler));
        self.register(Arc::new(F64Handler));
        self.register(Arc::new(BoolHandler));
        self.register(Arc::new(StringHandler));
        #[cfg(feature = "chrono")]
        self.register(Arc::new(ChronoHandler));
        #[cfg(feature = "uuid")]
        self.register(Arc::new(UuidHandler));
    }

    /// 注册一个处理器（按 type_name 索引）
    pub fn register(&mut self, handler: Arc<dyn TypeHandler>) {
        self.handlers.insert(handler.type_name().to_string(), handler);
    }

    /// 按类型名查找处理器
    pub fn get(&self, type_name: &str) -> Option<&Arc<dyn TypeHandler>> {
        self.handlers.get(type_name)
    }

    /// 已注册的类型名列表
    pub fn type_names(&self) -> Vec<&str> {
        self.handlers.keys().map(|s| s.as_str()).collect()
    }

    /// 已注册处理器数量
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_registry_defaults() {
        let reg = TypeHandlerRegistry::with_defaults();
        assert!(reg.get("i64").is_some());
        assert!(reg.get("i32").is_some());
        assert!(reg.get("f64").is_some());
        assert!(reg.get("bool").is_some());
        assert!(reg.get("string").is_some());
        assert!(reg.get("nonexistent").is_none());
        assert!(reg.len() >= 5);
    }

    #[test]
    fn test_registry_custom_register() {
        let mut reg = TypeHandlerRegistry::new();
        reg.register(Arc::new(StringHandler));
        assert_eq!(reg.len(), 1);
        assert!(reg.get("string").is_some());

        let names = reg.type_names();
        assert!(names.contains(&"string"));
    }

    #[test]
    fn test_i64_handler_set_parameter_accepts_number() {
        use sqlx::Arguments;
        let mut args: AnyArguments = AnyArguments::default();
        I64Handler.set_parameter(&json!(42), &mut args).unwrap();
        I64Handler.set_parameter(&json!(null), &mut args).unwrap();
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn test_i64_handler_set_parameter_rejects_string() {
        let mut args: AnyArguments = AnyArguments::default();
        let result = I64Handler.set_parameter(&json!("not a number"), &mut args);
        assert!(result.is_err());
    }

    #[test]
    fn test_string_handler_set_parameter_coerces_non_string() {
        use sqlx::Arguments;
        let mut args: AnyArguments = AnyArguments::default();
        // String handler 接受字符串、null，并将其他类型序列化
        StringHandler.set_parameter(&json!("hello"), &mut args).unwrap();
        StringHandler.set_parameter(&json!(123), &mut args).unwrap();
        StringHandler.set_parameter(&json!(null), &mut args).unwrap();
        assert_eq!(args.len(), 3);
    }

    #[test]
    fn test_bool_handler_strict() {
        let mut args: AnyArguments = AnyArguments::default();
        assert!(BoolHandler.set_parameter(&json!(true), &mut args).is_ok());
        assert!(BoolHandler.set_parameter(&json!("yes"), &mut args).is_err());
    }

    #[tokio::test]
    async fn test_i64_and_string_handler_get_result_roundtrip() {
        sqlx::any::install_default_drivers();
        let pool = sqlx::any::AnyPoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE rt (id INTEGER, name TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO rt VALUES (7, 'hello')")
            .execute(&pool)
            .await
            .unwrap();

        let row: AnyRow = sqlx::query("SELECT id, name FROM rt")
            .fetch_one(&pool)
            .await
            .unwrap();

        let id_val = I64Handler.get_result(&row, "id").unwrap();
        let name_val = StringHandler.get_result(&row, "name").unwrap();
        assert_eq!(id_val, json!(7));
        assert_eq!(name_val, json!("hello"));
    }

    #[test]
    fn test_f64_handler_set_parameter() {
        use sqlx::Arguments;
        let mut args: AnyArguments = AnyArguments::default();
        F64Handler.set_parameter(&json!(3.14), &mut args).unwrap();
        F64Handler.set_parameter(&json!(null), &mut args).unwrap();
        assert_eq!(args.len(), 2);
    }
}
