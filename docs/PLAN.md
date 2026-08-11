# hirust-mapper v2 — 完整 ORM 框架设计方案

> **本文件是项目的权威实施计划。在任何电脑上克隆本仓库后，阅读此文件即可了解架构全貌、当前进度与后续步骤。**
>
> 计划文件位置：`docs/PLAN.md`（项目内，随 git 同步）

---

## 执行状态（实时更新）

| 阶段 | 状态 | 完成时间 | 验证结果 |
|------|------|----------|----------|
| P1 Workspace 重构 | ✅ 已完成 | 2026-08-10 | core 15/15 测试通过 |
| P2 配置系统 | ✅ 已完成 | 2026-08-10 | runtime 9/9 测试通过 |
| P3 Registry + Environment | ✅ 已完成 | 2026-08-11 | runtime 17/17 测试通过（含 8 个新测试） |
| P4 BoundSql 两阶段重构 | ✅ 已完成 | 2026-08-11 | core 30/30 测试通过（含 15 个 BoundSql 测试） |
| P5 TypeHandler + 参数绑定 | ✅ 已完成 | 2026-08-11 | runtime 34/34 测试通过（含 17 个 P5 新测试） |
| P6 Executor + SqlSession | ✅ 已完成 | 2026-08-11 | runtime 34 + e2e 10 测试通过（完整 ORM 可用） |
| P7 热重载 | ✅ 已完成 | 2026-08-11 | runtime 39 + hot_reload 3 测试通过（修改 XML 后查询自动更新） |
| P8 ResultMap 增强 | ✅ 已完成 | 2026-08-11 | core 41 + nested_mapping 3 测试通过（association/collection/selectKey/size） |
| P9 Proc Macros | ✅ 已完成 | 2026-08-11 | macros 6 测试通过（DAO CRUD + MapperModel 列映射） |
| P10 门面 + 文档 + 示例 | ✅ 已完成 | 2026-08-11 | 2 示例可运行，102 测试全通过，README v2 完成 |

**当前可在全新电脑上运行的验证命令：**

```bash
cargo test --workspace     # 应输出 core 41 + runtime 39 + crud 10 + hot_reload 3 + nested 3 + macros 6 = 102 个测试通过
```

---

## 如何在其他电脑上接续实施

1. **克隆并验证当前状态**
   ```bash
   git clone <repo-url> hirust-mapper
   cd hirust-mapper
   cargo test --workspace          # 确认 102 个测试全通过
   ```

2. **识别下一个待实施阶段**：查看上方"执行状态"表格，找到第一个 `⬜ 待实施` 的阶段。

3. **阅读该阶段的详细要求**：在下方"实施阶段计划"章节定位对应 P 编号。

4. **实施完成后**：更新本文件顶部的"执行状态"表格（状态改为 ✅，填入完成时间与验证结果），然后提交。

5. **关键约束**：
   - 每个 P 阶段必须保持已有测试全通过（回归保护）
   - 新增功能需配套单元测试
   - 不要破坏 core crate 的公开 API（`build_sql`、`generate_sql`、`ParamsAccess`）

---

## Context

当前 hirust-mapper 是一个 workspace 结构的 MyBatis XML 动态 SQL 解析与生成库，core crate（15 测试）+ runtime crate（9 测试）已就绪。目标：扩展为对标 MyBatis 全家桶的完整 ORM 框架，同时提供运行时弱类型 API 和编译时强类型 proc_macro API 两套接口。

---

## 1. Workspace Crate 结构（P1 已落地）

