//! SQL 执行日志
//!
//! 受配置 `[settings] sql_log` 开关控制。启用后，在每次 SQL 执行点记录
//! 「耗时 + 可读 SQL（参数内联进 `?`）」一条日志，经 `log` facade 输出。
//!
//! ## 输出示例
//!
//! 后端（如 `env_logger` / `tracing_subscriber`）会附带时间戳与级别前缀：
//!
//! ```text
//! [2026-08-12 15:32:03 INFO hirust_mapper::sql] Consume Time: 44 ms
//!  Execute SQL: SELECT `examId`, `examName` FROM exam WHERE (`examId` IN (69902) AND `isDelete` = 0)
//! ```
//!
//! ## 消费方
//!
//! 本 crate 仅通过 `log` facade 发射日志，**不自带输出后端**。要看到输出，
//! 应用需初始化一个日志后端并设置级别，例如：
//!
//! ```ignore
//! env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
//! // 或按 target 精确过滤：RUST_LOG=hirust_mapper::sql=info
//! ```

use std::time::Duration;

use hirust_mapper_core::BoundSql;
use serde_json::Value;

/// 日志 target（便于用 `RUST_LOG=hirust_mapper::sql=info` 精确过滤）
pub const LOG_TARGET: &str = "hirust_mapper::sql";

/// SQL 日志配置（从 `[settings]` 解析）
#[derive(Debug, Clone, Default)]
pub struct SqlLogConfig {
    /// 是否开启 SQL 执行日志
    pub enabled: bool,
    /// 慢查询阈值（毫秒）；仅记录耗时 ≥ 此值的 SQL。`0` 表示记录全部。
    pub slow_threshold_ms: u64,
}

impl SqlLogConfig {
    /// 该次执行是否应被记录（开关开启 + 达到慢查询阈值）
    pub fn should_log(&self, elapsed: Duration) -> bool {
        if !self.enabled {
            return false;
        }
        if self.slow_threshold_ms == 0 {
            return true;
        }
        elapsed.as_millis() as u64 >= self.slow_threshold_ms
    }
}

/// 把 [`BoundSql`] 的参数内联进 `?` 占位符，生成可读的日志 SQL。
///
/// 字符串值加单引号并转义内嵌引号；数字/布尔/NULL 原样输出；参数少于占位符时保留 `?`。
///
/// XML 中多行书写的 SQL 其换行符（`\n` / `\r\n` / `\r`，单个或连续多个）会被折叠为
/// 单个空格，保证日志单行输出。
///
/// 注意：按字节扫描 `?` 做替换，若 SQL 文本中存在字面 `?`（如字符串字面量内）会被误替换，
/// 仅供日志可读性，不影响实际执行。
pub fn render_sql_for_log(bound: &BoundSql) -> String {
    let sql = bound.sql.as_str();
    let mut out = String::with_capacity(sql.len() + bound.parameters.len() * 4);
    let mut params = bound.parameters.iter();
    let mut last = 0usize;
    for (idx, _) in sql.match_indices('?') {
        out.push_str(&sql[last..idx]);
        match params.next() {
            Some(Value::Null) => out.push_str("NULL"),
            Some(Value::Bool(b)) => out.push_str(if *b { "1" } else { "0" }),
            Some(Value::Number(n)) => out.push_str(&n.to_string()),
            Some(Value::String(s)) => {
                out.push('\'');
                out.push_str(&s.replace('\'', "''"));
                out.push('\'');
            }
            Some(other) => out.push_str(&other.to_string()),
            None => out.push('?'),
        }
        last = idx + 1;
    }
    out.push_str(&sql[last..]);
    collapse_newlines(&out)
}

/// 将连续换行符（`\n` / `\r\n` / `\r`，单个或连续多个）折叠为单个空格。
///
/// 无换行时原样返回（仅一次拷贝）。也覆盖内联参数值中含换行的情况，
/// 确保整条日志始终单行。
fn collapse_newlines(s: &str) -> String {
    if !s.contains(['\n', '\r']) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for ch in s.chars() {
        if ch == '\n' || ch == '\r' {
            if !in_run {
                out.push(' ');
                in_run = true;
            }
        } else {
            out.push(ch);
            in_run = false;
        }
    }
    out
}

