//! ORM 生命周期事件
//!
//! 在 SQL 执行前后由执行器自动派发，配合 [`super::EventBus`] 实现 SQL 层的观察者。
//! 对应 ThinkPHP 模型事件的 `on_before_*` / `on_after_*` 思路（观察语义）。

use std::time::Duration;

use serde_json::Value;

use super::Event;

/// SQL 操作种类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlKind {
    Select,
    Insert,
    Update,
    Delete,
}

impl std::fmt::Display for SqlKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SqlKind::Select => write!(f, "SELECT"),
            SqlKind::Insert => write!(f, "INSERT"),
            SqlKind::Update => write!(f, "UPDATE"),
            SqlKind::Delete => write!(f, "DELETE"),
        }
    }
}

/// 按首关键字（尽力而为）判定 SQL 种类，用于事件分类
pub fn classify_sql(sql: &str) -> SqlKind {
    let upper = sql.trim_start().trim_start_matches('(').trim_start();
    let upper = upper.to_ascii_uppercase();
    if upper.starts_with("SELECT") {
        SqlKind::Select
    } else if upper.starts_with("INSERT") || upper.starts_with("REPLACE") {
        SqlKind::Insert
    } else if upper.starts_with("UPDATE") {
        SqlKind::Update
    } else if upper.starts_with("DELETE") {
        SqlKind::Delete
    } else {
        SqlKind::Select
    }
}

/// SQL 执行**前**事件（观察用；不可修改 SQL 或参数）
#[derive(Debug, Clone)]
pub struct BeforeSqlEvent {
    /// 含 `?` 占位符的原始 SQL
    pub raw_sql: String,
    /// 绑定参数（按出现顺序）
    pub params: Vec<Value>,
    /// 操作种类
    pub kind: SqlKind,
}

/// SQL 执行**后**事件（含耗时与结果摘要）
#[derive(Debug, Clone)]
pub struct AfterSqlEvent {
    /// 含 `?` 占位符的原始 SQL
    pub raw_sql: String,
    /// 绑定参数（按出现顺序）
    pub params: Vec<Value>,
    /// 操作种类
    pub kind: SqlKind,
    /// 执行耗时
    pub elapsed: Duration,
    /// 结果摘要
    pub outcome: SqlOutcome,
}

/// SQL 执行结果摘要
#[derive(Debug, Clone)]
pub enum SqlOutcome {
    /// SELECT 返回的行数
    Fetched(usize),
    /// INSERT/UPDATE/DELETE 受影响行数
    Affected(u64),
    /// 执行失败（错误信息）
    Failed(String),
}

impl SqlOutcome {
    /// 是否执行成功
    pub fn is_ok(&self) -> bool {
        !matches!(self, SqlOutcome::Failed(_))
    }
}

impl Event for BeforeSqlEvent {}
impl Event for AfterSqlEvent {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_sql() {
        assert_eq!(classify_sql("SELECT * FROM t"), SqlKind::Select);
        assert_eq!(classify_sql("  insert into t"), SqlKind::Insert);
        assert_eq!(classify_sql("REPLACE INTO t"), SqlKind::Insert);
        assert_eq!(classify_sql("(SELECT 1) UNION (SELECT 2)"), SqlKind::Select);
        assert_eq!(classify_sql("UPDATE t SET"), SqlKind::Update);
        assert_eq!(classify_sql("delete from t"), SqlKind::Delete);
    }

    #[test]
    fn test_sqlkind_display() {
        assert_eq!(SqlKind::Select.to_string(), "SELECT");
        assert_eq!(SqlKind::Insert.to_string(), "INSERT");
    }

    #[test]
    fn test_outcome_is_ok() {
        assert!(SqlOutcome::Fetched(0).is_ok());
        assert!(SqlOutcome::Affected(3).is_ok());
        assert!(!SqlOutcome::Failed("boom".into()).is_ok());
    }
}