```
hirust-mapper/
├── Cargo.toml                         # workspace root ✅
├── hirust-mapper-core/                # 现有代码迁移，纯解析+生成 ✅
│   └── src/
│       ├── model.rs                  # Mapper, DynamicSqlNode, MapperError
│       ├── parser.rs                 # MyBatisXmlParser
│       └── sql_generator.rs         # generate_sql + build_sql + generate_bound_sql + BoundSql (P4)
│
├── hirust-mapper-runtime/            # ORM 运行时（部分已实现）
│   └── src/
│       ├── config.rs                 # ✅ HirustMapperConfig (TOML 解析)
│       ├── error.rs                  # ✅ MapperRuntimeError
│       ├── registry.rs               # ✅ MapperRegistry + TypeAliasRegistry
│       ├── environment.rs            # ✅ Environment + EnvironmentRegistry (P3)
│       ├── session_factory.rs        # ✅ SqlSessionFactory + SqlSession (P3)
│       ├── bound_sql.rs              # ✅ BoundSql 重新导出 + 便捷绑定 (P4)
│       ├── type_handler/             # ✅ TypeHandler trait + 标准/可选处理器 (P5)
│       │   ├── trait_def.rs
│       │   └── standard.rs
│       ├── handler/                  # ✅ ParameterHandler + ResultSetHandler (P5)
│       │   ├── parameter.rs
│       │   └── result_set.rs
│       ├── executor/                 # ✅ SimpleExecutor (泛型 sqlx 执行) (P6)
│       │   └── simple.rs
│       ├── session.rs                # ✅ SqlSession 全 CRUD + 事务 + MapperProxy (P6)
│       ├── hot_reload/               # ✅ MapperWatcher (notify + 去抖) (P7)
│           └── watcher.rs
│       ├── session.rs                # ⬜ SqlSession (P6)
│       ├── executor/                 # ⬜ (P6)
│       │   ├── simple.rs
│       │   ├── batch.rs
│       │   └── caching.rs
│       ├── handler/                  # ⬜ (P5)
│       │   ├── parameter.rs
│       │   └── result_set.rs
│       ├── type_handler/             # ⬜ (P5)
│       │   ├── trait_def.rs
│       │   └── standard.rs
│       ├── hot_reload/               # ⬜ (P7)
│       │   └── watcher.rs
│       ├── bound_sql.rs              # ⬜ BoundSql (P4)
│
├── hirust-mapper-macros/              # proc_macro（✅ P9 已实现）
│   └── src/
│       ├── lib.rs                    # 宏入口
│       ├── gen_mapper.rs            # #[hirust_mapper(xml)] DAO 生成
│       └── derive_model.rs          # #[derive(MapperModel)] 列映射
│
├── hirust-mapper/                     # 门面 crate ✅
│   ├── src/lib.rs                    # feature gate 聚合
│   └── examples/                     # ✅ 可运行示例 (P10)
│       ├── runtime_basic.rs
│       ├── proc_macro_usage.rs
│       └── mappers/UserDao.xml
│
└── docs/
    └── PLAN.md                       # 本文件
```

### 依赖关系

```
hirust-mapper-macros (proc-macro=true, 无 workspace crate 依赖)
        ↓ (生成代码调用)
hirust-mapper-runtime → 依赖 → hirust-mapper-core, sqlx, tokio, notify, toml
hirust-mapper (facade) → 依赖 → core + runtime(可选) + macros(可选)
```

### 关键依赖版本

| 库 | 版本 | 用途 | 状态 |
|----|------|------|------|
| quick-xml | 0.38 | XML 解析 (core) | ✅ 已用 |
| serde_json | 1 | 参数值中间表示 | ✅ 已用 |
| regex | 1 | 条件表达式 + 参数占位符 | ✅ 已用 |
| thiserror | 2 | 错误类型派生 | ✅ 已用 (runtime) |
| glob | 0.3 | mapper 文件发现 | ✅ 已用 (runtime) |
| toml | 0.8 | 配置文件解析 | ✅ 已用 (runtime) |
| sqlx | 0.8 | 数据库执行层 | ✅ P3 已引入 |
| tokio | 1 | async runtime | ✅ P3 已引入 |
| notify | 7 | 文件变更监控/热重载 | ✅ P7 已引入 |
| chrono/uuid | 0.4/1 | 可选类型处理器 | ✅ P5 已引入（feature-gated） |

---

## 2. TOML 配置文件 (`hirust-mapper.toml`)

P2 已实现解析，格式如下：

```toml
[environment]
driver = "mysql"                      # "mysql" | "postgres" | "sqlite"
url = "mysql://user:pass@localhost:3306/mydb"
pool_max_connections = 10             # 默认 10
pool_min_connections = 2              # 默认 2

[environments.staging]               # 可选：多环境支持
driver = "postgres"
url = "postgres://user:pass@staging-host:5432/mydb"

[settings]
mapper_paths = ["mappers/**/*.xml"]   # XML 文件 glob 发现（默认值）
mapper_refresh_interval_ms = 3000     # 热重载轮询间隔 (0=禁用，默认禁用)

[type_aliases]                        # XML 中的短名 → Rust 全限定名
"int" = "i32"
"long" = "i64"
"string" = "String"

[[type_handlers]]                    # 自定义类型处理器（可选）
type = "myapp::types::MyEnum"
handler = "myapp::handlers::MyEnumHandler"
```

