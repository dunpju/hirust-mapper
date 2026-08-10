use std::collections::HashMap;
use std::fmt;

/// MyBatis映射文件模型
#[derive(Debug, Default, Clone)]
pub struct Mapper {
    /// 命名空间
    pub namespace: String,
    /// SQL语句映射
    pub statements: HashMap<String, SqlStatement>,
    /// 结果映射
    pub result_maps: HashMap<String, ResultMap>,
    /// SQL片段映射
    pub sql_fragments: HashMap<String, Vec<DynamicSqlNode>>,
}

/// MyBatis映射器结构化错误类型
#[derive(Debug)]
pub enum MapperError {
    /// XML解析错误
    ParseError { message: String },
    /// 参数缺失
    MissingParam { param: String, context: String },
    /// SQL片段引用不存在
    MissingFragment { ref_id: String },
    /// 条件表达式无效
    InvalidCondition { expr: String, reason: String },
    /// 语句不存在
    StatementNotFound { id: String },
    /// SQL生成错误
    SqlGenerationError { message: String },
}

impl fmt::Display for MapperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapperError::ParseError { message } => write!(f, "XML解析错误: {}", message),
            MapperError::MissingParam { param, context } => {
                write!(f, "参数 '{}' 不存在 ({})", param, context)
            },
            MapperError::MissingFragment { ref_id } => {
                write!(f, "SQL片段 '{}' 不存在", ref_id)
            },
            MapperError::InvalidCondition { expr, reason } => {
                write!(f, "无效条件 '{}': {}", expr, reason)
            },
            MapperError::StatementNotFound { id } => {
                write!(f, "语句 '{}' 不存在", id)
            },
            MapperError::SqlGenerationError { message } => {
                write!(f, "SQL生成错误: {}", message)
            },
        }
    }
}

impl std::error::Error for MapperError {}

impl From<quick_xml::Error> for MapperError {
    fn from(e: quick_xml::Error) -> Self {
        MapperError::ParseError { message: e.to_string() }
    }
}

impl From<std::str::Utf8Error> for MapperError {
    fn from(e: std::str::Utf8Error) -> Self {
        MapperError::ParseError { message: e.to_string() }
    }
}

/// SQL语句类型
#[derive(Debug, Clone, PartialEq)]
pub enum StatementType {
    Select,
    Insert,
    Update,
    Delete,
}

/// SQL语句模型
#[derive(Debug, Default, Clone)]
pub struct SqlStatement {
    /// 语句ID
    pub id: String,
    /// 语句类型
    pub stmt_type: Option<StatementType>,
    /// 参数类型
    pub parameter_type: Option<String>,
    /// 返回值类型
    pub result_type: Option<String>,
    /// 结果映射ID
    pub result_map: Option<String>,
    /// SQL内容
    pub sql: String,
    /// 动态SQL片段
    pub dynamic_sql: Option<DynamicSqlNode>,
    /// 参数列表
    pub parameters: Vec<String>,
}

/// 结果映射模型
#[derive(Debug, Default, Clone)]
pub struct ResultMap {
    /// 结果映射ID
    pub id: String,
    /// 类型
    pub type_name: String,
    /// 结果列映射
    pub result_columns: Vec<ResultColumn>,
}

/// 结果列映射
#[derive(Debug, Default, Clone)]
pub struct ResultColumn {
    /// 属性名
    pub property: String,
    /// 列名
    pub column: String,
    /// Java类型
    pub java_type: Option<String>,
    /// JDBC类型
    pub jdbc_type: Option<String>,
}

/// 动态SQL节点
#[derive(Debug, Clone)]
pub enum DynamicSqlNode {
    Text(String),
    If {
        test: String,
        contents: Vec<DynamicSqlNode>,
    },
    Choose {
        whens: Vec<(String, Vec<DynamicSqlNode>)>,
        otherwise: Option<Vec<DynamicSqlNode>>,
    },
    Foreach {
        collection: String,
        item: String,
        index: Option<String>,
        open: String,
        separator: String,
        close: String,
        contents: Vec<DynamicSqlNode>,
    },
    Trim {
        prefix: Option<String>,
        prefix_overrides: Option<String>,
        suffix: Option<String>,
        suffix_overrides: Option<String>,
        contents: Vec<DynamicSqlNode>,
    },
    Bind {
        name: String,
        value: String,
    },
    Include {
        ref_id: String,
    },
    Where {
        prefix_overrides: Option<String>,
        suffix_overrides: Option<String>,
        contents: Vec<DynamicSqlNode>,
    },
    Set {
        prefix_overrides: Option<String>,
        suffix_overrides: Option<String>,
        contents: Vec<DynamicSqlNode>,
    },
    /// 混合内容容器：包含多个动态SQL节点的序列
    Mixed {
        contents: Vec<DynamicSqlNode>,
    },
}
