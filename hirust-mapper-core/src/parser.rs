use quick_xml::Reader;
use quick_xml::events::Event;
use quick_xml::events::BytesStart;
use super::model::*;
use std::io::Cursor;

/// MyBatis XML解析器
pub struct MyBatisXmlParser {
    reader: Reader<Cursor<Vec<u8>>>,
    buf: Vec<u8>,
}

/// 从XML标签中按名称查找属性值
fn get_attr(e: &BytesStart, name: &[u8], err_msg: &str) -> Result<String, MapperError> {
    let attr = e.attributes()
        .find(|a| a.as_ref().map(|a| a.key.as_ref() == name).unwrap_or(false))
        .ok_or_else(|| MapperError::ParseError { message: err_msg.to_string() })?
        .map_err(|e| MapperError::ParseError { message: e.to_string() })?;
    Ok(std::str::from_utf8(&attr.value)?.to_string())
}

/// 获取可选属性，空字符串视为 None
fn get_optional_attr(e: &BytesStart, name: &[u8]) -> Option<String> {
    get_attr(e, name, "").ok().filter(|s| !s.is_empty())
}

/// 字节切片转字符串
fn bytes_to_str(bytes: &[u8]) -> Result<String, MapperError> {
    Ok(std::str::from_utf8(bytes)?.to_string())
}

impl MyBatisXmlParser {
    /// 从字符串创建解析器
    pub fn new(xml_content: &str) -> Self {
        Self::new_from_bytes(xml_content.as_bytes())
    }

    /// 从字节切片创建解析器
    pub fn new_from_bytes(xml_bytes: &[u8]) -> Self {
        let reader = Reader::from_reader(Cursor::new(xml_bytes.to_vec()));
        MyBatisXmlParser {
            reader,
            buf: Vec::new(),
        }
    }

