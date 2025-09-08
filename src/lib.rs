pub mod mapper;
pub use mapper::*;

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use serde_json::Value;
    use crate::sql_generator::generate_sql;
    use super::*;

    // cargo test run -- --show-output
    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }

    // cargo test it_works2 -- --show-output
    #[test]
    fn it_works2() {
        // 示例XML内容
        let xml_content = r#"<?xml version="1.0" encoding="UTF-8"?>
    <mapper namespace="com.example.UserMapper">
        <select id="findUserById" parameterType="Long" resultType="User">
            SELECT * FROM users
            WHERE 1=1
            <if test="id != null">
                AND id = #{id}
            </if>
            <if test="name != null and name != ''">
                AND name = #{name}
            </if>
        </select>
    </mapper>"#;

        // 解析XML
        let mut parser = MyBatisXmlParser::new(xml_content);
        let mapper = parser.parse_mapper().unwrap();
        println!("解析结果: {:?}", mapper);

        // 获取SQL语句
        if let Some(statement) = mapper.statements.get("findUserById") {
            // 准备参数
            let mut params = HashMap::new();
            params.insert("id".to_string(), Value::Number(1.into()));
            params.insert("name".to_string(), Value::String("张三".to_string()));

            // 生成最终SQL
            if let Some(dynamic_sql) = &statement.dynamic_sql {
                let sql = generate_sql(dynamic_sql, &params);
                println!("生成的SQL: {}", sql);
            }
        }
    }
}