use std::collections::HashMap;
use super::{
    def::{DialectType, RegexReplacement, XmlParsedState},
    parse_helper,
};

use lazy_static::lazy_static;
use log::{debug, warn};
use regex::Regex;
use std::{fs, process};
use std::io::BufReader;
use std::sync::{Arc, Mutex};
use xml::{attribute::OwnedAttribute, name::OwnedName, EventReader};
use xml::reader::XmlEvent;
use crate::def::{Mode, SqlKey, SqlStatement};
use crate::parse_helper::{match_statement, replace_included_sql, search_matched_attr};

lazy_static! {
    static ref RE: Regex = Regex::new("DTD Mapper 3\\.0").unwrap_or_else(|e| {
        warn!("Unable to parse the regex: {e}");
        process::exit(-1);
    });
    static ref XML_REGEX: Regex = Regex::new("XML-FILE:").unwrap_or_else(|e| {
        warn!("Unable to parse the regex: {e}");
        process::exit(-1);
    });
    static ref STAT_REGEX: Regex = Regex::new("STAT-ID:").unwrap_or_else(|e| {
        warn!("Unable to parse the regex: {e}");
        process::exit(-1);
    });
    static ref ORA_QUERY_PLAN_REGEX: Regex = Regex::new("DBMS_XPLAN").unwrap_or_else(|e| {
        warn!("Unable to parse the regex: {e}");
        process::exit(-1);
    });
    static ref INC_REGEX: Regex = Regex::new("__INCLUDE_ID_").unwrap_or_else(|e| {
        warn!("Unable to parse the regex: {e}");
        process::exit(-1);
    });
}


/// 解析器
pub trait Parser {
    fn setup_gen_explain(&mut self, gen_explain: bool);

    fn is_gen_explain(&self) -> bool;

    fn setup_replace_num(&mut self, replace_num: i16);

    fn setup_sql_limit(&mut self, sql_limit: i16);

    fn replace_num(&self) -> i16;

    fn is_sql_limit(&self) -> bool;

    fn sql_limit(&self) -> i16;

    fn dialect_type(&self) -> &DialectType;

    fn parse(
        &self,
        file_bytes: &[u8],
        arc_global_inc_map: Arc<Mutex<HashMap<String, String>>>,
    ) -> Option<Vec<String>> {
        let mut sql_store: Vec<String> = Vec::new();
        if let Ok(mut global_inc_map) = arc_global_inc_map.lock() {
            if self.check_and_parse(file_bytes, &mut sql_store, &mut global_inc_map) {
                Some(sql_store)
            } else {
                None
            }
        } else {
            None
        }
    }

    fn replace_final_sql(
        &self,
        arc_global_inc_map: Arc<Mutex<HashMap<String, String>>>,
        sql: &str,
    ) -> String {
        if let Ok(global_inc_map) = arc_global_inc_map.lock() {
            let sql = self.replace_inc_between_xml(&String::from(sql), &global_inc_map);
            self.replace_sql_by_regex(&sql)
        } else {
            "".to_string()
        }
    }

    fn replace_sql_by_regex(&self, origin_sql: &str) -> String {
        let regex_replacements = self.vec_regex();
        let mut sql;
        if XML_REGEX.is_match(origin_sql)
            || STAT_REGEX.is_match(origin_sql)
            || ORA_QUERY_PLAN_REGEX.is_match(origin_sql)
        {
            sql = String::from(origin_sql);
        } else {
            sql = String::from(origin_sql.to_ascii_uppercase().trim());
            for regex_replacement in regex_replacements.iter() {
                sql = self.regex_clear_and_push(&sql, regex_replacement);
            }
        }
        sql
    }

    fn replace_inc_between_xml(
        &self,
        sql: &String,
        global_inc_map: &HashMap<String, String>,
    ) -> String {
        debug!("--------------------------------");
        debug!("{sql}");
        let mut new_sql = sql.clone();
        for key in global_inc_map.keys() {
            let target = format!("{}{}{}", "__INCLUDE_ID_", key, "_END__").to_ascii_uppercase();
            debug!("{target}");
            new_sql = replace_included_sql(
                &new_sql,
                key.to_ascii_uppercase().as_str(),
                global_inc_map.get(key).unwrap_or(&target).as_str(),
            )
        }
        debug!("{new_sql}");
        debug!("--------------------------------");
        new_sql
    }

