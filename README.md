# hirust-mapper

用 Rust 实现的 [MyBatis](https://mybatis.org/mybatis-3/) 风格异步 ORM 框架。解析 MyBatis XML 映射文件，动态组装参数化 SQL，经 sqlx 异步执行并映射结果——同时提供运行时弱类型 API 与编译时类型安全 proc_macro 两套接口。

## 特性

- **完整动态 SQL** — `<if>` / `<choose>` / `<foreach>` / `<where>` / `<set>` / `<trim>` / `<bind>` / `<include>` / `<sql>`
- **两阶段 SQL** — `build_sql`（内联）与 `build_bound_sql`（参数化 `?` + 参数列表，防注入）并行提供
- **异步执行层** — 基于 sqlx，内置连接池、事务（begin/commit/rollback）、SimpleExecutor
- **流式查询** — `select_for_each`（回调式）与 `query_stream` / `query_rows_stream`（sqlx fetch 流），大结果集低内存峰值
- **SQL 执行日志** — `[settings] sql_log` 开关控制，输出「耗时 + 参数内联的可读 SQL」，经 `log` facade 输出，支持慢查询阈值
- **事件系统** — 类型化 `Event`/`Listener` + `EventBus` 分发器 + `Subscriber` 批量订阅；内置 SQL 执行前/后生命周期事件，线程安全、无监听器零开销
- **类型处理** — TypeHandler 体系（i32/i64/f64/bool/String + feature-gated chrono/uuid），`serde_json::Value` 通用中间表示
- **ResultMap 嵌套映射** — `<association>` 一对一、`<collection>` 一对多分组、`<id>` 身份、`<selectKey>` 主键回填
- **条件增强** — 支持 `.size()` / `.isEmpty()` 方法调用与布尔字面量
- **热重载** — `notify` 监控 XML 变更，去抖后原子替换（开发期零重启）
- **编译时类型安全** — `#[hirust_mapper(xml)]` 编译时校验 XML 并生成 DAO 方法；`#[dao]`+`#[mapper_query]` 按方法签名生成类型化 DAO
- **多数据库** — mysql / postgres / sqlite（feature gates，默认 sqlite）

## 快速开始

### 添加依赖

```toml
[dependencies]
hirust-mapper = { version = "0.2", features = ["full"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Feature 分层：`core`（默认，纯解析）/ `runtime`（ORM 运行时）/ `macros`（proc_macro，隐含 runtime）/ `full`（runtime + macros）。

### 运行时 API（弱类型，灵活）

```rust
use std::collections::HashMap;
use hirust_mapper::{EnvironmentConfig, HirustMapperConfig, SqlSessionFactory};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct User { id: i64, name: String, age: i64 }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = HirustMapperConfig::new()
        .with_environment(EnvironmentConfig {
            driver: "sqlite".into(), url: "sqlite::memory:".into(),
            pool_max_connections: 10, pool_min_connections: 1,
        })
        .with_mapper_paths(vec!["mappers/**/*.xml".into()]);

    let factory = SqlSessionFactory::build(config, ".").await?;
    let mut session = factory.open_session();

    let id = session.insert("app.UserDao", "insert",
        &User { id: 0, name: "张三".into(), age: 30 }).await?;

    let mut params = HashMap::new();
    params.insert("id".into(), serde_json::json!(id.unwrap()));
    let user: User = session.select_one("app.UserDao", "findById", &params).await?.unwrap();

    // 事务
    let mut tx = factory.open_session();
    tx.begin().await?;
    tx.insert("app.UserDao", "insert", &User { id: 0, name: "李四".into(), age: 25 }).await?;
    tx.commit().await?;

    factory.close().await;
    Ok(())
}
```

### 流式查询（大结果集）

对于大结果集，用流式接口逐行拉取，避免 `select_list` 一次性物化整表（`fetch_all`）导致的内存峰值：

```rust
use std::collections::HashMap;
use futures_util::StreamExt;

// 方式一：session 级回调式（自动选 pool / 事务，内部构建 BoundSql）
session
    .select_for_each("app.UserDao", "findAll", &HashMap::new(), |u: &User| {
        println!("{u:?}"); // 逐行处理；返回 Err 可提前终止
        Ok(())
    })
    .await?;

// 方式二：executor 级 Stream（调用方持有 BoundSql，可组合、可异步逐行处理）
let bound = session.build_bound_sql("app.UserDao", "findAll", &HashMap::new())?;
let mut stream = session
    .executor()
    .query_stream::<_, User>(&bound, session.pool());
