//! 结果集处理器
//!
//! [`ResultSetHandler`] 将 sqlx 查询结果行（`AnyRow`）映射为 `serde_json::Value`，
//! 再通过 serde 反序列化为目标类型 `T`。
//!
//! 列值按其 `AnyTypeInfoKind` 分派解码：
//!
//! | AnyTypeInfoKind | serde_json::Value |
//! |-----------------|-------------------|
//! | `Null`           | `Null`            |
//! | `Bool`           | `Bool`            |
//! | `SmallInt`/`Integer`/`BigInt` | `Number`(i64) |
//! | `Real`/`Double`  | `Number`(f64)     |
//! | `Text`           | `String`          |
//! | `Blob`           | `String`（UTF-8 lossy）|

use std::collections::HashMap;

use serde::de::DeserializeOwned;
use serde_json::{Number, Value};
use sqlx::any::{AnyRow, AnyTypeInfoKind};
use sqlx::Column;
use sqlx::Row;
use sqlx::ValueRef;

use crate::error::{MapperRuntimeError, Result};
use crate::type_handler::TypeHandlerRegistry;

/// 结果集处理器：AnyRow → serde_json::Value → T
pub struct ResultSetHandler {
    /// 类型处理器注册表（用于自定义类型解码，可选）
    type_handlers: TypeHandlerRegistry,
}

impl Default for ResultSetHandler {
    fn default() -> Self {
        Self {
            type_handlers: TypeHandlerRegistry::with_defaults(),
        }
    }
}

impl ResultSetHandler {
    /// 创建带默认类型处理器的 ResultSetHandler
    pub fn new() -> Self {
        Self::default()
    }

    /// 使用指定的类型处理器注册表
    pub fn with_handlers(type_handlers: TypeHandlerRegistry) -> Self {
        Self { type_handlers }
    }

    /// 将一行的指定列解码为 `serde_json::Value`
    ///
    /// 按**实际值的类型种类**（而非列声明类型）分派——通过 [`AnyRow::try_get_raw`]
    /// 读取原始值的 `type_info`，可正确处理计算列（如 `count(*)`，声明类型为 NULL
    /// 但实际值是整数）。
    pub fn column_to_value(row: &AnyRow, index: usize, _rust_type: Option<&str>) -> Result<Value> {
        let columns = row.columns();
        if index >= columns.len() {
            return Err(MapperRuntimeError::TypeConversion(format!(
                "列索引 {} 越界（共 {} 列）", index, columns.len()
            )));
        }

        // 取实际值（而非列声明）的类型种类
        let value_ref = row
            .try_get_raw(index)
            .map_err(decode_err("raw", index))?;
        let kind = value_ref.type_info().kind();

        match kind {
            AnyTypeInfoKind::Null => Ok(Value::Null),
            AnyTypeInfoKind::Bool => {
                let v: Option<bool> = row.try_get(index).map_err(decode_err("bool", index))?;
                Ok(v.map(Value::Bool).unwrap_or(Value::Null))
            }
            AnyTypeInfoKind::SmallInt | AnyTypeInfoKind::Integer | AnyTypeInfoKind::BigInt => {
                let v: Option<i64> = row.try_get(index).map_err(decode_err("i64", index))?;
                Ok(v.map(|x| Value::Number(x.into())).unwrap_or(Value::Null))
            }
            AnyTypeInfoKind::Real => {
                let v: Option<f32> = row.try_get(index).map_err(decode_err("f32", index))?;
                Ok(v
                    .and_then(|x| Number::from_f64(x as f64))
                    .map(Value::Number)
                    .unwrap_or(Value::Null))
            }
            AnyTypeInfoKind::Double => {
                let v: Option<f64> = row.try_get(index).map_err(decode_err("f64", index))?;
                Ok(v
                    .and_then(Number::from_f64)
                    .map(Value::Number)
                    .unwrap_or(Value::Null))
            }
            AnyTypeInfoKind::Text => {
                let v: Option<String> = row.try_get(index).map_err(decode_err("string", index))?;
                Ok(v.map(Value::String).unwrap_or(Value::Null))
            }
            AnyTypeInfoKind::Blob => {
                let v: Option<Vec<u8>> = row.try_get(index).map_err(decode_err("blob", index))?;
                Ok(v
                    .map(|b| Value::String(String::from_utf8_lossy(&b).into_owned()))
                    .unwrap_or(Value::Null))
            }
        }
    }

    /// 将整行映射为 `serde_json::Value::Object`（列名 → 值）
    pub fn row_to_value(row: &AnyRow) -> Result<Value> {
        let mut obj = serde_json::Map::with_capacity(row.columns().len());
        for (idx, col) in row.columns().iter().enumerate() {
            let name = col.name().to_string();
            let val = Self::column_to_value(row, idx, None)?;
            obj.insert(name, val);
        }
        Ok(Value::Object(obj))
    }

    /// 将一行反序列化为目标类型 `T`
    pub fn map_row<T: DeserializeOwned>(row: &AnyRow) -> Result<T> {
        let value = Self::row_to_value(row)?;
        serde_json::from_value(value.clone()).map_err(|e| {
            MapperRuntimeError::TypeConversion(format!(
                "反序列化行失败: {}（行数据: {}）", e, value
            ))
        })
    }