/// 若配置启用且达到阈值，记录一条 SQL 执行日志（耗时 + 可读 SQL）。
///
/// 成功与失败路径均会记录（耗时本身有诊断价值）。
pub fn log_execution(config: &SqlLogConfig, bound: &BoundSql, elapsed: Duration) {
    if !config.should_log(elapsed) {
        return;
    }
    let sql = render_sql_for_log(bound);
    log::info!(
        target: LOG_TARGET,
        "Consume Time: {} ms\n Execute SQL: {}",
        elapsed.as_millis(),
        sql
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bound(sql: &str, params: Vec<Value>) -> BoundSql {
        BoundSql {
            sql: sql.to_string(),
            parameters: params,
        }
    }

    #[test]
    fn test_render_inlines_params() {
        let b = bound(
            "SELECT id, name FROM users WHERE id = ? AND name = ? AND active = ?",
            vec![json!(42), json!("张三"), json!(true)],
        );
        let rendered = render_sql_for_log(&b);
        assert_eq!(
            rendered,
            "SELECT id, name FROM users WHERE id = 42 AND name = '张三' AND active = 1"
        );
    }

    #[test]
    fn test_render_null_and_float() {
        let b = bound("INSERT INTO t (a, b) VALUES (?, ?)", vec![Value::Null, json!(3.5)]);
        let rendered = render_sql_for_log(&b);
        assert_eq!(rendered, "INSERT INTO t (a, b) VALUES (NULL, 3.5)");
    }

    #[test]
    fn test_render_escapes_quote() {
        let b = bound("WHERE n = ?", vec![json!("it's")]);
        assert_eq!(render_sql_for_log(&b), "WHERE n = 'it''s'");
    }

    #[test]
    fn test_render_keeps_placeholder_when_params_short() {
        let b = bound("VALUES (?, ?, ?)", vec![json!(1)]);
        assert_eq!(render_sql_for_log(&b), "VALUES (1, ?, ?)");
    }

    #[test]
    fn test_render_collapses_newlines() {
        // XML 多行 SQL：单个换行 → 空格
        let b = bound("SELECT id\nFROM users\nWHERE id = ?", vec![json!(7)]);
        assert_eq!(render_sql_for_log(&b), "SELECT id FROM users WHERE id = 7");
    }

    #[test]
    fn test_render_collapses_consecutive_newlines() {
        // 连续多个换行 → 仍是单个空格
        let b = bound("SELECT id\n\n\n\nFROM users", vec![]);
        assert_eq!(render_sql_for_log(&b), "SELECT id FROM users");
    }

    #[test]
    fn test_render_collapses_crlf_and_cr() {
        // \r\n 与 \r 混合连续 → 单个空格
        let b = bound("SELECT a\r\n\r\nFROM t\rWHERE x = ?", vec![json!(1)]);
        assert_eq!(render_sql_for_log(&b), "SELECT a FROM t WHERE x = 1");
    }

    #[test]
    fn test_render_collapses_multiline_xml_style_sql_with_params() {
        // 贴近真实 XML 的多行动态 SQL + 参数内联，整条日志保持单行。
        // 仅折叠换行符；续行的缩进空格原样保留（\n 后跟两空格 → 空格 + 两空格）。
        let b = bound(
            "SELECT id, name FROM users\n  WHERE status = ?\n  AND id IN (?, ?)",
            vec![json!(1), json!(2), json!(3)],
        );
        assert_eq!(
            render_sql_for_log(&b),
            "SELECT id, name FROM users   WHERE status = 1   AND id IN (2, 3)"
        );
    }

    #[test]
    fn test_render_collapses_newline_in_param_value() {
        // 参数值内含换行同样折叠，保证日志单行
        let b = bound("INSERT INTO t (v) VALUES (?)", vec![json!("a\nb")]);
        assert_eq!(render_sql_for_log(&b), "INSERT INTO t (v) VALUES ('a b')");
    }

    #[test]
    fn test_collapse_newlines_fast_path_and_runs() {
        // 无换行原样返回
        assert_eq!(collapse_newlines("abc"), "abc");
        // 首尾换行同样折叠为空格
        assert_eq!(collapse_newlines("\nSELECT 1\n"), " SELECT 1 ");
        // 仅 \r
        assert_eq!(collapse_newlines("a\rb"), "a b");
    }

    #[test]
    fn test_should_log_threshold() {
        let mut cfg = SqlLogConfig { enabled: true, slow_threshold_ms: 0 };
        assert!(cfg.should_log(Duration::from_millis(1)));
        assert!(cfg.should_log(Duration::from_millis(1000)));

        cfg.slow_threshold_ms = 100;
        assert!(!cfg.should_log(Duration::from_millis(50)));
        assert!(cfg.should_log(Duration::from_millis(100)));
        assert!(cfg.should_log(Duration::from_millis(500)));

        // 开关关闭时一律不记录
        cfg.enabled = false;
        cfg.slow_threshold_ms = 0;
        assert!(!cfg.should_log(Duration::from_secs(10)));
    }
}
