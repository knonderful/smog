use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::quote;
use std::collections::BTreeSet;
use syn::{
    parse_macro_input,
    visit_mut::{self, VisitMut},
    Expr, FnArg, ItemFn, Lifetime, ReceiverKind, ReturnType, Stmt, StmtMacro, Token, Type,
};

#[proc_macro_attribute]
pub fn generator(attr: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as GeneratorArgs);
    let mut function = parse_macro_input!(input as ItemFn);

    let result = expand_generator(&mut function, args.yield_type);

    match result {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

struct GeneratorArgs {
    yield_type: Type,
}

impl GeneratorArgs {
    fn parse_yield_type(input: &syn::parse::ParseStream<'_>) -> syn::Result<Type> {
        input.parse()
    }
}

impl syn::parse::Parse for GeneratorArgs {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let full_span = input.span();
        let mut yield_type = None;

        while !input.is_empty() {
            let name = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;

            match name.to_string().as_str() {
                "yield_type" => {
                    if yield_type.is_some() {
                        return Err(syn::Error::new(name.span(), "duplicate `yield_type`"));
                    }
                    yield_type = Some(Self::parse_yield_type(&input)?)
                }
                other => return Err(syn::Error::new(name.span(), format!("invalid attribute `{other}`"))),
            }

            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }

        let Some(yield_type) = yield_type else {
            return Err(syn::Error::new(full_span, "missing attribute `yield_type`"));
        };

        Ok(Self { yield_type })
    }
}

enum MyReturnType {
    Unit,
    Never,
    Type(Box<Type>),
}

fn expand_generator(function: &mut ItemFn, yield_type: Type) -> syn::Result<proc_macro2::TokenStream> {
    let body = &mut function.block;

    // Rewrite:
    //     yield_value!(expression);
    // into:
    //     ctx.yield_value(expression).await;
    let mut rewriter = YieldRewriter;
    rewriter.visit_block_mut(body);

    let my_return_type = match &function.sig.output {
        ReturnType::Default => MyReturnType::Unit,
        ReturnType::Type(_, ty) => match &**ty {
            Type::Never(_) => MyReturnType::Never,
            Type::Tuple(tuple) => {
                if tuple.elems.is_empty() {
                    MyReturnType::Unit
                } else {
                    MyReturnType::Type(ty.clone())
                }
            }
            _ => MyReturnType::Type(ty.clone()),
        },
    };

    let return_type = match &my_return_type {
        MyReturnType::Unit => quote! { () },
        MyReturnType::Never => quote! { ::smog::Never },
        MyReturnType::Type(ret) => quote! { ::smog::Return<#ret> },
    };

    let future_mapper = match &my_return_type {
        MyReturnType::Unit => quote! { ::std::convert::identity },
        MyReturnType::Never => quote! { ::smog::future::map_to_never },
        MyReturnType::Type(_) => quote! { ::smog::future::map_to_return },
    };

    // Return type mappings:
    // - `()` or nothing: Generator<impl Future<Output = ()>, Y>
    //   - i.e. a generator doesn't have a final result, but can complete
    // - generic `T`: Generator<impl Future<Output = Return<T>, Y>
    //   - i.e. a generator that has a final result
    // - `!`: Generator<impl Future<Output = Never, Y>
    //   - i.e. an infinite generator

    let attrs = &function.attrs;
    let vis = &function.vis;
    let name = &function.sig.ident;
    let generics = &function.sig.generics;
    let inputs = &function.sig.inputs;

    // Collect all the input lifetimes...
    let mut lifetime_idents: BTreeSet<Ident> = BTreeSet::new();
    let mut add_lifetime = |lifetime: Option<Lifetime>| {
        lifetime_idents.insert(
            lifetime
                .map(|lt| lt.ident)
                .unwrap_or_else(|| Ident::new("_", Span::call_site())),
        );
    };

    for arg in inputs {
        match arg {
            FnArg::Receiver(recv) => match &recv.kind {
                ReceiverKind::Reference(_, lifetime, _) => add_lifetime(lifetime.clone()),
                _ => {}
            },
            FnArg::Typed(pat_type) => match pat_type.ty.as_ref() {
                Type::Reference(reference) => add_lifetime(reference.lifetime.clone()),
                _ => {}
            },
        }
    }

    // ... and generate a `+ use < '_, 'a, >` to append to the future
    let mut use_lifetimes = quote! {};
    for ident in lifetime_idents {
        let lifetime = Lifetime {
            apostrophe: Span::call_site(),
            ident,
        };
        use_lifetimes = quote! { #use_lifetimes #lifetime , };
    }

    let where_clause = &function.sig.generics.where_clause;

    let body = &function.block;

    let expanded = quote! {
        #(#attrs)*
        #vis fn #name #generics(
            #inputs
        ) -> ::smog::Generator<
            impl ::core::future::Future<Output = #return_type> + use< #use_lifetimes >,
            #yield_type
        >
        #where_clause
        {
            let future_factory = async move |mut ctx: ::smog::GeneratorContext<#yield_type>| {
                #body
            };
            ::smog::generator_mapped(future_factory, #future_mapper)
        }
    };

    Ok(expanded)
}

struct YieldRewriter;

impl VisitMut for YieldRewriter {
    fn visit_stmt_mut(&mut self, stmt: &mut Stmt) {
        if let Stmt::Macro(StmtMacro {
            attrs: _,
            mac,
            semi_token,
        }) = stmt
        {
            if mac.path.is_ident("yield_value") {
                let tokens = mac.tokens.clone();

                let expr: Expr = syn::parse_quote! {
                    ctx.yield_value(#tokens).await
                };

                *stmt = Stmt::Expr(expr, *semi_token);

                return;
            }
        }

        visit_mut::visit_stmt_mut(self, stmt);
    }
}
