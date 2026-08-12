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

use hirust_mapper_core::{NestedMapping, ResultMap};
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
        // 直接 move value（成功路径零克隆）；错误信息不附带完整行数据以避免每行克隆
        serde_json::from_value::<T>(value).map_err(|e| {
            MapperRuntimeError::TypeConversion(format!("反序列化行失败: {}", e))
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

    // ─── ResultMap 嵌套映射（P8）──────────────────────────────────

    /// 按列名读取单格值（列不存在返回 Null）
    ///
    /// 通过预构建的 `col_index`（列名→列序号）做 O(1) 查找，避免每格线性扫描。
    fn column_value_by_name(
        row: &AnyRow,
        name: &str,
        col_index: &HashMap<&str, usize>,
    ) -> Result<Value> {
        match col_index.get(name) {
            Some(&idx) => Self::column_to_value(row, idx, None),
            None => Ok(Value::Null),
        }
    }

    /// 构建嵌套对象（association / collection 子项共用）。
    /// 若所有结果列为 null，返回 `Value::Null`（表示无关联对象）。
    fn build_nested_object(
        row: &AnyRow,
        mapping: &NestedMapping,
        col_index: &HashMap<&str, usize>,
    ) -> Result<Value> {
        let mut obj = serde_json::Map::new();
        let mut any_non_null = false;
        for col in &mapping.result_columns {
            let v = Self::column_value_by_name(row, &col.column, col_index)?;
            if !v.is_null() {
                any_non_null = true;
            }
            obj.insert(col.property.clone(), v);
        }
        if any_non_null {
            Ok(Value::Object(obj))
        } else {
            Ok(Value::Null)
        }
    }

    /// 构建单个父对象（含顶层列 + association + collection 首元素）
    fn build_parent_object(
        row: &AnyRow,
        result_map: &ResultMap,
        col_index: &HashMap<&str, usize>,
    ) -> Result<Value> {
        let mut obj = serde_json::Map::new();

        // 顶层 id/result 列
        for col in &result_map.result_columns {
            let v = Self::column_value_by_name(row, &col.column, col_index)?;
            obj.insert(col.property.clone(), v);
        }

        // association（一对一）：列为空则 Null
        for assoc in &result_map.associations {
            let nested = Self::build_nested_object(row, assoc, col_index)?;
            obj.insert(assoc.property.clone(), nested);
        }

        // collection（一对多）：首行放入数组，后续行追加
        for coll in &result_map.collections {
            let child = Self::build_nested_object(row, coll, col_index)?;
            if child.is_null() {
                obj.insert(coll.property.clone(), Value::Array(Vec::new()));
            } else {
                obj.insert(coll.property.clone(), Value::Array(vec![child]));
            }
        }

        Ok(Value::Object(obj))
    }

    /// 父对象的分组键（由 id 列的值拼接；无 id 列时使用行序号）
    fn parent_key(
        row: &AnyRow,
        id_cols: &[String],
        row_idx: usize,
        col_index: &HashMap<&str, usize>,
    ) -> Result<String> {
        if id_cols.is_empty() {
            return Ok(format!("__row_{}", row_idx));
        }
        let mut parts = Vec::with_capacity(id_cols.len());
        for c in id_cols {
            let v = Self::column_value_by_name(row, c, col_index)?;
            parts.push(v.to_string());
        }
        Ok(parts.join("\u{1F}")) // 单元分隔符，避免值内逗号冲突
    }

    /// 使用 ResultMap 将多行映射为 `Vec<T>`（支持 association 一对一 + collection 一对多分组）
    ///
    /// - `<id>` 列决定父对象分组：相同 id 的行合并，collection 追加子项
    /// - association：从扁平 join 行的列构建嵌套对象（列为空 → null）
    /// - collection：按父 id 分组，每行贡献一个子项
    pub fn map_rows_with_result_map<T: DeserializeOwned>(
        rows: Vec<AnyRow>,
        result_map: &ResultMap,
    ) -> Result<Vec<T>> {
        if rows.is_empty() {
            return Ok(Vec::new());
        }

        // 一次性建立列名→列序号索引（列序在结果集内稳定），后续按 O(1) 查找，
        // 取代每格线性扫描（原 O(行数 × 映射列数 × 行总列数) → 现 O(行数 × 映射列数)）。
        // 索引的 &str 键借用 rows[0] 的列名，rows 在整个函数期内存活，借用有效。
        let col_index: HashMap<&str, usize> = rows[0]
            .columns()
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name(), i))
            .collect();

        let id_cols: Vec<String> = result_map
            .result_columns
            .iter()
            .filter(|c| c.is_id)
            .map(|c| c.column.clone())
            .collect();

        let mut parents: Vec<Value> = Vec::new();
        let mut key_index: HashMap<String, usize> = HashMap::new();

        for (row_idx, row) in rows.iter().enumerate() {
            let key = Self::parent_key(row, &id_cols, row_idx, &col_index)?;
            if let Some(&idx) = key_index.get(&key) {
                // 已存在父：仅追加 collection 子项
                for coll in &result_map.collections {
                    let child = Self::build_nested_object(row, coll, &col_index)?;
                    if child.is_null() {
                        continue;
                    }
                    if let Some(arr) = parents[idx]
                        .as_object_mut()
                        .and_then(|o| o.get_mut(&coll.property))
                        .and_then(|v| v.as_array_mut())
                    {
                        arr.push(child);
                    }
                }
            } else {
                key_index.insert(key, parents.len());
                parents.push(Self::build_parent_object(row, result_map, &col_index)?);
            }
        }

        parents
            .into_iter()
            .map(|v| {
                serde_json::from_value::<T>(v).map_err(|e| {
                    MapperRuntimeError::TypeConversion(format!("ResultMap 反序列化失败: {}", e))
                })
            })
            .collect()
    }

    /// 使用 ResultMap 映射单行（`select_one` 路径；多于一行报错）
    pub fn map_row_with_result_map<T: DeserializeOwned>(
        rows: Vec<AnyRow>,
        result_map: &ResultMap,
    ) -> Result<Option<T>> {
        match rows.len() {
            0 => Ok(None),
            1 => {
                let mut mapped = Self::map_rows_with_result_map::<T>(rows, result_map)?;
                Ok(mapped.pop())
            }
            n => Err(MapperRuntimeError::TooManyRows { actual: n }),
        }
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
        let row: AnyRow = sqlx::query_with(sqlx::AssertSqlSafe(&*bound.sql), args)
            .fetch_one(&pool)
            .await
            .unwrap();

        let user: User = ResultSetHandler::map_row(&row).unwrap();
        assert_eq!(user.id, 2);
        assert_eq!(user.name, "李四");
    }
}