对应的 Rust 结构体在 `hirust-mapper-runtime/src/config.rs`，通过 `HirustMapperConfig::parse_toml()` / `load_file()` 加载。

---

## 3. 扩展的 XML Mapper 格式（✅ P8 已实现）

在现有格式基础上新增（向后兼容）：

```xml
<mapper namespace="myapp::dao::UserDao">
    <!-- P8: resultMap 支持 id/association/collection（含深层嵌套） -->
    <resultMap id="userResultMap" type="User">
        <id property="id" column="id"/>
        <result property="name" column="user_name" rustType="String"/>
        <association property="department" javaType="Department">
            <id property="id" column="dept_id"/>
            <result property="name" column="dept_name"/>
        </association>
        <collection property="roles" ofType="Role">
            <id property="id" column="role_id"/>
        </collection>
    </resultMap>

    <!-- P8: selectKey（主键回填，Before/After） -->
    <insert id="insertWithKey">
        <selectKey keyProperty="id" resultType="i64" order="AFTER">
            SELECT LAST_INSERT_ID()
        </selectKey>
        INSERT INTO users (name) VALUES (#{name})
    </insert>

    <!-- P8: 条件支持 .size() / .isEmpty() -->
    <select id="findByIds" resultMap="userResultMap">
        SELECT * FROM users
        <if test="ids != null and ids.size() > 0">
            WHERE id IN (<foreach collection="ids" item="x" separator=",">#{x}</foreach>)
        </if>
    </select>
</mapper>
```

> **嵌套映射运行时**：`SqlSession.select_*` 自动按 statement 的 `resultMap` 走嵌套映射——
> association 从扁平 join 行构建嵌套对象（列为空→null），collection 按父 `<id>` 分组累加子项。

---

## 4. 核心 Trait 定义（P5/P6 待实施）

### 4.1 TypeHandler (`hirust-mapper-runtime/src/type_handler/trait_def.rs`)

```rust
pub trait TypeHandler: Send + Sync + 'static {
    type RustType: Send + 'static;
    fn type_name(&self) -> &'static str;
    fn to_sql_param(&self, value: &Self::RustType) -> Result<Box<dyn sqlx::Encode<'_, sqlx::Any>>>;
    fn from_row<R: sqlx::Row>(&self, row: &R, column: &str) -> Result<Self::RustType>;
}
```

内置实现：`I32Handler`, `I64Handler`, `StringHandler`, `BoolHandler`, `F64Handler`，以及 feature-gated 的 `ChronoHandler`、`UuidHandler`。

### 4.2 SqlSession (`hirust-mapper-runtime/src/session.rs`)

```rust
pub struct SqlSession {
    pool: sqlx::Pool<sqlx::Any>,
    transaction: Option<sqlx::Transaction<'static, sqlx::Any>>,
    registry: Arc<RwLock<MapperRegistry>>,
    type_handlers: Arc<TypeHandlerRegistry>,
    executor: Arc<dyn Executor>,
}

impl SqlSession {
    pub async fn select_one<T: DeserializeOwned>(&self, ns: &str, id: &str, params: &HashMap<String, Value>) -> Result<T>;
    pub async fn select_list<T: DeserializeOwned>(&self, ns: &str, id: &str, params: &HashMap<String, Value>) -> Result<Vec<T>>;
    pub async fn insert<T: Serialize>(&self, ns: &str, id: &str, params: &T) -> Result<Option<i64>>;
    pub async fn update<T: Serialize>(&self, ns: &str, id: &str, params: &T) -> Result<u64>;
    pub async fn delete<T: Serialize>(&self, ns: &str, id: &str, params: &T) -> Result<u64>;
    pub async fn begin(&mut self) -> Result<()>;
    pub async fn commit(self) -> Result<()>;
    pub async fn rollback(self) -> Result<()>;
    pub fn get_mapper(&self, namespace: &str) -> Result<MapperProxy<'_>>;
}
```

