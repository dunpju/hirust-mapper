//! # hirust-mapper-macros
//!
//! 编译时类型安全层。当前为骨架，具体宏将在 P9 阶段实现：
//!
//! - `#[derive(MapperModel)]` — 自动行映射
//! - `#[hirust_mapper(xml = "...")]` — 编译时 Mapper 方法生成

use proc_macro::TokenStream;

/// 占位宏，后续阶段实现
#[proc_macro_attribute]
pub fn hirust_mapper(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// 占位 derive 宏，后续阶段实现
#[proc_macro_derive(MapperModel, attributes(mapper))]
pub fn derive_mapper_model(_item: TokenStream) -> TokenStream {
    TokenStream::new()
}
