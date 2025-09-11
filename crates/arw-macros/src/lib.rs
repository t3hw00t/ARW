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
    // Basic compile-time validations
    let mut preamble = proc_macro2::TokenStream::new();
    if id.as_deref().is_none() || id.as_deref().unwrap().trim().is_empty() {
        preamble.extend(quote! { compile_error!("#[arw_tool] requires a non-empty id=\"...\""); });
    } else if let Some(ref s) = id {
        if !s.contains('.') || s.contains(' ') {
            preamble.extend(quote! { compile_error!("arw_tool id should include a namespace (e.g., ns.name) and no spaces"); });
        }
    }
    if version.as_deref().is_none() || version.as_deref().unwrap().trim().is_empty() {
        preamble
            .extend(quote! { compile_error!("#[arw_tool] requires a semver version=\"x.y.z\""); });
    } else if let Some(ref v) = version {
        if !v.chars().all(|c| c.is_ascii_digit() || c == '.') || v.split('.').count() < 3 {
            preamble.extend(quote! { compile_error!("arw_tool version should look like x.y.z"); });
        }
    }

    let gen = quote! {
        #preamble
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

/// Include a WASM plug‑in at compile time and instantiate it using `arw-core`'s
/// runtime helpers. Usage: `load_wasm_tool!("path/to/plugin.wasm")`.
#[proc_macro]
pub fn load_wasm_tool(input: TokenStream) -> TokenStream {
    let path = parse_macro_input!(input as LitStr);
    let gen = quote! {{
        use arw_core::wasm::{Engine, WasmTool};
        static WASM_BYTES: &[u8] = include_bytes!(#path);
        let engine = Engine::default();
        WasmTool::from_bytes(&engine, WASM_BYTES).expect("invalid WASM module")
    }};
    gen.into()
}

/// Gate an axum handler by a central gating key.
/// Usage: #[arw_gate("tools:run")]
#[proc_macro_attribute]
pub fn arw_gate(attr: TokenStream, item: TokenStream) -> TokenStream {
    let key_lit = parse_macro_input!(attr as LitStr);
    let mut func = parse_macro_input!(item as ItemFn);
    let key = key_lit.value();
    let gate_stmt = quote! {
        if !arw_core::gating::allowed(#key) {
            return (axum::http::StatusCode::FORBIDDEN, "gated").into_response();
        }
    };
    // Prepend gate check to the function body
    let orig_block = func.block;
    let wrapped = quote! {{
        #gate_stmt
        #orig_block
    }};
    func.block = Box::new(syn::parse2(wrapped).expect("wrap body"));
    quote! { #func }.into()
}

/// Register an admin HTTP endpoint for discovery. Usage:
/// #[arw_admin(method="GET", path="/admin/foo", summary="...")]
/// async fn handler(...) { ... }
#[proc_macro_attribute]
pub fn arw_admin(attr: TokenStream, item: TokenStream) -> TokenStream {
    let metas = Punctuated::<Meta, Token![,]>::parse_terminated
        .parse(attr)
        .unwrap_or_default();
    let input_fn = parse_macro_input!(item as ItemFn);

    let mut method: Option<String> = None;
    let mut path: Option<String> = None;
    let mut summary: Option<String> = None;

    for m in metas {
        match m {
            Meta::NameValue(nv) => {
                let key = nv.path.get_ident().map(|i| i.to_string());
                if let Some(k) = key {
                    match (k.as_str(), &nv.value) {
                        (
                            "method",
                            Expr::Lit(ExprLit {
                                lit: Lit::Str(s), ..
                            }),
                        ) => method = Some(s.value()),
                        (
                            "path",
                            Expr::Lit(ExprLit {
                                lit: Lit::Str(s), ..
                            }),
                        ) => path = Some(s.value()),
                        (
                            "summary",
                            Expr::Lit(ExprLit {
                                lit: Lit::Str(s), ..
                            }),
                        ) => summary = Some(s.value()),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    let method_s = method.unwrap_or_else(|| "GET".to_string());
    let path_s = path.unwrap_or_else(|| "/admin".to_string());
    let summary_s = summary.unwrap_or_else(|| "".to_string());

    // Minimal compile-time validations
    let mut preamble = proc_macro2::TokenStream::new();
    let valid_methods = [
        "GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "HEAD",
    ];
    if !valid_methods.iter().any(|m| *m == method_s) {
        preamble.extend(quote! { compile_error!("arw_admin: method must be an HTTP verb like GET/POST"); });
    }
    if !path_s.starts_with("/admin") {
        preamble.extend(quote! { compile_error!("arw_admin: path must start with '/admin'"); });
    }

    let method_lit = LitStr::new(&method_s, Span::call_site());
    let path_lit = LitStr::new(&path_s, Span::call_site());
    let summary_lit = LitStr::new(&summary_s, Span::call_site());

    let gen = quote! {
        #preamble
        #input_fn
        inventory::submit! {
            arw_core::AdminEndpoint { method: #method_lit, path: #path_lit, summary: #summary_lit }
        }
    };
    gen.into()
}
