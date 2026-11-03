//! Instruments disposable copies and checks independently expanded projection bodies.
#![forbid(unsafe_code)]

use quote::{quote, ToTokens};
use std::{env, fs, path::Path};
use syn::visit_mut::{self, VisitMut};

fn json(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"'=>out.push_str("\\\""), '\\'=>out.push_str("\\\\"),
            '\n'=>out.push_str("\\n"), '\r'=>out.push_str("\\r"), '\t'=>out.push_str("\\t"),
            c if c < ' '=>out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c=>out.push(c),
        }
    }
    out.push('"');
    out
}
fn strings(values: &[String]) -> String {
    format!("[{}]", values.iter().map(|v| json(v)).collect::<Vec<_>>().join(","))
}
fn guards(attrs: &[syn::Attribute]) -> Vec<String> {
    attrs.iter().filter(|a| a.path().is_ident("cfg") || a.path().is_ident("cfg_attr") || a.path().is_ident("repr"))
        .map(|a| a.meta.to_token_stream().to_string()).collect()
}
fn field_metadata(fields: &syn::Fields) -> String {
    format!("[{}]", fields.iter().enumerate().map(|(i, f)| {
        format!("{{\"name\":{},\"type\":{},\"guards\":{}}}",
            json(&f.ident.as_ref().map_or_else(|| i.to_string(), ToString::to_string)),
            json(&f.ty.to_token_stream().to_string()), strings(&guards(&f.attrs)))
    }).collect::<Vec<_>>().join(","))
}
fn compiled_fields(fields: &syn::Fields) -> String {
    format!("[{}]", fields.iter().enumerate().map(|(i, f)| {
        format!("{{\"name\":{},\"type\":{{\"compiler_tokens\":{}}}}}",
            json(&f.ident.as_ref().map_or_else(|| i.to_string(), ToString::to_string)),
            json(&f.ty.to_token_stream().to_string()))
    }).collect::<Vec<_>>().join(","))
}