while let Some(user) = stream.next().await {
    let user = user?;
    // ...
}
```

> 方式二消费 Stream 需在 `Cargo.toml` 添加 `futures-util = "0.3"`；方式一（`select_for_each`）无需额外依赖。

- 流式仅支持普通列映射（`AnyRow → T`），**不支持 ResultMap 嵌套分组**（分组需聚集全部行，与流式语义冲突）。
- 回调返回 `Err` 会向上传递并终止流；空结果集不触发回调。
- `select_one` / `select_list` / `fetch_all` 行为不变（含 `TooManyRows` 校验）。

### Proc Macro API（编译时类型安全）

```rust
use hirust_mapper::{hirust_mapper, EnvironmentConfig, HirustMapperConfig, MapperModel, SqlSessionFactory};

// 编译时读取 + 解析 mappers/UserDao.xml，校验并生成 UserDao 方法
#[hirust_mapper(xml = "mappers/UserDao.xml")]
struct UserDao;

#[derive(MapperModel, serde::Deserialize)]
#[allow(dead_code)]
struct User {
    #[mapper(column = "user_name")]
    name: String,
    age: i64,
}

// 方法名即 statement id，返回类型由调用方指定
let dao = UserDao::new(factory);
let users: Vec<User> = dao.findById(&params).await?;
```

XML 文件缺失、解析失败、非法语句 id 等均在**编译期**报错。

### 类型化 DAO（`#[dao]` + `#[mapper_query]`，推荐）

`#[hirust_mapper(xml)]` 生成的方法参数是 `HashMap`、返回靠 turbofish 推断。`#[dao]` 进一步
**按 Rust 方法签名**生成方法体——参数名即 SQL 参数键、方法名即 statement_id、返回类型自动分派，
最大程度消除样板：

```rust
use hirust_mapper::{dao, Result};   // mapper_query 是 #[dao] 消费的标记，无需 import

#[dao]                               // struct 侧：自动加 factory 字段 + new()
struct UserDao;

#[dao(namespace = "app.dao.user", xml = "mappers/UserDao.xml")]
impl UserDao {
    #[mapper_query]                  // Result<Option<T>> → select_one；方法名→"find_by_id"
    async fn find_by_id(&self, id: i64) -> Result<Option<User>> {}

    #[mapper_query]                  // Result<Vec<T>> → select_list
    async fn list_by_status(&self, status: String) -> Result<Vec<User>> {}

    #[mapper_query(kind = "insert")] // 写操作须显式 kind；形参 name/age → #{name}/#{age}
    async fn create(&self, name: String, age: i64) -> Result<i64> {}

    #[mapper_query(kind = "update", id = "setAge")]  // id= 覆盖 statement_id
    async fn set_age(&self, id: i64, age: i64) -> Result<u64> {}

    #[mapper_query]                  // Vec<i64> 形参 ↔ foreach collection="project_ids"
    async fn get_by_privilege_project_ids(&self, project_ids: Vec<i64>) -> Result<Vec<User>> {}
}

let dao = UserDao::new(factory);
let u: Option<User> = dao.find_by_id(42).await?;
```

**改造前（手写样板）→ 改造后（`#[dao]`）：**

```rust
// 改造前：每个方法手写 namespace / statement_id / HashMap
const NS: &str = "app.dao.user";
async fn find_by_id(dao: &UserDao, id: i64) -> Result<Option<User>> {
    let mut p = HashMap::new();
    p.insert("id".into(), json!(id));                 // 易拼错、无类型提示
    let mut s = dao.factory().open_session();
    s.select_one(NS, "find_by_id", &p).await
}
// 改造后：见上方 #[mapper_query] —— 零样板，全类型化。
```

返回类型分派规则：`Result<Vec<T>>`→select_list、`Result<Option<T>>`→select_one、`Result<i64>`+`kind=insert`→生成主键、
`Result<u64>`+`kind=update/delete`→受影响行数、`Result<()>`→执行后丢弃。namespace 默认 `module_path!()`
（可用 `namespace=` 显式覆盖）；`xml=` 启用编译期 statement_id 存在性校验。与 `#[hirust_mapper(xml)]` 并存，可逐 DAO 迁移。

## Mapper XML 格式

