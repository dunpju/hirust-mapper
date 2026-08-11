//! 运行时错误类型
//!
//! 包装核心层 `MapperError` 并扩展运行时特定的错误变体。

use hirust_mapper_core::MapperError;

/// ORM 运行时综合错误类型
#[derive(Debug, thiserror::Error)]
pub enum MapperRuntimeError {
    /// 核心层 XML 解析 / SQL 生成错误
    #[error("Mapper 错误: {0}")]
    Mapper(#[from] MapperError),

    /// 数据库执行错误（sqlx）
    #[error("数据库错误: {0}")]
    Database(#[from] sqlx::Error),

    /// 连接 / 池错误
    #[error("连接错误: {0}")]
    Connection(String),

    /// 事务错误
    #[error("事务错误: {0}")]
    Transaction(String),

    /// Rust ↔ SQL 类型转换错误
    #[error("类型转换错误: {0}")]
    TypeConversion(String),

    /// 配置错误
    #[error("配置错误: {0}")]
    Config(String),

    /// 期望恰好一行但无数据
    #[error("未找到数据: {namespace}.{id}")]
    NoData { namespace: String, id: String },

    /// 期望恰好一行但返回多行
    #[error("返回行数过多: 期望 1, 实际 {actual}")]
    TooManyRows { actual: usize },

    /// 找不到指定 namespace 的 Mapper
    #[error("Mapper 不存在: {0}")]
    MapperNotFound(String),

    /// 热重载错误
    #[error("热重载错误: {0}")]
    HotReload(String),

    /// IO 错误
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// 运行时 Result 别名
pub type Result<T> = std::result::Result<T, MapperRuntimeError>;
