//! # hirust-mapper-runtime
//!
//! ORM 运行时层：配置管理、Mapper 注册表、数据库环境、会话工厂、执行器、热重载。
//!
//! 本 crate 在 `hirust-mapper-core` 的解析与生成能力之上，提供完整的 ORM 基础设施。

pub mod bound_sql;
pub mod config;
pub mod environment;
pub mod error;
pub mod executor;
pub mod handler;
pub mod registry;
pub mod session;
pub mod session_factory;
pub mod type_handler;

pub use bound_sql::BoundSql;
pub use config::*;
pub use environment::*;
pub use error::*;
pub use executor::SimpleExecutor;
pub use handler::{ParameterHandler, ResultSetHandler};
pub use registry::*;
pub use session::{MapperProxy, SqlSession};
pub use session_factory::SqlSessionFactory;
pub use type_handler::{
    BoolHandler, F64Handler, I32Handler, I64Handler, StringHandler, TypeHandler,
    TypeHandlerRegistry,
};
