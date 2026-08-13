extern crate proc_macro;

use proc_macro::TokenStream;

#[proc_macro]
pub fn make_dep(_input: TokenStream) -> TokenStream {
    "dep_lib::VALUE".parse().expect("static proc-macro output should parse")
}
