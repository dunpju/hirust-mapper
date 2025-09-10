use std::collections::HashMap;

/// MyBatis映射文件模型
#[derive(Debug, Default)]
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

/// SQL语句类型
#[derive(Debug, Clone, PartialEq)]
pub enum StatementType {
    Select,
    Insert,
    Update,
    Delete,
}

/// SQL语句模型
#[derive(Debug, Default)]
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
#[derive(Debug, Default)]
pub struct ResultMap {
    /// 结果映射ID
    pub id: String,
    /// 类型
    pub type_name: String,
    /// 结果列映射
    pub result_columns: Vec<ResultColumn>,
}

/// 结果列映射
#[derive(Debug)]
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
    // 添加Where节点类型，类似于Trim但有默认的prefix和suffix处理
    Where {
        prefix_overrides: Option<String>,
        suffix_overrides: Option<String>,
        contents: Vec<DynamicSqlNode>,
    },
    // 添加Set节点类型，类似于Trim但有默认的prefix和suffix_overrides处理
    Set {
        prefix_overrides: Option<String>,
        suffix_overrides: Option<String>,
        contents: Vec<DynamicSqlNode>,
    },
}