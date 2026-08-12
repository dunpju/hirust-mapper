//! `#[dao]` 属性宏 + `#[mapper_query]` 方法标记 —— 签名驱动的类型化 DAO。
//!
//! - `#[dao]` on **unit struct**：改写为带 `__hm_factory: Arc<SqlSessionFactory>` 字段 + `new` / `factory`。
//! - `#[dao]` on **`impl`**：遍历带 `#[mapper_query]` 的 async 方法，按签名生成方法体，委托 `SqlSession`。
//!
//! 方法名→statement_id、模块路径(`module_path!()`)→namespace、形参名→SQL 参数键、返回类型→select/insert/...。

use std::path::PathBuf;

use hirust_mapper_core::MyBatisXmlParser;
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{parse_macro_input, Attribute, FnArg, ItemImpl, ItemStruct, Pat, ReturnType, Type};

const FACTORY_FIELD: &str = "__hm_factory";
const METHOD_MARKER: &str = "mapper_query";

/// 入口：按 item 类型分派
pub fn dao_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    // 先尝试解析为 impl 块
    if let Ok(impl_item) = syn::parse::<ItemImpl>(item.clone()) {
        return dao_on_impl(attr, impl_item);
    }
    // 否则按 struct 处理
    let struct_item = parse_macro_input!(item as ItemStruct);
    dao_on_struct(struct_item)
}

// ─── struct 侧：加 factory 字段 + new/factory ──────────────────────

fn dao_on_struct(mut item: ItemStruct) -> TokenStream {
    if !matches!(item.fields, syn::Fields::Unit) {
        return syn::Error::new_spanned(&item, "#[dao] 要求单元 struct（如 `struct UserDao;`）")
            .to_compile_error()
            .into();
    }
    let name = &item.ident;
    item.fields = syn::Fields::Named(syn::parse_quote!({
        __hm_factory: ::std::sync::Arc<::hirust_mapper_runtime::SqlSessionFactory>
    }));

    let expanded = quote! {
        #item

        impl #name {
            pub fn new(factory: ::hirust_mapper_runtime::SqlSessionFactory) -> Self {
                Self { __hm_factory: ::std::sync::Arc::new(factory) }
            }
            pub fn factory(&self) -> &::std::sync::Arc<::hirust_mapper_runtime::SqlSessionFactory> {
                &self.__hm_factory
            }
        }
    };
    expanded.into()
}

// ─── impl 侧：生成方法体 ───────────────────────────────────────────

#[derive(Default)]
struct DaoImplArgs {
    namespace: Option<String>,
    xml: Option<String>,
    field: Option<String>,
}

impl DaoImplArgs {
    fn parse(attr: TokenStream) -> syn::Result<Self> {
        if attr.is_empty() {
            return Ok(Self::default());
        }
        let mut args = Self::default();
        use syn::parse::Parser;
        let parsed = syn::punctuated::Punctuated::<syn::MetaNameValue, syn::Token![,]>::parse_terminated
            .parse2(attr.into())?;
        for nv in parsed {
            let ident = nv
                .path
                .get_ident()
                .ok_or_else(|| syn::Error::new_spanned(&nv.path, "期望标识符键"))?;
            let lit: syn::LitStr = match nv.value {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) => s,
                other => {
                    return Err(syn::Error::new_spanned(other, "期望字符串字面量"));
                }
            };
            match ident.to_string().as_str() {
                "namespace" => args.namespace = Some(lit.value()),
                "xml" => args.xml = Some(lit.value()),
                "field" => args.field = Some(lit.value()),
                other => {
                    return Err(syn::Error::new_spanned(
                        ident,
                        format!("未知属性 '{}'，支持 namespace / xml / field", other),
                    ));
                }
            }
        }
        Ok(args)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Select,
    Insert,
    Update,
    Delete,
}

