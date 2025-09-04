use super::model::DynamicSqlNode;
use std::collections::HashMap;

pub fn generate_sql(node: &DynamicSqlNode, params: &HashMap<String, serde_json::Value>) -> String {
    match node {
        DynamicSqlNode::Text(content) => content.clone(),
        DynamicSqlNode::If { test, contents } => {
            if evaluate_condition(test, params) {
                contents.iter()
                    .map(|n| generate_sql(n, params))
                    .collect()
            } else {
                String::new()
            }
        },
        DynamicSqlNode::Foreach { collection, item, index, open, separator, close, contents } => {
            // 实现foreach逻辑
            let mut result = open.clone();
            // ... 遍历集合并生成SQL ...
            result
        },
        // ... 处理其他动态节点 ...
        _ => String::new()
    }
}

fn evaluate_condition(condition: &str, params: &HashMap<String, serde_json::Value>) -> bool {
    // 使用parse_helper中的KeyValue解析条件
    let kvs = super::parse_helper::KeyValue::parse_conditions(condition).unwrap_or_default();
    kvs.iter().all(|kv| {
        // 实现条件评估逻辑
        match params.get(&kv.key) {
            // ... 根据参数值判断条件是否成立 ...
            _ => false
        }
    })
}