    fn check_and_parse(
        &self,
        file_bytes: &[u8],
        sql_store: &mut Vec<String>,
        global_inc_map: &mut HashMap<String, String>,
    ) -> bool {
        // if self.detect_match(file) {
        //     info!("try to parse [{file}]");
        //     self.read_and_parse(file, sql_store, global_inc_map);
        //     true
        // } else {
        //     false
        // }
        self.read_and_parse(file_bytes, sql_store, global_inc_map);
        true
    }

    fn detect_match(&self, file: &str) -> bool;

    fn detect_match_with_regex(&self, file: &str, re: &Regex) -> bool {
        let result = fs::read_to_string(file);
        if let Ok(content) = result {
            re.is_match(content.as_str())
        } else {
            false
        }
    }

    fn read_and_parse(
        &self,
        file_bytes: &[u8],
        sql_store: &mut Vec<String>,
        global_inc_map: &mut HashMap<String, String>,
    ) {
        let mut sql_parsed = Vec::new();
        self.read_xml(file_bytes, &mut sql_parsed, global_inc_map);
        for sql in sql_parsed {
            sql_store.push(sql);
        }
    }

    fn read_xml(
        &self,
        file_bytes: &[u8],
        sql_store: &mut Vec<String>,
        global_inc_map: &mut HashMap<String, String>,
    ) {
        let mut file_inc_map = HashMap::new();

        // let filename_sql = compose_comment(
        //     &comment_leading(self.dialect_type()),
        //     &filename.to_string(),
        //     &comment_tailing(self.dialect_type()),
        // );

        let cursor = std::io::Cursor::new(file_bytes);
        let buf = BufReader::new(cursor);

        // let file = fs::File::open(filename).unwrap_or_else(|e| {
        //     warn!("open file [{filename}] failed: {e}");
        //     process::exit(-1);
        // });

        //let buf = BufReader::new(file);
        let parser = EventReader::new(buf);
        let mut state = XmlParsedState::new();
        //state.filename = filename.clone();
        state.filename = "ttt".to_string();
        for e in parser {
            match e {
                Ok(XmlEvent::StartElement {
                       name, attributes, ..
                   }) => self.parse_start_element(name, attributes, &mut state),
                Ok(XmlEvent::EndElement { name }) => {
                    self.parse_end_element(name, &mut state, global_inc_map, &mut file_inc_map)
                }
                Ok(XmlEvent::CData(content)) => self.fill_xml_content(&mut state, content),
                Ok(XmlEvent::Characters(content)) => self.fill_xml_content(&mut state, content),
                Err(e) => {
                    warn!("Error: {e}");
                    break;
                }
                _ => {}
            }
        }

        //println!("statements {:?}", &state.statements);
        // for statement in &state.statements {
        //     println!("sql {:?}", &statement.sql);
        // }
        // println!("file_inc_map {:?}", &file_inc_map);


        self.replace_and_fill(sql_store, &state.statements, &file_inc_map);
        if !sql_store.is_empty() {
            //sql_store.insert(0, filename_sql);
            sql_store.insert(0, "filename_sql".to_string());
        }
    }

    fn fill_xml_content(&self, state: &mut XmlParsedState, content: String) {
        self.fill_content(state, content);
        if state.in_loop {
            self.fill_content(state, state.loop_def.separator.clone());
        }
    }

    fn fill_content(&self, state: &mut XmlParsedState, content: String) {
        if state.in_statement {
            if state.in_sql_key {
                state.key_sql_builder += content.as_str();
            } else {
                state.sql_builder += content.as_str();
            }
        }
    }

