use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input,
    visit_mut::{self, VisitMut},
    Expr, ItemFn, ReturnType, Stmt, StmtMacro, Token, Type,
};

#[proc_macro_attribute]
pub fn generator(attr: TokenStream, input: TokenStream) -> TokenStream {
    let yield_type = parse_macro_input!(attr as GeneratorArgs).yield_type;
    let mut function = parse_macro_input!(input as ItemFn);

    let result = expand_generator(&mut function, yield_type);

    match result {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

struct GeneratorArgs {
    yield_type: Type,
}

impl syn::parse::Parse for GeneratorArgs {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let name: syn::Ident = input.parse()?;

        if name != "yield_type" {
            return Err(syn::Error::new(name.span(), "expected `yield_type = Type`"));
        }

        input.parse::<Token![=]>()?;

        let yield_type = input.parse()?;

        if !input.is_empty() {
            return Err(input.error("unexpected tokens"));
        }

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

    // Keep the original function arguments.
    //
    // The generated function itself does NOT receive GeneratorContext.

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

    let future_mapping = match &my_return_type {
        MyReturnType::Unit => quote! { future },
        MyReturnType::Never => quote! { future },
        MyReturnType::Type(_) => quote! { ::smog::future::map_to_return(future) },
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
    let where_clause = &function.sig.generics.where_clause;

    // Preserve asyncness? The outer function must NOT itself be async.
    //
    // The asyncness is supplied by the generated closure instead.
    let body = &function.block;

    let expanded = quote! {
        #(#attrs)*
        #vis fn #name #generics(
            #inputs
        ) -> ::smog::Generator<
            impl ::core::future::Future<Output = #return_type>,
            #yield_type
        >
        #where_clause
        {
            let future_factory = move |mut ctx: ::smog::GeneratorContext<usize>| {
                let future = async move {
                    #body
                };
                #future_mapping
            };
            ::smog::generator(future_factory)
        }
    };

    Ok(expanded)
}

struct YieldRewriter;

impl VisitMut for YieldRewriter {
    // This handles `yield_value!(expr)` when it occurs as an expression
    // rather than as a statement.
    // fn visit_expr_mut(&mut self, expr: &mut Expr) {
    //     if let Expr::Macro(ExprMacro { mac, .. }) = expr {
    //         if mac.path.is_ident("yield_value") {
    //             let tokens = mac.tokens.clone();
    //
    //             *expr = syn::parse_quote! {
    //                 ctx.yield_value(#tokens).await
    //             };
    //
    //             return;
    //         }
    //     }
    //
    //     visit_mut::visit_expr_mut(self, expr);
    // }

    fn visit_stmt_mut(&mut self, stmt: &mut Stmt) {
        // `yield_value!(expr);` is parsed as `Stmt::Macro`.
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

                // Preserve any attributes that were attached to the
                // original macro invocation.
                // if !attrs.is_empty() {
                //     expr.attrs_mut().extend(attrs.iter().cloned());
                // }

                *stmt = Stmt::Expr(expr, *semi_token);

                return;
            }
        }

        visit_mut::visit_stmt_mut(self, stmt);
    }
}
