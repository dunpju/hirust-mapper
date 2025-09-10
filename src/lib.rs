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
        <select id="test_foreach">
        SELECT * FROM tab1 where column155555 in
        <foreach collection="list" index="index" item="item" open="(" separator="," close=")">
            #{item}
        </foreach>
    </select>
    <sql id="sql1">
        select a,b,c,d,e,f,g
    </sql>
    <select id="select0">
        <include refid="sql1"></include>
        from tab1
    </select>
    <insert id="insert2">
        insert into tab2 (ID) values (#{id})
    </insert>
    <insert id="batchInsert">
        INSERT INTO book_attach_ocr_result(
            book_attach_ocr_task_id, book_attach_id
        )
        VALUES
        <foreach collection="list" separator="," item="entity">
            (#{entity.bookAttachOcrTaskId}, #{entity.bookAttachId})
        </foreach>
    </insert>
    <update id="batchUpdateCaseWhen">
    UPDATE company
    <set>
    <trim prefix="`company_name`= CASE company_id" suffix="END,">
        <foreach collection="companies" item="company">
            WHEN #{company.companyId} THEN #{company.companyName}
        </foreach>
    </trim>
    <trim prefix="`is_delete` = CASE company_id" suffix="END,">
        <foreach collection="companies" item="company">
            WHEN #{company.companyId} THEN #{company.isDelete}
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

        //let xml_content = include_str!("../privilege_project.xml");

        // 解析XML
        let mut parser = MyBatisXmlParser::new(xml_content);
        let mapper = parser.parse_mapper().unwrap();
        println!("解析结果: {:?} \n", mapper);

        /*// 获取SQL语句
        if let Some(statement) = mapper.statements.get("findUserById") {
            // 准备参数
            let mut params: HashMap<String, Value> = HashMap::new();
            //params.insert("id".to_string(), Value::Number(1.into()));
            params.insert("name".to_string(), Value::String("张三".to_string()));

            // 生成最终SQL
            if let Some(dynamic_sql) = &statement.dynamic_sql {
                let sql = generate_sql(dynamic_sql, &params, &mapper);
                //println!("dynamic_sql内容: {:?}", dynamic_sql);
                println!("生成的SQL: {}", sql);
            }
        }
        // 获取SQL语句
        if let Some(statement) = mapper.statements.get("test_foreach") {
            // 准备参数
            let mut params: HashMap<String, Vec<Value>> = HashMap::new();
            params.insert("list".to_string(), vec![Value::Number(1.into()),
                                                   Value::Number(2.into()),
                                                   Value::Number(3.into())]);

            // 生成最终SQL
            if let Some(dynamic_sql) = &statement.dynamic_sql {
                //println!("dynamic_sql内容: {:?}", dynamic_sql);
                let sql = generate_sql(dynamic_sql, &params, &mapper);
                println!("生成的SQL: {}", sql);
            }
        }
        // 获取SQL语句
        if let Some(statement) = mapper.statements.get("select0") {
            // 添加调试信息
            //println!("SQL片段列表: {:?}", mapper.sql_fragments.keys());

            // 准备参数
            let params: HashMap<String, Vec<Value>> = HashMap::new();

            // 生成最终SQL
            if let Some(dynamic_sql) = &statement.dynamic_sql {
                //println!("dynamic_sql内容: {:?}", dynamic_sql);
                let sql = generate_sql(dynamic_sql, &params, &mapper);
                println!("生成的SQL: {}", sql);
            }
        }
        // 获取SQL语句
        if let Some(statement) = mapper.statements.get("insert2") {
            // 添加调试信息
            //println!("SQL片段列表: {:?}", mapper.sql_fragments.keys());

            // 准备参数
            let mut params: HashMap<String, Value> = HashMap::new();
            params.insert("id".to_string(), Value::Number(1.into()));

            // 生成最终SQL
            if let Some(dynamic_sql) = &statement.dynamic_sql {
                //println!("dynamic_sql内容: {:?}", dynamic_sql);
                let sql = generate_sql(dynamic_sql, &params, &mapper);
                println!("生成的SQL: {}", sql);
            }
        }
        // 获取SQL语句
        if let Some(statement) = mapper.statements.get("batchInsert") {
            // 添加调试信息
            //println!("SQL片段列表: {:?}", mapper.sql_fragments.keys());

            // 准备参数
            let mut params: HashMap<String, Vec<Value>> = HashMap::new();
            params.insert("list".to_string(), vec![Value::Number(1.into())]);
            // 生成最终SQL
            if let Some(dynamic_sql) = &statement.dynamic_sql {
                //println!("dynamic_sql内容: {:?}", dynamic_sql);
                let sql = generate_sql(dynamic_sql, &params, &mapper);
                println!("生成的SQL: {}", sql);
            }
        }*/
        // 获取SQL语句
        if let Some(statement) = mapper.statements.get("batchUpdateCaseWhen") {
            // 添加调试信息
            //println!("SQL片段列表: {:?}", mapper.sql_fragments.keys());

            // 准备参数
            let mut params: HashMap<String, Vec<Value>> = HashMap::new();
            params.insert("companies".to_string(), vec![Value::Number(1.into())]);
            // 生成最终SQL
            if let Some(dynamic_sql) = &statement.dynamic_sql {
                //println!("dynamic_sql内容: {:?}", dynamic_sql);
                let sql = generate_sql(dynamic_sql, &params, &mapper);
                println!("生成的SQL: {}", sql);
            }
        }
    }

    // cargo test update_case_when -- --show-output
    #[test]
    fn update_case_when() {
        // 示例XML内容
        let xml_content = r#"<?xml version="1.0" encoding="UTF-8"?>
    <mapper namespace="com.example.UserMapper">
       <update id="batchUpdateCaseWhen">
        UPDATE company
        <set>
        <trim prefix="`company_name`= CASE company_id" suffix="END,">
            <foreach collection="companies" item="company">
                WHEN #{company.companyId} THEN #{company.companyName}
            </foreach>
        </trim>
        <trim prefix="`is_delete` = CASE company_id" suffix="END,">
            <foreach collection="companies" item="company">
                WHEN #{company.companyId} THEN #{company.isDelete}
            </foreach>
        </trim>
        </trim>
        </set>
        <where>
            <foreach collection="companies" item="company" separator="AND">
                company_id = #{company.companyId}
            </foreach>
        </where>
        </update>
    </mapper>"#;

        //let xml_content = include_str!("../privilege_project.xml");

        // 解析XML
        let mut parser = MyBatisXmlParser::new(xml_content);
        let mapper = parser.parse_mapper().unwrap();
        println!("解析结果: {:?} \n", mapper);

        // 获取SQL语句
        if let Some(statement) = mapper.statements.get("batchUpdateCaseWhen") {
            // 添加调试信息
            //println!("SQL片段列表: {:?}", mapper.sql_fragments.keys());

            // 准备参数
            let mut params: HashMap<String, Vec<Value>> = HashMap::new();
            params.insert("companies".to_string(), vec![Value::Number(1.into())]);
            // 生成最终SQL
            if let Some(dynamic_sql) = &statement.dynamic_sql {
                //println!("dynamic_sql内容: {:?}", dynamic_sql);
                let sql = generate_sql(dynamic_sql, &params, &mapper);
                println!("生成的SQL: {}", sql);
            }
        }
    }

    // cargo test choose -- --show-output
    #[test]
    fn choose() {
        // 示例XML内容
        let xml_content = r#"<?xml version="1.0" encoding="UTF-8"?>
    <mapper namespace="com.example.UserMapper">
    <select id="getCourseExamList" resultType="com.qimingdaren.errorbook.dto.exam.ExamJoinSysExamTypeVO">
        <foreach collection="newExamCourseList" item="newExamCourse" separator="UNION">
            (SELECT
            A.examId,
            A.areaCode,
            A.startDate,
            A.examYear,
            A.examMonth,
            B.moduleType,
            #{newExamCourse.courseIds} AS courseIds,
            #{newExamCourse.uniqueKey} AS uniqueKey
            FROM
            exam A,
            sys_exam_type B
            WHERE
            A.examTypeId = B.sysExamTypeId
            AND A.examId IN
            <foreach collection="examIds" item="id" open="(" separator="," close=")">
                #{id}
            </foreach>
            AND A.examStatus IN (2, 3)
            AND A.isDelete = 0
            AND A.examId IN (
            SELECT
            C.examId
            FROM
            report_data C
            WHERE
            C.examId IN
            <foreach collection="examIds" item="id" open="(" separator="," close=")">
                #{id}
            </foreach>
            <choose>
                <when test="newExamCourse.selectContainCourse != null and newExamCourse.selectContainCourse != ''">
                    AND C.sysCourseId IN(#{newExamCourse.selectContainCourse})
                </when>
                <otherwise>
                    AND C.sysCourseId IN(0)
                </otherwise>
            </choose>
            )
            ORDER BY
            A.startDate DESC
            LIMIT 10)
        </foreach>
    </select>
    </mapper>"#;

        // 解析XML
        let mut parser = MyBatisXmlParser::new(xml_content);
        let mapper = parser.parse_mapper().unwrap();
        println!("解析结果: {:?} \n", mapper);

        // 获取SQL语句
        if let Some(statement) = mapper.statements.get("getCourseExamList") {
            // 添加调试信息
            //println!("SQL片段列表: {:?}", mapper.sql_fragments.keys());

            // 准备参数
            let mut params: HashMap<String, Value> = HashMap::new();
            // 创建newExamCourse对象
            let mut new_exam_course1 = serde_json::Map::new();
            new_exam_course1.insert("selectContainCourse".to_string(), Value::String("1,2,3".to_string()));
            new_exam_course1.insert("courseIds".to_string(), Value::String("1001".to_string()));
            new_exam_course1.insert("uniqueKey".to_string(), Value::String("test-key".to_string()));

            let mut new_exam_course2 = serde_json::Map::new();
            new_exam_course2.insert("selectContainCourse".to_string(), Value::String("1,2,3".to_string()));
            new_exam_course2.insert("courseIds".to_string(), Value::String("1001".to_string()));
            new_exam_course2.insert("uniqueKey".to_string(), Value::String("test-key".to_string()));

            // 将newExamCourse对象添加到newExamCourseList数组中
            let new_exam_course_list = vec![Value::Object(new_exam_course1), Value::Object(new_exam_course2)];

            // 添加examIds数组参数
            let exam_ids = vec![Value::Number(1.into()), Value::Number(2.into())];

            // 设置参数
            params.insert("newExamCourseList".to_string(), Value::Array(new_exam_course_list));
            params.insert("examIds".to_string(), Value::Array(exam_ids));
            // 生成最终SQL
            if let Some(dynamic_sql) = &statement.dynamic_sql {
                //println!("dynamic_sql内容: {:?}", dynamic_sql);
                let sql = generate_sql(dynamic_sql, &params, &mapper);
                println!("生成的SQL: {}", sql);
            }
        }
    }

}