use super::model::DynamicSqlNode;
use super::model::MapperError;
use std::collections::HashMap;
use std::sync::RwLock;
use serde_json::Value;
use crate::Mapper;
use regex::Regex;
use lazy_static::lazy_static;

lazy_static! {
    static ref PARAM_REGEX: Regex = Regex::new(r#"#\{([^}]*)\}"#).unwrap();
    static ref DOLLAR_PARAM_REGEX: Regex = Regex::new(r#"\$\{([^}]*)\}"#).unwrap();
    static ref CONDITION_REGEX: Regex = Regex::new(r"^\s*([\w\.\(\)]+)\s*([!=<>]+)\s*(.+?)\s*$").unwrap();
    /// <if>/<when> 的 test 表达式预编译缓存（test 文本 → 解析结果）。
    ///
    /// test 文本来自静态 XML，集合有限且稳定，故用全局缓存按需解析一次后复用，
    /// 避免每次查询对同一表达式重复 regex 解析与 String 分配。读多写少，读路径并发。
    static ref CONDITION_CACHE: RwLock<HashMap<String, Vec<ConditionGroup>>> = RwLock::new(HashMap::new());
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
            let mut iter = key.split('.');
            let mut current = self.get(iter.next()?)?;
            for part in iter {
                current = current.get(part)?;
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

#[derive(Debug, Clone)]
struct KeyValue {
    key: String,
    condition: String,
    value: String,
}

/// 条件组：一组由 and 连接的条件，多个组之间用 or 连接
#[derive(Clone)]
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

/// 解析参数键的值，支持 `.size()` / `.isEmpty()` 方法调用
///
/// - `key.size()` → 集合/字符串/对象的长度（Number）；缺失或 null → 0
/// - `key.isEmpty()` → 是否为空（Bool）；缺失或 null → true
/// - 其他 → 原始参数值
fn resolve_param(key: &str, params: &impl ParamsAccess) -> Option<Value> {
    if let Some(base) = key.strip_suffix(".size()") {
        match params.get_param(base) {
            Some(Value::Array(a)) => Some(Value::Number(a.len().into())),
            Some(Value::String(s)) => Some(Value::Number(s.chars().count().into())),
            Some(Value::Object(o)) => Some(Value::Number(o.len().into())),
            Some(Value::Null) | None => Some(Value::Number(0.into())),
            Some(other) => Some(other.clone()),
        }
    } else if let Some(base) = key.strip_suffix(".isEmpty()") {
        match params.get_param(base) {
            Some(Value::Array(a)) => Some(Value::Bool(a.is_empty())),
            Some(Value::String(s)) => Some(Value::Bool(s.is_empty())),
            Some(Value::Object(o)) => Some(Value::Bool(o.is_empty())),
            Some(Value::Null) | None => Some(Value::Bool(true)),
            Some(_) => Some(Value::Bool(false)),
        }
    } else {
        params.get_param(key).cloned()
    }
}

/// 评估单个比较条件
fn evaluate_single(kv: &KeyValue, params: &impl ParamsAccess) -> bool {
    match resolve_param(&kv.key, params) {
        Some(value) => {
            // 布尔字面量判定辅助
            let value_bool = if kv.value.eq_ignore_ascii_case("true") {
                Some(true)
            } else if kv.value.eq_ignore_ascii_case("false") {
                Some(false)
            } else {
                None
            };

            match kv.condition.as_str() {
                "=" | "==" => {
                    if kv.value == "null" {
                        return false;
                    } else if let Some(b) = value_bool {
                        matches!(value, Value::Bool(x) if x == b)
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
                    } else if let Some(b) = value_bool {
                        matches!(value, Value::Bool(x) if x != b)
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
                    // 支持 i64 / f64 数值比较
                    let target = kv.value.parse::<f64>().ok();
                    if let Some(num) = target {
                        let cur = value.as_f64();
                        if let Some(n) = cur {
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

/// 对已解析的条件组求值（组间 or，组内 and）
fn eval_groups(groups: &[ConditionGroup], params: &impl ParamsAccess) -> bool {
    groups.iter().any(|group| {
        group.conditions.iter().all(|kv| evaluate_single(kv, params))
    })
}

fn evaluate_condition(condition: &str, params: &impl ParamsAccess) -> bool {
    // 快路径：缓存命中（读锁，并发友好）
    {
        let cache = CONDITION_CACHE.read().unwrap();
        if let Some(groups) = cache.get(condition) {
            return eval_groups(groups, params);
        }
    }
    // 慢路径：首次解析（失败回退空 → 恒 false，保持旧行为）并写入缓存。
    // 并发 miss 至多重复解析一次（or_insert 幂等），无害。
    let groups = ConditionGroup::parse(condition).unwrap_or_default();
    if let Ok(mut cache) = CONDITION_CACHE.write() {
        cache.entry(condition.to_string()).or_insert(groups.clone());
    }
    eval_groups(&groups, params)
}

// ─── 辅助函数 ─────────────────────────────────────────────────────

fn get_parent_params<P: ParamsAccess>(params: &P) -> HashMap<String, Value> {
    params.as_hash_map().cloned().unwrap_or_default()
}

/// 将任意空白（含 `\n` `\r` 连续空格）归一化为单词间单个空格，无中间 `Vec`
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut first = true;
    for w in s.split_whitespace() {
        if !first {
            out.push(' ');
        }
        out.push_str(w);
        first = false;
    }
    out
}

/// 将节点序列拼接为SQL，支持bind变量注入
fn join_with_spaces<P: ParamsAccess>(nodes: &[DynamicSqlNode], params: &P, mapper: &Mapper) -> Result<String, MapperError> {
    let mut raw = String::new();
    // 仅在遇到 <bind> 时才克隆父参数表（绝大多数语句无 bind，零克隆）
    let mut enriched: Option<HashMap<String, Value>> = None;

    for n in nodes {
        match n {
            DynamicSqlNode::Bind { name, value } => {
                let map = enriched.get_or_insert_with(|| get_parent_params(params));
                let resolved = replace_parameters(value, map)?;
                map.insert(name.clone(), Value::String(resolved));
            },
            _ => {
                let sql = match &enriched {
                    Some(map) => generate_sql(n, map, mapper)?,
                    None => generate_sql(n, params, mapper)?,
                };
                if !sql.trim().is_empty() {
                    if !raw.is_empty() {
                        raw.push(' ');
                    }
                    raw.push_str(&sql);
                }
            },
        }
    }

    Ok(collapse_whitespace(&raw))
}

/// 替换 #{...} 和 ${...} 占位符
fn replace_parameters(content: &str, params: &impl ParamsAccess) -> Result<String, MapperError> {
    // 短路：无占位符的字面文本直接返回，跳过双趟 regex 与分配
    if !content.contains("#{") && !content.contains("${") {
        return Ok(content.to_string());
    }
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
///
/// 取得 `sql` 的所有权：未命中任何 override 时原样返回（零分配），命中时才重组。
fn strip_overrides(sql: String, overrides: Option<&str>, default: Option<&str>, strip_prefix: bool) -> String {
    let effective = overrides.or(default).unwrap_or("");
    if effective.is_empty() {
        return sql;
    }

    for part in effective.split('|').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if strip_prefix {
            if let Some(rest) = sql.strip_prefix(part) {
                return rest.trim_start().to_string();
            }
        } else if let Some(rest) = sql.strip_suffix(part) {
            return rest.trim_end().to_string();
        }
    }
    sql
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

            // 克隆一次父参数表，循环内仅覆盖 item/index（避免每元素全表克隆）
            let mut temp = get_parent_params(params);
            let mut result = String::with_capacity(open.len() + close.len());
            result.push_str(open);

            for (i, item_val) in items.iter().enumerate() {
                if i > 0 {
                    result.push_str(separator);
                }
                temp.insert(item.clone(), item_val.clone());
                if let Some(idx_name) = index {
                    temp.insert(idx_name.clone(), Value::Number(i.into()));
                }
                result.push_str(&join_with_spaces(contents, &temp, mapper)?);
            }

            result.push_str(close);
            Ok(result)
        },

        DynamicSqlNode::Trim { prefix, prefix_overrides, suffix, suffix_overrides, contents } => {
            let mut sql = join_with_spaces(contents, params, mapper)?;
            sql = strip_overrides(sql,prefix_overrides.as_deref(), None, true);
            sql = strip_overrides(sql,suffix_overrides.as_deref(), None, false);

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

            let mut result = String::new();
            for node in fragment.iter() {
                let sql = generate_sql(node, params, mapper)?;
                if sql.trim().is_empty() {
                    continue;
                }
                if !result.is_empty() {
                    result.push(' ');
                }
                result.push_str(&sql);
            }
            Ok(result)
        },

        DynamicSqlNode::Where { prefix_overrides, suffix_overrides, contents } => {
            let sql = join_with_spaces(contents, params, mapper)?;
            let sql = strip_overrides(sql,prefix_overrides.as_deref(), Some("AND |OR "), true);
            let sql = strip_overrides(sql,suffix_overrides.as_deref(), None, false);

            if sql.is_empty() {
                Ok(String::new())
            } else {
                Ok(format!("WHERE {}", sql.trim_start()))
            }
        },

        DynamicSqlNode::Set { prefix_overrides, suffix_overrides, contents } => {
            let sql = join_with_spaces(contents, params, mapper)?;
            let sql = strip_overrides(sql,prefix_overrides.as_deref(), None, true);
            let sql = strip_overrides(sql,suffix_overrides.as_deref(), Some(","), false);

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
    /// 一站式 SQL 生成：按 statement id 查找并生成最终 SQL（内联模式）
    ///
    /// 所有 `#{param}` / `${param}` 都直接内联到 SQL 字符串中（字符串值自动加引号）。
    /// 这是传统的内联生成方式，向后兼容。
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

    /// 两阶段绑定：按 statement id 查找并生成 [`BoundSql`]（参数化模式）
    ///
    /// 与 [`build_sql`](Self::build_sql) 的区别：
    /// - `#{param}` → 替换为 `?` 占位符，参数值按出现顺序进入 [`BoundSql::parameters`]
    /// - `${param}` → 原样内联（无法参数化，保持原行为）
    ///
    /// 当 SQL 同时包含 `?` 占位符与 `${}` 内联值时即为「混合模式」（自动发生，
    /// 无需额外标记）。建议优先使用本方法以获得参数化查询的安全性（防 SQL 注入）。
    pub fn build_bound_sql(&self, statement_id: &str, params: &HashMap<String, Value>) -> Result<BoundSql, MapperError> {
        let stmt = self.statements.get(statement_id)
            .ok_or_else(|| MapperError::StatementNotFound { id: statement_id.to_string() })?;

        match &stmt.dynamic_sql {
            Some(node) => generate_bound_sql(node, params, self),
            None => replace_parameters_bound(&stmt.sql, params),
        }
    }
}

// ─── BoundSql 两阶段绑定（Phase 2）──────────────────────────────────

/// 绑定后的 SQL：含 `?` 占位符的 SQL 字符串 + 有序参数列表
///
/// 对应两阶段解析的 Phase 2（绑定阶段）输出。`?` 占位符与 `parameters`
/// 一一对应（按出现顺序），可直接绑定到数据库驱动（如 sqlx）执行。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BoundSql {
    /// 含 `?` 占位符的 SQL（`${}` 内联部分保持原样）
    pub sql: String,
    /// 有序参数列表，与 SQL 中 `?` 占位符一一对应（按出现顺序）
    pub parameters: Vec<Value>,
}

impl BoundSql {
    /// 创建一个带初始 SQL 的 BoundSql（参数列表为空）
    pub fn new(sql: String) -> Self {
        Self { sql, parameters: Vec::new() }
    }

    /// 参数数量
    pub fn param_count(&self) -> usize {
        self.parameters.len()
    }

    /// 是否包含参数
    pub fn has_params(&self) -> bool {
        !self.parameters.is_empty()
    }
}

/// 替换 `#{...}` 为 `?` 占位符（参数进列表），`${...}` 原样内联
///
/// 这是 [`replace_parameters`] 的「绑定版本」：`#{}` 不再内联值，
/// 而是输出 `?` 并将值收集到 [`BoundSql::parameters`]。
fn replace_parameters_bound(content: &str, params: &impl ParamsAccess) -> Result<BoundSql, MapperError> {
    // 短路：无占位符的字面文本直接返回（零参数）
    if !content.contains("#{") && !content.contains("${") {
        return Ok(BoundSql::new(content.to_string()));
    }
    // 先处理 ${...} — 原样内联（无法参数化）
    let with_dollar = DOLLAR_PARAM_REGEX.replace_all(content, |caps: &regex::Captures| {
        let path = &caps[1];
        match params.get_param(path) {
            Some(Value::String(s)) => s.to_string(),
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::Bool(b)) => if *b { "1".to_string() } else { "0".to_string() },
            Some(Value::Null) => "NULL".to_string(),
            Some(v) => serde_json::to_string(v).unwrap_or_else(|_| "NULL".to_string()),
            None => format!("/* MISSING:${} */", path),
        }
    }).to_string();

    // 再处理 #{...} — 替换为 ? 占位符 + 参数进列表（按出现顺序）
    let mut parameters = Vec::new();
    let sql = PARAM_REGEX.replace_all(&with_dollar, |caps: &regex::Captures| {
        let path = &caps[1];
        match params.get_param(path) {
            Some(value) => {
                parameters.push(value.clone());
                "?".to_string()
            },
            None => format!("/* MISSING:#{} */", path),
        }
    }).to_string();

    Ok(BoundSql { sql, parameters })
}

/// 将节点序列拼接为 [`BoundSql`]，支持 bind 变量注入
///
/// [`join_with_spaces`] 的「绑定版本」：在拼接 SQL 文本的同时保持参数顺序。
fn join_with_spaces_bound<P: ParamsAccess>(
    nodes: &[DynamicSqlNode],
    params: &P,
    mapper: &Mapper,
) -> Result<BoundSql, MapperError> {
    let mut result = BoundSql::new(String::new());
    // 仅在遇到 <bind> 时才克隆父参数表
    let mut enriched: Option<HashMap<String, Value>> = None;

    for n in nodes {
        match n {
            DynamicSqlNode::Bind { name, value } => {
                let map = enriched.get_or_insert_with(|| get_parent_params(params));
                let resolved = replace_parameters(value, map)?;
                map.insert(name.clone(), Value::String(resolved));
            },
            _ => {
                let child = match &enriched {
                    Some(map) => generate_bound_sql(n, map, mapper)?,
                    None => generate_bound_sql(n, params, mapper)?,
                };
                if !child.sql.trim().is_empty() {
                    if !result.sql.is_empty() {
                        result.sql.push(' ');
                    }
                    result.sql.push_str(&child.sql);
                    result.parameters.extend(child.parameters);
                }
            },
        }
    }

    // 合并连续空白（不影响 ? 与参数的对应关系）
    result.sql = collapse_whitespace(&result.sql);
    Ok(result)
}

/// 生成 [`BoundSql`]（两阶段绑定的 Phase 2）
///
/// 与 [`generate_sql`] 结构完全一致，区别仅在于叶子节点（`Text`）的参数处理：
/// - `#{param}` → `?` 占位符 + 参数进入 [`BoundSql::parameters`]
/// - `${param}` → 原样内联
///
/// 结构节点（if/foreach/trim/where/set/choose/include/bind）的求值逻辑与
/// [`generate_sql`] 完全相同，仅累积目标从 `String` 变为 `BoundSql`。
pub fn generate_bound_sql<P: ParamsAccess>(
    node: &DynamicSqlNode,
    params: &P,
    mapper: &Mapper,
) -> Result<BoundSql, MapperError> {
    match node {
        DynamicSqlNode::Text(content) => replace_parameters_bound(content, params),

        DynamicSqlNode::If { test, contents } => {
            if evaluate_condition(test, params) {
                join_with_spaces_bound(contents, params, mapper)
            } else {
                Ok(BoundSql::new(String::new()))
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
                _ => return Ok(BoundSql::new(String::new())),
            };

            // 克隆一次父参数表，循环内仅覆盖 item/index（避免每元素全表克隆）
            let mut temp = get_parent_params(params);
            let mut result = BoundSql::new(open.clone());

            for (i, item_val) in items.iter().enumerate() {
                if i > 0 {
                    result.sql.push_str(separator);
                }
                temp.insert(item.clone(), item_val.clone());
                if let Some(idx_name) = index {
                    temp.insert(idx_name.clone(), Value::Number(i.into()));
                }
                let child = join_with_spaces_bound(contents, &temp, mapper)?;
                result.sql.push_str(&child.sql);
                result.parameters.extend(child.parameters);
            }

            result.sql.push_str(close);
            Ok(result)
        },

        DynamicSqlNode::Trim { prefix, prefix_overrides, suffix, suffix_overrides, contents } => {
            let mut bound = join_with_spaces_bound(contents, params, mapper)?;
            bound.sql = strip_overrides(bound.sql, prefix_overrides.as_deref(), None, true);
            bound.sql = strip_overrides(bound.sql, suffix_overrides.as_deref(), None, false);

            if let Some(p) = prefix {
                if !bound.sql.is_empty() && !p.trim_end().is_empty() {
                    bound.sql = format!("{} {}", p.trim_end(), bound.sql.trim_start());
                }
            }
            if let Some(s) = suffix {
                if !bound.sql.is_empty() && !s.trim_start().is_empty() {
                    bound.sql = format!("{} {}", bound.sql.trim_end(), s.trim_start());
                }
            }

            Ok(bound)
        },

        DynamicSqlNode::Choose { whens, otherwise } => {
            for (condition, contents) in whens {
                if evaluate_condition(condition, params) {
                    return join_with_spaces_bound(contents, params, mapper);
                }
            }
            match otherwise {
                Some(contents) => join_with_spaces_bound(contents, params, mapper),
                None => Ok(BoundSql::new(String::new())),
            }
        },

        DynamicSqlNode::Bind { .. } => Ok(BoundSql::new(String::new())),

        DynamicSqlNode::Include { ref_id } => {
            let fragment = if ref_id.contains('.') {
                let (_ns, id) = ref_id.split_once('.')
                    .ok_or_else(|| MapperError::MissingFragment { ref_id: ref_id.clone() })?;
                mapper.sql_fragments.get(id)
                    .ok_or_else(|| MapperError::MissingFragment { ref_id: ref_id.clone() })?
            } else {
                mapper.sql_fragments.get(ref_id)
                    .ok_or_else(|| MapperError::MissingFragment { ref_id: ref_id.clone() })?
            };

            let parts: Vec<BoundSql> = fragment.iter()
                .map(|node| generate_bound_sql(node, params, mapper))
                .collect::<Result<Vec<_>, _>>()?;

            let mut result = BoundSql::new(String::new());
            for part in parts {
                if part.sql.trim().is_empty() { continue; }
                if !result.sql.is_empty() {
                    result.sql.push(' ');
                }
                result.sql.push_str(&part.sql);
                result.parameters.extend(part.parameters);
            }
            Ok(result)
        },

        DynamicSqlNode::Where { prefix_overrides, suffix_overrides, contents } => {
            let mut bound = join_with_spaces_bound(contents, params, mapper)?;
            bound.sql = strip_overrides(bound.sql, prefix_overrides.as_deref(), Some("AND |OR "), true);
            bound.sql = strip_overrides(bound.sql, suffix_overrides.as_deref(), None, false);

            if bound.sql.is_empty() {
                Ok(bound)
            } else {
                bound.sql = format!("WHERE {}", bound.sql.trim_start());
                Ok(bound)
            }
        },

        DynamicSqlNode::Set { prefix_overrides, suffix_overrides, contents } => {
            let mut bound = join_with_spaces_bound(contents, params, mapper)?;
            bound.sql = strip_overrides(bound.sql, prefix_overrides.as_deref(), None, true);
            bound.sql = strip_overrides(bound.sql, suffix_overrides.as_deref(), Some(","), false);

            if bound.sql.is_empty() {
                Ok(bound)
            } else {
                bound.sql = format!("SET {}", bound.sql.trim_start());
                Ok(bound)
            }
        },

        DynamicSqlNode::Mixed { contents } => {
            join_with_spaces_bound(contents, params, mapper)
        },
    }
}