fn dao_on_impl(attr: TokenStream, mut item: ItemImpl) -> TokenStream {
    let args = match DaoImplArgs::parse(attr) {
        Ok(a) => a,
        Err(e) => return e.to_compile_error().into(),
    };
    let field = args.field.as_deref().unwrap_or(FACTORY_FIELD);
    let field_ident = match syn::parse_str::<syn::Ident>(field) {
        Ok(i) => i,
        Err(_) => {
            return syn::Error::new(Span::call_site(), format!("field '{}' 非合法标识符", field))
                .to_compile_error()
                .into();
        }
    };

    // 可选：编译期加载 XML 校验 statement_id 存在
    let mapper = match args.xml.as_ref() {
        Some(p) => match load_mapper(p) {
            Ok(m) => Some(m),
            Err(e) => return e.to_compile_error().into(),
        },
        None => None,
    };

    // namespace 表达式
    let ns_expr: proc_macro2::TokenStream = match &args.namespace {
        Some(ns) => {
            let lit = ns.clone();
            quote! { #lit }
        }
        None => quote! { ::std::module_path!() },
    };

    // 遍历 impl 内的方法
    let mut errors: Vec<syn::Error> = Vec::new();
    for impl_item in item.items.iter_mut() {
        if let syn::ImplItem::Fn(method) = impl_item {
            // 找 #[mapper_query] 属性
            let marker_pos = method.attrs.iter().position(|a| a.path().is_ident(METHOD_MARKER));
            let marker_pos = match marker_pos {
                Some(p) => p,
                None => continue, // 无标记，跳过
            };
            let marker = method.attrs.remove(marker_pos);
            // 解析标记参数
            let (id_override, kind_arg) = match parse_marker(&marker) {
                Ok(v) => v,
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };

            // 生成方法体
            match gen_method_body(method, &id_override, kind_arg, &ns_expr, &field_ident, mapper.as_ref()) {
                Ok(()) => {}
                Err(e) => errors.push(e),
            }
        }
    }

    if !errors.is_empty() {
        let mut combined = errors.into_iter().fold(None::<syn::Error>, |acc, e| match acc {
            Some(mut a) => {
                a.combine(e);
                Some(a)
            }
            None => Some(e),
        });
        return combined.take().unwrap().to_compile_error().into();
    }

    quote! { #item }.into()
}

/// 解析 `#[mapper_query(id = "...", kind = "...")]`
fn parse_marker(attr: &Attribute) -> syn::Result<(Option<String>, Option<Kind>)> {
    let mut id = None;
    let mut kind = None;
    let _ = attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("id") {
            id = Some(meta.value()?.parse::<syn::LitStr>()?.value());
        } else if meta.path.is_ident("kind") {
            let s: syn::LitStr = meta.value()?.parse()?;
            kind = Some(match s.value().as_str() {
                "select" => Kind::Select,
                "insert" => Kind::Insert,
                "update" => Kind::Update,
                "delete" => Kind::Delete,
                other => {
                    return Err(syn::Error::new(
                        s.span(),
                        format!("非法 kind '{}'（select/insert/update/delete）", other),
                    ))
                }
            });
        } else {
            return Err(meta.error("未知参数，支持 id / kind"));
        }
        Ok(())
    });
    Ok((id, kind))
}