    fn parse_start_element(
        &self,
        name: OwnedName,
        attributes: Vec<OwnedAttribute>,
        state: &mut XmlParsedState,
    ) {
        let element_name = name.local_name.as_str().to_ascii_lowercase();
        if element_name == "mapper" || element_name == "sqlmap" {
            search_matched_attr(&attributes, "namespace", |attr| {
                state.namespace = attr.value.clone();
                debug!("namespace: {}", state.namespace);
            });
        } else if match_statement(&element_name) {
            state.in_statement = true;
            search_matched_attr(&attributes, "id", |attr| {
                state.current_id = attr.value.clone();
            });
        } else if element_name == "selectkey" {
            state.in_sql_key = true;
            state.has_sql_key = true;
            state.current_key_id = state.current_id.as_str().to_string() + ".selectKey";
        } else if element_name == "where" {
            state.sql_builder += " where ";
        } else if element_name == "include" {
            debug!("{}, {}", state.filename, state.current_id);
            search_matched_attr(&attributes, "refid", |attr| {
                state.sql_builder += " __INCLUDE_ID_";
                let refid = attr.value.clone();
                state.sql_builder += refid.as_str();
                state.sql_builder += "_END__";
                state.has_include = true;
            });
        } else if element_name == "if" {
            println!("{}:{} {} {} {:?}", file!(), line!(), &element_name, name, &attributes);
        } else {
            self.ex_parse_start_element(name, &element_name, &attributes, state);
        }
    }

    fn ex_parse_start_element(
        &self,
        name: OwnedName,
        element_name: &str,
        attributes: &[OwnedAttribute],
        state: &mut XmlParsedState,
    );

    fn parse_end_element(
        &self,
        name: OwnedName,
        state: &mut XmlParsedState,
        global_inc_map: &mut HashMap<String, String>,
        file_inc_map: &mut HashMap<String, String>,
    ) {
        let element_name = name.local_name.as_str().to_ascii_lowercase();
        if match_statement(&element_name) {
            let mode = Mode::from(element_name.as_str());
            match mode {
                Mode::SqlPart => self.handle_end_sql_part(state, global_inc_map, file_inc_map),
                _ => self.handle_end_statement(mode, state),
            }
        } else if element_name == "selectkey" {
            state.in_sql_key = false;
        } else {
            self.ex_parse_end_element(name, &element_name, state);
        }
    }

    fn ex_parse_end_element(&self, name: OwnedName, element_name: &str, state: &mut XmlParsedState);

    fn handle_end_sql_part(
        &self,
        state: &mut XmlParsedState,
        global_inc_map: &mut HashMap<String, String>,
        file_inc_map: &mut HashMap<String, String>,
    ) {
        file_inc_map.insert(state.current_id.clone(), state.sql_builder.to_string());
        global_inc_map.insert(
            format!("{}.{}", state.namespace, state.current_id.clone()),
            state.sql_builder.to_string(),
        );
        state.reset();
    }

    fn handle_end_statement(&self, mode: Mode, state: &mut XmlParsedState) {
        let sql_stat = SqlStatement::new(
            mode,
            state.current_id.clone(),
            state.sql_builder.to_string(),
            state.has_include,
            state.has_sql_key,
            SqlKey {
                key: state.current_key_id.clone(),
                sql: state.key_sql_builder.to_string(),
            },
        );
        state.statements.push(sql_stat);
        state.reset();
    }

    fn replace_and_fill(
        &self,
        sql_store: &mut Vec<String>,
        statements: &Vec<SqlStatement>,
        file_inc_map: &HashMap<String, String>,
    ) {
        let comment_leading = comment_leading2(self.dialect_type());
        let comment_tailing = comment_tailing2(self.dialect_type());
        for stat in statements {
            self.replace_single_statement(
                sql_store,
                &comment_leading,
                stat,
                &comment_tailing,
                file_inc_map,
            );
        }
    }

    fn replace_single_statement(
        &self,
        sql_store: &mut Vec<String>,
        comment_leading: &String,
        stat: &SqlStatement,
        comment_tailing: &String,
        file_inc_map: &HashMap<String, String>,
    ) {
        debug!("----------------------------------------------------------------");
        let stat_id_sql = compose_comment(
            &comment_leading.to_string(),
            &String::from(&stat.id),
            &comment_tailing.to_string(),
        );
        if stat.has_include {
            self.clear_and_push(
                sql_store,
                &stat_id_sql,
                &loop_replace_include_part(stat, file_inc_map, self.replace_num()),
                self.is_gen_explain(),
            );
        } else {
            self.clear_and_push(sql_store, &stat_id_sql, &stat.sql, self.is_gen_explain());
        }
        if stat.has_sql_key {
            let stat_id_key_sql = compose_comment(
                &comment_leading.to_string(),
                &stat.sql_key.key,
                &comment_tailing.to_string(),
            );
            self.clear_and_push(
                sql_store,
                &stat_id_key_sql,
                &stat.sql_key.sql,
                self.is_gen_explain(),
            );
        }
    }

