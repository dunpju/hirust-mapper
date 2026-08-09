# hirust-mapper

用 Rust 实现的 [MyBatis](https://mybatis.org/mybatis-3/) 动态 SQL 解析与生成引擎。解析 MyBatis XML 映射文件，根据运行时参数动态组装 SQL 语句。

## 特性

- **完整动态 SQL 支持** — `<if>`、`<choose>/<when>/<otherwise>`、`<foreach>`、`<where>`、`<set>`、`<trim>`、`<bind>`、`<include>`、`<sql>` 片段
- **条件表达式** — 支持 `and`、`or` 逻辑组合及 `=`、`!=`、`>`、`<`、`>=`、`<=` 比较运算符，`and` 优先级高于 `or`
- **嵌套属性访问** — 参数支持点号路径（如 `#{company.companyId}`）
- **两种参数占位符** — `#{param}` 自动加引号（安全），`${param}` 原样拼接（用于 IN 列表等场景）
- **CDATA 支持** — 正确处理 `<![CDATA[...]]>` 段
- **自闭合标签** — 兼容 `<include refid="sql1"/>` 等自闭合写法
- **结构化错误** — `MapperError` 枚举提供清晰的错误分类（解析错误、参数缺失、片段不存在等）
- **SQL 注入防护** — `#{}` 模式自动转义单引号（`'` → `''`）

## 快速开始

### 添加依赖

```toml
[dependencies]
hirust-mapper = "0.1"
serde_json = "1"
```

### 基本用法

```rust
use hirust_mapper::*;
use serde_json::Value;
use std::collections::HashMap;

let xml = r#"
<mapper namespace="com.example.UserMapper">
    <sql id="baseColumns">id, name, email</sql>

    <select id="findUserById" resultType="User">
        SELECT <include refid="baseColumns"/>
        FROM users
        WHERE 1=1
        <if test="id != null">AND id = #{id}</if>
        <if test="name != null and name != ''">AND name = #{name}</if>
    </select>

    <select id="findByIds">
        SELECT * FROM users WHERE id IN
        <foreach collection="ids" item="id" open="(" separator="," close=")">
            #{id}
        </foreach>
    </select>
</mapper>
"#;

// 1. 解析 XML
let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();

// 2. 准备参数
let mut params = HashMap::new();
params.insert("id".to_string(), Value::Number(42.into()));
params.insert("name".to_string(), Value::String("张三".into()));

// 3. 生成 SQL
let sql = mapper.build_sql("findUserById", &params).unwrap();
// => SELECT id, name, email FROM users WHERE 1=1 AND id = 42 AND name = '张三'
```

### 批量操作

```rust
let xml = r#"
<mapper namespace="com.example.UserMapper">
    <insert id="batchInsert">
        INSERT INTO users (name, email) VALUES
        <foreach collection="list" item="user" separator=",">
            (#{user.name}, #{user.email})
        </foreach>
    </insert>
</mapper>
"#;

let mapper = MyBatisXmlParser::new(xml).parse_mapper().unwrap();

// 构建参数数组
let mut user1 = serde_json::Map::new();
user1.insert("name", Value::String("Alice".into()));
user1.insert("email", Value::String("alice@example.com".into()));

let mut user2 = serde_json::Map::new();
user2.insert("name", Value::String("Bob".into()));
user2.insert("email", Value::String("bob@example.com".into()));

let mut params = HashMap::new();
params.insert("list", Value::Array(vec![
    Value::Object(user1), Value::Object(user2),
]));

let sql = mapper.build_sql("batchInsert", &params).unwrap();
// => INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com'),('Bob', 'bob@example.com')
```

### 动态条件查询

```rust
let xml = r#"
<mapper namespace="com.example.OrderMapper">
    <select id="findOrders">
        SELECT * FROM orders
        <where>
            <if test="status != null">AND status = #{status}</if>
            <if test="minAmount != null">AND amount >= #{minAmount}</if>
            <choose>
                <when test="sortBy == 'date'">ORDER BY create_time DESC</when>
                <when test="sortBy == 'amount'">ORDER BY amount DESC</when>
                <otherwise>ORDER BY id ASC</otherwise>
            </choose>
        </where>
    </select>
</mapper>
"#;
```

## API 概览

### 核心类型

| 类型 | 说明 |
|------|------|
| `MyBatisXmlParser` | XML 解析器，从字符串或字节创建 |
| `Mapper` | 解析结果，包含语句、结果映射、SQL 片段 |
| `SqlStatement` | 单条 SQL 语句的定义与动态节点 |
| `DynamicSqlNode` | 动态 SQL 抽象语法树（AST）节点枚举 |
| `MapperError` | 结构化错误类型 |

### 主要方法

```rust
// 解析
let mapper = MyBatisXmlParser::new(xml_content).parse_mapper()?;

// 生成（高层 API，推荐）
let sql = mapper.build_sql("statementId", &params)?;

// 生成（底层 API，用于自定义控制）
let sql = generate_sql(&dynamic_node, &params, &mapper)?;
```

### 参数传递

参数使用 `HashMap<String, serde_json::Value>` 传递：

```rust
let mut params = HashMap::new();

// 简单值
params.insert("id", Value::Number(1.into()));
params.insert("name", Value::String("hello".into()));

// 集合（用于 foreach）
params.insert("ids", Value::Array(vec![
    Value::Number(1.into()),
    Value::Number(2.into()),
]));

// 嵌套对象（支持点号路径访问）
let mut user = serde_json::Map::new();
user.insert("name", Value::String("Alice".into()));
user.insert("address.city", Value::String("Beijing".into()));
params.insert("user", Value::Object(user));
// XML 中使用 #{user.name} 访问
```

## 支持的动态 SQL 标签

| 标签 | 属性 | 说明 |
|------|------|------|
| `<if>` | `test` | 条件判断，test 支持 `and`/`or`/比较运算符 |
| `<choose>` | — | 多条件分支 |
| `<when>` | `test` | choose 内的条件分支 |
| `<otherwise>` | — | 默认分支 |
| `<foreach>` | `collection`, `item`, `index`, `open`, `separator`, `close` | 集合迭代，支持嵌套 |
| `<where>` | `prefixOverrides`, `suffixOverrides` | 自动添加 WHERE 并去除前导 AND/OR |
| `<set>` | `prefixOverrides`, `suffixOverrides` | 自动添加 SET 并去除尾部逗号 |
| `<trim>` | `prefix`, `prefixOverrides`, `suffix`, `suffixOverrides` | 通用前缀/后缀处理 |
| `<bind>` | `name`, `value` | 变量绑定（value 中支持参数引用替换） |
| `<include>` | `refid` | 引用 SQL 片段 |
| `<sql>` | `id` | 定义可复用的 SQL 片段 |

## 条件表达式语法

支持 `key operator value` 格式，通过 `and` 和 `or` 连接：

```
key != null                    # 参数存在性检查
key == 'hello'                 # 字符串比较
key > 100                      # 数值比较
key != null and key > 0         # and 组合（高优先级）
key == 'a' or key == 'b'        # or 组合（低优先级）
key == 1 and name != '' or status == 'active'
                                 # 等价于: (key==1 AND name!='') OR status=='active'
```

支持的运算符：`=`, `==`, `!=`, `>`, `<`, `>=`, `<=`

## 错误处理

```rust
use hirust_mapper::MapperError;

match mapper.build_sql("unknownId", &params) {
    Ok(sql) => println!("生成的 SQL: {}", sql),
    Err(MapperError::StatementNotFound { id }) => {
        eprintln!("语句 '{}' 不存在", id);
    }
    Err(MapperError::MissingFragment { ref_id }) => {
        eprintln!("SQL 片段 '{}' 不存在", ref_id);
    }
    Err(MapperError::ParseError { message }) => {
        eprintln!("XML 解析失败: {}", message);
    }
    Err(e) => eprintln!("其他错误: {}", e),
}
```

## 依赖

| 库 | 版本 | 用途 |
|----|------|------|
| [quick-xml](https://crates.io/crates/quick-xml) | 0.38 | XML 解析 |
| [serde_json](https://crates.io/crates/serde_json) | 1 | JSON 参数值 |
| [regex](https://crates.io/crates/regex) | 1 | 条件表达式和参数占位符匹配 |
| [lazy_static](https://crates.io/crates/lazy_static) | 1.5 | 编译期正则缓存 |

## License

MIT
