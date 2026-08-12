//! 执行器模块
//!
//! [`SimpleExecutor`] 提供 SQL 执行的核心能力：绑定参数 → 执行 → 映射结果。
//! 通过泛型 `E: sqlx::Executor` 同时支持连接池（`&AnyPool`）与事务（`&mut AnyConnection`）。

pub mod simple;

pub use simple::{execute_rows_affected, SimpleExecutor};
