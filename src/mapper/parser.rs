use quick_xml::Reader;
use quick_xml::events::Event;
use super::model::*;
use std::error::Error;
use std::io::Cursor;

/// MyBatis XML解析器
pub struct MyBatisXmlParser {
    reader: Reader<Cursor<Vec<u8>>>,
    buf: Vec<u8>,
}

impl MyBatisXmlParser {
    /// 从字符串创建解析器
    pub fn new(xml_content: &str) -> Self {
        Self::new_from_bytes(xml_content.as_bytes())
    }

    /// 从字节切片创建解析器
    pub fn new_from_bytes(xml_bytes: &[u8]) -> Self {
        let vec_bytes = xml_bytes.to_vec();
        let cursor = Cursor::new(vec_bytes);

        let mut reader = Reader::from_reader(cursor);
        reader.trim_text(true);

        MyBatisXmlParser {
            reader,
            buf: Vec::new(),
        }
    }

    /// 解析mapper文件
    pub fn parse_mapper(&mut self) -> Result<Mapper, Box<dyn Error>> {
        let mut mapper = Mapper::default();
        let mut in_mapper = false;

        loop {
            match self.reader.read_event_into(&mut self.buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"mapper" => {
                        in_mapper = true;
                        // 解析namespace属性
                        if let Some(ns_attr) = e.attributes().find(|a| {
                            a.as_ref().unwrap().key.as_ref() == b"namespace"
                        }) {
                            let attr = ns_attr.unwrap();
                            let ns = std::str::from_utf8(&attr.value)?;
                            mapper.namespace = ns.to_string();
                        }
                    },
                    b"select" if in_mapper => {
                        let e = e.into_owned(); // 将借用事件转换为所有权模式
                        let stmt = self.parse_sql_statement(StatementType::Select, &e)?;
                        mapper.statements.insert(stmt.id.clone(), stmt);
                    },
                    b"insert" if in_mapper => {
                        let e = e.into_owned(); // 将借用事件转换为所有权模式
                        let stmt = self.parse_sql_statement(StatementType::Insert, &e)?;
                        mapper.statements.insert(stmt.id.clone(), stmt);
                    },
                    b"update" if in_mapper => {
                        let e = e.into_owned(); // 将借用事件转换为所有权模式
                        let stmt = self.parse_sql_statement(StatementType::Update, &e)?;
                        mapper.statements.insert(stmt.id.clone(), stmt);
                    },
                    b"delete" if in_mapper => {
                        let e = e.into_owned(); // 将借用事件转换为所有权模式
                        let stmt = self.parse_sql_statement(StatementType::Delete, &e)?;
                        mapper.statements.insert(stmt.id.clone(), stmt);
                    },
                    b"resultMap" if in_mapper => {
                        let e = e.into_owned(); // 将借用事件转换为所有权模式
                        let result_map = self.parse_result_map(&e)?;
                        mapper.result_maps.insert(result_map.id.clone(), result_map);
                    },
                    _ => {}
                },
                Ok(Event::End(e)) => {
                    if e.name().as_ref() == b"mapper" {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                Err(e) => return Err(Box::new(e)),
                _ => {}
            }
        }

        Ok(mapper)
    }

    /// 解析SQL语句
    fn parse_sql_statement(&mut self, stmt_type: StatementType, start_event: &quick_xml::events::BytesStart)
                           -> Result<SqlStatement, Box<dyn Error>> {
        let mut stmt = SqlStatement {
            stmt_type: Some(stmt_type),
            ..Default::default()
        };

        // 解析属性
        for attr in start_event.attributes() {
            let attr = attr?;
            match attr.key.as_ref() {
                b"id" => stmt.id = std::str::from_utf8(&attr.value)?.to_string(),
                b"parameterType" => stmt.parameter_type = Some(std::str::from_utf8(&attr.value)?.to_string()),
                b"resultType" => stmt.result_type = Some(std::str::from_utf8(&attr.value)?.to_string()),
                b"resultMap" => stmt.result_map = Some(std::str::from_utf8(&attr.value)?.to_string()),
                _ => {}
            }
        }

        // 解析SQL内容和动态SQL
        let mut sql_buffer = String::new();
        let mut dynamic_nodes = Vec::new();
        self.parse_sql_content(&mut sql_buffer, &mut dynamic_nodes)?;

        stmt.sql = sql_buffer;
        if !dynamic_nodes.is_empty() {
            // 如果有多个节点，用Text包裹
            if dynamic_nodes.len() == 1 {
                stmt.dynamic_sql = dynamic_nodes.into_iter().next();
            } else {
                stmt.dynamic_sql = Some(DynamicSqlNode::Trim {
                    prefix: None,
                    prefix_overrides: None,
                    suffix: None,
                    suffix_overrides: None,
                    contents: dynamic_nodes,
                });
            }
        }

        // 提取参数
        stmt.parameters = self.extract_parameters(&stmt.sql);

        Ok(stmt)
    }

    /// 解析SQL内容和动态SQL节点
    fn parse_sql_content(&mut self, sql_buffer: &mut String, dynamic_nodes: &mut Vec<DynamicSqlNode>)
                         -> Result<(), Box<dyn Error>> {
        loop {
            match self.reader.read_event_into(&mut self.buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"if" => {
                        let test_attr = e.attributes().find(|a| {
                            a.as_ref().unwrap().key.as_ref() == b"test"
                        }).ok_or("if标签缺少test属性")?;
                        let test = std::str::from_utf8(&test_attr.unwrap().value)?.to_string();
                        let test = test.trim().to_string();

                        let mut contents = Vec::new();
                        self.parse_sql_content(&mut String::new(), &mut contents)?;
                        dynamic_nodes.push(DynamicSqlNode::If {
                            test,
                            contents,
                        });
                    },
                    b"foreach" => {
                        // 解析foreach属性
                        let mut collection = String::new();
                        let mut item = String::new();
                        let mut index = None;
                        let mut open = String::new();
                        let mut separator = String::new();
                        let mut close = String::new();

                        for attr in e.attributes() {
                            let attr = attr?;
                            match attr.key.as_ref() {
                                b"collection" => collection = std::str::from_utf8(&attr.value)?.to_string(),
                                b"item" => item = std::str::from_utf8(&attr.value)?.to_string(),
                                b"index" => index = Some(std::str::from_utf8(&attr.value)?.to_string()),
                                b"open" => open = std::str::from_utf8(&attr.value)?.to_string(),
                                b"separator" => separator = std::str::from_utf8(&attr.value)?.to_string(),
                                b"close" => close = std::str::from_utf8(&attr.value)?.to_string(),
                                _ => {}
                            }
                        }

                        let mut contents = Vec::new();
                        self.parse_sql_content(&mut String::new(), &mut contents)?;
                        dynamic_nodes.push(DynamicSqlNode::Foreach {
                            collection,
                            item,
                            index,
                            open,
                            separator,
                            close,
                            contents,
                        });
                    },
                    // 处理其他动态SQL标签...
                    _ => {
                        // 未知标签，作为普通文本处理
                        sql_buffer.push_str(&format!("<{}/>", std::str::from_utf8(e.name().as_ref())?.to_string()));
                    }
                },
                Ok(Event::Text(t)) => {
                    let text = std::str::from_utf8(&t)?;
                    sql_buffer.push_str(text);
                    if !text.trim().is_empty() {
                        dynamic_nodes.push(DynamicSqlNode::Text(text.to_string()));
                    }
                },
                Ok(Event::End(_)) => break,
                Ok(Event::Eof) => break,
                Err(e) => return Err(Box::new(e)),
                _ => {}
            }
        }

        Ok(())
    }

