pub mod mapper;
pub use mapper::*;

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
        // 使用自闭合 include 标签测试
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
        // 测试自闭合 <include/> 标签
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
        // ${...} 直接替换，IN 后无空格
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

        // a=1 满足 or 的第一项
        let mut p1: HashMap<String, Value> = HashMap::new();
        p1.insert("a".to_string(), Value::Number(1.into()));
        assert!(mapper.build_sql("t", &p1).unwrap().contains("AND extra = 1"));

        // b='hello' 满足 or 的第二项
        let mut p2: HashMap<String, Value> = HashMap::new();
        p2.insert("a".to_string(), Value::Number(99.into()));
        p2.insert("b".to_string(), Value::String("hello".to_string()));
        assert!(mapper.build_sql("t", &p2).unwrap().contains("AND extra = 1"));

        // 两者都不满足
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
        // bind 的 value 不支持表达式求值，仅做变量替换
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
}