### 4.3 Executor (`hirust-mapper-runtime/src/executor/mod.rs`)

```rust
pub trait Executor: Send + Sync {
    async fn query<T: DeserializeOwned>(&self, stmt: &MappedStatement, params: &dyn ParamsAccess) -> Result<Vec<T>>;
    async fn execute(&self, stmt: &MappedStatement, params: &dyn ParamsAccess) -> Result<u64>;
    async fn batch_update(&self, stmts: &[MappedStatement], params: &[&dyn ParamsAccess]) -> Result<Vec<u64>>;
}
```

实现：`SimpleExecutor`（基础）、`BatchExecutor`（批累积）、`CachingExecutor`（装饰器 + LRU 缓存）。

---

## 5. 两阶段 SQL 解析（P4 已完成）

`generate_sql`（内联模式）与 `generate_bound_sql`（绑定模式）并存：

| 阶段 | 时机 | 输出 |
|------|------|------|
| Phase 1: 解析 | 启动时 | `Mapper` (DynamicSqlNode AST) — ✅ 现有代码 |
| Phase 2: 绑定 | 每次查询时 | `BoundSql { sql: String(含?占位符), parameters: Vec<Value> }` — ✅ P4 |

- `#{param}` → `?` 占位符 + 参数按出现顺序进入 `parameters` 列表
- `${param}` → 原样内联（无法参数化的部分保持内联模式）
- 检测到 `${}` 时自动降级为混合模式（部分内联 + 部分 ?，自动发生）
- `build_sql`（内联）与 `build_bound_sql`（绑定）并行提供，向后兼容

---

## 6. 运行时 API 使用示例（✅ P6 已可用）

```rust
use hirust_mapper_runtime::{SqlSessionFactory, HirustMapperConfig, MapperProxy};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use serde_json::json;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct User { id: i64, name: String, age: i64 }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. 加载配置 + 创建 SessionFactory（应用级，线程安全）
    let config = HirustMapperConfig::load_file("hirust-mapper.toml")?;
    let factory = SqlSessionFactory::build(config, ".").await?;

    // 2. 打开 Session（请求级，&mut self 用于 DB 操作）
    let mut session = factory.open_session();

    // 3. 插入（返回自增主键）
    let id = session.insert("myapp::dao::UserDao", "insert",
        &User { id: 0, name: "张三".into(), age: 30 }).await?;
    println!("生成主键: {:?}", id);

    // 4. 查询单行（多于一行报 TooManyRows）
    let mut params = HashMap::new();
    params.insert("id".into(), json!(id.unwrap()));
    let user: User = session.select_one("myapp::dao::UserDao", "findById", &params)
        .await?.unwrap();

    // 5. 查询多行
    let users: Vec<User> = session.select_list("myapp::dao::UserDao", "findAll", &HashMap::new())
        .await?;

    // 6. Mapper 代理（省去每次传 namespace）
    let mut dao: MapperProxy = session.mapper("myapp::dao::UserDao")?;
    dao.update("updateAge", &serde_json::json!({"id": id.unwrap(), "age": 31})).await?;

    // 7. 事务（begin/commit/rollback）
    let mut tx_session = factory.open_session();
    tx_session.begin().await?;
    tx_session.insert("myapp::dao::UserDao", "insert", &User { id: 0, name: "李四".into(), age: 25 }).await?;
    tx_session.commit().await?; // 或 rollback()

    factory.close().await;
    Ok(())
}
```

> **P6 里程碑达成**：完整 ORM 可用——加载 XML → 执行 SQL → 映射结果 → 事务管理。
> 完整可运行端到端测试见 `hirust-mapper-runtime/tests/crud_e2e.rs`（10 个测试覆盖 CRUD + 事务提交/回滚 + MapperProxy）。


---

## 7. Proc Macro API 使用示例（P9 待实施）

### `#[derive(MapperModel)]` — 列映射内省（✅ P9 已实现）

```rust
use hirust_mapper_macros::MapperModel;

#[derive(MapperModel, Deserialize)]
#[allow(dead_code)]
struct User {
    #[mapper(column = "user_name")]
    name: String,
    email: String,
    #[mapper(column = "created_at", type_handler = "chrono::DateTime<chrono::Utc>")]
    created_at: String,
}

// 生成 User::column_mappings() -> &'static [(&str, &str)]
// 生成 User::type_handlers()   -> &'static [(&str, &str)]
```

