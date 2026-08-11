//! `#[derive(MapperModel)]` 派生宏实现
//!
//! 解析字段上的 `#[mapper(column = "...", type_handler = "...")]` 属性，
//! 生成 `column_mappings()` 内省方法（字段名 → 列名），供高级映射场景使用。

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, LitStr};

/// 派生宏入口
pub fn derive_mapper_model_impl(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // 仅支持 struct
    let fields = match &input.data {
        syn::Data::Struct(s) => &s.fields,
        _ => {
            return syn::Error::new_spanned(&input, "MapperModel 仅支持 struct")
                .to_compile_error()
                .into();
        }
    };

    // 收集 (字段名, 列名, 可选 type_handler)
    let mut mappings = Vec::new();
    let mut handlers = Vec::new();
    for field in fields.iter() {
        let field_ident = match &field.ident {
            Some(i) => i,
            None => continue, // 元组字段跳过
        };
        let field_name = field_ident.to_string();
        let mut column = field_name.clone();
        let mut type_handler: Option<String> = None;

        for attr in &field.attrs {
            if !attr.path().is_ident("mapper") {
                continue;
            }
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("column") {
                    let v = meta.value()?;
                    let s: LitStr = v.parse()?;
                    column = s.value();
                } else if meta.path.is_ident("type_handler") {
                    let v = meta.value()?;
                    let s: LitStr = v.parse()?;
                    type_handler = Some(s.value());
                } else {
                    return Err(meta.error("未知属性，支持 `column` 与 `type_handler`"));
                }
                Ok(())
            });
        }

        mappings.push(quote! { (#field_name, #column) });
        if let Some(h) = type_handler {
            handlers.push(quote! { (#field_name, #h) });
        }
    }

    let expanded = quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            /// 字段名 → 列名映射（未声明 column 时默认与字段名相同）
            pub fn column_mappings() -> &'static [(&'static str, &'static str)] {
                &[#(#mappings),*]
            }

            /// 声明了 type_handler 的字段 → 处理器类型名
            pub fn type_handlers() -> &'static [(&'static str, &'static str)] {
                &[#(#handlers),*]
            }
        }
    };
    expanded.into()
}
