use super::model::DynamicSqlNode;
use super::model::MapperError;
use std::collections::HashMap;
use serde_json::Value;
use crate::Mapper;
use regex::Regex;
use lazy_static::lazy_static;

lazy_static! {
    static ref PARAM_REGEX: Regex = Regex::new(r#"#\{([^}]*)\}"#).unwrap();
    static ref DOLLAR_PARAM_REGEX: Regex = Regex::new(r#"\$\{([^}]*)\}"#).unwrap();
    static ref CONDITION_REGEX: Regex = Regex::new(r"^\s*([\w\.\(\)]+)\s*([!=<>]+)\s*(.+?)\s*$").unwrap();
}

// ─── 参数访问 trait ───────────────────────────────────────────────

/// 参数访问抽象 trait
pub trait ParamsAccess {
    /// 获取单个参数值（支持嵌套属性，如 "user.name"）
    fn get_param(&self, key: &str) -> Option<&Value>;

    /// 获取集合参数
    fn get_collection(&self, key: &str) -> Option<&Vec<Value>>;

    /// 获取参数的HashMap表示（用于嵌套参数传递）
    fn as_hash_map(&self) -> Option<&HashMap<String, Value>> {
        None
    }
}

impl ParamsAccess for HashMap<String, Value> {
    fn get_param(&self, key: &str) -> Option<&Value> {
        if key.contains('.') {
            let parts: Vec<&str> = key.split('.').collect();
            let mut current = self.get(parts[0])?;
            for part in &parts[1..] {
                current = current.get(*part)?;
            }
            Some(current)
        } else {
            self.get(key)
        }
    }

    fn get_collection(&self, key: &str) -> Option<&Vec<Value>> {
        if let Some(Value::Array(arr)) = self.get(key) {
            Some(arr)
        } else {
            None
        }
    }

    fn as_hash_map(&self) -> Option<&HashMap<String, Value>> {
        Some(self)
    }
}

// ─── 条件表达式求值 ──────────────────────────────────────────────

#[derive(Debug)]
struct KeyValue {
    key: String,
    condition: String,
    value: String,
}

/// 条件组：一组由 and 连接的条件，多个组之间用 or 连接
struct ConditionGroup {
    conditions: Vec<KeyValue>,
}

impl ConditionGroup {
    /// 解析条件表达式，支持 "and" 和 "or" 连接（and 优先级高于 or）
    fn parse(expr: &str) -> Result<Vec<Self>, MapperError> {
        let mut groups = Vec::new();

        for or_part in expr.split(" or ") {
            let trimmed_or = or_part.trim();
            if trimmed_or.is_empty() {
                continue;
            }

            let mut and_conditions = Vec::new();
            for cond in trimmed_or.split(" and ") {
                let trimmed = cond.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let caps = CONDITION_REGEX.captures(trimmed)
                    .ok_or_else(|| MapperError::InvalidCondition {
                        expr: trimmed.to_string(),
                        reason: "无法匹配 key op value 格式".to_string(),
                    })?;

                and_conditions.push(KeyValue {
                    key: caps[1].to_string(),
                    condition: caps[2].to_string(),
                    value: caps[3].to_string(),
                });
            }

            if !and_conditions.is_empty() {
                groups.push(ConditionGroup { conditions: and_conditions });
            }
        }

        Ok(groups)
    }
}

/// 评估单个比较条件
fn evaluate_single(kv: &KeyValue, params: &impl ParamsAccess) -> bool {
    match params.get_param(&kv.key) {
        Some(value) => {
            match kv.condition.as_str() {
                "=" | "==" => {
                    if kv.value == "null" {
                        return false;
                    } else if kv.value.starts_with('\'') && kv.value.ends_with('\'') {
                        let str_val = kv.value.trim_matches('\'');
                        matches!(value, Value::String(s) if s == str_val)
                    } else if let Ok(num) = kv.value.parse::<i64>() {
                        matches!(value, Value::Number(n) if n.as_i64() == Some(num))
                    } else {
                        false
                    }
                },
                "!=" => {
                    if kv.value == "null" {
                        return true;
                    } else if kv.value.starts_with('\'') && kv.value.ends_with('\'') {
                        let str_val = kv.value.trim_matches('\'');
                        matches!(value, Value::String(s) if s != str_val)
                    } else if let Ok(num) = kv.value.parse::<i64>() {
                        matches!(value, Value::Number(n) if n.as_i64() != Some(num))
                    } else {
                        false // 类型不匹配时返回false，而非true
                    }
                },
                op @ (">" | "<" | ">=" | "<=") => {
                    if let Ok(num) = kv.value.parse::<i64>() {
                        if let Some(n) = value.as_i64() {
                            return match op {
                                ">" => n > num,
                                "<" => n < num,
                                ">=" => n >= num,
                                "<=" => n <= num,
                                _ => false,
                            };
                        }
                    }
                    false
                },
                _ => false
            }
        },
        None => {
            // 参数不存在时，只有 "key == null" 为 true
            kv.condition == "=" && kv.value == "null"
        }
    }
}

fn evaluate_condition(condition: &str, params: &impl ParamsAccess) -> bool {
    let groups = ConditionGroup::parse(condition).unwrap_or_default();
    // 组间 or，组内 and
    groups.iter().any(|group| {
        group.conditions.iter().all(|kv| evaluate_single(kv, params))
    })
}

// ─── 辅助函数 ─────────────────────────────────────────────────────

fn get_parent_params<P: ParamsAccess>(params: &P) -> HashMap<String, Value> {
    params.as_hash_map().cloned().unwrap_or_default()
}

/// 创建foreach迭代时的临时参数上下文（继承父参数 + 注入 item/index）
fn create_temp_params(
    item: &str, item_value: &Value,
    index: &Option<String>, index_value: usize,
    parent_params: &HashMap<String, Value>,
) -> HashMap<String, Value> {
    let mut temp = parent_params.clone();
    temp.insert(item.to_string(), item_value.clone());
    if let Some(idx_name) = index {
        temp.insert(idx_name.clone(), Value::Number(index_value.into()));
    }
    temp
}

/// 将节点序列拼接为SQL，支持bind变量注入
fn join_with_spaces<P: ParamsAccess>(nodes: &[DynamicSqlNode], params: &P, mapper: &Mapper) -> Result<String, MapperError> {
    let mut parts = Vec::new();
    let mut enriched = get_parent_params(params);

    for n in nodes {
        match n {
            DynamicSqlNode::Bind { name, value } => {
                let resolved = replace_parameters(value, &enriched)?;
                enriched.insert(name.clone(), Value::String(resolved));
            },
            _ => {
                let sql = generate_sql(n, &enriched, mapper)?;
                if !sql.trim().is_empty() {
                    parts.push(sql.replace('\n', " ").replace('\r', ""));
                }
            },
        }
    }

    let result = parts.join(" ");
    // 合并连续空白
    Ok(result.split_whitespace().collect::<Vec<&str>>().join(" "))
}

/// 替换 #{...} 和 ${...} 占位符
fn replace_parameters(content: &str, params: &impl ParamsAccess) -> Result<String, MapperError> {
    // 先处理 ${...} — 原样替换，不加引号
    let with_dollar = DOLLAR_PARAM_REGEX.replace_all(content, |caps: &regex::Captures| {
        let path = &caps[1];
        match params.get_param(path) {
            Some(Value::String(s)) => s.to_string(),
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::Bool(b)) => if *b { "1".to_string() } else { "0".to_string() },
            Some(Value::Null) => "NULL".to_string(),
            Some(v) => serde_json::to_string(v).unwrap_or_else(|_| "NULL".to_string()),
            None => return format!("/* MISSING:${} */", path),
        }
    }).to_string();

    // 再处理 #{...} — 字符串加引号
    Ok(PARAM_REGEX.replace_all(&with_dollar, |caps: &regex::Captures| {
        let path = &caps[1];
        match params.get_param(path) {
            Some(Value::String(s)) => {
                let escaped = s.replace('\'', "''");
                format!("'{escaped}'")
            },
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::Bool(b)) => if *b { "1".to_string() } else { "0".to_string() },
            Some(Value::Null) => "NULL".to_string(),
            Some(v) => {
                let json = serde_json::to_string(v).unwrap_or_else(|_| "NULL".to_string());
                format!("'{json}'")
            },
            None => return format!("/* MISSING:#{} */", path),
        }
    }).to_string())
}