### `#[hirust_mapper(xml = "...")]` — 编译时 Mapper DAO 生成（✅ P9 已实现）

```rust
use hirust_mapper_macros::hirust_mapper;
use hirust_mapper_runtime::{SqlSessionFactory, HirustMapperConfig, EnvironmentConfig};

#[hirust_mapper(xml = "mappers/UserDao.xml")]  // 编译时读取+解析+校验
struct UserDao;

// 配置 + 运行时加载同一 XML（namespace 注册）
let config = HirustMapperConfig::new()
    .with_environment(EnvironmentConfig { driver: "sqlite".into(), url: "sqlite::memory:".into(), ..Default::default() })
    .with_mapper_paths(vec!["mappers/UserDao.xml".to_string()]);
let factory = SqlSessionFactory::build(config, ".").await?;

let dao = UserDao::new(factory);

// 为每个 <select>/<insert>/<update>/<delete> 生成同名方法（返回类型泛型，调用方指定）
let users: Vec<User> = dao.findById(&params).await?;   // select → Vec<T>
let id = dao.insertUser(&new_user).await?;              // insert → Option<i64>
let n = dao.updateAge(&update).await?;                  // update → u64
```

> **设计说明**：编译时宏读取 XML（路径相对 `CARGO_MANIFEST_DIR`）并用 core 解析器校验——
> 文件缺失、解析错误、非单元 struct、非法语句 id 均在编译期报错。生成的 DAO 持有
> `Arc<SqlSessionFactory>`，每方法开一个 session 委托 `select_list`/`insert`/`update`/`delete`。
> select 返回 `Vec<T>`（调用方用 `T: DeserializeOwned` 指定类型；单行取 `.pop()`）。

---

## 8. 热重载机制（✅ P7 已实现）

`MapperWatcher`（`hot_reload/watcher.rs`）实现：

1. `SqlSessionFactory::build()` 启动时，若 `mapper_refresh_interval_ms > 0`，启动 `MapperWatcher`
2. `extract_watch_dirs` 从 glob 模式推导监视目录（取首个通配符前的静态前缀），递归监视
3. `notify::RecommendedWatcher` 回调将变更路径经 mpsc channel 发送到专用 worker 线程
4. worker 线程收集变更（仅 `.xml`），安静期（≥ `refresh_interval_ms`，最小 50ms）后批量重解析
5. 每个变更文件调用 `registry.register_from_file()` → `insert_mapper()` 原子替换（`MapperRegistry` 的 `Arc<RwLock<HashMap>>` 保证并发安全，P2 就位）
6. 热重载失败不阻断工厂构建（降级为无热重载）；`Drop` 时优雅关闭（watcher 断开 → worker 退出 → join）

> **验证**：`tests/hot_reload.rs` 3 个测试——修改 XML 后 SQL 自动变更、新增 statement 自动可用、默认禁用热重载。

---

## 9. 错误处理（已实现）

```rust
// hirust-mapper-runtime/src/error.rs (P1 已实现)
#[derive(Debug, thiserror::Error)]
pub enum MapperRuntimeError {
    #[error("Mapper 错误: {0}")]
    Mapper(#[from] MapperError),       // 核心层错误
    #[error("连接错误: {0}")]
    Connection(String),
    #[error("事务错误: {0}")]
    Transaction(String),
    #[error("类型转换错误: {0}")]
    TypeConversion(String),
    #[error("配置错误: {0}")]
    Config(String),
    #[error("未找到数据: {namespace}.{id}")]
    NoData { namespace: String, id: String },
    #[error("返回行数过多: 期望 1, 实际 {actual}")]
    TooManyRows { actual: usize },
    #[error("Mapper 不存在: {0}")]
    MapperNotFound(String),
    #[error("热重载错误: {0}")]
    HotReload(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}
```

P6 已补充 `Database(#[from] sqlx::Error)` 变体（见上表注释，当前共 11 变体）。

---

## 10. 实施阶段计划（详细任务）