    /// 将多行反序列化为 `Vec<T>`
    pub fn map_rows<T: DeserializeOwned>(rows: Vec<AnyRow>) -> Result<Vec<T>> {
        rows.iter()
            .map(Self::map_row)
            .collect()
    }

    /// 按列名 → 值的方式映射（用于列名与字段名不一致的场景）
    ///
    /// 返回 `HashMap<列名, Value>`，调用方可自行构造 Value::Object 再反序列化。
    pub fn row_to_map(row: &AnyRow) -> Result<HashMap<String, Value>> {
        let mut map = HashMap::with_capacity(row.columns().len());
        for (idx, col) in row.columns().iter().enumerate() {
            let val = Self::column_to_value(row, idx, None)?;
            map.insert(col.name().to_string(), val);
        }
        Ok(map)
    }

    /// 获取类型处理器注册表引用
    pub fn type_handlers(&self) -> &TypeHandlerRegistry {
        &self.type_handlers
    }
}

fn decode_err(typ: &str, idx: usize) -> impl Fn(sqlx::Error) -> MapperRuntimeError + '_ {
    move |e: sqlx::Error| {
        MapperRuntimeError::TypeConversion(format!("解码列 {} 为 {} 失败: {}", idx, typ, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct User {
        id: i64,
        name: String,
        score: f64,
        active: i64,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct OptUser {
        id: i64,
        nickname: Option<String>,
    }

    async fn setup_pool() -> sqlx::AnyPool {
        sqlx::any::install_default_drivers();
        // 单连接池：sqlite::memory: 每个连接是独立数据库，需固定单连接以保证表持久
        // 注：sqlite 无原生 BOOLEAN，布尔以 INTEGER(0/1) 存储
        let pool = sqlx::any::AnyPoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE users (id INTEGER, name TEXT, score REAL, active INTEGER)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO users VALUES (1, '张三', 95.5, 1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO users VALUES (2, '李四', 80.0, 0)")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn test_map_row_struct() {
        let pool = setup_pool().await;
        let row: AnyRow = sqlx::query("SELECT id, name, score, active FROM users WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();

        let user: User = ResultSetHandler::map_row(&row).unwrap();
        assert_eq!(user, User { id: 1, name: "张三".into(), score: 95.5, active: 1 });
    }

    #[tokio::test]
    async fn test_map_rows_multiple() {
        let pool = setup_pool().await;
        let rows: Vec<AnyRow> = sqlx::query("SELECT id, name, score, active FROM users ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();

        let users: Vec<User> = ResultSetHandler::map_rows(rows).unwrap();
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].name, "张三");
        assert_eq!(users[1].name, "李四");
        assert_eq!(users[1].active, 0);
    }

    #[tokio::test]
    async fn test_row_to_value_object() {
        let pool = setup_pool().await;
        let row: AnyRow = sqlx::query("SELECT id, name FROM users WHERE id = 2")
            .fetch_one(&pool)
            .await
            .unwrap();

        let val = ResultSetHandler::row_to_value(&row).unwrap();
        let obj = val.as_object().unwrap();
        assert_eq!(obj.get("id").and_then(|v| v.as_i64()), Some(2));
        assert_eq!(obj.get("name").and_then(|v| v.as_str()), Some("李四"));
    }

    #[tokio::test]
    async fn test_null_column_to_value() {
        sqlx::any::install_default_drivers();
        let pool = sqlx::any::AnyPoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE n (id INTEGER, nickname TEXT)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO n VALUES (1, NULL)").execute(&pool).await.unwrap();

        let row: AnyRow = sqlx::query("SELECT id, nickname FROM n WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();

        let user: OptUser = ResultSetHandler::map_row(&row).unwrap();
        assert_eq!(user.id, 1);
        assert!(user.nickname.is_none());
    }

    #[tokio::test]
    async fn test_row_to_map_by_name() {
        let pool = setup_pool().await;
        let row: AnyRow = sqlx::query("SELECT id, name FROM users WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();

        let map = ResultSetHandler::row_to_map(&row).unwrap();
        assert_eq!(map.get("id").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(map.get("name").and_then(|v| v.as_str()), Some("张三"));
    }

    #[tokio::test]
    async fn test_full_roundtrip_with_bound_sql() {
        // 端到端：BoundSql 参数化插入 → ResultSetHandler 映射读取
        use hirust_mapper_core::BoundSql;
        use crate::handler::parameter::ParameterHandler;

        let pool = setup_pool().await;

        let bound = BoundSql {
            sql: "SELECT id, name, score, active FROM users WHERE id = ?".to_string(),
            parameters: vec![Value::Number(2.into())],
        };

        let args = ParameterHandler::bind_arguments(&bound).unwrap();
        let row: AnyRow = sqlx::query_with(&bound.sql, args)
            .fetch_one(&pool)
            .await
            .unwrap();

        let user: User = ResultSetHandler::map_row(&row).unwrap();
        assert_eq!(user.id, 2);
        assert_eq!(user.name, "李四");
    }
}