```xml
<mapper namespace="app.UserDao">
    <resultMap id="userMap" type="User">
        <id property="id" column="id"/>
        <result property="name" column="user_name"/>
        <association property="dept" javaType="Dept">
            <id property="id" column="dept_id"/>
            <result property="name" column="dept_name"/>
        </association>
        <collection property="roles" ofType="Role">
            <id property="id" column="role_id"/>
        </collection>
    </resultMap>

    <select id="findById" resultMap="userMap">
        SELECT * FROM users WHERE 1=1
        <if test="id != null">AND id = #{id}</if>
        <if test="ids != null and ids.size() > 0">
            AND id IN (<foreach collection="ids" item="x" separator=",">#{x}</foreach>)
        </if>
    </select>

    <insert id="insertWithKey">
        <selectKey keyProperty="id" resultType="i64" order="AFTER">
            SELECT LAST_INSERT_ID()
        </selectKey>
        INSERT INTO users (name) VALUES (#{name})
    </insert>
</mapper>
```

- `#{param}` → 参数化 `?` 占位符（防注入，推荐）
- `${param}` → 原样内联（动态表名/排序列等）
- 条件支持 `and`/`or`、`= != > < >= <=`、`.size()`/`.isEmpty()`、`== true/false`

## 配置文件（`hirust-mapper.toml`）

```toml
[environment]
driver = "mysql"                       # mysql | postgres | sqlite
url = "mysql://user:pass@host:3306/db"
pool_max_connections = 10
pool_min_connections = 2

[settings]
mapper_paths = ["mappers/**/*.xml"]
mapper_refresh_interval_ms = 3000      # 热重载间隔，0 = 禁用
sql_log = true                         # SQL 执行日志开关（默认 false）
sql_log_slow_threshold_ms = 0          # 慢查询阈值(ms)：仅记录耗时≥此值的 SQL；0 = 全部

[type_aliases]
"int" = "i32"
"long" = "i64"
```

## 配置优先级与环境变量

配置来源分三层，优先级 **环境变量 > 编程设置 > TOML 默认值**。环境变量只在**设置时**覆盖对应字段，
未设置则保留编程/TOML 值——适合容器/CI 部署时无需改 TOML 即可覆盖连接、日志等。

```rust
use hirust_mapper::HirustMapperConfig;

// 链式：TOML → 编程 → env（env 最后应用，优先级最高）
let config = HirustMapperConfig::load_file("hirust-mapper.toml")?
    .with_url("programmatic-url")      // 编程覆盖
    .with_type_alias("money", "Decimal")
    .with_env_overrides()?;            // 应用进程环境变量覆盖
```

也可一站式加载：`HirustMapperConfig::load_layered("hirust-mapper.toml")?`（= TOML + env）。

支持的环境变量：

| 变量 | 覆盖字段 | 示例 |
|------|----------|------|
| `HIRUST_MAPPER_DRIVER` | `environment.driver` | `postgres` |
| `HIRUST_MAPPER_URL`（或 `DATABASE_URL`） | `environment.url` | `postgres://u:p@h/db` |
| `HIRUST_MAPPER_POOL_MAX` / `_POOL_MIN` | 连接池大小 | `20` / `2` |
| `HIRUST_MAPPER_PATHS` | `settings.mapper_paths` | `a/*.xml,b/**/*.xml` |
| `HIRUST_MAPPER_REFRESH_MS` | 热重载间隔 | `3000` |
| `HIRUST_MAPPER_SQL_LOG` | SQL 日志开关 | `true` |
| `HIRUST_MAPPER_SQL_LOG_SLOW_MS` | 慢查询阈值 | `100` |
| `HIRUST_MAPPER_TYPE_ALIASES` | 类型别名（合并） | `int=i32,long=i64` |

```sh
# 12-factor 部署示例：仅用环境变量覆盖连接与慢查询阈值
DATABASE_URL=postgres://prod:secret@db:5432/app \
HIRUST_MAPPER_SQL_LOG=true \
HIRUST_MAPPER_SQL_LOG_SLOW_MS=200 \
    ./your_app
```

非法值（如非数字、非布尔）会返回 `MapperRuntimeError::Config`，不静默吞错。

## SQL 执行日志

在 `[settings]` 中设置 `sql_log = true`（或编程式 `.with_sql_log(true)`），即可对每次 SQL 执行输出「耗时 + 参数内联的可读 SQL」：

```text
[2026-08-12 15:32:03 INFO hirust_mapper::sql] Consume Time: 44 ms
 Execute SQL: SELECT `examId`,`examName` FROM exam WHERE (`examId` IN (69902) AND `isDelete` = 0)
```

