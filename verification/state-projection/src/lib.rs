//! Pure borrowed field projections, compiled only in disposable verification copies.
#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Fields, GenericParam};

fn fields(fields: &Fields, omit_first: bool) -> (Tokens, Tokens) {
    let bindings: Vec<_> = (0..fields.len()).map(|i| format_ident!("__field_{i}")).collect();
    let labels: Vec<_> = fields.iter().enumerate().map(|(i, f)| {
        f.ident.as_ref().map_or_else(|| i.to_string(), ToString::to_string)
    }).collect();
    let pattern = match fields {
        Fields::Named(named) => {
            let names = named.named.iter().map(|f| &f.ident);
            quote!({ #(#names: #bindings),* })
        }
        Fields::Unnamed(_) => quote!((#(#bindings),*)),
        Fields::Unit => quote!(),
    };
    let pairs = labels.iter().zip(&bindings).skip(usize::from(omit_first))
        .map(|(label, binding)| quote!((#label, #binding)));
    (pattern, quote!((#(#pairs,)*)))
}

fn sum_path(value: Tokens, index: usize, start: usize, end: usize) -> Tokens {
    if end - start == 1 { return value; }
    let mid = start + (end - start) / 2;
    if index < mid {
        let inner = sum_path(value, index, start, mid);
        quote!(::core::result::Result::Ok(#inner))
    } else {
        let inner = sum_path(value, index, mid, end);
        quote!(::core::result::Result::Err(#inner))
    }
}

#[proc_macro_derive(StateProjection, attributes(state_projection_id))]
pub fn derive_projection(input: TokenStream) -> TokenStream {
    match projection(parse_macro_input!(input as DeriveInput)) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn projection(input: DeriveInput) -> syn::Result<Tokens> {
    // The driver clears this verification-only control during normal proofs.
    let mutation = std::env::var("FAIRING_STATE_PROJECTION_MUTATION").unwrap_or_default();
    if !["", "omit-field", "wrong-variant"].contains(&mutation.as_str()) {
        return Err(syn::Error::new_spanned(&input.ident, "unknown projection mutation"));
    }
    let id = input.attrs.iter().find(|a| a.path().is_ident("state_projection_id"))
        .ok_or_else(|| syn::Error::new_spanned(&input.ident, "missing source identity"))?
        .parse_args::<syn::LitStr>()?.value();
    let name = &input.ident;
    let body = match &input.data {
        Data::Struct(s) => {
            let (pattern, value) = fields(&s.fields, mutation == "omit-field" && name == "Shell");
            quote!({ let Self #pattern = self; #value })
        }
        Data::Enum(e) => {
            let mut arms = Vec::new();
            for (i, variant) in e.variants.iter().enumerate() {
                let ident = &variant.ident;
                let label = if mutation == "wrong-variant" && name == "Lifetime" && i == 1 {
                    e.variants[0].ident.to_string()
                } else { ident.to_string() };
                let (pattern, value) = fields(&variant.fields, false);
                let value = sum_path(quote!((#label, #value)), i, 0, e.variants.len());
                arms.push(quote!(Self::#ident #pattern => #value));
            }
            quote!({ match self { #(#arms),* } })
        }
        Data::Union(_) => return Err(syn::Error::new_spanned(name, "unions require an explicit representation")),
    };
    let lifetime = syn::Lifetime::new("'__state_projection", proc_macro2::Span::call_site());
    let parameters: Vec<Tokens> = input.generics.params.iter().map(|p| match p {
        GenericParam::Lifetime(p) => { let p = &p.lifetime; quote!(#p) }
        GenericParam::Type(p) => { let p = &p.ident; quote!(#p) }
        GenericParam::Const(p) => { let p = &p.ident; quote!(#p) }
    }).collect();
    let (implementation, arguments, constraints) = input.generics.split_for_impl();
    let method = quote! {
        #[allow(dead_code)]
        fn __fairing_state_projection<#lifetime>(&#lifetime self)
            -> impl Sized + #lifetime + use<#lifetime, #(#parameters),*> {
            (::core::marker::PhantomData::<Self>, #body)
        }
    };
    let certificate = format!("FAIRING_STATE_PROJECTION:{id}\n{method}");
    Ok(quote! {
        impl #implementation #name #arguments #constraints {
            #[doc = #certificate]
            #method
        }
    })
}
