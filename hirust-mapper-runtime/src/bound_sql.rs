//! BoundSql 运行时集成模块
//!
//! 本模块从 [`hirust_mapper_core`] 重新导出 [`BoundSql`] 与 [`generate_bound_sql`]，
//! 并提供 `SqlSession` 级别的便捷绑定方法。
//!
//! `BoundSql` 是两阶段 SQL 解析的 Phase 2 输出：
//! - `#{param}` → `?` 占位符 + 参数进入列表
//! - `${param}` → 原样内联（无法参数化）
//!
//! 运行时通过 [`SqlSession::build_bound_sql`] 可直接获得参数化 SQL，
//! 为 P6 的 Executor 执行层（绑定到 sqlx）做准备。

// 重新导出 core 的 BoundSql 类型
pub use hirust_mapper_core::BoundSql;

use std::collections::HashMap;

use serde_json::Value;

use crate::error::{MapperRuntimeError, Result};

/// 运行时便捷绑定：根据 namespace + statement id + 参数生成 [`BoundSql`]
///
/// 这是对 `Mapper::build_bound_sql` 的封装，自动处理 namespace 查找与错误转换。
/// 可在任何持有 mapper 引用的地方使用。
pub fn build_bound_sql(
    mapper: &hirust_mapper_core::Mapper,
    statement_id: &str,
    params: &HashMap<String, Value>,
) -> Result<BoundSql> {
    mapper
        .build_bound_sql(statement_id, params)
        .map_err(MapperRuntimeError::from)
}