- 日志 target 固定为 `hirust_mapper::sql`，参数按 `?` 顺序内联（字符串加引号、`NULL`/布尔/数字原样）。
- XML 中多行书写的 SQL，其换行符（单个或连续多个）在日志输出时折叠为单个空格，保证每条日志单行。
- `sql_log_slow_threshold_ms > 0` 时只记录达到阈值的慢查询；`0` 记录全部。
- 本 crate **只经 `log` facade 发射日志，不自带输出后端**——需应用初始化一个日志后端方能见到输出：

```rust
// 方式一：env_logger
env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
// 精确过滤：RUST_LOG=hirust_mapper::sql=info

// 方式二：tracing（sqlx 也用 tracing，可统一）
tracing_subscriber::fmt().with_env_filter("hirust_mapper::sql=info").init();
```

`log` facade 在无后端时为零开销；关闭 `sql_log` 时执行点不做任何格式化与计时之外的工作。

## 事件系统

类型化的事件监听与订阅（灵感来自 ThinkPHP 模型事件，按 Rust 最佳实践实现）：

- **`Event`** trait —— 任何实现它的类型即可作为事件（含自定义业务事件）。
- **`Listener`** trait + 闭包 —— `bus.on(|e: &E| {...})` 即订阅；监听器收到不可变引用（观察者语义）。
- **`Subscriber`** trait —— 在一个实现里批量注册多个事件（对应 ThinkPHP 的「事件订阅」）。
- **`EventBus`** —— 线程安全的类型擦除分发器；派发时先克隆监听器列表、**释放锁后再回调**（监听器内可安全重入订阅/派发）；**无监听器时经原子读零开销跳过**。

内置 ORM 生命周期事件，在 SQL 执行点自动派发：`BeforeSqlEvent`（执行前）/ `AfterSqlEvent`（含耗时与 `SqlOutcome` 结果摘要）。

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use hirust_mapper::{
    AfterSqlEvent, EventBus, Event, HirustMapperConfig, EnvironmentConfig,
    SqlSessionFactory, SqlOutcome, Subscriber,
};

// 1) 自定义事件
struct LoginEvent { user: String }
impl Event for LoginEvent {}

// 2) 订阅器：批量注册多个事件
struct Audit { count: Arc<AtomicUsize> }
impl Subscriber for Audit {
    fn subscribe(&self, bus: &EventBus) {
        let c = Arc::clone(&self.count);
        bus.on(move |e: &AfterSqlEvent| {
            c.fetch_add(1, Ordering::Relaxed);
            println!("{:?} {:?}", e.kind, e.outcome);
        });
    }
}

let factory = SqlSessionFactory::build(config, ".").await?;
factory.event_bus().add_subscriber(&Audit { count: Arc::new(AtomicUsize::new(0)) });
factory.event_bus().on(|e: &LoginEvent| println!("{} 登录", e.user));

let mut session = factory.open_session();
session.insert("app.U", "insert", &user).await?;       // 触发 Before/After SQL 事件
factory.event_bus().dispatch(&LoginEvent { user: "张三".into() }); // 派发自定义事件
```

> 监听器为**同步回调**（在派发点内联调用）；耗时或异步工作请在监听器内 `tokio::spawn`。
> 流式查询（`select_for_each`/`query_stream`）暂不派发 SQL 事件（按行流式的「耗时」语义不明确）。

## 示例

仓库内含可运行示例：

```sh
cargo run --example runtime_basic --features runtime      # 运行时 API CRUD + 事务
cargo run --example events_usage --features runtime       # 事件监听与订阅
cargo run --example proc_macro_usage --features full       # 编译时 DAO 生成
```

## 架构

| crate | 职责 |
|-------|------|
| `hirust-mapper-core` | XML 解析 + 动态 SQL 生成 + BoundSql（无 async 依赖） |
| `hirust-mapper-runtime` | 配置 / Session / Executor / TypeHandler / 热重载 / ResultMap 映射 |
| `hirust-mapper-macros` | `#[hirust_mapper]` / `#[dao]` / `#[derive(MapperModel)]` 编译时层 |
| `hirust-mapper` | 门面 crate，feature gate 聚合 |

依赖：[sqlx](https://crates.io/crates/sqlx) 0.9（执行层）、[quick-xml](https://crates.io/crates/quick-xml) 0.41（解析）、[notify](https://crates.io/crates/notify) 8（热重载）、[futures-util](https://crates.io/crates/futures-util) 0.3（流式查询）、[tokio](https://crates.io/crates/tokio) 1（异步运行时）。

## 测试

```sh
cargo test --workspace     # 129 个测试（core 41 + runtime 56 + 集成 26（含流式 4、SQL 日志 3、事件 3）+ macros 6）
```

## License

MIT