/// 去除SQL前缀/后缀的公共逻辑
fn strip_overrides(sql: &str, overrides: Option<&str>, default: Option<&str>, strip_prefix: bool) -> String {
    let effective = overrides.or(default).unwrap_or("");
    let mut result = sql.to_string();

    for part in effective.split('|').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if strip_prefix && result.starts_with(part) {
            result = result[part.len()..].trim_start().to_string();
            break;
        }
        if !strip_prefix && result.ends_with(part) {
            result = result[..result.len() - part.len()].trim_end().to_string();
            break;
        }
    }

    result
}

// ─── 核心 SQL 生成 ────────────────────────────────────────────────

pub fn generate_sql<P: ParamsAccess>(node: &DynamicSqlNode, params: &P, mapper: &Mapper) -> Result<String, MapperError> {
    match node {
        DynamicSqlNode::Text(content) => replace_parameters(content, params),

        DynamicSqlNode::If { test, contents } => {
            if evaluate_condition(test, params) {
                join_with_spaces(contents, params, mapper)
            } else {
                Ok(String::new())
            }
        },

        DynamicSqlNode::Foreach { collection, item, index, open, separator, close, contents } => {
            let items = params.get_collection(collection)
                .or_else(|| {
                    params.get_param(collection).and_then(|v| {
                        if let Value::Array(arr) = v { Some(arr) } else { None }
                    })
                });

            let items = match items {
                Some(arr) if !arr.is_empty() => arr,
                _ => return Ok(String::new()),
            };

            let parent = get_parent_params(params);
            let mut result = open.clone();

            for (i, item_val) in items.iter().enumerate() {
                if i > 0 {
                    result.push_str(separator);
                }
                let temp = create_temp_params(item, item_val, index, i, &parent);
                result.push_str(&join_with_spaces(contents, &temp, mapper)?);
            }

            result.push_str(close);
            Ok(result)
        },

        DynamicSqlNode::Trim { prefix, prefix_overrides, suffix, suffix_overrides, contents } => {
            let mut sql = join_with_spaces(contents, params, mapper)?;
            sql = strip_overrides(&sql, prefix_overrides.as_deref(), None, true);
            sql = strip_overrides(&sql, suffix_overrides.as_deref(), None, false);

            if let Some(p) = prefix {
                if !sql.is_empty() && !p.trim_end().is_empty() {
                    sql = format!("{} {}", p.trim_end(), sql.trim_start());
                }
            }
            if let Some(s) = suffix {
                if !sql.is_empty() && !s.trim_start().is_empty() {
                    sql = format!("{} {}", sql.trim_end(), s.trim_start());
                }
            }

            Ok(sql)
        },

        DynamicSqlNode::Choose { whens, otherwise } => {
            for (condition, contents) in whens {
                if evaluate_condition(condition, params) {
                    return join_with_spaces(contents, params, mapper);
                }
            }
            match otherwise {
                Some(contents) => join_with_spaces(contents, params, mapper),
                None => Ok(String::new()),
            }
        },

        DynamicSqlNode::Bind { .. } => Ok(String::new()), // 在 join_with_spaces 中处理

        DynamicSqlNode::Include { ref_id } => {
            // 支持 namespace.id 跨文件引用格式
            let fragment = if ref_id.contains('.') {
                let (_ns, id) = ref_id.split_once('.')
                    .ok_or_else(|| MapperError::MissingFragment { ref_id: ref_id.clone() })?;
                // 当前仅支持同 mapper 内查找（跨 mapper 需要外部注册）
                mapper.sql_fragments.get(id)
                    .ok_or_else(|| MapperError::MissingFragment { ref_id: ref_id.clone() })?
            } else {
                mapper.sql_fragments.get(ref_id)
                    .ok_or_else(|| MapperError::MissingFragment { ref_id: ref_id.clone() })?
            };

            let parts: Vec<String> = fragment.iter()
                .map(|node| generate_sql(node, params, mapper))
                .filter(|s| s.as_ref().map(|sql| !sql.trim().is_empty()).unwrap_or(false))
                .collect::<Result<Vec<_>, _>>()?;

            Ok(parts.join(" "))
        },

        DynamicSqlNode::Where { prefix_overrides, suffix_overrides, contents } => {
            let sql = join_with_spaces(contents, params, mapper)?;
            let sql = strip_overrides(&sql, prefix_overrides.as_deref(), Some("AND |OR "), true);
            let sql = strip_overrides(&sql, suffix_overrides.as_deref(), None, false);

            if sql.is_empty() {
                Ok(String::new())
            } else {
                Ok(format!("WHERE {}", sql.trim_start()))
            }
        },

        DynamicSqlNode::Set { prefix_overrides, suffix_overrides, contents } => {
            let sql = join_with_spaces(contents, params, mapper)?;
            let sql = strip_overrides(&sql, prefix_overrides.as_deref(), None, true);
            let sql = strip_overrides(&sql, suffix_overrides.as_deref(), Some(","), false);

            if sql.is_empty() {
                Ok(String::new())
            } else {
                Ok(format!("SET {}", sql.trim_start()))
            }
        },

        DynamicSqlNode::Mixed { contents } => {
            join_with_spaces(contents, params, mapper)
        },
    }
}

// ─── 高层便捷 API ─────────────────────────────────────────────────

impl Mapper {
    /// 一站式 SQL 生成：按 statement id 查找并生成最终 SQL
    ///
    /// # 示例
    /// ```ignore
    /// let mut parser = MyBatisXmlParser::new(xml);
    /// let mapper = parser.parse_mapper()?;
    /// let mut params = HashMap::new();
    /// params.insert("id".to_string(), Value::Number(1.into()));
    /// let sql = mapper.build_sql("findUserById", &params)?;
    /// ```
    pub fn build_sql(&self, statement_id: &str, params: &HashMap<String, Value>) -> Result<String, MapperError> {
        let stmt = self.statements.get(statement_id)
            .ok_or_else(|| MapperError::StatementNotFound { id: statement_id.to_string() })?;

        match &stmt.dynamic_sql {
            Some(node) => generate_sql(node, params, self),
            None => {
                // 纯静态SQL，仅做参数替换
                replace_parameters(&stmt.sql, params)
            }
        }
    }
}