    /// 解析mapper文件
    pub fn parse_mapper(&mut self) -> Result<Mapper, MapperError> {
        let mut mapper = Mapper::default();
        let mut in_mapper = false;

        loop {
            match self.reader.read_event_into(&mut self.buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"mapper" => {
                        in_mapper = true;
                        if let Ok(ns) = get_attr(&e, b"namespace", "<mapper>缺少namespace属性") {
                            mapper.namespace = ns;
                        }
                    },
                    b"select" if in_mapper => {
                        let e = e.into_owned();
                        let stmt = self.parse_sql_statement(StatementType::Select, &e)?;
                        mapper.statements.insert(stmt.id.clone(), stmt);
                    },
                    b"insert" if in_mapper => {
                        let e = e.into_owned();
                        let stmt = self.parse_sql_statement(StatementType::Insert, &e)?;
                        mapper.statements.insert(stmt.id.clone(), stmt);
                    },
                    b"update" if in_mapper => {
                        let e = e.into_owned();
                        let stmt = self.parse_sql_statement(StatementType::Update, &e)?;
                        mapper.statements.insert(stmt.id.clone(), stmt);
                    },
                    b"delete" if in_mapper => {
                        let e = e.into_owned();
                        let stmt = self.parse_sql_statement(StatementType::Delete, &e)?;
                        mapper.statements.insert(stmt.id.clone(), stmt);
                    },
                    b"resultMap" if in_mapper => {
                        let e = e.into_owned();
                        let result_map = self.parse_result_map(&e)?;
                        mapper.result_maps.insert(result_map.id.clone(), result_map);
                    },
                    b"sql" if in_mapper => {
                        let e = e.into_owned();
                        let id = get_attr(&e, b"id", "<sql>标签缺少id属性")?;
                        let mut contents = Vec::new();
                        self.parse_sql_content(&mut String::new(), &mut contents)?;
                        mapper.sql_fragments.insert(id, contents);
                    },
                    _ => {}
                },
                Ok(Event::End(e)) => {
                    if e.name().as_ref() == b"mapper" {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                Err(e) => return Err(MapperError::from(e)),
                _ => {}
            }
        }

        Ok(mapper)
    }

    /// 解析SQL语句
    fn parse_sql_statement(&mut self, stmt_type: StatementType, start_event: &BytesStart)
                           -> Result<SqlStatement, MapperError> {
        let mut stmt = SqlStatement {
            stmt_type: Some(stmt_type),
            ..Default::default()
        };

        for attr in start_event.attributes() {
            let attr = attr.map_err(|e| MapperError::ParseError { message: e.to_string() })?;
            match attr.key.as_ref() {
                b"id" => stmt.id = bytes_to_str(&attr.value)?,
                b"parameterType" => stmt.parameter_type = Some(bytes_to_str(&attr.value)?),
                b"resultType" => stmt.result_type = Some(bytes_to_str(&attr.value)?),
                b"resultMap" => stmt.result_map = Some(bytes_to_str(&attr.value)?),
                _ => {}
            }
        }

        let mut sql_buffer = String::new();
        let mut dynamic_nodes = Vec::new();
        self.parse_sql_content(&mut sql_buffer, &mut dynamic_nodes)?;

        stmt.sql = sql_buffer;
        if !dynamic_nodes.is_empty() {
            if dynamic_nodes.len() == 1 {
                stmt.dynamic_sql = dynamic_nodes.into_iter().next();
            } else {
                stmt.dynamic_sql = Some(DynamicSqlNode::Mixed {
                    contents: dynamic_nodes,
                });
            }
        }

        stmt.parameters = Self::extract_parameters(&stmt.sql);

        Ok(stmt)
    }

    /// 解析SQL内容和动态SQL节点
    fn parse_sql_content(&mut self, sql_buffer: &mut String, dynamic_nodes: &mut Vec<DynamicSqlNode>)
                         -> Result<(), MapperError> {
        loop {
            match self.reader.read_event_into(&mut self.buf) {
                Ok(Event::Start(e)) => {
                    let owned = e.into_owned();
                    self.handle_dynamic_tag(&owned, sql_buffer, dynamic_nodes, true)?;
                },
                Ok(Event::Empty(e)) => {
                    let owned = e.into_owned();
                    self.handle_dynamic_tag(&owned, sql_buffer, dynamic_nodes, false)?;
                },
                Ok(Event::Text(t)) => {
                    let text = bytes_to_str(&t)?;
                    sql_buffer.push_str(&text);
                    if !text.trim().is_empty() {
                        dynamic_nodes.push(DynamicSqlNode::Text(text));
                    }
                },
                Ok(Event::CData(t)) => {
                    let text = bytes_to_str(&t)?;
                    sql_buffer.push_str(&text);
                    if !text.trim().is_empty() {
                        dynamic_nodes.push(DynamicSqlNode::Text(text));
                    }
                },
                Ok(Event::End(_)) => break,
                Ok(Event::Eof) => break,
                Err(e) => return Err(MapperError::from(e)),
                _ => {}
            }
        }

        Ok(())
    }

    /// 处理动态SQL标签（同时支持 Start 有内容 和 Empty 自闭合两种情况）
    /// has_body: true 表示有子内容需要递归解析，false 表示自闭合（内容为空）
    fn handle_dynamic_tag(
        &mut self,
        e: &BytesStart,
        _sql_buffer: &mut String,
        dynamic_nodes: &mut Vec<DynamicSqlNode>,
        has_body: bool,
    ) -> Result<(), MapperError> {
        let mut parse_contents = || -> Result<Vec<DynamicSqlNode>, MapperError> {
            if has_body {
                let mut contents = Vec::new();
                self.parse_sql_content(&mut String::new(), &mut contents)?;
                Ok(contents)
            } else {
                Ok(Vec::new())
            }
        };

        match e.name().as_ref() {
            b"if" => {
                let test = get_attr(e, b"test", "<if>标签缺少test属性")?.trim().to_string();
                let contents = parse_contents()?;
                dynamic_nodes.push(DynamicSqlNode::If { test, contents });
            },
            b"bind" => {
                let name = get_attr(e, b"name", "<bind>标签缺少name属性")?;
                let value = get_attr(e, b"value", "<bind>标签缺少value属性")?;
                dynamic_nodes.push(DynamicSqlNode::Bind { name, value });
            },
            b"include" => {
                let ref_id = get_attr(e, b"refid", "<include>标签缺少refid属性")?;
                dynamic_nodes.push(DynamicSqlNode::Include { ref_id });
            },
            b"foreach" => {
                let collection = get_attr(e, b"collection", "<foreach>标签缺少collection属性")?;
                let item = get_attr(e, b"item", "<foreach>标签缺少item属性")?;
                let index = get_optional_attr(e, b"index");
                let open = get_attr(e, b"open", "").unwrap_or_default();
                let separator = get_attr(e, b"separator", "").unwrap_or_default();
                let close = get_attr(e, b"close", "").unwrap_or_default();
                let contents = parse_contents()?;
                dynamic_nodes.push(DynamicSqlNode::Foreach {
                    collection, item, index, open, separator, close, contents,
                });
            },
            b"where" => {
                let prefix_overrides = get_optional_attr(e, b"prefixOverrides");
                let suffix_overrides = get_optional_attr(e, b"suffixOverrides");
                let contents = parse_contents()?;
                dynamic_nodes.push(DynamicSqlNode::Where {
                    prefix_overrides, suffix_overrides, contents,
                });
            },
            b"trim" => {
                let prefix = get_optional_attr(e, b"prefix");
                let prefix_overrides = get_optional_attr(e, b"prefixOverrides");
                let suffix = get_optional_attr(e, b"suffix");
                let suffix_overrides = get_optional_attr(e, b"suffixOverrides");
                let contents = parse_contents()?;
                dynamic_nodes.push(DynamicSqlNode::Trim {
                    prefix, prefix_overrides, suffix, suffix_overrides, contents,
                });
            },
            b"set" => {
                let prefix_overrides = get_optional_attr(e, b"prefixOverrides");
                let suffix_overrides = get_optional_attr(e, b"suffixOverrides");
                let contents = parse_contents()?;
                dynamic_nodes.push(DynamicSqlNode::Set {
                    prefix_overrides, suffix_overrides, contents,
                });
            },
            b"choose" => {
                if has_body {
                    let (whens, otherwise) = self.parse_choose()?;
                    dynamic_nodes.push(DynamicSqlNode::Choose { whens, otherwise });
                }
                // 自闭合的choose无意义，忽略
            },
            _ => {
                // 未知标签，跳过（有body时需要跳过子树）
                if has_body {
                    self.skip_element()?;
                }
            }
        }

        Ok(())
    }

    /// 解析choose标签内部结构
    fn parse_choose(&mut self) -> Result<(Vec<(String, Vec<DynamicSqlNode>)>, Option<Vec<DynamicSqlNode>>), MapperError> {
        let mut whens = Vec::new();
        let mut otherwise = None;

        loop {
            match self.reader.read_event_into(&mut self.buf) {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    b"when" => {
                        let test = get_attr(&e, b"test", "<when>标签缺少test属性")?.trim().to_string();
                        let mut contents = Vec::new();
                        self.parse_sql_content(&mut String::new(), &mut contents)?;
                        whens.push((test, contents));
                    },
                    b"otherwise" => {
                        let mut contents = Vec::new();
                        self.parse_sql_content(&mut String::new(), &mut contents)?;
                        otherwise = Some(contents);
                    },
                    _ => {
                        self.skip_element()?;
                    }
                },
                Ok(Event::End(e)) => {
                    if e.name().as_ref() == b"choose" {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                Err(e) => return Err(MapperError::from(e)),
                _ => {}
            }
        }

        Ok((whens, otherwise))
    }

    /// 解析结果映射
    fn parse_result_map(&mut self, start_event: &BytesStart) -> Result<ResultMap, MapperError> {
        let mut result_map = ResultMap::default();

        for attr in start_event.attributes() {
            let attr = attr.map_err(|e| MapperError::ParseError { message: e.to_string() })?;
            match attr.key.as_ref() {
                b"id" => result_map.id = bytes_to_str(&attr.value)?,
                b"type" => result_map.type_name = bytes_to_str(&attr.value)?,
                _ => {}
            }
        }

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
                            let attr = attr.map_err(|e| MapperError::ParseError { message: e.to_string() })?;
                            match attr.key.as_ref() {
                                b"property" => column.property = bytes_to_str(&attr.value)?,
                                b"column" => column.column = bytes_to_str(&attr.value)?,
                                b"javaType" => column.java_type = Some(bytes_to_str(&attr.value)?),
                                b"jdbcType" => column.jdbc_type = Some(bytes_to_str(&attr.value)?),
                                _ => {}
                            }
                        }
                        result_map.result_columns.push(column);
                        self.reader.read_event_into(&mut self.buf)?;
                    },
                    _ => { self.skip_element()?; }
                },
                Ok(Event::End(_)) => break,
                Ok(Event::Eof) => break,
                Err(e) => return Err(MapperError::from(e)),
                _ => {}
            }
        }

        Ok(result_map)
    }

    /// 提取SQL中的参数（支持 #{param} 和 ${param} 两种格式）
    fn extract_parameters(sql: &str) -> Vec<String> {
        use std::collections::HashSet;
        let mut params = HashSet::new();
        let mut chars = sql.chars().peekable();

        while let Some(c) = chars.next() {
            if (c == '#' || c == '$') && chars.next_if_eq(&'{').is_some() {
                let mut param = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '}' {
                        chars.next();
                        break;
                    }
                    param.push(chars.next().unwrap());
                }
                let param_name = param.split(|c| c == ':' || c == ',').next().unwrap_or(&param).trim();
                if !param_name.is_empty() {
                    params.insert(param_name.to_string());
                }
            }
        }

        params.into_iter().collect()
    }

    /// 跳过未知元素的完整子树
    fn skip_element(&mut self) -> Result<(), MapperError> {
        let mut depth = 1;
        loop {
            match self.reader.read_event_into(&mut self.buf)? {
                Event::Start(_) | Event::Empty(_) => depth += 1,
                Event::End(_) => depth -= 1,
                Event::Eof => break,
                _ => {},
            }
            if depth == 0 {
                break;
            }
        }
        Ok(())
    }
}
