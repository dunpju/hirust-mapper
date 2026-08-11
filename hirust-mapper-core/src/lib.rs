//! # hirust-mapper-core
//!
//! MyBatis XML 动态 SQL 解析与生成的核心库。
//!
//! 本 crate 提供纯解析与生成能力，不包含数据库连接、事务管理等运行时功能。
//! 解析后的 `Mapper` 可通过 `build_sql` 方法根据参数生成最终 SQL。

pub mod model;
pub mod parser;
pub mod sql_generator;

pub use model::*;
pub use parser::*;
pub use sql_generator::ParamsAccess;
pub use sql_generator::generate_sql;
pub use sql_generator::generate_bound_sql;
pub use sql_generator::BoundSql;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use serde_json::Value;
    use super::*;

    /// 辅助函数：标准化SQL空白用于断言（去除多余空格、换行）
    fn normalize_sql(sql: &str) -> String {
        sql.split_whitespace().collect::<Vec<&str>>().join(" ")
    }

    #[test]
    fn parse_and_generate_if() {
        let xml = r#"<mapper namespace="com.example.UserMapper">
        <select id="findUserById" parameterType="Long" resultType="User">
            SELECT * FROM users WHERE 1=1
            <if test="id != null">AND id = #{id}</if>
            <if test="name != null and name != ''">AND name = #{name}</if>
        </select>
        </mapper>"#;

        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();

        let mut params: HashMap<String, Value> = HashMap::new();
        params.insert("id".to_string(), Value::Number(1.into()));
        params.insert("name".to_string(), Value::String("张三".to_string()));

        let sql = normalize_sql(&mapper.build_sql("findUserById", &params).unwrap());
        assert!(sql.contains("AND id = 1"), "SQL: {}", sql);
        assert!(sql.contains("AND name = '张三'"), "SQL: {}", sql);
    }

    #[test]
    fn test_foreach() {
        let xml = r#"<mapper namespace="com.example.UserMapper">
        <select id="test_foreach">
            SELECT * FROM tab1 where column1 in
            <foreach collection="list" index="index" item="item" open="(" separator="," close=")">
                #{item}
            </foreach>
        </select>
        </mapper>"#;

        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();

        let mut params: HashMap<String, Value> = HashMap::new();
        params.insert("list".to_string(), Value::Array(vec![
            Value::Number(1.into()), Value::Number(2.into()), Value::Number(3.into()),
        ]));

        let sql = normalize_sql(&mapper.build_sql("test_foreach", &params).unwrap());
        assert_eq!(sql, "SELECT * FROM tab1 where column1 in (1,2,3)");
    }

    #[test]
    fn test_include() {
        let xml = r#"<mapper namespace="com.example.UserMapper">
        <sql id="sql1">select a,b,c</sql>
        <select id="select0"><include refid="sql1"/> from tab1</select>
        </mapper>"#;

        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();
        let sql = normalize_sql(&mapper.build_sql("select0", &HashMap::new()).unwrap());
        assert!(sql.contains("select a,b,c"), "SQL: {}", sql);
        assert!(sql.contains("from tab1"), "SQL: {}", sql);
    }

    #[test]
    fn test_include_self_closing() {
        let xml = r#"<mapper namespace="com.example.UserMapper">
        <sql id="sql1">select a,b,c</sql>
        <select id="select1"><include refid="sql1"/> from tab1</select>
        </mapper>"#;

        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();
        let sql = normalize_sql(&mapper.build_sql("select1", &HashMap::new()).unwrap());
        assert!(sql.contains("select a,b,c"), "SQL: {}", sql);
    }

    #[test]
    fn test_insert() {
        let xml = r#"<mapper namespace="com.example.UserMapper">
        <insert id="insert2">
            insert into tab2 (ID) values (#{id})
        </insert>
        </mapper>"#;

        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();

        let mut params: HashMap<String, Value> = HashMap::new();
        params.insert("id".to_string(), Value::Number(42.into()));

        let sql = normalize_sql(&mapper.build_sql("insert2", &params).unwrap());
        assert!(sql.contains("insert into tab2 (ID) values (42)"), "SQL: {}", sql);
    }

    #[test]
    fn test_batch_insert() {
        let xml = r#"<mapper namespace="com.example.UserMapper">
        <insert id="batchInsert">
            INSERT INTO book_attach_ocr_result(book_attach_ocr_task_id, book_attach_id) VALUES
            <foreach collection="list" separator="," item="entity">
                (#{entity.bookAttachOcrTaskId}, #{entity.bookAttachId})
            </foreach>
        </insert>
        </mapper>"#;

        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();

        let mut entity1 = serde_json::Map::new();
        entity1.insert("bookAttachOcrTaskId".to_string(), Value::Number(1.into()));
        entity1.insert("bookAttachId".to_string(), Value::Number(2.into()));

        let mut params: HashMap<String, Value> = HashMap::new();
        params.insert("list".to_string(), Value::Array(vec![Value::Object(entity1)]));

        let sql = normalize_sql(&mapper.build_sql("batchInsert", &params).unwrap());
        assert!(sql.contains("(1, 2)"), "SQL: {}", sql);
    }

    #[test]
    fn test_batch_update_case_when() {
        let xml = r#"<mapper namespace="com.example.UserMapper">
        <update id="batchUpdateCaseWhen">
            UPDATE company
            <set>
                <trim prefix="`company_name`= CASE company_id" suffix="END,">
                    <foreach collection="companies" item="company">
                        WHEN #{company.companyId} THEN #{company.companyName}
                    </foreach>
                </trim>
            </set>
            <where>
                company_id in
                <foreach collection="companies" item="company" separator="," open="(" close=")">
                    #{company.companyId}
                </foreach>
            </where>
        </update>
        </mapper>"#;

        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();

        let mut company = serde_json::Map::new();
        company.insert("companyId".to_string(), Value::Number(1.into()));
        company.insert("companyName".to_string(), Value::String("Test".to_string()));

        let mut params: HashMap<String, Value> = HashMap::new();
        params.insert("companies".to_string(), Value::Array(vec![Value::Object(company)]));

        let sql = normalize_sql(&mapper.build_sql("batchUpdateCaseWhen", &params).unwrap());
        assert!(sql.contains("UPDATE company"), "SQL: {}", sql);
        assert!(sql.contains("SET"), "SQL: {}", sql);
        assert!(sql.contains("WHERE"), "SQL: {}", sql);
    }

    #[test]
    fn test_choose_with_nested_foreach() {
        let xml = r#"<mapper namespace="com.example.UserMapper">
        <select id="getCourseExamList">
            <foreach collection="newExamCourseList" item="newExamCourse" separator="UNION">
                (SELECT #{newExamCourse.courseIds} AS courseIds
                FROM exam A WHERE A.examId IN
                <foreach collection="examIds" item="id" open="(" separator="," close=")">
                    #{id}
                </foreach>
                <choose>
                    <when test="newExamCourse.selectContainCourse != null and newExamCourse.selectContainCourse != ''">
                        AND A.sysCourseId IN(${newExamCourse.selectContainCourse})
                    </when>
                    <otherwise>
                        AND A.sysCourseId IN(0)
                    </otherwise>
                </choose>
                LIMIT 10)
            </foreach>
        </select>
        </mapper>"#;

        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();

        let mut new_exam_course = serde_json::Map::new();
        new_exam_course.insert("courseIds".to_string(), Value::String("1001".to_string()));
        new_exam_course.insert("selectContainCourse".to_string(), Value::String("1,2,3".to_string()));

        let mut new_exam_course2 = serde_json::Map::new();
        new_exam_course2.insert("courseIds".to_string(), Value::String("1002".to_string()));
        new_exam_course2.insert("selectContainCourse".to_string(), Value::String("4,5,6".to_string()));

        let mut params: HashMap<String, Value> = HashMap::new();
        params.insert("newExamCourseList".to_string(), Value::Array(vec![
            Value::Object(new_exam_course), Value::Object(new_exam_course2),
        ]));
        params.insert("examIds".to_string(), Value::Array(vec![Value::Number(1.into()), Value::Number(2.into())]));

        let sql = normalize_sql(&mapper.build_sql("getCourseExamList", &params).unwrap());
        assert!(sql.contains("UNION"), "SQL should contain UNION (2 items): {}", sql);
        assert!(sql.contains("IN (1,2)"), "SQL should contain IN(1,2): {}", sql);
        assert!(sql.contains("IN(1,2,3)"), "SQL should contain IN(1,2,3): {}", sql);
    }

    #[test]
    fn test_insert_duplicate_key_update() {
        let xml = r#"<mapper namespace="com.example.UserMapper">
        <insert id="insertDuplicateKeyUpdate" useGeneratedKeys="true" keyProperty="bookSchoolId">
            INSERT INTO book_school (book_id, school_id, is_delete, create_time, update_time) VALUES
            <foreach collection="entityList" item="entity" separator=",">
                (#{entity.bookId}, #{entity.schoolId}, 1, NOW(), NOW())
            </foreach>
            ON DUPLICATE KEY UPDATE
            is_delete = values(is_delete)
        </insert>
        </mapper>"#;

        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();

        let mut entity1 = serde_json::Map::new();
        entity1.insert("bookId".to_string(), Value::Number(100.into()));
        entity1.insert("schoolId".to_string(), Value::Number(200.into()));
        let mut entity2 = serde_json::Map::new();
        entity2.insert("bookId".to_string(), Value::Number(101.into()));
        entity2.insert("schoolId".to_string(), Value::Number(201.into()));

        let mut params: HashMap<String, Value> = HashMap::new();
        params.insert("entityList".to_string(), Value::Array(vec![
            Value::Object(entity1), Value::Object(entity2),
        ]));

        let sql = normalize_sql(&mapper.build_sql("insertDuplicateKeyUpdate", &params).unwrap());
        assert!(sql.contains("(100, 200, 1,"), "SQL should contain (100,200,1,): {}", sql);
        assert!(sql.contains("ON DUPLICATE KEY UPDATE"), "SQL: {}", sql);
    }

    #[test]
    fn test_missing_param_produces_marker() {
        let xml = r#"<mapper namespace="com.example.UserMapper">
        <select id="t">SELECT #{missing_param}</select>
        </mapper>"#;

        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();
        let sql = mapper.build_sql("t", &HashMap::new()).unwrap();
        assert!(sql.contains("/* MISSING:#missing_param */"), "SQL: {}", sql);
    }

    #[test]
    fn test_missing_fragment_returns_error() {
        let xml = r#"<mapper namespace="com.example.UserMapper">
        <select id="t"><include refid="nonexistent"/></select>
        </mapper>"#;

        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();
        let result = mapper.build_sql("t", &HashMap::new());
        assert!(result.is_err(), "Expected error for missing fragment, got: {:?}", result);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("nonexistent"), "Error message should mention fragment id: {}", err);
    }

    #[test]
    fn test_statement_not_found() {
        let xml = r#"<mapper namespace="com.example.UserMapper"></mapper>"#;
        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();
        let result = mapper.build_sql("nonexistent", &HashMap::new());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nonexistent"));
    }

    #[test]
    fn test_or_condition() {
        let xml = r#"<mapper namespace="com.example.UserMapper">
        <select id="t">
            SELECT * FROM t WHERE 1=1
            <if test="a == 1 or b == 'hello'">AND extra = 1</if>
        </select>
        </mapper>"#;

        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();

        let mut p1: HashMap<String, Value> = HashMap::new();
        p1.insert("a".to_string(), Value::Number(1.into()));
        assert!(mapper.build_sql("t", &p1).unwrap().contains("AND extra = 1"));

        let mut p2: HashMap<String, Value> = HashMap::new();
        p2.insert("a".to_string(), Value::Number(99.into()));
        p2.insert("b".to_string(), Value::String("hello".to_string()));
        assert!(mapper.build_sql("t", &p2).unwrap().contains("AND extra = 1"));

        let mut p3: HashMap<String, Value> = HashMap::new();
        p3.insert("a".to_string(), Value::Number(99.into()));
        p3.insert("b".to_string(), Value::String("nope".to_string()));
        assert!(!mapper.build_sql("t", &p3).unwrap().contains("AND extra = 1"));
    }

    #[test]
    fn test_bind_tag() {
        let xml = r#"<mapper namespace="com.example.UserMapper">
        <select id="t">
            <bind name="pattern" value="'%' + name + '%'"/>
            SELECT * FROM t WHERE name LIKE #{pattern}
        </select>
        </mapper>"#;

        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();

        let mut params: HashMap<String, Value> = HashMap::new();
        params.insert("name".to_string(), Value::String("test".to_string()));
        let sql = mapper.build_sql("t", &params).unwrap();
        assert!(sql.contains("LIKE"), "SQL: {}", sql);
    }

    #[test]
    fn test_error_display() {
        let err = MapperError::MissingParam {
            param: "foo".to_string(),
            context: "findUser".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("foo"), "Error message: {}", msg);
        assert!(msg.contains("findUser"), "Error message: {}", msg);
    }

    // ─── BoundSql（P4）测试 ─────────────────────────────────────────

    /// 辅助：解析 XML 并生成 BoundSql
    fn build_bound(xml: &str, stmt_id: &str, params: &HashMap<String, Value>) -> BoundSql {
        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();
        mapper.build_bound_sql(stmt_id, params).unwrap()
    }

    #[test]
    fn bound_sql_hash_becomes_placeholder() {
        let xml = r#"<mapper namespace="t">
        <select id="findById">SELECT * FROM users WHERE id = #{id}</select>
        </mapper>"#;

        let mut params = HashMap::new();
        params.insert("id".to_string(), Value::Number(42.into()));

        let bound = build_bound(xml, "findById", &params);
        assert_eq!(bound.sql, "SELECT * FROM users WHERE id = ?");
        assert_eq!(bound.param_count(), 1);
        assert_eq!(bound.parameters[0], Value::Number(42.into()));
    }

    #[test]
    fn bound_sql_string_param_not_inlined() {
        // 关键差异：字符串值不再内联加引号，而是作为参数
        let xml = r#"<mapper namespace="t">
        <select id="findByName">SELECT * FROM users WHERE name = #{name}</select>
        </mapper>"#;

        let mut params = HashMap::new();
        params.insert("name".to_string(), Value::String("O'Brien".to_string()));

        let bound = build_bound(xml, "findByName", &params);
        assert_eq!(bound.sql, "SELECT * FROM users WHERE name = ?");
        assert_eq!(bound.parameters[0], Value::String("O'Brien".to_string()));
        // 验证 SQL 中不含内联的引号或转义
        assert!(!bound.sql.contains("O'Brien"), "SQL should not inline value: {}", bound.sql);
    }

    #[test]
    fn bound_sql_dollar_still_inlined() {
        // ${} 必须保持内联行为（如动态表名 / 排序字段）
        let xml = r#"<mapper namespace="t">
        <select id="dynamic">SELECT * FROM ${table} ORDER BY ${col}</select>
        </mapper>"#;

        let mut params = HashMap::new();
        params.insert("table".to_string(), Value::String("users".to_string()));
        params.insert("col".to_string(), Value::String("name".to_string()));

        let bound = build_bound(xml, "dynamic", &params);
        assert_eq!(bound.sql, "SELECT * FROM users ORDER BY name");
        assert_eq!(bound.param_count(), 0); // ${} 不产生参数
    }

    #[test]
    fn bound_sql_mixed_mode_hash_and_dollar() {
        // 混合模式：同时包含 #{} (?) 和 ${} (内联)
        let xml = r#"<mapper namespace="t">
        <select id="mixed">SELECT * FROM ${table} WHERE id = #{id} AND status = #{status}</select>
        </mapper>"#;

        let mut params = HashMap::new();
        params.insert("table".to_string(), Value::String("orders".to_string()));
        params.insert("id".to_string(), Value::Number(7.into()));
        params.insert("status".to_string(), Value::String("active".to_string()));

        let bound = build_bound(xml, "mixed", &params);
        assert_eq!(bound.sql, "SELECT * FROM orders WHERE id = ? AND status = ?");
        assert_eq!(bound.param_count(), 2);
        // 参数顺序与 ? 出现顺序一致
        assert_eq!(bound.parameters[0], Value::Number(7.into()));
        assert_eq!(bound.parameters[1], Value::String("active".to_string()));
    }

    #[test]
    fn bound_sql_param_order_preserved() {
        // 多个 #{param} 的顺序必须与 ? 出现顺序严格对应
        let xml = r#"<mapper namespace="t">
        <select id="insert">INSERT INTO t (a, b, c, d) VALUES (#{a}, #{b}, #{c}, #{d})</select>
        </mapper>"#;

        let mut params = HashMap::new();
        params.insert("a".to_string(), Value::Number(1.into()));
        params.insert("b".to_string(), Value::String("two".to_string()));
        params.insert("c".to_string(), Value::Bool(true));
        params.insert("d".to_string(), Value::Number(4.into()));

        let bound = build_bound(xml, "insert", &params);
        assert_eq!(bound.sql, "INSERT INTO t (a, b, c, d) VALUES (?, ?, ?, ?)");
        assert_eq!(bound.param_count(), 4);
        assert_eq!(bound.parameters[0], Value::Number(1.into()));
        assert_eq!(bound.parameters[1], Value::String("two".to_string()));
        assert_eq!(bound.parameters[2], Value::Bool(true));
        assert_eq!(bound.parameters[3], Value::Number(4.into()));
    }

    #[test]
    fn bound_sql_foreach_placeholder_count() {
        // foreach 展开应产生与元素数相等的 ? 占位符
        let xml = r#"<mapper namespace="t">
        <select id="inClause">
            SELECT * FROM tab1 WHERE column1 IN
            <foreach collection="list" item="item" open="(" separator="," close=")">
                #{item}
            </foreach>
        </select>
        </mapper>"#;

        let mut params = HashMap::new();
        params.insert("list".to_string(), Value::Array(vec![
            Value::Number(1.into()), Value::Number(2.into()), Value::Number(3.into()),
        ]));

        let bound = build_bound(xml, "inClause", &params);
        assert_eq!(bound.sql, "SELECT * FROM tab1 WHERE column1 IN (?,?,?)");
        assert_eq!(bound.param_count(), 3);
        assert_eq!(bound.parameters[0], Value::Number(1.into()));
        assert_eq!(bound.parameters[2], Value::Number(3.into()));
    }

    #[test]
    fn bound_sql_if_conditional() {
        let xml = r#"<mapper namespace="t">
        <select id="find">
            SELECT * FROM users WHERE 1=1
            <if test="id != null">AND id = #{id}</if>
            <if test="name != null and name != ''">AND name = #{name}</if>
        </select>
        </mapper>"#;

        // 仅 id 存在
        let mut params = HashMap::new();
        params.insert("id".to_string(), Value::Number(1.into()));
        let bound = build_bound(xml, "find", &params);
        assert_eq!(bound.sql, "SELECT * FROM users WHERE 1=1 AND id = ?");
        assert_eq!(bound.param_count(), 1);

        // id + name 都存在
        let mut params = HashMap::new();
        params.insert("id".to_string(), Value::Number(1.into()));
        params.insert("name".to_string(), Value::String("张三".to_string()));
        let bound = build_bound(xml, "find", &params);
        assert_eq!(bound.sql, "SELECT * FROM users WHERE 1=1 AND id = ? AND name = ?");
        assert_eq!(bound.param_count(), 2);
    }

    #[test]
    fn bound_sql_where_and_set_tags() {
        let xml = r#"<mapper namespace="t">
        <update id="update">
            UPDATE users
            <set>
                <if test="name != null">name = #{name},</if>
                <if test="age != null">age = #{age},</if>
            </set>
            <where>
                <if test="id != null">id = #{id}</if>
            </where>
        </update>
        </mapper>"#;

        let mut params = HashMap::new();
        params.insert("name".to_string(), Value::String("李四".to_string()));
        params.insert("age".to_string(), Value::Number(30.into()));
        params.insert("id".to_string(), Value::Number(5.into()));

        let bound = build_bound(xml, "update", &params);
        // SET 后逗号被剥离，WHERE 前缀正确
        assert!(bound.sql.contains("SET name = ?, age = ?"), "SQL: {}", bound.sql);
        assert!(bound.sql.contains("WHERE id = ?"), "SQL: {}", bound.sql);
        assert!(!bound.sql.contains(",,"), "no double comma: {}", bound.sql);
        assert_eq!(bound.param_count(), 3);
        // 顺序：name, age, id
        assert_eq!(bound.parameters[0], Value::String("李四".to_string()));
        assert_eq!(bound.parameters[1], Value::Number(30.into()));
        assert_eq!(bound.parameters[2], Value::Number(5.into()));
    }

    #[test]
    fn bound_sql_choose_with_dollar() {
        // choose 中 when 使用 ${} (内联)，otherwise 用字面量
        let xml = r#"<mapper namespace="t">
        <select id="q">
            SELECT * FROM t WHERE 1=1
            <choose>
                <when test="filter != null">AND id IN (${filter})</when>
                <otherwise>AND id = #{defaultId}</otherwise>
            </choose>
        </select>
        </mapper>"#;

        // when 分支命中 → ${} 内联，无参数
        let mut params = HashMap::new();
        params.insert("filter".to_string(), Value::String("1,2,3".to_string()));
        let bound = build_bound(xml, "q", &params);
        assert_eq!(bound.sql, "SELECT * FROM t WHERE 1=1 AND id IN (1,2,3)");
        assert_eq!(bound.param_count(), 0);

        // otherwise 分支 → defaultId 参数化（filter 不存在 → 走 otherwise）
        let mut params2 = HashMap::new();
        params2.insert("defaultId".to_string(), Value::Number(99.into()));
        let bound = build_bound(xml, "q", &params2);
        assert_eq!(bound.sql, "SELECT * FROM t WHERE 1=1 AND id = ?");
        assert_eq!(bound.param_count(), 1);
        assert_eq!(bound.parameters[0], Value::Number(99.into()));

        // 缺失参数 → 标记，不产生 ?（otherwise 命中但 defaultId 缺失）
        let bound_missing = build_bound(xml, "q", &HashMap::new());
        assert!(bound_missing.sql.contains("MISSING"), "SQL: {}", bound_missing.sql);
        assert_eq!(bound_missing.param_count(), 0);
    }

    #[test]
    fn bound_sql_bind_tag() {
        let xml = r#"<mapper namespace="t">
        <select id="like">
            <bind name="pattern" value="%${name}%"/>
            SELECT * FROM t WHERE name LIKE #{pattern}
        </select>
        </mapper>"#;

        let mut params = HashMap::new();
        params.insert("name".to_string(), Value::String("test".to_string()));

        let bound = build_bound(xml, "like", &params);
        assert_eq!(bound.sql, "SELECT * FROM t WHERE name LIKE ?");
        assert_eq!(bound.param_count(), 1);
        // bind 解析后 pattern = "%test%"（${name} 内联为 test），作为参数
        match &bound.parameters[0] {
            Value::String(s) => assert!(s.contains("test"), "pattern: {}", s),
            other => panic!("expected string param, got {:?}", other),
        }
    }

    #[test]
    fn bound_sql_include_fragment() {
        let xml = r#"<mapper namespace="t">
        <sql id="cols">a, b, c</sql>
        <select id="sel">SELECT <include refid="cols"/> FROM tab WHERE id = #{id}</select>
        </mapper>"#;

        let mut params = HashMap::new();
        params.insert("id".to_string(), Value::Number(9.into()));

        let bound = build_bound(xml, "sel", &params);
        assert_eq!(bound.sql, "SELECT a, b, c FROM tab WHERE id = ?");
        assert_eq!(bound.param_count(), 1);
    }

    #[test]
    fn bound_sql_missing_param_marker() {
        // 缺失参数：保持与内联模式一致的 MISSING 标记，且不产生多余的 ?
        let xml = r#"<mapper namespace="t">
        <select id="m">SELECT #{missing}</select>
        </mapper>"#;

        let bound = build_bound(xml, "m", &HashMap::new());
        assert!(bound.sql.contains("/* MISSING:#missing */"), "SQL: {}", bound.sql);
        assert_eq!(bound.param_count(), 0); // 缺失参数不进列表
    }

    #[test]
    fn bound_sql_static_statement() {
        // 纯静态 SQL（无 dynamic_sql）也应走绑定路径
        let xml = r#"<mapper namespace="t">
        <select id="static">SELECT * FROM t WHERE a = #{a} AND b = #{b}</select>
        </mapper>"#;

        let mut params = HashMap::new();
        params.insert("a".to_string(), Value::Number(1.into()));
        params.insert("b".to_string(), Value::String("x".to_string()));

        let bound = build_bound(xml, "static", &params);
        assert_eq!(bound.sql, "SELECT * FROM t WHERE a = ? AND b = ?");
        assert_eq!(bound.param_count(), 2);
    }

    #[test]
    fn bound_sql_no_params() {
        // 无任何占位符的 SQL
        let xml = r#"<mapper namespace="t">
        <select id="all">SELECT * FROM t</select>
        </mapper>"#;

        let bound = build_bound(xml, "all", &HashMap::new());
        assert_eq!(bound.sql, "SELECT * FROM t");
        assert!(!bound.has_params());
        assert_eq!(bound.param_count(), 0);
    }

    #[test]
    fn bound_sql_consistent_with_inline_for_structure() {
        // 验证：结构上 bound.sql 去掉 ? 替换后，与内联模式的非值部分一致
        // （验证绑定模式没有破坏动态结构的求值）
        let xml = r#"<mapper namespace="t">
        <select id="q">
            SELECT * FROM users
            <where>
                <if test="id != null">AND id = #{id}</if>
                <if test="name != null">AND name = #{name}</if>
            </where>
        </select>
        </mapper>"#;

        let mut params = HashMap::new();
        params.insert("id".to_string(), Value::Number(1.into()));
        params.insert("name".to_string(), Value::String("a".to_string()));

        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();
        let inline = mapper.build_sql("q", &params).unwrap();
        let bound = mapper.build_bound_sql("q", &params).unwrap();

        // 内联模式: ...WHERE id = 1 AND name = 'a'
        // 绑定模式: ...WHERE id = ? AND name = ?
        // 结构前缀（WHERE/AND）应一致
        assert!(inline.contains("WHERE id ="), "inline: {}", inline);
        assert!(bound.sql.contains("WHERE id = ?"), "bound: {}", bound.sql);
        assert!(bound.sql.contains("AND name = ?"), "bound: {}", bound.sql);
    }

    // ─── P8：ResultMap 增强 + selectKey + .size()/.isEmpty() 测试 ─────

    #[test]
    fn p8_parse_result_map_with_id_result() {
        let xml = r#"<mapper namespace="t">
        <resultMap id="userMap" type="User">
            <id property="id" column="user_id"/>
            <result property="name" column="user_name" rustType="String"/>
        </resultMap>
        </mapper>"#;
        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();
        let rm = mapper.result_maps.get("userMap").unwrap();
        assert_eq!(rm.type_name, "User");
        assert_eq!(rm.result_columns.len(), 2);
        assert!(rm.result_columns[0].is_id, "<id> 应标记为 id");
        assert_eq!(rm.result_columns[0].property, "id");
        assert_eq!(rm.result_columns[0].column, "user_id");
        assert!(!rm.result_columns[1].is_id, "<result> 不是 id");
        assert_eq!(rm.result_columns[1].rust_type.as_deref(), Some("String"));
        assert!(rm.associations.is_empty());
        assert!(rm.collections.is_empty());
    }

    #[test]
    fn p8_parse_result_map_self_closing() {
        // 自闭合形式 <id/> <result/>
        let xml = r#"<mapper namespace="t">
        <resultMap id="m" type="User">
            <id property="id" column="id"/>
            <result property="name" column="name"/>
        </resultMap>
        </mapper>"#;
        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();
        let rm = mapper.result_maps.get("m").unwrap();
        assert_eq!(rm.result_columns.len(), 2);
    }

    #[test]
    fn p8_parse_association_and_collection() {
        let xml = r#"<mapper namespace="t">
        <resultMap id="userMap" type="User">
            <id property="id" column="id"/>
            <result property="name" column="name"/>
            <association property="department" javaType="Department">
                <id property="id" column="dept_id"/>
                <result property="name" column="dept_name"/>
            </association>
            <collection property="roles" ofType="Role">
                <id property="id" column="role_id"/>
                <result property="name" column="role_name"/>
            </collection>
        </resultMap>
        </mapper>"#;
        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();
        let rm = mapper.result_maps.get("userMap").unwrap();

        assert_eq!(rm.associations.len(), 1, "应解析 1 个 association");
        let assoc = &rm.associations[0];
        assert_eq!(assoc.property, "department");
        assert_eq!(assoc.nested_type.as_deref(), Some("Department"));
        assert_eq!(assoc.result_columns.len(), 2);
        assert!(assoc.result_columns[0].is_id);

        assert_eq!(rm.collections.len(), 1, "应解析 1 个 collection");
        let coll = &rm.collections[0];
        assert_eq!(coll.property, "roles");
        assert_eq!(coll.nested_type.as_deref(), Some("Role"));
        assert_eq!(coll.result_columns.len(), 2);
    }

    #[test]
    fn p8_parse_nested_association_inside_collection() {
        // 深层嵌套：collection 内含 association
        let xml = r#"<mapper namespace="t">
        <resultMap id="m" type="User">
            <id property="id" column="id"/>
            <collection property="orders" ofType="Order">
                <id property="id" column="order_id"/>
                <association property="addr" javaType="Addr">
                    <result property="city" column="city"/>
                </association>
            </collection>
        </resultMap>
        </mapper>"#;
        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();
        let rm = mapper.result_maps.get("m").unwrap();
        let coll = &rm.collections[0];
        assert_eq!(coll.associations.len(), 1, "collection 内应含 association");
        assert_eq!(coll.associations[0].property, "addr");
        assert_eq!(coll.associations[0].result_columns[0].column, "city");
    }

    #[test]
    fn p8_parse_select_key() {
        let xml = r#"<mapper namespace="t">
        <insert id="insertWithKey">
            <selectKey keyProperty="id" resultType="i64" order="AFTER">
                SELECT LAST_INSERT_ID()
            </selectKey>
            INSERT INTO users (name) VALUES (#{name})
        </insert>
        </mapper>"#;
        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();
        let stmt = mapper.statements.get("insertWithKey").unwrap();
        let sk = stmt.select_key.as_ref().expect("应解析 selectKey");
        assert_eq!(sk.key_property, "id");
        assert_eq!(sk.result_type, "i64");
        assert_eq!(sk.order, crate::SelectKeyOrder::After);
        assert!(sk.sql.contains("LAST_INSERT_ID"), "selectKey SQL: {}", sk.sql);
        // selectKey 不应混入主 SQL
        assert!(!stmt.sql.contains("LAST_INSERT_ID"), "主 SQL 不应含 selectKey: {}", stmt.sql);
        assert!(stmt.sql.contains("INSERT INTO users"), "主 SQL: {}", stmt.sql);
    }

    #[test]
    fn p8_parse_select_key_before_order() {
        let xml = r#"<mapper namespace="t">
        <insert id="i">
            <selectKey keyProperty="id" resultType="i64" order="BEFORE">
                SELECT seq.nextval
            </selectKey>
            INSERT INTO t VALUES (#{id})
        </insert>
        </mapper>"#;
        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();
        let sk = mapper.statements.get("i").unwrap().select_key.as_ref().unwrap();
        assert_eq!(sk.order, crate::SelectKeyOrder::Before);
    }

    // ─── .size() / .isEmpty() 条件测试 ──────────────────────────────

    fn sql_for(xml: &str, id: &str, params: &HashMap<String, Value>) -> String {
        let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();
        mapper.build_sql(id, params).unwrap()
    }

    #[test]
    fn p8_condition_size_greater_than_zero() {
        let xml = r#"<mapper namespace="t">
        <select id="q">
            SELECT * FROM t
            <if test="list != null and list.size() > 0">
                WHERE id IN (<foreach collection="list" item="x" separator=",">#{x}</foreach>)
            </if>
        </select>
        </mapper>"#;

        // 非空列表 → 条件成立
        let mut p = HashMap::new();
        p.insert("list".to_string(), Value::Array(vec![Value::Number(1.into())]));
        let sql = sql_for(xml, "q", &p);
        assert!(sql.contains("WHERE id IN"), "非空列表应进入条件: {}", sql);

        // 空列表 → 条件不成立（size() == 0）
        let mut p = HashMap::new();
        p.insert("list".to_string(), Value::Array(vec![]));
        let sql = sql_for(xml, "q", &p);
        assert!(!sql.contains("WHERE"), "空列表应跳过条件: {}", sql);
    }

    #[test]
    fn p8_condition_isempty() {
        let xml = r#"<mapper namespace="t">
        <select id="q">
            SELECT * FROM t
            <if test="name.isEmpty() == false">WHERE active = 1</if>
        </select>
        </mapper>"#;

        // 非空字符串 → isEmpty() == false → true
        let mut p = HashMap::new();
        p.insert("name".to_string(), Value::String("hi".into()));
        let sql = sql_for(xml, "q", &p);
        assert!(sql.contains("WHERE active = 1"), "非空应进入: {}", sql);

        // 空字符串 → isEmpty() == true → 条件 (== false) 不成立
        let mut p = HashMap::new();
        p.insert("name".to_string(), Value::String("".into()));
        let sql = sql_for(xml, "q", &p);
        assert!(!sql.contains("WHERE"), "空字符串应跳过: {}", sql);
    }

    #[test]
    fn p8_condition_bool_literal_comparison() {
        let xml = r#"<mapper namespace="t">
        <select id="q">SELECT 1<if test="flag == true"> WHERE a=1</if></select>
        </mapper>"#;
        let mut p = HashMap::new();
        p.insert("flag".to_string(), Value::Bool(true));
        assert!(sql_for(xml, "q", &p).contains("WHERE a=1"));

        let mut p = HashMap::new();
        p.insert("flag".to_string(), Value::Bool(false));
        assert!(!sql_for(xml, "q", &p).contains("WHERE"));
    }

    #[test]
    fn p8_condition_size_with_string() {
        // 字符串的 .size() → 字符数
        let xml = r#"<mapper namespace="t">
        <select id="q">SELECT 1<if test="s.size() >= 3"> WHERE ok=1</if></select>
        </mapper>"#;
        let mut p = HashMap::new();
        p.insert("s".to_string(), Value::String("abc".into())); // size 3
        assert!(sql_for(xml, "q", &p).contains("WHERE ok=1"));

        let mut p = HashMap::new();
        p.insert("s".to_string(), Value::String("a".into())); // size 1
        assert!(!sql_for(xml, "q", &p).contains("WHERE"));
    }

    #[test]
    fn p8_condition_size_missing_param_is_zero() {
        let xml = r#"<mapper namespace="t">
        <select id="q">SELECT 1<if test="missing.size() > 0"> WHERE x=1</if></select>
        </mapper>"#;
        // 缺失参数 .size() → 0，条件不成立
        assert!(!sql_for(xml, "q", &HashMap::new()).contains("WHERE"));
    }
}
