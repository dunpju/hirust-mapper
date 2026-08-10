//! # hirust-mapper-runtime
//!
//! ORM 运行时层：配置管理、Mapper 注册表、会话、执行器、热重载。
//!
//! 本 crate 在 `hirust-mapper-core` 的解析与生成能力之上，提供完整的 ORM 基础设施。
//! 当前为初始骨架，具体模块将在后续阶段逐步实现。

pub mod config;
pub mod error;
pub mod registry;

pub use config::*;
pub use error::*;
pub use registry::*;
