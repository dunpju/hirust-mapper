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
        let mut obj = serde_json::Map::with_capacity(mapping.result_columns.len());
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

    /// 构建单个父对象（含顶层列 + association + collection 首元素）。
    ///
    /// `id_values` 为调用方已解码的 `<id>` 列值（按 result_columns 中 is_id 出现顺序），
    /// 传入后避免重复解码；为空或耗尽时回退为按列名解码（供单行路径复用）。
    fn build_parent_object(
        row: &AnyRow,
        result_map: &ResultMap,
        col_index: &HashMap<&str, usize>,
        id_values: Vec<Value>,
    ) -> Result<Value> {
        let mut obj = serde_json::Map::with_capacity(
            result_map.result_columns.len()
                + result_map.associations.len()
                + result_map.collections.len(),
        );
        let mut id_iter = id_values.into_iter();

        // 顶层 id/result 列（id 列复用预解码值）
        for col in &result_map.result_columns {
            let v = if col.is_id {
                match id_iter.next() {
                    Some(v) => v,
                    None => Self::column_value_by_name(row, &col.column, col_index)?,
                }
            } else {
                Self::column_value_by_name(row, &col.column, col_index)?
            };
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

    /// 一次性建立列名→列序号索引（列序在结果集内稳定），后续按 O(1) 查找
    fn col_index_of(row: &AnyRow) -> HashMap<&str, usize> {
        row.columns()
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name(), i))
            .collect()
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

        // 一次性建立列名→列序号索引（列序在结果集内稳定），后续按 O(1) 查找。
        // 索引的 &str 键借用 rows[0] 的列名，rows 在整个函数期内存活，借用有效。
        let col_index = Self::col_index_of(&rows[0]);

        let id_cols: Vec<&str> = result_map
            .result_columns
            .iter()
            .filter(|c| c.is_id)
            .map(|c| c.column.as_str())
            .collect();

        // 上界 = 行数（每行至多产生一个父对象）
        let mut parents: Vec<Value> = Vec::with_capacity(rows.len());
        let mut key_index: HashMap<String, usize> = HashMap::with_capacity(rows.len());

        for (row_idx, row) in rows.iter().enumerate() {
            // id 列每行只解码一次：分组键与父对象构建共用，避免双重解码
            let id_values: Vec<Value> = id_cols
                .iter()
                .map(|c| Self::column_value_by_name(row, c, &col_index))
                .collect::<Result<_>>()?;
            let key = if id_cols.is_empty() {
                format!("__row_{}", row_idx)
            } else if id_values.len() == 1 {
                id_values[0].to_string() // 常见单 id 列：免 Vec/join
            } else {
                // 单元分隔符拼接，避免值内逗号冲突
                id_values
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join("\u{1F}")
            };

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
                parents.push(Self::build_parent_object(row, result_map, &col_index, id_values)?);
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
    ///
    /// 单行无需分组：直接构建父对象（跳过分组键/索引搭建成本）。
    pub fn map_row_with_result_map<T: DeserializeOwned>(
        rows: Vec<AnyRow>,
        result_map: &ResultMap,
    ) -> Result<Option<T>> {
        match rows.len() {
            0 => Ok(None),
            1 => {
                let row = &rows[0];
                let col_index = Self::col_index_of(row);
                let value = Self::build_parent_object(row, result_map, &col_index, Vec::new())?;
                let t = serde_json::from_value::<T>(value).map_err(|e| {
                    MapperRuntimeError::TypeConversion(format!("ResultMap 反序列化失败: {}", e))
                })?;
                Ok(Some(t))
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
