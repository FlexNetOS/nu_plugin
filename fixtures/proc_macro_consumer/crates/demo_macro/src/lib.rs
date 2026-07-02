use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn demo_attr(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