/// 生成单个方法体（原地替换 method.block）
fn gen_method_body(
    method: &mut syn::ImplItemFn,
    id_override: &Option<String>,
    kind_arg: Option<Kind>,
    ns_expr: &proc_macro2::TokenStream,
    field: &syn::Ident,
    mapper: Option<&hirust_mapper_core::Mapper>,
) -> syn::Result<()> {
    let sig = &method.sig;
    let method_name = sig.ident.to_string();
    let statement_id = id_override.clone().unwrap_or_else(|| method_name.clone());

    // 校验：async
    if sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            sig.ident.clone(),
            "#[mapper_query] 方法必须是 async fn",
        ));
    }
    // 校验：有 receiver
    let has_self = matches!(sig.inputs.first(), Some(FnArg::Receiver(_)));
    if !has_self {
        return Err(syn::Error::new_spanned(
            sig.ident.clone(),
            "#[mapper_query] 方法首参数必须是 &self / &mut self",
        ));
    }

    // 收集参数（跳过 self）
    let mut param_inserts: Vec<proc_macro2::TokenStream> = Vec::new();
    for arg in sig.inputs.iter().skip(1) {
        match arg {
            FnArg::Typed(pt) => {
                let name = match &*pt.pat {
                    Pat::Ident(pi) => pi.ident.to_string(),
                    Pat::Wild(_) => {
                        return Err(syn::Error::new_spanned(
                            pt.pat.clone(),
                            "#[mapper_query] 不支持匿名参数 `_`（需具名以映射 SQL 参数键）",
                        ));
                    }
                    _ => {
                        return Err(syn::Error::new_spanned(
                            pt.pat.clone(),
                            "#[mapper_query] 仅支持简单具名参数",
                        ));
                    }
                };
                let pat = &pt.pat;
                param_inserts.push(quote! {
                    __p.insert(#name.to_string(),
                        ::serde_json::to_value(#pat)
                            .map_err(|e| ::hirust_mapper_runtime::MapperRuntimeError::TypeConversion(e.to_string()))?);
                });
            }
            FnArg::Receiver(_) => {}
        }
    }

    // 推断 kind（结合返回类型）
    let inner = extract_result_inner(&sig.output)?;
    let kind = match (kind_arg, type_shape(inner)) {
        (Some(k), _) => k,
        (None, Shape::Vec) => Kind::Select,
        (None, Shape::Option) => Kind::Select,
        (None, _) => {
            return Err(syn::Error::new_spanned(
                inner,
                "无法推断操作类型：写操作须 #[mapper_query(kind=\"insert|update|delete\")]，查询用 Result<Vec<T>> 或 Result<Option<T>>",
            ));
        }
    };

    // 可选：编译期校验 statement_id 存在
    if let Some(m) = mapper
        && !m.statements.contains_key(&statement_id)
    {
        return Err(syn::Error::new_spanned(
            sig.ident.clone(),
            format!("XML 中不存在 statement id '{}'", statement_id),
        ));
    }

    // 拼装方法体
    let id_lit = statement_id.clone();
    let body = match kind {
        Kind::Select => match type_shape(inner) {
            Shape::Vec => quote! {
                let __ns = #ns_expr;
                let __id = #id_lit;
                let mut __p = <::std::collections::HashMap<String, ::serde_json::Value>>::new();
                #(#param_inserts)*
                let mut __s = self.#field.open_session();
                __s.select_list(__ns, __id, &__p).await
            },
            Shape::Option => quote! {
                let __ns = #ns_expr;
                let __id = #id_lit;
                let mut __p = <::std::collections::HashMap<String, ::serde_json::Value>>::new();
                #(#param_inserts)*
                let mut __s = self.#field.open_session();
                __s.select_one(__ns, __id, &__p).await
            },
            _ => {
                // 裸 T：select_one + 无行报错
                quote! {
                    let __ns = #ns_expr;
                    let __id = #id_lit;
                    let mut __p = <::std::collections::HashMap<String, ::serde_json::Value>>::new();
                    #(#param_inserts)*
                    let mut __s = self.#field.open_session();
                    __s.select_one(__ns, __id, &__p).await
                        .and_then(|__o| __o.ok_or_else(|| ::hirust_mapper_runtime::MapperRuntimeError::NoData {
                            namespace: __ns.into(), id: __id.into()
                        }))
                }
            }
        },
        Kind::Insert => write_body(&id_lit, ns_expr, &param_inserts, field, WriteKind::Insert, type_shape(inner)),
        Kind::Update => write_body(&id_lit, ns_expr, &param_inserts, field, WriteKind::Update, type_shape(inner)),
        Kind::Delete => write_body(&id_lit, ns_expr, &param_inserts, field, WriteKind::Delete, type_shape(inner)),
    };

    method.block = syn::parse_quote!({ #body });
    Ok(())
}

enum WriteKind {
    Insert,
    Update,
    Delete,
}

fn write_body(
    id_lit: &str,
    ns_expr: &proc_macro2::TokenStream,
    param_inserts: &[proc_macro2::TokenStream],
    field: &syn::Ident,
    wk: WriteKind,
    shape: Shape,
) -> proc_macro2::TokenStream {
    let method = match wk {
        WriteKind::Insert => quote! { insert },
        WriteKind::Update => quote! { update },
        WriteKind::Delete => quote! { delete },
    };
    let tail = match (wk, shape) {
        (WriteKind::Insert, Shape::OptionI64) => quote! { __s.#method(__ns, __id, &__pv).await },
        (WriteKind::Insert, Shape::I64) => {
            quote! { __s.#method(__ns, __id, &__pv).await.map(|__o| __o.unwrap_or(0)) }
        }
        (_, Shape::Unit) => quote! { __s.#method(__ns, __id, &__pv).await.map(|_| ()) },
        (_, Shape::U64) => quote! { __s.#method(__ns, __id, &__pv).await },
        _ => quote! { __s.#method(__ns, __id, &__pv).await.map(|_| ()) },
    };
    quote! {
        let __ns = #ns_expr;
        let __id = #id_lit;
        let mut __p = <::std::collections::HashMap<String, ::serde_json::Value>>::new();
        #(#param_inserts)*
        let __pv = ::serde_json::Value::Object(__p.into_iter().collect());
        let mut __s = self.#field.open_session();
        #tail
    }
}

// ─── 类型识别辅助 ──────────────────────────────────────────────────

#[derive(PartialEq)]
enum Shape {
    Vec,
    Option,
    I64,
    OptionI64,
    U64,
    Unit,
    Other,
}

fn type_shape(ty: &Type) -> Shape {
    if let Type::Tuple(t) = ty
        && t.elems.is_empty()
    {
        return Shape::Unit;
    }
    let Some(seg) = last_path_segment(ty) else {
        return Shape::Other;
    };
    let ident = seg.ident.to_string();
    match ident.as_str() {
        "Vec" => Shape::Vec,
        "Option" => {
            // Option<i64> 单独识别（insert 主键）
            if let Some(arg) = first_generic_arg(seg)
                && let Type::Path(p) = arg
                && p.path.segments.last().is_some_and(|s| s.ident == "i64")
            {
                return Shape::OptionI64;
            }
            Shape::Option
        }
        "i64" => Shape::I64,
        "u64" => Shape::U64,
        _ => Shape::Other,
    }
}

fn last_path_segment(ty: &Type) -> Option<&syn::PathSegment> {
    match ty {
        Type::Path(p) => p.path.segments.last(),
        _ => None,
    }
}

fn first_generic_arg(seg: &syn::PathSegment) -> Option<&Type> {
    let syn::PathArguments::AngleBracketed(ab) = &seg.arguments else {
        return None;
    };
    for arg in &ab.args {
        if let syn::GenericArgument::Type(t) = arg {
            return Some(t);
        }
    }
    None
}

/// 从返回类型 `Result<INNER>` 提取 INNER
fn extract_result_inner(ret: &ReturnType) -> syn::Result<&Type> {
    let ty = match ret {
        ReturnType::Default => {
            return Err(syn::Error::new(
                Span::call_site(),
                "#[mapper_query] 方法必须声明返回类型（hirust_mapper_runtime::Result<...>）",
            ));
        }
        ReturnType::Type(_, t) => t.as_ref(),
    };
    let seg = last_path_segment(ty).ok_or_else(|| {
        syn::Error::new_spanned(ty, "无法解析返回类型；期望 Result<...>")
    })?;
    if seg.ident != "Result" {
        return Err(syn::Error::new_spanned(
            ty,
            "返回类型应为 Result<...>（hirust_mapper_runtime::Result）",
        ));
    }
    first_generic_arg(seg).ok_or_else(|| {
        syn::Error::new_spanned(ty, "Result 缺少类型参数")
    })
}

// ─── XML 加载（编译期校验用）──────────────────────────────────────

fn load_mapper(xml_path: &str) -> syn::Result<hirust_mapper_core::Mapper> {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let full = PathBuf::from(&manifest).join(xml_path);
    let content = std::fs::read_to_string(&full).map_err(|e| {
        syn::Error::new(
            Span::call_site(),
            format!("#[dao] 无法读取 XML '{}': {}", full.display(), e),
        )
    })?;
    MyBatisXmlParser::new(&content).parse_mapper().map_err(|e| {
        syn::Error::new(
            Span::call_site(),
            format!("#[dao] 解析 XML '{}' 失败: {}", full.display(), e),
        )
    })
}
