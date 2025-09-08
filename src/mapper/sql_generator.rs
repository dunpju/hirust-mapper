use super::model::DynamicSqlNode;
use std::collections::HashMap;

// 辅助函数：拼接节点内容并添加适当的空格
fn join_with_spaces(nodes: &[DynamicSqlNode], params: &HashMap<String, serde_json::Value>) -> String {
    let parts: Vec<String> = nodes.iter()
        .map(|n| generate_sql(n, params))
        .filter(|s| !s.trim().is_empty())  // 过滤掉空字符串
        .map(|s|
            // 替换换行符为空格，并将连续的多个空格合并为一个
            s.replace('\n', " ")
            .replace('\r', "")
            .split_whitespace()
            .collect::<Vec<&str>>()
            .join(" "))
        .collect();

    // 对非空部分添加空格连接
    parts.join(" ")
}

pub fn generate_sql(node: &DynamicSqlNode, params: &HashMap<String, serde_json::Value>) -> String {
    match node {
        DynamicSqlNode::Text(content) => content.clone(),
        DynamicSqlNode::If { test, contents } => {
            if evaluate_condition(test, params) {
                join_with_spaces(contents, params)
            } else {
                String::new()
            }
        },
        DynamicSqlNode::Foreach { collection, item, index, open, separator, close, contents } => {
            // 实现foreach逻辑
            match params.get(collection) {
                Some(serde_json::Value::Array(items)) => {
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

                        // 临时参数，用于替换item和index
                        let mut temp_params = params.clone();
                        temp_params.insert(item.clone(), item_value.clone());

                        if let Some(index_name) = index {
                            temp_params.insert(index_name.clone(), serde_json::Value::Number(i.into()));
                        }

                        // 生成子节点SQL
                        let item_sql = join_with_spaces(contents, &temp_params);
                        result.push_str(&item_sql);
                    }

                    result.push_str(close);
                    result
                },
                _ => String::new()
            }
        },
        DynamicSqlNode::Trim { prefix, prefix_overrides, suffix, suffix_overrides, contents } => {
            let mut sql = join_with_spaces(contents, params);

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
                    return join_with_spaces(contents, params);
                }
            }

            // 如果没有when条件满足，使用otherwise
            if let Some(contents) = otherwise {
                join_with_spaces(contents, params)
            } else {
                String::new()
            }
        },
        DynamicSqlNode::Bind { name:_, value: _value } => {
            // Bind节点只是绑定变量，不生成SQL
            // 在实际应用中，这里应该将绑定的值添加到参数中
            String::new()
        },
    }
}

fn evaluate_condition(condition: &str, params: &HashMap<String, serde_json::Value>) -> bool {
    // 使用parse_helper中的KeyValue解析条件
    let kvs = super::parser::KeyValue::parse_conditions(condition).unwrap_or_default();

    // 检查所有条件是否都满足
    kvs.iter().all(|kv| {
        match params.get(&kv.key) {
            Some(value) => {
                // 处理各种比较操作符
                match kv.condition.as_str() {
                    "=" | "==" => {
                        if kv.value == "null" {
                            return false; // 不等于null
                        } else if kv.value.starts_with('\'') && kv.value.ends_with('\'') {
                            let str_value = kv.value.trim_matches('\'');
                            if let serde_json::Value::String(s) = value {
                                return s == str_value;
                            }
                        } else if let Ok(num_value) = kv.value.parse::<i64>() {
                            if let serde_json::Value::Number(n) = value {
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
                            if let serde_json::Value::String(s) = value {
                                return s != str_value;
                            }
                        } else if let Ok(num_value) = kv.value.parse::<i64>() {
                            if let serde_json::Value::Number(n) = value {
                                if let Some(n_int) = n.as_i64() {
                                    return n_int != num_value;
                                }
                            }
                        }
                        true // 默认返回true表示条件成立
                    },
                    ">" => {
                        if let Ok(num_value) = kv.value.parse::<i64>() {
                            if let serde_json::Value::Number(n) = value {
                                if let Some(n_int) = n.as_i64() {
                                    return n_int > num_value;
                                }
                            }
                        }
                        false
                    },
                    "<" => {
                        if let Ok(num_value) = kv.value.parse::<i64>() {
                            if let serde_json::Value::Number(n) = value {
                                if let Some(n_int) = n.as_i64() {
                                    return n_int < num_value;
                                }
                            }
                        }
                        false
                    },
                    ">=" => {
                        if let Ok(num_value) = kv.value.parse::<i64>() {
                            if let serde_json::Value::Number(n) = value {
                                if let Some(n_int) = n.as_i64() {
                                    return n_int >= num_value;
                                }
                            }
                        }
                        false
                    },
                    "<=" => {
                        if let Ok(num_value) = kv.value.parse::<i64>() {
                            if let serde_json::Value::Number(n) = value {
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
                kv.condition == "!=" && kv.value == "null"
            }
        }
    })
}