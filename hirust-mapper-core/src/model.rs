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
    /// selectKey（主键回填，仅 INSERT/UPDATE）
    pub select_key: Option<SelectKey>,
}

/// selectKey 的执行时机
#[derive(Debug, Clone, PartialEq, Default)]
pub enum SelectKeyOrder {
    /// 在主 SQL 之前执行
    Before,
    /// 在主 SQL 之后执行（默认）
    #[default]
    After,
}

/// selectKey 配置（生成主键回填）
#[derive(Debug, Default, Clone)]
pub struct SelectKey {
    /// 主键属性名（参数对象上的字段）
    pub key_property: String,
    /// 主键类型
    pub result_type: String,
    /// 执行时机
    pub order: SelectKeyOrder,
    /// 取主键的 SQL
    pub sql: String,
}

/// 结果映射模型
#[derive(Debug, Default, Clone)]
pub struct ResultMap {
    /// 结果映射ID
    pub id: String,
    /// 类型
    pub type_name: String,
    /// 结果列映射（含 <id> 与 <result>）
    pub result_columns: Vec<ResultColumn>,
    /// 一对一嵌套关联（<association>）
    pub associations: Vec<NestedMapping>,
    /// 一对多嵌套集合（<collection>）
    pub collections: Vec<NestedMapping>,
}

/// 结果列映射
#[derive(Debug, Default, Clone)]
pub struct ResultColumn {
    /// 属性名
    pub property: String,
    /// 列名
    pub column: String,
    /// Java 类型
    pub java_type: Option<String>,
    /// JDBC 类型
    pub jdbc_type: Option<String>,
    /// Rust 类型（rustType 属性）
    pub rust_type: Option<String>,
    /// 是否为 <id>（标记身份，用于 collection 分组）
    pub is_id: bool,
}

/// 嵌套映射（<association> / <collection> 共用）
#[derive(Debug, Default, Clone)]
pub struct NestedMapping {
    /// 属性名
    pub property: String,
    /// 外键列（用于关联判空 / 分组）
    pub column: Option<String>,
    /// association: javaType；collection: ofType
    pub nested_type: Option<String>,
    /// 嵌套查询 ID（select 属性，延迟加载；本实现暂不支持）
    pub select: Option<String>,
    /// 嵌套结果列（含 <id> 与 <result>）
    pub result_columns: Vec<ResultColumn>,
    /// 更深层嵌套关联
    pub associations: Vec<NestedMapping>,
    /// 更深层嵌套集合
    pub collections: Vec<NestedMapping>,
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
