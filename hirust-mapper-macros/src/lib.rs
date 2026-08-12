//! # hirust-mapper-macros
//!
//! 编译时类型安全层（proc_macro）：
//!
//! - `#[hirust_mapper(xml = "...")]` — 编译时加载并解析 mapper XML，校验合法性，
//!   为每个语句生成类型化方法（委托 `SqlSession`）。
//! - `#[derive(MapperModel)]` — 解析 `#[mapper(column, type_handler)]` 属性，
//!   生成列映射内省方法。
//! - `#[dao]` + `#[mapper_query]` — 签名驱动的类型化 DAO：方法名→statement_id、
//!   模块路径→namespace、形参名→SQL 参数键、返回类型→select/insert/...。

mod dao;
mod derive_model;
mod gen_mapper;

use proc_macro::TokenStream;

/// 编译时 Mapper 方法生成。
///
/// 要求单元 struct（如 `struct UserDao;`）。在编译时读取并解析 `xml` 指向的 mapper
/// 文件（路径相对 `CARGO_MANIFEST_DIR`），生成持有 `Arc<SqlSessionFactory>` 的 DAO，
/// 为每个 `<select>`/`<insert>`/`<update>`/`<delete>` 生成同名方法。
///
/// ```ignore
/// #[hirust_mapper::hirust_mapper(xml = "mappers/UserDao.xml")]
/// struct UserDao;
///
/// let dao = UserDao::new(factory);
/// let users: Vec<User> = dao.findById(&params).await?;
/// ```
#[proc_macro_attribute]
pub fn hirust_mapper(attr: TokenStream, item: TokenStream) -> TokenStream {
    gen_mapper::gen_mapper_impl(attr, item)
}

/// 自动行映射模型派生。
///
/// 解析字段上的 `#[mapper(column = "...", type_handler = "...")]` 属性，
/// 生成 `column_mappings()` 与 `type_handlers()` 内省方法。
///
/// ```ignore
/// #[derive(MapperModel, Deserialize)]
/// struct User {
///     #[mapper(column = "user_name")]
///     name: String,
///     age: i64,
/// }
/// ```
#[proc_macro_derive(MapperModel, attributes(mapper))]
pub fn derive_mapper_model(item: TokenStream) -> TokenStream {
    derive_model::derive_mapper_model_impl(item)
}

/// 类型化 DAO 属性宏。
///
/// 双用途（按 item 类型分派）：
/// - **`#[dao]` on unit struct**：改写为持有 `Arc<SqlSessionFactory>` 的 struct，
///   生成 `new(factory)` / `factory()`。
/// - **`#[dao]` on `impl`**：遍历带 `#[mapper_query]` 的 async 方法，按签名生成方法体。
///   可选 `namespace = "..."`（默认 `module_path!()`）、`xml = "..."`（编译期校验 statement_id）、`field = "..."`。
///
/// ```ignore
/// use hirust_mapper::{dao, mapper_query, Result};
///
/// #[dao]
/// struct UserDao;
///
/// #[dao]
/// impl UserDao {
///     #[mapper_query]
///     async fn find_by_id(&self, id: i64) -> Result<Option<User>> {}
///     #[mapper_query(kind = "insert")]
///     async fn create(&self, name: String, age: i64) -> Result<i64> {}
/// }
/// ```
#[proc_macro_attribute]
pub fn dao(attr: TokenStream, item: TokenStream) -> TokenStream {
    dao::dao_impl(attr, item)
}