    /// 解析结果映射
    fn parse_result_map(&mut self, start_event: &quick_xml::events::BytesStart)
                        -> Result<ResultMap, Box<dyn Error>> {
        let mut result_map = ResultMap::default();

        // 解析属性
        for attr in start_event.attributes() {
            let attr = attr?;
            match attr.key.as_ref() {
                b"id" => result_map.id = std::str::from_utf8(&attr.value)?.to_string(),
                b"type" => result_map.type_name = std::str::from_utf8(&attr.value)?.to_string(),
                _ => {}
            }
        }

        // 解析result子节点
        loop {
            match self.reader.read_event_into(&mut self.buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"result" => {
                        let mut column = ResultColumn {
                            property: String::new(),
                            column: String::new(),
                            java_type: None,
                            jdbc_type: None,
                        };

                        for attr in e.attributes() {
                            let attr = attr?;
                            match attr.key.as_ref() {
                                b"property" => column.property = std::str::from_utf8(&attr.value)?.to_string(),
                                b"column" => column.column = std::str::from_utf8(&attr.value)?.to_string(),
                                b"javaType" => column.java_type = Some(std::str::from_utf8(&attr.value)?.to_string()),
                                b"jdbcType" => column.jdbc_type = Some(std::str::from_utf8(&attr.value)?.to_string()),
                                _ => {}
                            }
                        }

                        result_map.result_columns.push(column);
                        // 消耗结束标签
                        self.reader.read_event_into(&mut self.buf)?;
                    },
                    // 处理其他resultMap子标签...
                    _ => {
                        // 跳过未知标签
                        self.skip_element()?;
                    }
                },
                Ok(Event::End(_)) => break,
                Ok(Event::Eof) => break,
                Err(e) => return Err(Box::new(e)),
                _ => {}
            }
        }

        Ok(result_map)
    }

    /// 提取SQL中的参数
    fn extract_parameters(&self, sql: &str) -> Vec<String> {
        let mut params = Vec::new();
        let mut chars = sql.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '#' && chars.next_if_eq(&'{').is_some() {
                let mut param = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '}' {
                        chars.next();
                        break;
                    }
                    param.push(chars.next().unwrap());
                }
                // 清理参数名，移除可能的属性
                let param_name = param.split(|c| c == ':' || c == ',').next().unwrap_or(&param).trim();
                if !param_name.is_empty() && !params.contains(&param_name.to_string()) {
                    params.push(param_name.to_string());
                }
            }
        }

        params
    }

    /// 跳过元素
    fn skip_element(&mut self) -> Result<(), Box<dyn Error>> {
        let mut depth = 1;
        while depth > 0 {
            match self.reader.read_event_into(&mut self.buf) {
                Ok(Event::Start(_)) => depth += 1,
                Ok(Event::End(_)) => depth -= 1,
                Ok(Event::Eof) => break,
                Err(e) => return Err(Box::new(e)),
                _ => {}
            }
        }
        Ok(())
    }
}