use super::model::DynamicSqlNode;
use std::collections::HashMap;
use serde_json::Value;
use crate::Mapper;

// 定义参数访问trait
pub trait ParamsAccess {
    // 获取单个参数值
    fn get_param(&self, key: &str) -> Option<&Value>;

    // 获取集合参数
    fn get_collection(&self, key: &str) -> Option<&Vec<Value>>;
}

// 为HashMap<String, Value>实现ParamsAccess
impl ParamsAccess for HashMap<String, Value> {
    fn get_param(&self, key: &str) -> Option<&Value> {
        self.get(key)
    }

    fn get_collection(&self, key: &str) -> Option<&Vec<Value>> {
        // 尝试从数组类型的值中获取集合
        if let Some(Value::Array(arr)) = self.get(key) {
            Some(arr)
        } else {
            None
        }
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
            println!("节点类型: {:?}, 生成的SQL: {}", n, sql);
            sql
        })
        .filter(|s| !s.trim().is_empty())  // 过滤掉空字符串
        .map(|s| {
            // 替换换行符为空格，并将连续的多个空格合并为一个
            s.replace('\n', " ")
                .replace('\r', "")
                .split_whitespace()
                .collect::<Vec<&str>>()
                .join(" ")
        })
        .collect();

    // 对非空部分添加空格连接
    parts.join(" ")
}

// 生成临时参数的辅助函数
fn create_temp_params(item: &str, item_value: &Value, index: &Option<String>, index_value: usize) -> HashMap<String, Value> {
    let mut temp_params = HashMap::new();
    temp_params.insert(item.to_string(), item_value.clone());

    if let Some(index_name) = index {
        temp_params.insert(index_name.clone(), Value::Number(index_value.into()));
    }

    temp_params
}

// 泛型版本的generate_sql函数
pub fn generate_sql<P: ParamsAccess>(node: &DynamicSqlNode, params: &P, mapper: &Mapper) -> String {
    match node {
        DynamicSqlNode::Text(content) => content.clone(),
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

                for (i, item_value) in items.iter().enumerate() {
                    if !is_first {
                        result.push_str(separator);
                    }
                    is_first = false;

                    // 创建临时参数
                    let temp_params = create_temp_params(item, item_value, index, i);

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

                for (i, item_value) in items.iter().enumerate() {
                    if !is_first {
                        result.push_str(separator);
                    }
                    is_first = false;

                    // 创建临时参数
                    let temp_params = create_temp_params(item, item_value, index, i);

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
            println!("处理Include标签，ref_id: {}, sql_fragments: {:?}",
                     ref_id, mapper.sql_fragments.keys());

            // 查找对应的SQL片段
            if let Some(fragment) = mapper.sql_fragments.get(ref_id) {
                println!("找到SQL片段，内容: {:?}", fragment);

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

                println!("处理后的SQL片段结果: {}", result);
                result
            } else {
                println!("警告：未找到SQL片段: {}", ref_id);
                String::new()
            }
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