    fn clear_and_push(
        &self,
        sql_store: &mut Vec<String>,
        id_sql: &str,
        origin_sql: &str,
        gen_explain: bool,
    ) {
        self.loop_clear_and_push(sql_store, id_sql, origin_sql, gen_explain, true);
    }

    fn loop_clear_and_push(
        &self,
        sql_store: &mut Vec<String>,
        id_sql: &str,
        origin_sql: &str,
        gen_explain: bool,
        append_semicolon: bool,
    ) {
        let sql = self.replace_sql_by_regex(origin_sql);
        if gen_explain && append_semicolon {
            let sql = format!("{}{}{}", explain_dialect(self.dialect_type()), sql, ";");
            self.push_to_sql_store(sql_store, id_sql, sql, true);
        } else if !gen_explain && append_semicolon {
            let sql = sql + ";";
            self.push_to_sql_store(sql_store, id_sql, sql, false);
        } else if !append_semicolon && gen_explain {
            let sql = format!("{}{}", explain_dialect(self.dialect_type()), sql);
            self.push_to_sql_store(sql_store, id_sql, sql, true);
        } else {
            self.push_to_sql_store(sql_store, id_sql, sql, false);
        }
    }

    fn push_to_sql_store(
        &self,
        sql_store: &mut Vec<String>,
        id_sql: &str,
        sql: String,
        append_oracle_list_plan: bool,
    ) {
        if !self.is_sql_limit() || (self.is_sql_limit() && sql.len() > self.sql_limit() as usize) {
            sql_store.push(String::from(id_sql));
            sql_store.push(sql);
            if append_oracle_list_plan {
                self.append_oracle_list_plan(sql_store);
            }
        }
    }

    fn regex_clear_and_push(
        &self,
        origin_sql: &str,
        regex_replacement: &RegexReplacement,
    ) -> String {
        regex_replacement
            .regex
            .replace_all(origin_sql, regex_replacement.target.as_str())
            .to_string()
    }

    fn vec_regex(&self) -> &Vec<RegexReplacement>;

    fn append_oracle_list_plan(&self, sql_store: &mut Vec<String>) {
        if let DialectType::Oracle = self.dialect_type() {
            sql_store.push(String::from("SELECT * FROM TABLE(DBMS_XPLAN.DISPLAY);"))
        }
    }
}

pub(crate) fn var_placeholder(dialect_type: &DialectType) -> &str {
    match dialect_type {
        DialectType::Oracle => ":?",
        DialectType::MySQL => "@1",
    }
}

/// `MyBatis` 实现
pub fn create_mybatis_parser(dialect_type: DialectType) -> MyBatisParser {
    let re_vec;
    {
        re_vec = create_replcements(&dialect_type);
    }
    MyBatisParser {
        dialect_type,
        re_vec,
        gen_explain: false,
        replace_num: 0,
        sql_limit: 0,
    }
}

fn create_replcements(dialect_type: &DialectType) -> Vec<RegexReplacement> {
    let placeholder = var_placeholder(dialect_type);
    vec![
        RegexReplacement::new("[\t ]?--[^\n]*\n", ""),
        RegexReplacement::new("[\r\n\t ]+", " "),
        RegexReplacement::new("\\$\\{[^${]+\\}\\.", "__REPLACE_SCHEMA__."),
        RegexReplacement::new("#\\{[^#{]+\\}", placeholder),
        RegexReplacement::new("\\$\\{[^${]+\\}", placeholder),
        RegexReplacement::new("WHERE[ ]+AND[ ]+", "WHERE "),
        RegexReplacement::new("WHERE[ ]+OR[ ]+", "WHERE "),
        RegexReplacement::new(",[ ]+WHERE", " WHERE"),
        RegexReplacement::new("[ ]*,[ ]*\\)", ")"),
        RegexReplacement::new("AND[ ]*$", ""),
        RegexReplacement::new("OR[ ]*$", ""),
        RegexReplacement::new(",$", ""),
    ]
}

pub struct MyBatisParser {
    dialect_type: DialectType,
    re_vec: Vec<RegexReplacement>,
    gen_explain: bool,
    replace_num: i16,
    sql_limit: i16,
}

impl Parser for MyBatisParser {
    fn setup_gen_explain(&mut self, gen_explain: bool) {
        self.gen_explain = gen_explain;
    }

