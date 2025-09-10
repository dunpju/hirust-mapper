use super::model::DynamicSqlNode;
use std::collections::HashMap;
use serde_json::Value;
use crate::Mapper;
use regex::Regex;
use lazy_static::lazy_static;

// 在lazy_static块中添加新的正则表达式
lazy_static! {
    static ref PARAM_REGEX: Regex = Regex::new(r#"#\{([^}]*)\}"#).unwrap();
    static ref DOLLAR_PARAM_REGEX: Regex = Regex::new(r#"\$\{([^}]*)\}"#).unwrap();
}

// 定义参数访问trait
pub trait ParamsAccess {
    // 获取单个参数值
    fn get_param(&self, key: &str) -> Option<&Value>;

    // 获取集合参数
    fn get_collection(&self, key: &str) -> Option<&Vec<Value>>;

    // 获取参数的HashMap表示（用于嵌套参数传递）
    fn as_hash_map(&self) -> Option<&HashMap<String, Value>> {
        None // 默认实现返回None
    }
}

// 为HashMap<String, Value>实现ParamsAccess
impl ParamsAccess for HashMap<String, Value> {
    fn get_param(&self, key: &str) -> Option<&Value> {
        // 支持嵌套属性访问，例如 newExamCourse.selectContainCourse
        if key.contains('.') {
            let parts: Vec<&str> = key.split('.').collect();
            // 先检查第一个属性是否存在于顶级参数中
            if let Some(first_value) = self.get(parts[0]) {
                let mut current_value = first_value;

                // 从第二个属性开始逐层查找
                for part in &parts[1..] {
                    if let Value::Object(map) = current_value {
                        if let Some(next_value) = map.get(*part) {
                            current_value = next_value;
                        } else {
                            return None; // 属性不存在
                        }
                    } else {
                        return None; // 中间层次不是对象类型
                    }
                }

                return Some(current_value);
            }
            return None; // 第一个属性不存在
        } else {
            // 保持原有的单级属性访问
            self.get(key)
        }
    }

    fn get_collection(&self, key: &str) -> Option<&Vec<Value>> {
        // 尝试从数组类型的值中获取集合
        if let Some(Value::Array(arr)) = self.get(key) {
            Some(arr)
        } else {
            None
        }
    }
    // 实现as_hash_map方法，返回自身的引用
    fn as_hash_map(&self) -> Option<&HashMap<String, Value>> {
        Some(self)
    }
}

// 为HashMap<String, Vec<Value>>实现ParamsAccess
impl ParamsAccess for HashMap<String, Vec<Value>> {
    fn get_param(&self, _key: &str) -> Option<&Value> {
        // 对于这种类型，默认不支持单个参数访问
        None
    }

    fn get_collection(&self, key: &str) -> Option<&Vec<Value>> {
        self.get(key)
    }
}

// 修改join_with_spaces辅助函数
fn join_with_spaces<P: ParamsAccess>(nodes: &[DynamicSqlNode], params: &P, mapper: &Mapper) -> String {
    let parts: Vec<String> = nodes.iter()
        .map(|n| {
            let sql = generate_sql(n, params, mapper);
            // 添加调试信息
            //println!("节点类型: {:?}, 生成的SQL: {}", n, sql);
            sql
        })
        .filter(|s| !s.trim().is_empty())  // 过滤掉空字符串
        .map(|s| {
            // 替换换行符为空格，并将连续的多个空格合并为一个
            s.replace('\n', " ")
                .replace('\r', "")
        })
        .collect();

    // 对非空部分添加空格连接，并保留SQL结构完整性
    let result = parts.join(" ");

    // 修复多余空格但保留SQL语句的逻辑结构
    result.split_whitespace().collect::<Vec<&str>>().join(" ")
}

// 改进的参数替换函数，支持两种格式参数
fn replace_parameters(content: &str, params: &impl ParamsAccess) -> String {
    // 先处理 ${...} 格式的参数 - 原样替换，不添加单引号
    let content_with_dollar_params = DOLLAR_PARAM_REGEX.replace_all(content, |caps: &regex::Captures| {
        let param_path = &caps[1];

        if let Some(value) = params.get_param(param_path) {
            match value {
                Value::String(s) => s.to_string(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => if *b { "1".to_string() } else { "0".to_string() },
                Value::Null => "NULL".to_string(),
                _ => {
                    // 对于其他类型，尝试转换为字符串表示
                    if let Ok(json_str) = serde_json::to_string(value) {
                        json_str
                    } else {
                        "NULL".to_string()
                    }
                }
            }
        } else {
            // 如果参数不存在，输出警告并返回NULL
            eprintln!("警告: 找不到参数 '{}'", param_path);
            "NULL".to_string()
        }
    }).to_string();

    // 然后处理 #{...} 格式的参数 - 添加单引号包裹
    PARAM_REGEX.replace_all(&content_with_dollar_params, |caps: &regex::Captures| {
        let param_path = &caps[1];

        if let Some(value) = params.get_param(param_path) {
            match value {
                Value::String(s) => {
                    let escaped = s.replace('\'', "''");
                    format!("'{escaped}'")
                },
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => if *b { "1".to_string() } else { "0".to_string() },
                Value::Null => "NULL".to_string(),
                _ => {
                    // 对于其他类型，尝试转换为字符串表示
                    if let Ok(json_str) = serde_json::to_string(value) {
                        format!("'{json_str}'")
                    } else {
                        "NULL".to_string()
                    }
                }
            }
        } else {
            // 如果参数不存在，输出警告并返回NULL
            eprintln!("警告: 找不到参数 '{}'", param_path);
            "NULL".to_string()
        }
    }).to_string()
}

// 生成临时参数的辅助函数
fn create_temp_params(item: &str, item_value: &Value, index: &Option<String>, index_value: usize, parent_params: &HashMap<String, Value>) -> HashMap<String, Value> {
    // 复制父参数，保留外层循环的参数
    let mut temp_params = parent_params.clone();
    // 设置当前循环的item和index参数
    temp_params.insert(item.to_string(), item_value.clone());

    if let Some(index_name) = index {
        temp_params.insert(index_name.clone(), Value::Number(index_value.into()));
    }

    temp_params
}

// 安全获取父参数的辅助函数
fn get_parent_params<P: ParamsAccess>(params: &P) -> HashMap<String, Value> {
    if let Some(map) = params.as_hash_map() {
        map.clone()
    } else {
        HashMap::new() // 无法获取父参数时使用空HashMap
    }
}

// 泛型版本的generate_sql函数
pub fn generate_sql<P: ParamsAccess>(node: &DynamicSqlNode, params: &P, mapper: &Mapper) -> String {
    match node {
        DynamicSqlNode::Text(content) => {
            // 添加参数替换逻辑
            replace_parameters(content, params)
        },
        DynamicSqlNode::If { test, contents } => {
            if evaluate_condition(test, params) {
                join_with_spaces(contents, params, mapper)
            } else {
                String::new()
            }
        },
        DynamicSqlNode::Foreach { collection, item, index, open, separator, close, contents } => {
            // 实现foreach逻辑，同时支持两种参数类型
            if let Some(items) = params.get_collection(collection) {
                if items.is_empty() {
                    return String::new();
                }

                let mut result = open.clone();
                let mut is_first = true;

                // 转换params为HashMap以便传递给create_temp_params
                let parent_params = get_parent_params(params);

                for (i, item_value) in items.iter().enumerate() {
                    if !is_first {
                        result.push_str(separator);
                    }
                    is_first = false;

                    // 创建临时参数，传递父参数
                    let temp_params = create_temp_params(item, item_value, index, i, &parent_params);

                    // 生成子节点SQL
                    let item_sql = join_with_spaces(contents, &temp_params, mapper);
                    result.push_str(&item_sql);
                }

                result.push_str(close);
                return result;
            }

            // 尝试从单个值类型参数获取（兼容旧版本）
            if let Some(Value::Array(items)) = params.get_param(collection) {
                if items.is_empty() {
                    return String::new();
                }

                let mut result = open.clone();
                let mut is_first = true;

                // 转换params为HashMap以便传递给create_temp_params
                let parent_params = get_parent_params(params);

                for (i, item_value) in items.iter().enumerate() {
                    if !is_first {
                        result.push_str(separator);
                    }
                    is_first = false;

                    // 创建临时参数，传递父参数
                    let temp_params = create_temp_params(item, item_value, index, i, &parent_params);

                    // 生成子节点SQL
                    let item_sql = join_with_spaces(contents, &temp_params, mapper);
                    result.push_str(&item_sql);
                }

                result.push_str(close);
                result
            } else {
                String::new()
            }
        },
        DynamicSqlNode::Trim { prefix, prefix_overrides, suffix, suffix_overrides, contents } => {
            let mut sql = join_with_spaces(contents, params, mapper);

            // 处理prefix_overrides
            if let Some(overrides) = prefix_overrides {
                for override_str in overrides.split(',').map(|s| s.trim()) {
                    if sql.starts_with(override_str) {
                        sql = sql[override_str.len()..].trim_start().to_string();
                        break;
                    }
                }
            }

            // 处理suffix_overrides
            if let Some(overrides) = suffix_overrides {
                for override_str in overrides.split(',').map(|s| s.trim()) {
                    if sql.ends_with(override_str) {
                        sql = sql[..sql.len() - override_str.len()].trim_end().to_string();
                        break;
                    }
                }
            }

            // 处理prefix
            if let Some(p) = prefix {
                if !sql.is_empty() {
                    // 确保prefix和sql之间有空格
                    sql = format!("{}{}{}",
                                  p.trim_end(),
                                  if !p.trim_end().is_empty() && !sql.trim_start().is_empty() { " " } else { "" },
                                  sql.trim_start()
                    );
                }
            }

            // 处理suffix
            if let Some(s) = suffix {
                if !sql.is_empty() {
                    // 确保sql和suffix之间有空格
                    sql = format!("{}{}{}",
                                  sql.trim_end(),
                                  if !sql.trim_end().is_empty() && !s.trim_start().is_empty() { " " } else { "" },
                                  s.trim_start()
                    );
                }
            }

            sql
        },
        DynamicSqlNode::Choose { whens, otherwise } => {
            // 尝试匹配第一个满足条件的when
            for (condition, contents) in whens {
                if evaluate_condition(condition, params) {
                    return join_with_spaces(contents, params, mapper);
                }
            }

            // 如果没有when条件满足，使用otherwise
            if let Some(contents) = otherwise {
                join_with_spaces(contents, params, mapper)
            } else {
                String::new()
            }
        },
        DynamicSqlNode::Bind { name:_, value: _value } => {
            // Bind节点只是绑定变量，不生成SQL
            String::new()
        },
        DynamicSqlNode::Include { ref_id } => {
            // 添加调试信息
            //println!("处理Include标签，ref_id: {}, sql_fragments: {:?}", ref_id, mapper.sql_fragments.keys());

            // 查找对应的SQL片段
            if let Some(fragment) = mapper.sql_fragments.get(ref_id) {
                //println!("找到SQL片段，内容: {:?}", fragment);

                // 直接处理SQL片段中的Text节点
                let result: String = fragment.iter()
                    .filter_map(|node| match node {
                        DynamicSqlNode::Text(text) => Some(text.clone()),
                        _ => {
                            // 对于非Text节点，使用generate_sql处理
                            let sql = generate_sql(node, params, mapper);
                            if !sql.trim().is_empty() { Some(sql) } else { None }
                        }
                    })
                    .collect::<Vec<String>>()
                    .join(" ");

                //println!("处理后的SQL片段结果: {}", result);
                result
            } else {
                //println!("警告：未找到SQL片段: {}", ref_id);
                String::new()
            }
        },
        DynamicSqlNode::Where { prefix_overrides, suffix_overrides, contents } => {
            // Where节点的处理逻辑，类似于Trim但有特定的默认值
            let mut sql = join_with_spaces(contents, params, mapper);

            // 处理prefix_overrides，默认值为"AND |OR "
            let effective_prefix_overrides = prefix_overrides.as_deref().unwrap_or("AND |OR ");
            for override_str in effective_prefix_overrides.split('|').map(|s| s.trim()) {
                if sql.starts_with(override_str) {
                    sql = sql[override_str.len()..].trim_start().to_string();
                    break;
                }
            }

            // 处理suffix_overrides
            if let Some(overrides) = suffix_overrides {
                for override_str in overrides.split(',').map(|s| s.trim()) {
                    if sql.ends_with(override_str) {
                        sql = sql[..sql.len() - override_str.len()].trim_end().to_string();
                        break;
                    }
                }
            }

            // 如果sql不为空，添加WHERE前缀
            if !sql.is_empty() {
                // 确保WHERE和sql之间有空格
                format!("WHERE {}", sql.trim_start())
            } else {
                String::new()
            }
        },
        // 添加Set节点处理逻辑
        DynamicSqlNode::Set { prefix_overrides, suffix_overrides, contents } => {
            // Set节点的处理逻辑，类似于Trim但有特定的默认值
            let mut sql = join_with_spaces(contents, params, mapper);

            // 处理prefix_overrides
            if let Some(overrides) = prefix_overrides {
                for override_str in overrides.split('|').map(|s| s.trim()) {
                    if sql.starts_with(override_str) {
                        sql = sql[override_str.len()..].trim_start().to_string();
                        break;
                    }
                }
            }

            // 处理suffix_overrides，默认值为","（去除结尾的逗号）
            let effective_suffix_overrides = suffix_overrides.as_deref().unwrap_or(",");
            for override_str in effective_suffix_overrides.split('|').map(|s| s.trim()) {
                if sql.ends_with(override_str) {
                    sql = sql[..sql.len() - override_str.len()].trim_end().to_string();
                    break;
                }
            }

            // 处理prefix，默认值为"SET"
            if !sql.is_empty() {
                sql = format!("SET {}", sql.trim_start());
            }

            sql
        },
    }
}

// 泛型版本的evaluate_condition函数
fn evaluate_condition<P: ParamsAccess>(condition: &str, params: &P) -> bool {
    // 使用parser中的KeyValue解析条件
    let kvs = super::parser::KeyValue::parse_conditions(condition).unwrap_or_default();

    // 检查所有条件是否都满足
    kvs.iter().all(|kv| {
        match params.get_param(&kv.key) {
            Some(value) => {
                // 处理各种比较操作符
                match kv.condition.as_str() {
                    "=" | "==" => {
                        if kv.value == "null" {
                            return false; // 不等于null
                        } else if kv.value.starts_with('\'') && kv.value.ends_with('\'') {
                            let str_value = kv.value.trim_matches('\'');
                            if let Value::String(s) = value {
                                return s == str_value;
                            }
                        } else if let Ok(num_value) = kv.value.parse::<i64>() {
                            if let Value::Number(n) = value {
                                if let Some(n_int) = n.as_i64() {
                                    return n_int == num_value;
                                }
                            }
                        }
                        false
                    },
                    "!=" => {
                        if kv.value == "null" {
                            return true; // 不为null
                        } else if kv.value.starts_with('\'') && kv.value.ends_with('\'') {
                            let str_value = kv.value.trim_matches('\'');
                            if let Value::String(s) = value {
                                return s != str_value;
                            }
                        } else if let Ok(num_value) = kv.value.parse::<i64>() {
                            if let Value::Number(n) = value {
                                if let Some(n_int) = n.as_i64() {
                                    return n_int != num_value;
                                }
                            }
                        }
                        true // 默认返回true表示条件成立
                    },
                    ">" => {
                        if let Ok(num_value) = kv.value.parse::<i64>() {
                            if let Value::Number(n) = value {
                                if let Some(n_int) = n.as_i64() {
                                    return n_int > num_value;
                                }
                            }
                        }
                        false
                    },
                    "<" => {
                        if let Ok(num_value) = kv.value.parse::<i64>() {
                            if let Value::Number(n) = value {
                                if let Some(n_int) = n.as_i64() {
                                    return n_int < num_value;
                                }
                            }
                        }
                        false
                    },
                    ">=" => {
                        if let Ok(num_value) = kv.value.parse::<i64>() {
                            if let Value::Number(n) = value {
                                if let Some(n_int) = n.as_i64() {
                                    return n_int >= num_value;
                                }
                            }
                        }
                        false
                    },
                    "<=" => {
                        if let Ok(num_value) = kv.value.parse::<i64>() {
                            if let Value::Number(n) = value {
                                if let Some(n_int) = n.as_i64() {
                                    return n_int <= num_value;
                                }
                            }
                        }
                        false
                    },
                    _ => false
                }
            },
            None => {
                // 参数不存在，检查是否是与null的比较
                // 参数不存在时，参数 == null 条件返回true，其他条件返回false
                kv.condition == "=" && kv.value == "null"
            }
        }
    })
}
