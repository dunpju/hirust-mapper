//! # hirust-mapper
//!
//! MyBatis 风格的 Rust ORM 框架。
//!
//! ## Feature 分层
//!
//! - `core`（默认启用）：XML 解析与动态 SQL 生成
//! - `runtime`：ORM 运行时（配置、session、executor、热重载）
//! - `macros`：编译时类型安全 proc_macro
//! - `full`：启用 runtime + macros
//!
//! ## 快速示例
//!
//! ```ignore
//! use hirust_mapper::*;
//!
//! let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();
//! let sql = mapper.build_sql("findById", &params).unwrap();
//! ```

// 核心层始终导出
pub use hirust_mapper_core::*;

// 运行时层（可选）
#[cfg(feature = "runtime")]
pub use hirust_mapper_runtime::*;

// 宏层（可选）
#[cfg(feature = "macros")]
pub use hirust_mapper_macros::*;
