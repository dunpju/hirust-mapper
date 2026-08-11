//! # hirust-mapper-macros
//!
//! 编译时类型安全层（proc_macro）：
//!
//! - `#[hirust_mapper(xml = "...")]` — 编译时加载并解析 mapper XML，校验合法性，
//!   为每个语句生成类型化方法（委托 `SqlSession`）。
//! - `#[derive(MapperModel)]` — 解析 `#[mapper(column, type_handler)]` 属性，
//!   生成列映射内省方法。

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