struct Instrument { source: String, next: usize, guards: Vec<String>, inline: Vec<String> }
impl Instrument {
    fn combined(&self, attrs: &[syn::Attribute]) -> Vec<String> {
        self.guards.iter().cloned().chain(guards(attrs)).collect()
    }
    fn attributes(&mut self, attrs: &mut Vec<syn::Attribute>) {
        let id = format!("{}#{}", self.source, self.next);
        self.next += 1;
        attrs.push(syn::parse_quote!(#[derive(fairing_state_projection::StateProjection)]));
        attrs.push(syn::parse_quote!(#[state_projection_id(#id)]));
    }
}
impl VisitMut for Instrument {
    fn visit_file_mut(&mut self, file: &mut syn::File) {
        let count = self.guards.len();
        self.guards.extend(guards(&file.attrs));
        visit_mut::visit_file_mut(self, file);
        self.guards.truncate(count);
    }
    fn visit_item_mut(&mut self, item: &mut syn::Item) {
        let attrs = match item {
            syn::Item::Const(v)=>&v.attrs, syn::Item::Enum(v)=>&v.attrs, syn::Item::Fn(v)=>&v.attrs,
            syn::Item::Impl(v)=>&v.attrs, syn::Item::Mod(v)=>&v.attrs, syn::Item::Static(v)=>&v.attrs,
            syn::Item::Struct(v)=>&v.attrs, syn::Item::Trait(v)=>&v.attrs, syn::Item::Type(v)=>&v.attrs,
            syn::Item::Union(v)=>&v.attrs, syn::Item::Macro(v)=>&v.attrs,
            _=>{ visit_mut::visit_item_mut(self, item); return; }
        };
        let count = self.guards.len();
        self.guards.extend(guards(attrs));
        visit_mut::visit_item_mut(self, item);
        self.guards.truncate(count);
    }
    fn visit_item_struct_mut(&mut self, item: &mut syn::ItemStruct) {
        println!("{{\"record\":\"type\",\"id\":{},\"name\":{},\"guards\":{},\"variants\":[{{\"name\":null,\"guards\":[],\"fields\":{}}}]}}",
            json(&format!("{}#{}", self.source, self.next)), json(&item.ident.to_string()),
            strings(&self.combined(&item.attrs)), field_metadata(&item.fields));
        let count = self.guards.len();
        self.guards.extend(guards(&item.attrs));
        self.attributes(&mut item.attrs);
        visit_mut::visit_item_struct_mut(self, item);
        self.guards.truncate(count);
    }
    fn visit_item_enum_mut(&mut self, item: &mut syn::ItemEnum) {
        let variants = item.variants.iter().map(|v| {
            format!("{{\"name\":{},\"guards\":{},\"fields\":{}}}",
                json(&v.ident.to_string()), strings(&guards(&v.attrs)), field_metadata(&v.fields))
        }).collect::<Vec<_>>().join(",");
        println!("{{\"record\":\"type\",\"id\":{},\"name\":{},\"guards\":{},\"variants\":[{}]}}",
            json(&format!("{}#{}", self.source, self.next)), json(&item.ident.to_string()),
            strings(&self.combined(&item.attrs)), variants);
        let count = self.guards.len();
        self.guards.extend(guards(&item.attrs));
        self.attributes(&mut item.attrs);
        visit_mut::visit_item_enum_mut(self, item);
        self.guards.truncate(count);
    }
    fn visit_item_union_mut(&mut self, item: &mut syn::ItemUnion) {
        println!("{{\"record\":\"unsupported-union\",\"source\":{},\"guards\":{}}}",
            json(&self.source), strings(&self.combined(&item.attrs)));
        visit_mut::visit_item_union_mut(self, item);
    }
    fn visit_item_mod_mut(&mut self, item: &mut syn::ItemMod) {
        if item.content.is_none() {
            let path = item.attrs.iter().find_map(|a| {
                if !a.path().is_ident("path") { return None; }
                let syn::Meta::NameValue(v) = &a.meta else { return None; };
                let syn::Expr::Lit(v) = &v.value else { return None; };
                let syn::Lit::Str(v) = &v.lit else { return None; };
                Some(v.value())
            });
            println!("{{\"record\":\"module\",\"source\":{},\"inline\":{},\"name\":{},\"path\":{},\"guards\":{}}}",
                json(&self.source), strings(&self.inline), json(&item.ident.to_string()),
                path.map_or_else(|| "null".into(), |p| json(&p)), strings(&self.combined(&item.attrs)));
        }
        let count = self.guards.len();
        self.guards.extend(guards(&item.attrs));
        self.inline.push(item.ident.to_string());
        visit_mut::visit_item_mod_mut(self, item);
        self.inline.pop();
        self.guards.truncate(count);
    }
    fn visit_item_fn_mut(&mut self, item: &mut syn::ItemFn) {
        let count = self.guards.len(); self.guards.extend(guards(&item.attrs));
        visit_mut::visit_item_fn_mut(self, item); self.guards.truncate(count);
    }
    fn visit_item_impl_mut(&mut self, item: &mut syn::ItemImpl) {
        let count = self.guards.len(); self.guards.extend(guards(&item.attrs));
        visit_mut::visit_item_impl_mut(self, item); self.guards.truncate(count);
    }
    fn visit_impl_item_fn_mut(&mut self, item: &mut syn::ImplItemFn) {
        let count = self.guards.len(); self.guards.extend(guards(&item.attrs));
        visit_mut::visit_impl_item_fn_mut(self, item); self.guards.truncate(count);
    }
    fn visit_item_const_mut(&mut self, item: &mut syn::ItemConst) {
        let count = self.guards.len(); self.guards.extend(guards(&item.attrs));
        visit_mut::visit_item_const_mut(self, item); self.guards.truncate(count);
    }
    fn visit_item_macro_mut(&mut self, item: &mut syn::ItemMacro) {
        fn data(tokens: proc_macro2::TokenStream) -> bool {
            tokens.into_iter().any(|t| match t {
                proc_macro2::TokenTree::Ident(i)=>["struct", "enum", "union", "mod"].iter().any(|s| i == *s),
                proc_macro2::TokenTree::Group(g)=>data(g.stream()), _=>false,
            })
        }
        if item.mac.path.is_ident("macro_rules") && data(item.mac.tokens.clone()) {
            println!("{{\"record\":\"unsupported-macro\",\"source\":{},\"guards\":{}}}",
                json(&self.source), strings(&self.combined(&item.attrs)));
        }
        if item.mac.path.is_ident("include") {
            println!("{{\"record\":\"unsupported-include\",\"source\":{},\"guards\":{}}}",
                json(&self.source), strings(&self.combined(&item.attrs)));
        }
        visit_mut::visit_item_macro_mut(self, item);
    }
}

struct Normalize;
impl VisitMut for Normalize {
    fn visit_expr_tuple_mut(&mut self, expr: &mut syn::ExprTuple) {
        if !expr.elems.empty_or_trailing() { expr.elems.push_punct(Default::default()); }
        visit_mut::visit_expr_tuple_mut(self, expr);
    }
    fn visit_expr_call_mut(&mut self, expr: &mut syn::ExprCall) {
        if !expr.args.empty_or_trailing() { expr.args.push_punct(Default::default()); }
        visit_mut::visit_expr_call_mut(self, expr);
    }
    fn visit_pat_struct_mut(&mut self, pat: &mut syn::PatStruct) {
        if !pat.fields.empty_or_trailing() { pat.fields.push_punct(Default::default()); }
        visit_mut::visit_pat_struct_mut(self, pat);
    }
    fn visit_pat_tuple_struct_mut(&mut self, pat: &mut syn::PatTupleStruct) {
        if !pat.elems.empty_or_trailing() { pat.elems.push_punct(Default::default()); }
        visit_mut::visit_pat_tuple_struct_mut(self, pat);
    }
    fn visit_expr_match_mut(&mut self, expr: &mut syn::ExprMatch) {
        for arm in &mut expr.arms { arm.comma = Some(Default::default()); }
        visit_mut::visit_expr_match_mut(self, expr);
    }
}
fn source_id(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find(|a| a.path().is_ident("state_projection_id"))
        .and_then(|a| a.parse_args::<syn::LitStr>().ok()).map(|s| s.value())
}
struct Inspector;
impl VisitMut for Inspector {
    fn visit_item_struct_mut(&mut self, item: &mut syn::ItemStruct) {
        if let Some(id) = source_id(&item.attrs) {
            println!("{{\"record\":\"definition\",\"id\":{},\"name\":{},\"definition\":{{\"kind\":\"struct\",\"fields\":{}}}}}",
                json(&id), json(&item.ident.to_string()), compiled_fields(&item.fields));
        }
        visit_mut::visit_item_struct_mut(self, item);
    }
    fn visit_item_enum_mut(&mut self, item: &mut syn::ItemEnum) {
        if let Some(id) = source_id(&item.attrs) {
            let variants = item.variants.iter().map(|v| {
                format!("{{\"name\":{},\"fields\":{}}}", json(&v.ident.to_string()), compiled_fields(&v.fields))
            }).collect::<Vec<_>>().join(",");
            println!("{{\"record\":\"definition\",\"id\":{},\"name\":{},\"definition\":{{\"kind\":\"enum\",\"variants\":[{}]}}}}",
                json(&id), json(&item.ident.to_string()), variants);
        }
        visit_mut::visit_item_enum_mut(self, item);
    }
    fn visit_impl_item_fn_mut(&mut self, method: &mut syn::ImplItemFn) {
        if method.sig.ident == "__fairing_state_projection" {
            let doc = method.attrs.iter().find_map(|a| {
                if !a.path().is_ident("doc") { return None; }
                let syn::Meta::NameValue(meta) = &a.meta else { return None; };
                let syn::Expr::Lit(expr) = &meta.value else { return None; };
                let syn::Lit::Str(value) = &expr.lit else { return None; };
                Some(value.value())
            });
            let Some(doc) = doc else { eprintln!("missing projection attestation"); std::process::exit(1); };
            let Some((id, source)) = doc.strip_prefix("FAIRING_STATE_PROJECTION:").and_then(|s| s.split_once('\n'))
                else { eprintln!("invalid projection attestation"); std::process::exit(1); };
            let mut expected = match syn::parse_str::<syn::ImplItemFn>(source) {
                Ok(method)=>method.block, Err(e)=>{eprintln!("{e}");std::process::exit(1);}
            };
            let mut actual = method.block.clone();
            Normalize.visit_block_mut(&mut expected);
            Normalize.visit_block_mut(&mut actual);
            if actual.to_token_stream().to_string() != expected.to_token_stream().to_string() {
                eprintln!("expanded projection differs from attestation: {id}");
                std::process::exit(1);
            }
            println!("{{\"record\":\"projection\",\"id\":{},\"body\":{}}}",
                json(id), json(&actual.to_token_stream().to_string()));
        }
        visit_mut::visit_impl_item_fn_mut(self, method);
    }
}
struct Tamper(bool);
impl VisitMut for Tamper {
    fn visit_impl_item_fn_mut(&mut self, method: &mut syn::ImplItemFn) {
        if !self.0 && method.sig.ident == "__fairing_state_projection" {
            method.block = syn::parse_quote!({ () });
            self.0 = true;
        }
        visit_mut::visit_impl_item_fn_mut(self, method);
    }
}


struct UiQueries;
impl VisitMut for UiQueries {
    fn visit_item_impl_mut(&mut self, item: &mut syn::ItemImpl) {
        let owner = match &*item.self_ty {
            syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
            _ => None,
        };
        if let Some(owner) = owner {
            for value in &item.items {
                if let syn::ImplItem::Fn(method) = value {
                    let name = method.sig.ident.to_string();
                    if (owner == "Focus" && name == "focused")
                        || (owner == "Memory" && ["focus", "focused"].contains(&name.as_str()))
                    {
                        println!("{{\"record\":\"query\",\"owner\":{},\"method\":{},\"body\":{}}}",
                            json(&owner), json(&name), json(&method.block.to_token_stream().to_string()));
                    }
                }
            }
        }
        visit_mut::visit_item_impl_mut(self, item);
    }
}


struct MutateFocusQuery(bool);
impl VisitMut for MutateFocusQuery {
    fn visit_item_impl_mut(&mut self, item: &mut syn::ItemImpl) {
        let focus = match &*item.self_ty {
            syn::Type::Path(p) => p.path.is_ident("Focus"),
            _ => false,
        };
        if focus {
            for value in &mut item.items {
                if let syn::ImplItem::Fn(method) = value {
                    if method.sig.ident == "focused" {
                        method.block = syn::parse_quote!({ None });
                        self.0 = true;
                    }
                }
            }
        }
        visit_mut::visit_item_impl_mut(self, item);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let path = args.next().ok_or("missing source path")?;
    let origin = args.next().ok_or("missing source identity")?;
    if args.next().is_some() { return Err("unexpected argument".into()); }
    let mut file = syn::parse_file(&fs::read_to_string(&path)?)?;
    if origin == "--mutate-focus-query" {
        let mut mutation = MutateFocusQuery(false);
        mutation.visit_file_mut(&mut file);
        if !mutation.0 { return Err("no native Focus::focused query to mutate".into()); }
        fs::write(Path::new(&path), quote!(#file).to_string())?;
        return Ok(());
    }
    if origin == "--inspect-egui" {
        Inspector.visit_file_mut(&mut file);
        UiQueries.visit_file_mut(&mut file);
        return Ok(());
    }
    if origin == "--inspect-expanded" {
        Inspector.visit_file_mut(&mut file);
        return Ok(());
    }
    if origin == "--tamper-expanded" {
        let mut tamper = Tamper(false);
        tamper.visit_file_mut(&mut file);
        if !tamper.0 { return Err("no compiled projection to tamper".into()); }
    } else {
        Instrument { source: origin, next: 0, guards: Vec::new(), inline: Vec::new() }.visit_file_mut(&mut file);
    }
    fs::write(Path::new(&path), quote!(#file).to_string())?;
    Ok(())
}
