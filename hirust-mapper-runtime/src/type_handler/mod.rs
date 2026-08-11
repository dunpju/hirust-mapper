//! 类型处理器模块
//!
//! 在 `serde_json::Value` 与数据库列值之间双向转换。
//!
//! - [`TypeHandler`]：类型处理器 trait
//! - 标准实现：`I64Handler` / `I32Handler` / `F64Handler` / `BoolHandler` / `StringHandler`
//! - feature-gated：`ChronoHandler`（`chrono`）/ `UuidHandler`（`uuid`）
//! - [`TypeHandlerRegistry`]：按类型名查找处理器

pub mod standard;
pub mod trait_def;

pub use standard::{
    BoolHandler, F64Handler, I32Handler, I64Handler, StringHandler, TypeHandlerRegistry,
};
#[cfg(feature = "chrono")]
pub use standard::ChronoHandler;
#[cfg(feature = "uuid")]
pub use standard::UuidHandler;
pub use trait_def::TypeHandler;