    fn is_gen_explain(&self) -> bool {
        self.gen_explain
    }

    fn setup_replace_num(&mut self, replace_num: i16) {
        self.replace_num = replace_num;
    }

    fn setup_sql_limit(&mut self, sql_limit: i16) {
        self.sql_limit = sql_limit;
    }

    fn replace_num(&self) -> i16 {
        self.replace_num
    }

    fn is_sql_limit(&self) -> bool {
        self.sql_limit > 0
    }

    fn sql_limit(&self) -> i16 {
        self.sql_limit
    }

    fn dialect_type(&self) -> &DialectType {
        &self.dialect_type
    }

    fn detect_match(&self, file: &str) -> bool {
        self.detect_match_with_regex(file, &RE)
    }

    fn ex_parse_start_element(
        &self,
        _name: OwnedName,
        element_name: &str,
        attributes: &[OwnedAttribute],
        state: &mut XmlParsedState,
    ) {
        if element_name == "set" {
            state.sql_builder += " set ";
        } else if element_name == "trim" {
            state.in_loop = true;
            parse_helper::search_matched_attr(attributes, "prefix", |attr| {
                self.fill_content(state, attr.value.clone());
            });
            parse_helper::search_matched_attr(attributes, "suffix", |attr| {
                state.loop_def.suffix = attr.value.clone();
            });
        } else if element_name == "foreach" {
            state.in_loop = true;
            parse_helper::search_matched_attr(attributes, "open", |attr| {
                self.fill_content(state, attr.value.clone());
            });
            parse_helper::search_matched_attr(attributes, "close", |attr| {
                state.loop_def.suffix = attr.value.clone();
            });
            parse_helper::search_matched_attr(attributes, "separator", |attr| {
                state.loop_def.separator = attr.value.clone();
            });
        }
    }

    fn ex_parse_end_element(
        &self,
        _name: OwnedName,
        element_name: &str,
        state: &mut XmlParsedState,
    ) {
        if element_name == "trim" || element_name == "foreach" {
            let suffix;
            {
                suffix = &state.loop_def.suffix;
            }
            self.fill_content(state, suffix.clone());
            state.in_loop = false;
            state.loop_def.reset();
        }
    }

    fn vec_regex(&self) -> &Vec<RegexReplacement> {
        &self.re_vec
    }
}


fn loop_replace_include_part(
    stat: &SqlStatement,
    file_inc_map: &HashMap<String, String>,
    replace_num: i16,
) -> String {
    let mut sql = stat.sql.clone();
    for _i in 0..replace_num {
        for key in file_inc_map.keys() {
            let (new_sql, replace) = replace_included_sql_by_key(&sql, stat, file_inc_map, key);
            if replace {
                sql = new_sql;
            }
        }
        if !INC_REGEX.is_match(&sql) {
            break;
        }
    }
    sql
}

fn replace_included_sql_by_key(
    sql: &str,
    stat: &SqlStatement,
    file_inc_map: &HashMap<String, String>,
    key: &String,
) -> (String, bool) {
    debug!("key:::{key}");
    let key_opt = file_inc_map.get(key);
    if let Some(sql_part) = key_opt {
        debug!("{key}:::-->{sql_part}");
        let new_sql = replace_included_sql(sql, key, sql_part);
        debug!("{new_sql}");
        (new_sql, true)
    } else {
        warn!(
            "can not find include_key[{}] in statement[{}]",
            key, stat.id
        );
        (String::from(""), false)
    }
}

fn comment_leading2(dialet_type: &DialectType) -> String {
    match dialet_type {
        DialectType::Oracle => "SELECT \"STAT-ID: ".to_string(),
        DialectType::MySQL => "SELECT \"STAT-ID: ".to_string(),
    }
}

fn comment_tailing2(dialet_type: &DialectType) -> String {
    match dialet_type {
        DialectType::Oracle => "\" AS STAT_ID FROM DUAL;".to_string(),
        DialectType::MySQL => "\" AS STAT_ID;".to_string(),
    }
}

fn compose_comment(leading: &String, line: &String, trailing: &String) -> String {
    format!("{leading}{line}{trailing}")
}

fn explain_dialect(dialect_type: &DialectType) -> &str {
    match dialect_type {
        DialectType::Oracle => "explain plan for ",
        DialectType::MySQL => "explain ",
    }
}