### ✅ P1: Workspace 重构（已完成）
- [x] 创建 workspace root Cargo.toml
- [x] 现有代码迁入 `hirust-mapper-core/`
- [x] 为 Mapper/SqlStatement/ResultMap/ResultColumn 添加 Clone derive
- [x] 创建 runtime/macros/facade 三个 stub crate
- [x] facade crate 实现 feature gate 聚合
- **验证**：`cargo test -p hirust-mapper-core` → 15 测试通过

### ✅ P2: 配置系统（已完成）
- [x] `HirustMapperConfig` 支持 serde::Deserialize
- [x] `parse_toml()` / `load_file()` TOML 加载
- [x] `discover_mapper_files()` glob 文件发现
- [x] `MapperRegistry::load_from_config()` 批量加载
- [x] `TypeAliasRegistry` 别名解析
- **验证**：`cargo test -p hirust-mapper-runtime` → 9 测试通过

### ✅ P3: Registry + Environment + SessionFactory
- [x] 引入 sqlx 依赖（feature-gated: mysql/postgres/sqlite）
- [x] `environment.rs`: `Environment` 包装 sqlx::AnyPool + `EnvironmentRegistry`
- [x] `session_factory.rs`: `SqlSessionFactory::build(config)` + `open_session()` + `SqlSession`
- [x] SqlSessionFactory 持有 `Arc<RwLock<MapperRegistry>>` + `Environment`
- [x] build() 内调用 `registry.load_from_config()` + `install_default_drivers()`
- [x] feature gates: `mysql`, `postgres`, `sqlite` (default: sqlite)
- **里程碑**：能从 TOML 配置启动，加载所有 mapper，创建 SessionFactory
- **验证**：runtime 17/17 测试通过（含 8 个 P3 新测试）

### ✅ P4: BoundSql 两阶段重构
- [x] core 新增 `BoundSql { sql, parameters }` 结构（sql_generator.rs）
- [x] 新增 `generate_bound_sql` / `replace_parameters_bound` / `join_with_spaces_bound`
- [x] `#{param}` → `?` 占位符 + 参数按出现顺序进列表
- [x] `${param}` 保持原样内联（与 `#{}` 同时出现即自动混合模式）
- [x] `Mapper::build_bound_sql` 高层 API + core lib.rs 导出
- [x] runtime `bound_sql.rs` 重新导出 + `SqlSession::build_bound_sql` 便捷方法
- [x] 保持 `build_sql`（内联模式）完全向后兼容
- **验证**：core 30/30 测试通过（原 15 + 新增 15 个 BoundSql 测试）

### ✅ P5: TypeHandler + 参数绑定
- [x] `type_handler/trait_def.rs`: TypeHandler trait（type_name / get_result / set_parameter）
- [x] `type_handler/standard.rs`: I32/I64/String/Bool/F64 内置 handler + TypeHandlerRegistry
- [x] feature-gated: ChronoHandler（`chrono`）/ UuidHandler（`uuid`）
- [x] `handler/parameter.rs`: ParameterHandler（Vec<Value> → sqlx::AnyArguments 绑定）
- [x] `handler/result_set.rs`: ResultSetHandler（AnyRow → Value::Object → T: DeserializeOwned，按 AnyTypeInfoKind 分派）
- [x] `bind_value` 按 Value 变体分派 JSON → sqlx 原语类型
- [x] 占位符数量校验 `validate_placeholder_count`
- **验证**：runtime 34/34 测试通过（含 17 个 P5 新测试，覆盖各类型 SQLite 内存库往返）

### ✅ P6: Executor + SqlSession
- [x] `executor/simple.rs`: SimpleExecutor（泛型 `E: sqlx::Executor`，同时支持 pool 与事务连接）
- [x] `session.rs`: SqlSession 全接口（select_one/select_list/insert/update/delete）
- [x] `session.rs`: MapperProxy 命名空间代理
- [x] 事务管理: begin/commit/rollback/close（基于 `sqlx::Transaction<'static, Any>`，close 隐式回滚）
- [x] MapperRuntimeError 补充 `Database(#[from] sqlx::Error)`
- [x] insert 生成主键：sqlite 同连接 `SELECT last_insert_rowid()`（Any 驱动不透传 last_insert_id）
- [x] ResultSetHandler 改为按**实际值类型**分派（修复 `count(*)` 等计算列）
- **里程碑**：✅ 完整 ORM 可用：加载 XML → 执行 SQL → 映射结果 → 事务
- **验证**：runtime 34 + e2e 10 测试通过（SQLite 内存库 + 真实 CRUD + 事务提交/回滚）

