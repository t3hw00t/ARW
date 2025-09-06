use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::Token;
use syn::{parse_macro_input, Expr, ExprArray, ExprLit, ItemFn, Lit, LitStr, Meta};

/// Register a tool into the global registry.
/// Usage:
/// #[arw_tool(
///   id="x",
///   version="1.0.0",
///   summary="...",
///   stability="experimental",
///   capabilities("read-only","write")
/// )]
/// fn any_name() {}
#[proc_macro_attribute]
pub fn arw_tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    let metas = Punctuated::<Meta, Token![,]>::parse_terminated
        .parse(attr)
        .unwrap_or_default();
    let input_fn = parse_macro_input!(item as ItemFn);

    let mut id: Option<String> = None;
    let mut version: Option<String> = None;
    let mut summary: Option<String> = None;
    let mut stability: Option<String> = None;
    let mut caps: Vec<String> = Vec::new();

    for m in metas {
        match m {
            Meta::NameValue(nv) => {
                let key = nv.path.get_ident().map(|i| i.to_string());
                if let Some(k) = key {
                    match (k.as_str(), &nv.value) {
                        (
                            "id",
                            Expr::Lit(ExprLit {
                                lit: Lit::Str(s), ..
                            }),
                        ) => id = Some(s.value()),
                        (
                            "version",
                            Expr::Lit(ExprLit {
                                lit: Lit::Str(s), ..
                            }),
                        ) => version = Some(s.value()),
                        (
                            "summary",
                            Expr::Lit(ExprLit {
                                lit: Lit::Str(s), ..
                            }),
                        ) => summary = Some(s.value()),
                        (
                            "stability",
                            Expr::Lit(ExprLit {
                                lit: Lit::Str(s), ..
                            }),
                        ) => stability = Some(s.value()),
                        ("capabilities", Expr::Array(ExprArray { elems, .. })) => {
                            for e in elems.iter() {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(s), ..
                                }) = e
                                {
                                    caps.push(s.value());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Meta::List(ml) => {
                if ml.path.is_ident("capabilities") {
                    if let Ok(list) =
                        Punctuated::<LitStr, Token![,]>::parse_terminated.parse2(ml.tokens)
                    {
                        for s in list {
                            caps.push(s.value());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let id_lit = LitStr::new(id.as_deref().unwrap_or("unknown"), Span::call_site());
    let version_lit = LitStr::new(version.as_deref().unwrap_or("0.0.0"), Span::call_site());
    let summary_lit = LitStr::new(summary.as_deref().unwrap_or(""), Span::call_site());
    let stability_lit = LitStr::new(
        stability.as_deref().unwrap_or("experimental"),
        Span::call_site(),
    );
    let cap_lits: Vec<LitStr> = caps
        .iter()
        .map(|c| LitStr::new(c, Span::call_site()))
        .collect();

    // IMPORTANT: use only static literals so inventory::submit! is const-friendly.
    let gen = quote! {
        #input_fn
        inventory::submit! {
            arw_core::ToolInfo {
                id: #id_lit,
                version: #version_lit,
                summary: #summary_lit,
                stability: #stability_lit,
                capabilities: &[ #( #cap_lits ),* ]
            }
        }
    };
    gen.into()
}
