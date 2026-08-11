//! `#[hirust_mapper(xml = "...")]` 属性宏实现
//!
//! 在编译时读取并解析 mapper XML，校验合法性，并为每个语句生成类型化的方法，
//! 方法体委托 `SqlSession`。生成的 DAO 结构体持有一个 `Arc<SqlSessionFactory>`。

use std::path::PathBuf;

use hirust_mapper_core::{MyBatisXmlParser, StatementType};
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{parse::Parse, parse_macro_input, Ident, ItemStruct, LitStr, Token};

/// 属性参数：`xml = "path"`
struct MapperAttr {
    xml_path: String,
}

impl Parse for MapperAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let ident: Ident = input.parse()?;
        if ident != "xml" {
            return Err(syn::Error::new(
                ident.span(),
                "未知属性，仅支持 `xml = \"路径\"`",
            ));
        }
        let _eq: Token![=] = input.parse()?;
        let lit: LitStr = input.parse()?;
        Ok(Self {
            xml_path: lit.value(),
        })
    }
}

/// 属性宏入口
pub fn gen_mapper_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as MapperAttr);
    let mut item_struct = parse_macro_input!(item as ItemStruct);
    let struct_name = item_struct.ident.clone();
    let struct_vis = item_struct.vis.clone();

    // 1. 解析 XML 文件路径（相对 CARGO_MANIFEST_DIR）
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let xml_path = PathBuf::from(&manifest).join(&args.xml_path);
    let xml_content = match std::fs::read_to_string(&xml_path) {
        Ok(c) => c,
        Err(e) => {
            return syn::Error::new(
                Span::call_site(),
                format!("无法读取 mapper XML '{}': {}", xml_path.display(), e),
            )
            .to_compile_error()
            .into();
        }
    };

    // 2. 编译时解析 + 校验
    let mapper = match MyBatisXmlParser::new(&xml_content).parse_mapper() {
        Ok(m) => m,
        Err(e) => {
            return syn::Error::new(
                Span::call_site(),
                format!("解析 mapper XML '{}' 失败: {}", xml_path.display(), e),
            )
            .to_compile_error()
            .into();
        }
    };
    let namespace = mapper.namespace.clone();

    // 3. 校验：必须是单元 struct
    if !matches!(item_struct.fields, syn::Fields::Unit) {
        return syn::Error::new_spanned(
            &item_struct,
            "#[hirust_mapper] 要求单元 struct（例如 `struct UserDao;`）",
        )
        .to_compile_error()
        .into();
    }

    // 4. 改写 struct：增加私有 factory 字段
    item_struct.fields = syn::Fields::Named(syn::parse_quote!({
        __hm_factory: ::std::sync::Arc<::hirust_mapper_runtime::SqlSessionFactory>
    }));

    // 5. 为每个语句生成方法
    let mut methods = Vec::new();
    let mut has_stmts = false;
    for (id, stmt) in &mapper.statements {
        has_stmts = true;
        // 语句 id → 方法名（校验为合法标识符）
        let method_ident = match validate_ident(id) {
            Ok(i) => i,
            Err(e) => return e.to_compile_error().into(),
        };
        let id_lit = id.clone();
        let ns_lit = namespace.clone();

        let method = match stmt.stmt_type {
            Some(StatementType::Select) => quote! {
                /// 查询多行（select_list）。
                ///
                /// 返回类型由调用方指定：`dao.find_by_id::<User>(&params).await`
                pub async fn #method_ident<T>(
                    &self,
                    params: &::std::collections::HashMap<String, ::serde_json::Value>,
                ) -> ::hirust_mapper_runtime::Result<::std::vec::Vec<T>>
                where
                    T: ::serde::de::DeserializeOwned + Send,
                {
                    let mut session = self.__hm_factory.open_session();
                    session.select_list(#ns_lit, #id_lit, params).await
                }
            },
            Some(StatementType::Insert) => quote! {
                /// 插入（返回生成主键，若驱动支持）
                pub async fn #method_ident<T: ::serde::Serialize>(
                    &self,
                    params: &T,
                ) -> ::hirust_mapper_runtime::Result<::std::option::Option<i64>> {
                    let mut session = self.__hm_factory.open_session();
                    session.insert(#ns_lit, #id_lit, params).await
                }
            },
            Some(StatementType::Update) => quote! {
                /// 更新（返回受影响行数）
                pub async fn #method_ident<T: ::serde::Serialize>(
                    &self,
                    params: &T,
                ) -> ::hirust_mapper_runtime::Result<u64> {
                    let mut session = self.__hm_factory.open_session();
                    session.update(#ns_lit, #id_lit, params).await
                }
            },
            Some(StatementType::Delete) => quote! {
                /// 删除（返回受影响行数）
                pub async fn #method_ident<T: ::serde::Serialize>(
                    &self,
                    params: &T,
                ) -> ::hirust_mapper_runtime::Result<u64> {
                    let mut session = self.__hm_factory.open_session();
                    session.delete(#ns_lit, #id_lit, params).await
                }
            },
            None => continue,
        };
        methods.push(method);
    }

    let _ = has_stmts;

    let expanded = quote! {
        #item_struct

        #struct_vis impl #struct_name {
            /// 由 SqlSessionFactory 构造 DAO
            pub fn new(factory: ::hirust_mapper_runtime::SqlSessionFactory) -> Self {
                Self { __hm_factory: ::std::sync::Arc::new(factory) }
            }

            /// 内部工厂引用
            pub fn factory(&self) -> &::std::sync::Arc<::hirust_mapper_runtime::SqlSessionFactory> {
                &self.__hm_factory
            }

            #(#methods)*
        }
    };
    expanded.into()
}

/// 校验字符串为合法 Rust 标识符并返回 Ident
fn validate_ident(s: &str) -> syn::Result<Ident> {
    // 简单校验：非空、首字符为字母/下划线、其余为字母数字下划线
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => {
            return Err(syn::Error::new(
                Span::call_site(),
                format!("语句 id '{}' 不是合法的 Rust 标识符", s),
            ))
        }
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(syn::Error::new(
            Span::call_site(),
            format!("语句 id '{}' 不是合法的 Rust 标识符", s),
        ));
    }
    Ok(format_ident!("{}", s))
}
