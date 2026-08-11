//! 处理器模块
//!
//! - [`parameter::ParameterHandler`]：`serde_json::Value` → sqlx 参数绑定
//! - [`result_set::ResultSetHandler`]：sqlx Row → `serde_json::Value` → `T`

pub mod parameter;
pub mod result_set;

pub use parameter::{bind_value, ParameterHandler};
pub use result_set::ResultSetHandler;
