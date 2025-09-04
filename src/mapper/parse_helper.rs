use log::warn;
use regex::Regex;
use std::{collections::HashMap, process, ptr, sync::atomic::{AtomicBool, Ordering}};
use std::sync::Mutex;
use xml::attribute::OwnedAttribute;

static REGEX_INC_MAP_INIT: AtomicBool = AtomicBool::new(false);

static MTX_VARS: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);

pub fn fetch_global_var_mut<T>(key: &str) -> Result<&'static mut T, String> {
    if let Ok(guard) = MTX_VARS.lock() {
        if guard.is_none() {
            Err("Failed to lock mutex".to_string())
        } else if let Some(boxed_ptr) = guard.as_ref().unwrap().get(key) {
            Ok(unsafe { &mut *(*boxed_ptr as *mut T) })
        } else {
            Err("Failed to find key".to_string())
        }
    } else {
        Err("Failed to lock mutex".to_string())
    }
}

pub fn init_global_var<T>(key: &str, val: T) {
    if let Ok(mut guard) = MTX_VARS.lock() {
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        let boxed = Box::new(val);
        let boxed_ptr = Box::leak(boxed);
        guard
            .as_mut()
            .unwrap()
            .insert(key.to_string(), ptr::addr_of_mut!(*boxed_ptr) as u64);
    }
}

/// 替换 `include`，用对应的 `sql` 进行合并
pub fn replace_included_sql(orig_sql: &str, id: &str, sql_part: &str) -> String {
    let rx = gen_regex_by_id(id);
    let replaced = sql_part;
    rx.replace_all(orig_sql, replaced).to_string()
}

fn gen_regex_by_id(id: &str) -> Regex {
    if !REGEX_INC_MAP_INIT.load(Ordering::SeqCst) {
        init_global_var::<HashMap<String, Regex>>("regex_inc_map", HashMap::new());
        REGEX_INC_MAP_INIT.store(true, Ordering::SeqCst);
    }
    let regex_inc_map = fetch_global_var_mut::<HashMap<String, Regex>>("regex_inc_map").unwrap();
    let replace_target = format!("{}{}{}", "__INCLUDE_ID_", id, "_END__");
    regex_inc_map
        .entry(replace_target.clone())
        .or_insert_with(|| {
            Regex::new(replace_target.as_str()).unwrap_or_else(|e| {
                warn!("build regex[{replace_target}] failed: {e}");
                process::exit(-1);
            })
        })
        .clone()
}

/// 检索属性，匹配情况下回调闭包
pub fn search_matched_attr(
    attributes: &[OwnedAttribute],
    matched_name: &str,
    mut f: impl FnMut(&OwnedAttribute),
) {
    for attr in attributes {
        if attr.name.local_name.as_str() == matched_name {
            f(attr);
            break;
        }
    }
}

/// 是否匹配语句块
pub fn match_statement(element_name: &String) -> bool {
    *element_name == "statement"
        || *element_name == "select"
        || *element_name == "insert"
        || *element_name == "update"
        || *element_name == "delete"
        || *element_name == "sql"
}

#[derive(Debug)]
pub(crate) struct KeyValue {
    pub key: String,
    pub condition: String,
    pub value: String,
}

impl KeyValue {
    /// 解析条件表达式为KeyValue向量
    pub fn parse_conditions(expr: &str) -> Result<Vec<Self>, String> {
        let mut conditions = Vec::new();
        // 按'and'分割多个条件
        for cond in expr.split(" and ") {
            let trimmed = cond.trim();
            // 使用正则表达式匹配key、condition和value
            let re = regex::Regex::new(r"^\s*([\w\.\(\)]+)\s*([!=<>]+)\s*(.+?)\s*$")
                .map_err(|e| format!("正则表达式编译失败: {}", e))?;

            let caps = re.captures(trimmed)
                .ok_or_else(|| format!("无效的条件格式: {}", trimmed))?;

            conditions.push(KeyValue {
                key: caps[1].to_string(),
                condition: caps[2].to_string(),
                value: caps[3].to_string(),
            });
        }
        Ok(conditions)
    }
}

// 使用示例
fn example_usage() {
    let expr = "schoolIdList != null and schoolIdList.size() > 0";
    match KeyValue::parse_conditions(expr) {
        Ok(kvs) => {
            println!("解析结果: {:?}", kvs);
            // 输出: [
            //   KeyValue { key: "schoolIdList", condition: "!=", value: "null" },
            //   KeyValue { key: "schoolIdList.size()", condition: ">", value: "0" }
            // ]
        },
        Err(e) => eprintln!("解析错误: {}", e)
    }
}