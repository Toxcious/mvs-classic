//! `#[derive(State)]`: the rollback state's two walks of a type's fields, written out per type.
//!
//! - `Clone`: `clone` clones every field; `clone_from` calls `clone_from` field by field, so a snapshot slot that
//!   already holds a world of the same shape copies into its own `Vec`s, `String`s and maps without allocating (the
//!   standard derive's `clone_from` is `*self = source.clone()`, which allocates everything again). For an enum, the
//!   same variant on both sides copies field by field; a different variant is replaced by a clone.
//! - `StateHash`: the desync hash's walk, field by field in declaration order, an enum's variant index first.
//!
//! `#[state(skip)]` on a field: left out of `clone_from` (the destination keeps its own value, as the oracle's snapshot
//! SKIP lists leave a field out of save and restore) and out of the hash. `clone` still clones it.
//!
//! The generated code names `::mvs_sim::state::StateHash`; mvs-sim itself declares `extern crate self as mvs_sim`.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Field, Fields, Ident, Index, parse_macro_input, parse_quote};

#[proc_macro_derive(State, attributes(state))]
pub fn derive_state(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as DeriveInput);
    match expand(&mut input) {
        Ok(t) => t.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn skipped(f: &Field) -> syn::Result<bool> {
    let mut skip = false;
    for a in &f.attrs {
        if !a.path().is_ident("state") {
            continue;
        }
        a.parse_nested_meta(|m| {
            if m.path.is_ident("skip") {
                skip = true;
                Ok(())
            } else {
                Err(m.error("expected `skip`"))
            }
        })?;
    }
    Ok(skip)
}

/// One field: its member in a struct expression, the binding names a match pattern gives it, and the skip flag.
struct Member {
    member: Tokens,
    a: Ident,
    b: Ident,
    skip: bool,
}

fn members(fields: &Fields) -> syn::Result<Vec<Member>> {
    let mut out = Vec::new();
    for (i, f) in fields.iter().enumerate() {
        let member = match &f.ident {
            Some(id) => quote!(#id),
            None => {
                let ix = Index::from(i);
                quote!(#ix)
            }
        };
        out.push(Member { member, a: format_ident!("a{}", i), b: format_ident!("b{}", i), skip: skipped(f)? });
    }
    Ok(out)
}

fn expand(input: &mut DeriveInput) -> syn::Result<Tokens> {
    let hash_trait: syn::Path = parse_quote!(::mvs_sim::state::StateHash);
    let hasher: syn::Path = parse_quote!(::mvs_sim::state::StateHasher);
    for p in input.generics.type_params_mut() {
        p.bounds.push(parse_quote!(::core::clone::Clone));
        p.bounds.push(parse_quote!(#hash_trait));
    }
    let name = &input.ident;
    let (imp, ty, wh) = input.generics.split_for_impl();

    let (clone, clone_from, hash) = match &input.data {
        Data::Struct(s) => {
            let ms = members(&s.fields)?;
            let cl = ms.iter().map(|m| {
                let mem = &m.member;
                quote!(#mem: ::core::clone::Clone::clone(&self.#mem))
            });
            let cf = ms.iter().filter(|m| !m.skip).map(|m| {
                let mem = &m.member;
                quote!(::core::clone::Clone::clone_from(&mut self.#mem, &source.#mem);)
            });
            let hs = ms.iter().filter(|m| !m.skip).map(|m| {
                let mem = &m.member;
                quote!(#hash_trait::hash_state(&self.#mem, h);)
            });
            (quote!(#name { #(#cl),* }), quote!(#(#cf)*), quote!(#(#hs)*))
        }
        Data::Enum(e) => {
            let mut cl = Vec::new();
            let mut cf = Vec::new();
            let mut hs = Vec::new();
            for (vi, v) in e.variants.iter().enumerate() {
                let vn = &v.ident;
                let ms = members(&v.fields)?;
                let vi = vi as u32;
                let pat = |side: fn(&Member) -> &Ident| -> Tokens {
                    let binds = ms.iter().map(|m| {
                        let mem = &m.member;
                        let id = side(m);
                        quote!(#mem: #id)
                    });
                    quote!(#name::#vn { #(#binds),* })
                };
                let pa = pat(|m| &m.a);
                let pb = pat(|m| &m.b);
                let built = ms.iter().map(|m| {
                    let mem = &m.member;
                    let a = &m.a;
                    quote!(#mem: ::core::clone::Clone::clone(#a))
                });
                cl.push(quote!(#pa => #name::#vn { #(#built),* },));
                let copies = ms.iter().filter(|m| !m.skip).map(|m| {
                    let (a, b) = (&m.a, &m.b);
                    quote!(::core::clone::Clone::clone_from(#a, #b);)
                });
                let unused_a = ms.iter().filter(|m| m.skip).map(|m| &m.a);
                let unused_b = ms.iter().filter(|m| m.skip).map(|m| &m.b);
                cf.push(quote!((#pa, #pb) => { #(let _ = #unused_a; let _ = #unused_b;)* #(#copies)* }));
                let walks = ms.iter().map(|m| {
                    let a = &m.a;
                    if m.skip { quote!(let _ = #a;) } else { quote!(#hash_trait::hash_state(#a, h);) }
                });
                hs.push(quote!(#pa => { h.write_u32(#vi); #(#walks)* }));
            }
            let fallback = if e.variants.len() > 1 {
                quote!((this, source) => *this = ::core::clone::Clone::clone(source),)
            } else {
                quote!()
            };
            (
                quote!(match self { #(#cl)* }),
                quote!(match (self, source) { #(#cf)* #fallback }),
                quote!(match self { #(#hs)* }),
            )
        }
        Data::Union(_) => return Err(syn::Error::new(Span::call_site(), "derive(State) does not take a union")),
    };

    Ok(quote! {
        impl #imp ::core::clone::Clone for #name #ty #wh {
            #[inline]
            fn clone(&self) -> Self {
                #clone
            }
            #[inline]
            #[allow(unused_variables)]
            fn clone_from(&mut self, source: &Self) {
                #clone_from
            }
        }
        impl #imp #hash_trait for #name #ty #wh {
            #[allow(unused_variables)]
            fn hash_state(&self, h: &mut #hasher) {
                #hash
            }
        }
    })
}