### ✅ P7: 热重载
- [x] 引入 notify 7 依赖
- [x] `hot_reload/watcher.rs`: `MapperWatcher`（notify + 去抖 worker 线程，安静期批量重解析）
- [x] `extract_watch_dirs`: 从 glob 模式推导监视目录
- [x] SqlSessionFactory::build() 启动 watcher（当 `mapper_refresh_interval_ms > 0`）
- [x] 回调重新解析 XML → `registry.insert_mapper()` 原子替换（线程安全）
- [x] 优雅关闭（Drop：watcher 断开事件通道 → worker 退出 → join）
- **验证**：runtime 39 + hot_reload 3 测试通过（修改 XML 文件后查询结果自动变化）

### ✅ P8: ResultMap 增强
- [x] model.rs: `ResultColumn`（is_id/rust_type）+ `ResultMap`（associations/collections）+ `NestedMapping` + `SelectKey`/`SelectKeyOrder` + `SqlStatement.select_key`
- [x] parser.rs: 解析 `<id>`/`<association>`/`<collection>`（递归 + 自闭合）+ `<selectKey>`
- [x] sql_generator.rs: 条件表达式支持 `.size()`/`.isEmpty()` + 布尔字面量比较（true/false）+ f64 比较
- [x] ResultSetHandler: 嵌套对象映射（association 一对一 + collection 一对多按 id 分组）
- [x] SqlSession: select_one/select_list 按 statement 的 resultMap 自动走嵌套映射
- **验证**：core 41（含 11 个 P8 解析/条件测试）+ nested_mapping 3（association/collection/select_one e2e）

### ✅ P9: Proc Macros
- [x] `hirust-mapper-macros/src/gen_mapper.rs`: `#[hirust_mapper(xml="...")]`
  - 编译时读取 XML（相对 `CARGO_MANIFEST_DIR`）+ `MyBatisXmlParser` 解析校验
  - 改写单元 struct 增加 `Arc<SqlSessionFactory>` 字段，生成 `new()` + 每语句同名方法
  - 方法委托 SqlSession（select→Vec\<T\>、insert→Option\<i64\>、update/delete→u64）
- [x] `hirust-mapper-macros/src/derive_model.rs`: `#[derive(MapperModel)]`
  - 解析 `#[mapper(column, type_handler)]`，生成 `column_mappings()` / `type_handlers()`
- [x] 语句 id 合法标识符校验；非单元 struct / 文件缺失 / 解析失败均编译期报错
- **验证**：macros 6 测试通过（dao_macro CRUD 3 + derive_model 列映射 3）

### ✅ P10: 门面 + 文档 + 示例
- [x] facade crate feature gates 完善：`macros` 隐含 `runtime`（生成代码依赖运行时）
- [x] `examples/runtime_basic.rs`（运行时 CRUD + 事务，`--features runtime`）
- [x] `examples/proc_macro_usage.rs` + `examples/mappers/UserDao.xml`（`--features full`）
- [x] README 重写为 v2（架构、feature 分层、运行时/宏 API、XML 格式、配置文件）
- [x] `[[example]]` required-features 声明，`cargo build --examples --features full` 零警告
- **验证**：2 示例可独立 `cargo run` 运行，workspace 102 测试全通过

**全部 10 个阶段已完成（P1-P10）✅**

> 可运行示例：
> ```sh
> cargo run --example runtime_basic --features runtime
> cargo run --example proc_macro_usage --features full
> ```

---

## 11. 关键设计决策

| 决策 | 选择 | 理由 |
|------|------|------|
| 数据库执行层 | sqlx | 异步、内置连接池、事务、多数据库 |
| 参数中间表示 | `serde_json::Value` | 与 MyBatis 一致，最大灵活性 |
| 共享状态 | `Arc<RwLock<MapperRegistry>>` | 读多写少，RwLock 允许并发读 |
| SQL 模式 | 自动降级：有 `${}` 时混合模式 | 部分内联无法参数化 |
| 宏加载方式 | `include_str!` | proc-macro 限制，热重载用运行时 API |
| crate 分层 | core(无async) / runtime(async) / macros(proc) | 关注点分离，core 可独立用于纯 SQL 生成 |

