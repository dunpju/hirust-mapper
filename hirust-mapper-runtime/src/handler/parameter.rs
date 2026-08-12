//! 参数处理器
//!
//! [`ParameterHandler`] 负责将 [`BoundSql`] 的参数列表（`Vec<serde_json::Value>`）
//! 绑定到 sqlx 查询。由于参数中间表示统一为 `serde_json::Value`，绑定按 Value 的
//! 变体分派，将每种 JSON 类型映射到 sqlx::Any 兼容的原语类型。

use hirust_mapper_core::BoundSql;
use serde_json::Value;
use sqlx::any::AnyArguments;
use sqlx::Arguments;

use crate::error::{MapperRuntimeError, Result};

/// JSON Value → sqlx::Any 参数绑定的映射
///
/// | serde_json::Value | sqlx::Any 绑定类型 |
/// |-------------------|-------------------|
/// | `Null`             | `Option::<i64>::None` |
/// | `Bool(b)`          | `bool`            |
/// | `Number` (整数)    | `i64`             |
/// | `Number` (浮点)    | `f64`             |
/// | `String(s)`        | `String`          |
/// | `Array` / `Object` | 序列化为 JSON 字符串 |
pub fn bind_value(arguments: &mut AnyArguments, value: &Value) -> Result<()> {
    let add_result: std::result::Result<(), sqlx::error::BoxDynError> = match value {
        Value::Null => arguments.add(Option::<i64>::None),
        Value::Bool(b) => arguments.add(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                arguments.add(i)
            } else if let Some(f) = n.as_f64() {
                arguments.add(f)
            } else {
                // 超出 i64/f64 范围的极端数字，降级为字符串
                arguments.add(n.to_string())
            }
        }
        Value::String(s) => arguments.add(s.clone()),
        Value::Array(_) | Value::Object(_) => {
            // 复杂类型序列化为 JSON 字符串（保持信息，可在 ResultSetHandler 反序列化）
            arguments.add(value.to_string())
        }
    };
    add_result.map_err(|e| {
        MapperRuntimeError::TypeConversion(format!("绑定参数 {:?} 失败: {}", value, e))
    })
}

/// 参数处理器：将 [`BoundSql`] 绑定为可执行的 sqlx 查询
pub struct ParameterHandler;

impl ParameterHandler {
    /// 将 BoundSql 的所有参数绑定到 `AnyArguments`，返回参数缓冲区
    ///
    /// 配合 [`sqlx::query_with`] 使用即可执行：
    /// ```ignore
    /// let args = ParameterHandler::bind_arguments(&bound)?;
    /// let query = sqlx::query_with(sqlx::AssertSqlSafe(&*bound.sql), args);
    /// ```
    pub fn bind_arguments(bound: &BoundSql) -> Result<AnyArguments> {
        let mut arguments = AnyArguments::default();
        arguments.reserve(bound.parameters.len(), 0);
        for value in &bound.parameters {
            bind_value(&mut arguments, value)?;
        }
        Ok(arguments)
    }

    /// 绑定单个 Value 到已有参数缓冲区
    pub fn bind_one(arguments: &mut AnyArguments, value: &Value) -> Result<()> {
        bind_value(arguments, value)
    }

    /// 验证 BoundSql 的参数数量与 SQL 中 `?` 占位符数量一致
    ///
    /// 不一致通常意味着参数绑定会错位（缺失参数产生 MISSING 标记而非 `?`）。
    pub fn validate_placeholder_count(bound: &BoundSql) -> Result<()> {
        let placeholder_count = bound.sql.matches('?').count();
        if placeholder_count != bound.parameters.len() {
            return Err(MapperRuntimeError::TypeConversion(format!(
                "参数数量不匹配: SQL 含 {} 个 ? 占位符, 但提供 {} 个参数（可能存在缺失参数）",
                placeholder_count,
                bound.parameters.len()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn bound_for(sql: &str) -> BoundSql {
        // 直接构造 BoundSql 用于绑定测试
        BoundSql {
            sql: sql.to_string(),
            parameters: vec![],
        }
    }

    #[tokio::test]
    async fn test_parameter_handler_roundtrip_sqlite() {
        sqlx::any::install_default_drivers();
        let pool = sqlx::any::AnyPoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        sqlx::query("CREATE TABLE t (id INTEGER, name TEXT, score REAL, active INTEGER)")
            .execute(&pool)
            .await
            .unwrap();

        // 构造 BoundSql: INSERT INTO t VALUES (?, ?, ?, ?)
        let mut bound = bound_for("INSERT INTO t (id, name, score, active) VALUES (?, ?, ?, ?)");
        bound.parameters = vec![
            Value::Number(1.into()),
            Value::String("张三".into()),
            Value::Number(serde_json::Number::from_f64(95.5).unwrap()),
            Value::Bool(true),
        ];

        let args = ParameterHandler::bind_arguments(&bound).unwrap();
        sqlx::query_with(sqlx::AssertSqlSafe(&*bound.sql), args)
            .execute(&pool)
            .await
            .unwrap();

        // 验证数据正确写入（bool true 经 sqlx 存储为整数 1）
        let row: (i64, String, f64, i64) =
            sqlx::query_as("SELECT id, name, score, active FROM t WHERE id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row.0, 1);
        assert_eq!(row.1, "张三");
        assert!((row.2 - 95.5).abs() < 1e-9);
        assert_eq!(row.3, 1); // Value::Bool(true) → 存储 1
    }

    #[test]
    fn test_validate_placeholder_count() {
        let bound = BoundSql {
            sql: "SELECT ?, ?, ?".to_string(),
            parameters: vec![Value::Null, Value::Null, Value::Null],
        };
        assert!(ParameterHandler::validate_placeholder_count(&bound).is_ok());

        let bound_mismatch = BoundSql {
            sql: "SELECT ?, ?".to_string(),
            parameters: vec![Value::Null],
        };
        let result = ParameterHandler::validate_placeholder_count(&bound_mismatch);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("参数数量不匹配"));
    }

    #[tokio::test]
    async fn test_bind_null_and_array_as_json() {
        sqlx::any::install_default_drivers();
        let pool = sqlx::any::AnyPoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE t2 (a TEXT, b TEXT)")
            .execute(&pool)
            .await
            .unwrap();

        let mut bound = bound_for("INSERT INTO t2 (a, b) VALUES (?, ?)");
        bound.parameters = vec![
            Value::Null,
            Value::Array(vec![Value::Number(1.into()), Value::Number(2.into())]),
        ];

        let args = ParameterHandler::bind_arguments(&bound).unwrap();
        sqlx::query_with(sqlx::AssertSqlSafe(&*bound.sql), args)
            .execute(&pool)
            .await
            .unwrap();

        let row: (Option<String>, String) = sqlx::query_as("SELECT a, b FROM t2")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(row.0.is_none());
        assert_eq!(row.1, "[1,2]"); // Array 序列化为 JSON 字符串
    }
}
