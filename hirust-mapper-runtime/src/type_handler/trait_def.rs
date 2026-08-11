//! TypeHandler trait 定义
//!
//! 类型处理器在 `serde_json::Value`（通用中间表示）与数据库列值之间双向转换。
//! 每个处理器负责一种逻辑类型（i32/i64/String/bool/f64，以及可选的 chrono/uuid）。

use crate::error::Result;
use serde_json::Value;
use sqlx::any::{AnyArguments, AnyRow};

/// 类型处理器 trait
///
/// - [`TypeHandler::get_result`]：从数据库行读取一列，转为 `serde_json::Value`
///   （用于 SELECT 结果映射的 ResultSetHandler）
/// - [`TypeHandler::set_parameter`]：将 `serde_json::Value` 绑定到 sqlx 参数缓冲区
///   （用于 INSERT/UPDATE 参数绑定）
///
/// # 设计说明
///
/// 参数中间表示统一为 `serde_json::Value`（与 MyBatis 一致，最大灵活性）。
/// 因此「参数写入方向」的通用入口是 [`crate::handler::ParameterHandler`]（按 Value
/// 变体分派），而 TypeHandler 主要在「结果读取方向」发挥作用——尤其当 ResultMap
/// 声明了特定 `rustType` 时，通过 [`TypeHandlerRegistry`] 查找对应处理器。
pub trait TypeHandler: Send + Sync + 'static {
    /// 处理器标识的类型名（如 "i64"、"string"），用于注册表查找
    fn type_name(&self) -> &'static str;

    /// 从数据库行的指定列读取值，转为 `serde_json::Value`
    ///
    /// `column` 为列名；若列不存在或类型不兼容将返回错误。
    fn get_result(&self, row: &AnyRow, column: &str) -> Result<Value>;

    /// 将 `serde_json::Value` 绑定到 sqlx 参数缓冲区
    ///
    /// 实现应按自身类型语义调用 [`sqlx::Arguments::add`]。
    /// 对不属于本类型的 Value（且非 Null）可返回错误或尽力转换。
    fn set_parameter(&self, value: &Value, arguments: &mut AnyArguments<'_>) -> Result<()>;
}