---

## 12. 验证策略

- **每个 P 阶段完成后**：`cargo test --workspace` 全绿（回归保护）
- **P1-P2 已验证**：core 15 + runtime 9 = 24 测试通过
- **P6 完成后**：端到端集成测试（SQLite 内存库 + 真实 CRUD）
- **P9 完成后**：proc macro 测试（trybuild crate 编译期验证）

---

## 13. 已实现模块索引（便于其他电脑快速定位代码）

| 模块 | 路径 | 功能 |
|------|------|------|
| `MyBatisXmlParser` | `hirust-mapper-core/src/parser.rs` | XML 解析（10 种动态标签） |
| `ResultMap` / `NestedMapping` / `SelectKey` | `hirust-mapper-core/src/model.rs` | 结果映射 + 嵌套 + 主键回填 (P8) |
| `DynamicSqlNode` | `hirust-mapper-core/src/model.rs` | 动态 SQL AST（10 变体） |
| `generate_sql` / `build_sql` | `hirust-mapper-core/src/sql_generator.rs` | SQL 生成 |
| `ParamsAccess` | `hirust-mapper-core/src/sql_generator.rs` | 参数访问 trait |
| `MapperError` | `hirust-mapper-core/src/model.rs` | 核心错误（6 变体） |
| `HirustMapperConfig` | `hirust-mapper-runtime/src/config.rs` | TOML 配置解析 |
| `MapperRegistry` | `hirust-mapper-runtime/src/registry.rs` | 线程安全 Mapper 注册表 |
| `TypeAliasRegistry` | `hirust-mapper-runtime/src/registry.rs` | 类型别名解析 |
| `MapperRuntimeError` | `hirust-mapper-runtime/src/error.rs` | 运行时错误（11 变体，含 Database） |
| `Environment` | `hirust-mapper-runtime/src/environment.rs` | 数据库连接池封装 (P3) |
| `EnvironmentRegistry` | `hirust-mapper-runtime/src/environment.rs` | 多数据库环境管理 (P3) |
| `SqlSessionFactory` | `hirust-mapper-runtime/src/session_factory.rs` | 应用级 Session 工厂 (P3) |
| `SqlSession` | `hirust-mapper-runtime/src/session_factory.rs` | 请求级轻量 Session (P3) |
| `BoundSql` | `hirust-mapper-core/src/sql_generator.rs` | 参数化绑定 SQL (?+参数列表) (P4) |
| `generate_bound_sql` | `hirust-mapper-core/src/sql_generator.rs` | 两阶段绑定 Phase 2 (P4) |
| `build_bound_sql` | `core: Mapper::build_bound_sql` / `runtime: SqlSession::build_bound_sql` | 绑定 SQL 生成入口 (P4) |
| `TypeHandler` / `TypeHandlerRegistry` | `hirust-mapper-runtime/src/type_handler/` | Value↔DB 列双向转换 + 注册表 (P5) |
| `ParameterHandler` | `hirust-mapper-runtime/src/handler/parameter.rs` | Vec<Value> → sqlx 参数绑定 (P5) |
| `ResultSetHandler` | `hirust-mapper-runtime/src/handler/result_set.rs` | AnyRow → T: DeserializeOwned (P5) |
| `SimpleExecutor` | `hirust-mapper-runtime/src/executor/simple.rs` | 泛型 sqlx 执行器（pool/事务） (P6) |
| `SqlSession` (完整) | `hirust-mapper-runtime/src/session.rs` | CRUD + 事务 + MapperProxy (P6) |
| `MapperProxy` | `hirust-mapper-runtime/src/session.rs` | 命名空间代理 (P6) |
| `MapperWatcher` | `hirust-mapper-runtime/src/hot_reload/watcher.rs` | 热重载监视器（notify + 去抖） (P7) |
| `#[hirust_mapper(xml)]` | `hirust-mapper-macros/src/gen_mapper.rs` | 编译时 DAO 方法生成 (P9) |
| `#[derive(MapperModel)]` | `hirust-mapper-macros/src/derive_model.rs` | 列映射内省派生 (P9) |
