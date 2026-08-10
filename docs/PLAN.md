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
| P3 Registry + Environment | ⬜ 待实施 | — | — |
| P4 BoundSql 两阶段重构 | ⬜ 待实施 | — | — |
| P5 TypeHandler + 参数绑定 | ⬜ 待实施 | — | — |
| P6 Executor + SqlSession | ⬜ 待实施 | — | — |
| P7 热重载 | ⬜ 待实施 | — | — |
| P8 ResultMap 增强 | ⬜ 待实施 | — | — |
| P9 Proc Macros | ⬜ 待实施 | — | — |
| P10 门面 + 文档 + 示例 | ⬜ 待实施 | — | — |

**当前可在全新电脑上运行的验证命令：**

```bash
cargo test --workspace     # 应输出 core 15 + runtime 9 = 24 个测试通过
```

---

## 如何在其他电脑上接续实施

1. **克隆并验证当前状态**
   ```bash
   git clone <repo-url> hirust-mapper
   cd hirust-mapper
   cargo test --workspace          # 确认 24 个测试全通过
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
│       └── sql_generator.rs         # generate_sql + build_sql
│
├── hirust-mapper-runtime/            # ORM 运行时（部分已实现）
│   └── src/
│       ├── config.rs                 # ✅ HirustMapperConfig (TOML 解析)
│       ├── error.rs                  # ✅ MapperRuntimeError
│       ├── registry.rs               # ✅ MapperRegistry + TypeAliasRegistry
│       ├── environment.rs            # ⬜ sqlx::Pool 包装 (P3)
│       ├── session_factory.rs        # ⬜ SqlSessionFactory (P3)
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
├── hirust-mapper-macros/              # proc_macro 骨架（占位宏已就位）
│   └── src/lib.rs                    # ⬜ 实际宏实现 (P9)
│
├── hirust-mapper/                     # 门面 crate ✅
│   └── src/lib.rs                    # feature gate 聚合
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
| sqlx | 0.8 | 数据库执行层 | ⬜ P3 引入 |
| tokio | 1 | async runtime | ⬜ P3 引入 |
| notify | 7 | 文件变更监控/热重载 | ⬜ P7 引入 |
| chrono/uuid | 0.4/1 | 可选类型处理器 | ⬜ P5 引入 |

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

## 3. 扩展的 XML Mapper 格式（P8 待实施）

在现有格式基础上新增（向后兼容）：

```xml
<mapper namespace="myapp::dao::UserDao">
    <!-- P8 新增: resultMap 支持 id/association/collection -->
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

    <!-- P8 新增: selectKey -->
    <insert id="insertWithKey">
        <selectKey keyProperty="id" resultType="i64" order="AFTER">
            SELECT LAST_INSERT_ID()
        </selectKey>
        INSERT INTO users (name) VALUES (#{name})
    </insert>
</mapper>
```

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

## 5. 两阶段 SQL 解析（P4 待实施）

现有 `generate_sql` 直接内联值。P4 重构为两阶段：

| 阶段 | 时机 | 输出 |
|------|------|------|
| Phase 1: 解析 | 启动时 | `Mapper` (DynamicSqlNode AST) — ✅ 现有代码 |
| Phase 2: 绑定 | 每次查询时 | `BoundSql { sql: String(含?占位符), params: Vec<BoundParameter> }` — ⬜ P4 |

- `#{param}` → `?` 占位符 + 参数加入列表
- `${param}` → 原样内联（无法参数化的部分保持内联模式）
- 检测到 `${}` 时自动降级为混合模式（部分内联 + 部分 ?）

---

## 6. 运行时 API 使用示例（P6 完成后可用）

```rust
use hirust_mapper::{SqlSessionFactory, HirustMapperConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct User { id: Option<i64>, name: String, email: String }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 加载配置
    let config = HirustMapperConfig::load_file("hirust-mapper.toml")?;

    // 2. 创建 SessionFactory (应用级, 线程安全)
    let factory = SqlSessionFactory::build(config).await?;

    // 3. 获取 Session (请求级)
    let mut session = factory.open_session().await?;

    // 4. 查询
    let mut params = HashMap::new();
    params.insert("id".into(), json!(42));
    let user: User = session.select_one("myapp::dao::UserDao", "findById", &params).await?;

    // 5. Mapper 代理模式
    let dao = session.get_mapper("myapp::dao::UserDao")?;
    let users: Vec<User> = dao.query("findByName", &params).await?;

    // 6. 事务
    session.begin().await?;
    session.insert("myapp::dao::UserDao", "insert", &new_user).await?;
    session.commit().await?;
    Ok(())
}
```

> **注意**：P2 已实现配置加载，P3-P6 完成后上述完整流程才可用。当前（P2 完成后）可用的流程是：
> ```rust
> let config = HirustMapperConfig::parse_toml(toml_str)?;
> let registry = MapperRegistry::new();
> registry.load_from_config(&config, &base_dir)?;
> let sql = registry.get_mapper("ns")?.build_sql("id", &params)?;
> ```

---

## 7. Proc Macro API 使用示例（P9 待实施）

### `#[derive(MapperModel)]` — 自动行映射

```rust
use hirust_mapper_macros::MapperModel;

#[derive(MapperModel, Deserialize)]
struct User {
    #[mapper(column = "user_name")]
    name: String,
    email: String,
    #[mapper(column = "created_at", type_handler = "chrono::DateTime<chrono::Utc>")]
    created_at: Option<chrono::DateTime<chrono::Utc>>,
}
```

### `#[hirust_mapper(xml = "...")]` — 编译时 Mapper 生成

```rust
#[hirust_mapper(xml = "mappers/UserDao.xml")]
struct UserDao;

let factory = SqlSessionFactory::build(config).await?;
let dao = UserDao::new(factory);
let user: Option<User> = dao.find_by_id(42).await?;  // 类型安全！
```

proc_macro 内部：`include_str!` → 同 core 解析器解析 XML → 按方法签名生成类型化代码 → 委托 `SqlSession` 方法。

---

## 8. 热重载机制（P7 待实施）

1. `SqlSessionFactory::build()` 启动时，若 `mapper_refresh_interval_ms > 0`，创建 `notify::Watcher`
2. watcher 监控 glob 匹配的 XML 文件所在目录
3. 文件变更事件通过 channel 发送到专用线程，200ms 去抖
4. 回调函数重新解析 XML → `MyBatisXmlParser::parse_mapper()` → 调用 `registry.insert_mapper()` 替换
5. `MapperRegistry` 使用 `Arc<RwLock<>>` 保证并发读写安全（P2 已就位）

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

P6 实施时需补充 `Database(#[from] sqlx::Error)` 变体。

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

### ⬜ P3: Registry + Environment + SessionFactory
- [ ] 引入 sqlx 依赖（feature-gated: mysql/postgres/sqlite）
- [ ] `environment.rs`: `Environment` 包装 sqlx::Pool
- [ ] `session_factory.rs`: `SqlSessionFactory::build(config)` + `open_session()`
- [ ] SqlSessionFactory 持有 `Arc<RwLock<MapperRegistry>>` + `Environment`
- [ ] build() 内调用 `registry.load_from_config()`
- **里程碑**：能从 TOML 配置启动，加载所有 mapper，创建 SessionFactory
- **验证**：集成测试用 SQLite 内存库验证连接池创建

### ⬜ P4: BoundSql 两阶段重构
- [ ] 新增 `bound_sql.rs`: `BoundSql { sql, params }` 结构
- [ ] 重构 `generate_sql` → 新增 `generate_bound_sql` 输出 BoundSql
- [ ] `#{param}` → `?` 占位符 + 参数进列表
- [ ] `${param}` 保持内联（检测到则标记混合模式）
- [ ] 保持 `build_sql`（内联模式）向后兼容
- **验证**：core 15 测试仍通过 + 新增 BoundSql 测试

### ⬜ P5: TypeHandler + 参数绑定
- [ ] `type_handler/trait_def.rs`: TypeHandler trait
- [ ] `type_handler/standard.rs`: i32/i64/String/bool/f64 内置 handler
- [ ] feature-gated: ChronoHandler, UuidHandler
- [ ] `handler/parameter.rs`: ParameterHandler (Value → sqlx bind)
- [ ] `handler/result_set.rs`: ResultSetHandler (Row → T via serde)
- **验证**：单元测试覆盖每种类型的双向转换

### ⬜ P6: Executor + SqlSession
- [ ] `executor/simple.rs`: SimpleExecutor (sqlx 执行)
- [ ] `session.rs`: SqlSession 全接口 (select_one/select_list/insert/update/delete)
- [ ] `session.rs`: MapperProxy 命名空间代理
- [ ] 事务管理: begin/commit/rollback (基于 sqlx::Transaction)
- [ ] MapperRuntimeError 补充 `Database(#[from] sqlx::Error)`
- **里程碑**：完整 ORM 可用：加载 XML → 执行 SQL → 映射结果 → 事务
- **验证**：端到端集成测试（SQLite 内存库 + 真实 CRUD）

### ⬜ P7: 热重载
- [ ] 引入 notify 依赖
- [ ] `hot_reload/watcher.rs`: MapperWatcher (去抖事件循环)
- [ ] SqlSessionFactory::build() 启动 watcher（当 refresh_interval > 0）
- [ ] 回调重新解析 XML → registry.insert_mapper() 替换
- **验证**：测试修改 XML 文件后查询结果变化

### ⬜ P8: ResultMap 增强
- [ ] model.rs: ResultMap 支持 association/collection 嵌套
- [ ] parser.rs: 解析 `<id>`/`<association>`/`<collection>`/`<selectKey>` 标签
- [ ] sql_generator.rs: 条件表达式支持 `.size()`/`.isEmpty()`
- [ ] ResultSetHandler: 嵌套对象映射
- **验证**：复杂 resultMap 映射测试

### ⬜ P9: Proc Macros
- [ ] `hirust-mapper-macros/src/derive_model.rs`: #[derive(MapperModel)]
- [ ] `hirust-mapper-macros/src/gen_mapper.rs`: #[hirust_mapper(xml="...")]
- [ ] include_str! 编译时加载 XML + 解析 + 方法签名生成
- [ ] 方法体委托 SqlSession
- **验证**：trybuild crate 编译期测试

### ⬜ P10: 门面 + 文档 + 示例
- [ ] facade crate 完善 feature gates
- [ ] 更新 README（v2 API 示例）
- [ ] examples/runtime_basic.rs
- [ ] examples/proc_macro_usage.rs
- **验证**：示例可独立编译运行

**总计剩余约 18-26 个工作日（P3-P10）。**

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
| `DynamicSqlNode` | `hirust-mapper-core/src/model.rs` | 动态 SQL AST（10 变体） |
| `generate_sql` / `build_sql` | `hirust-mapper-core/src/sql_generator.rs` | SQL 生成 |
| `ParamsAccess` | `hirust-mapper-core/src/sql_generator.rs` | 参数访问 trait |
| `MapperError` | `hirust-mapper-core/src/model.rs` | 核心错误（6 变体） |
| `HirustMapperConfig` | `hirust-mapper-runtime/src/config.rs` | TOML 配置解析 |
| `MapperRegistry` | `hirust-mapper-runtime/src/registry.rs` | 线程安全 Mapper 注册表 |
| `TypeAliasRegistry` | `hirust-mapper-runtime/src/registry.rs` | 类型别名解析 |
| `MapperRuntimeError` | `hirust-mapper-runtime/src/error.rs` | 运行时错误（10 变体） |
