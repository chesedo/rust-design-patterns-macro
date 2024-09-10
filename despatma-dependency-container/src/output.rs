use std::{cell::RefCell, rc::Rc};

use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::{
    parse_quote,
    punctuated::Punctuated,
    token::{Async, Await, Const, Fn, Paren, Unsafe},
    Abi, AngleBracketedGenericArguments, Attribute, Block, Field, FieldValue, FieldsNamed, FnArg,
    Generics, Ident, Signature, Token, Type, Variadic,
};

use crate::processing::{self, Lifetime};

#[cfg_attr(test, derive(Eq, PartialEq, Debug))]
pub struct Container {
    attrs: Vec<Attribute>,
    self_ty: Type,
    lifetime_generic: Option<AngleBracketedGenericArguments>,
    fields: Option<FieldsNamed>,
    constructors: Punctuated<FieldValue, Token![,]>,
    scope_constructors: Punctuated<FieldValue, Token![,]>,
    dependencies: Vec<Dependency>,
}

#[cfg_attr(test, derive(Eq, PartialEq, Debug))]
pub struct Dependency {
    attrs: Vec<Attribute>,
    block: Block,
    constness: Option<Const>,
    asyncness: Option<Async>,
    unsafety: Option<Unsafe>,
    abi: Option<Abi>,
    fn_token: Fn,
    ident: Ident,
    generics: Generics,
    paren_token: Paren,
    inputs: Punctuated<FnArg, Token![,]>,
    variadic: Option<Variadic>,
    ty: Type,
    create_asyncness: Option<Async>,
    create_ident: Ident,
    is_managed: bool,
    create_ty: Type,
    dependencies: Vec<ChildDependency>,
}

#[cfg_attr(test, derive(Eq, PartialEq, Debug))]
pub struct ChildDependency {
    ident: Ident,
    awaitness: Option<Await>,
    is_ref: bool,
}

impl From<processing::Container> for Container {
    fn from(container: processing::Container) -> Self {
        let processing::Container {
            attrs,
            self_ty,
            dependencies,
        } = container;

        let singleton_and_scoped_dependencies: Vec<_> = dependencies
            .iter()
            .filter(|dep| {
                matches!(
                    dep.borrow().lifetime,
                    Lifetime::Singleton | Lifetime::Scoped
                )
            })
            .cloned()
            .collect();

        let lifetime_generic = if dependencies.iter().any(|d| {
            let d_ref = d.borrow();
            d_ref.is_boxed && matches!(d_ref.lifetime, Lifetime::Singleton | Lifetime::Scoped)
        }) {
            Some(parse_quote! { <'a> })
        } else {
            None
        };

        let fields = get_struct_fields(&singleton_and_scoped_dependencies);

        let constructors = get_struct_field_constructors(&singleton_and_scoped_dependencies);

        let scope_constructors = get_new_scope_constructors(&singleton_and_scoped_dependencies);

        let dependencies = dependencies
            .into_iter()
            .map(|d| d.borrow().clone().into())
            .collect();

        Self {
            attrs,
            self_ty,
            lifetime_generic,
            fields,
            constructors,
            scope_constructors,
            dependencies,
        }
    }
}

fn get_struct_fields(
    managed_dependencies: &[Rc<RefCell<processing::Dependency>>],
) -> Option<FieldsNamed> {
    if managed_dependencies.is_empty() {
        None
    } else {
        let fields: Vec<Field> = managed_dependencies
            .iter()
            .map(|dep| {
                let dep_ref = dep.borrow();
                let ident = &dep_ref.sig.ident;
                let ty = if dep_ref.is_boxed {
                    let ty = &dep_ref.ty;
                    &parse_quote! { std::boxed::Box<#ty + 'a> }
                } else {
                    &dep_ref.ty
                };
                let wrapper_ty = match dep_ref.lifetime {
                    Lifetime::Singleton => quote! { std::rc::Rc<std::cell::OnceCell<#ty>> },
                    Lifetime::Scoped => quote! { std::cell::OnceCell<#ty> },
                    Lifetime::Transient => {
                        unreachable!("we filtered for only singleton and scoped dependencies")
                    }
                };

                parse_quote! {
                    #ident: #wrapper_ty
                }
            })
            .collect();

        Some(parse_quote! {
            {
                #(#fields)*
            }
        })
    }
}

fn get_struct_field_constructors(
    managed_dependencies: &[Rc<RefCell<processing::Dependency>>],
) -> Punctuated<FieldValue, Token![,]> {
    if managed_dependencies.is_empty() {
        Default::default()
    } else {
        let fields: Vec<FieldValue> = managed_dependencies
            .iter()
            .map(|dep| {
                let dep_ref = dep.borrow();
                let ident = &dep_ref.sig.ident;

                parse_quote! {
                    #ident: Default::default()
                }
            })
            .collect();

        parse_quote! { #(#fields),* }
    }
}

fn get_new_scope_constructors(
    managed_dependencies: &[Rc<RefCell<processing::Dependency>>],
) -> Punctuated<FieldValue, Token![,]> {
    if managed_dependencies.is_empty() {
        Default::default()
    } else {
        let fields: Vec<FieldValue> = managed_dependencies
            .iter()
            .map(|dep| {
                let dep_ref = dep.borrow();
                let ident = &dep_ref.sig.ident;
                let init = match dep_ref.lifetime {
                    Lifetime::Singleton => quote! { self.#ident.clone() },
                    Lifetime::Scoped => quote! { Default::default() },
                    Lifetime::Transient => {
                        unreachable!("we filtered for only singleton and scoped dependencies")
                    }
                };

                parse_quote! {
                    #ident: #init
                }
            })
            .collect();

        parse_quote! { #(#fields),* }
    }
}

impl From<processing::Dependency> for Dependency {
    fn from(dependency: processing::Dependency) -> Self {
        let processing::Dependency {
            attrs,
            sig,
            block,
            is_async,
            is_boxed,
            lifetime,
            ty,
            dependencies,
        } = dependency;

        let Signature {
            constness,
            asyncness,
            unsafety,
            abi,
            fn_token,
            ident,
            generics,
            paren_token,
            inputs,
            variadic,
            output: _,
        } = sig;

        let create_asyncness = asyncness;

        let asyncness = if is_async {
            Some(<Token![async]>::default())
        } else {
            None
        };

        let ty = if is_boxed {
            parse_quote! { std::boxed::Box<#ty + '_> }
        } else {
            ty
        };

        let create_ty = ty.clone();

        let ty = if matches!(lifetime, Lifetime::Singleton | Lifetime::Scoped) {
            parse_quote!(&#ty)
        } else {
            ty
        };

        let dependencies = dependencies
            .into_iter()
            .map(ChildDependency::from)
            .collect();

        Self {
            create_ident: Ident::new(&format!("create_{}", ident), ident.span()),
            create_asyncness,
            create_ty,
            attrs,
            block,
            constness,
            asyncness,
            unsafety,
            abi,
            fn_token,
            ident,
            generics,
            paren_token,
            inputs,
            variadic,
            ty,
            is_managed: matches!(lifetime, Lifetime::Singleton | Lifetime::Scoped),
            dependencies,
        }
    }
}

impl From<processing::ChildDependency> for ChildDependency {
    fn from(child_dependency: processing::ChildDependency) -> Self {
        let dep_ref = child_dependency.inner.borrow();

        Self {
            ident: dep_ref.sig.ident.clone(),
            awaitness: if dep_ref.is_async {
                Some(<Token![await]>::default())
            } else {
                None
            },
            is_ref: child_dependency.is_ref,
        }
    }
}

impl ToTokens for Container {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let Self {
            attrs,
            self_ty,
            lifetime_generic,
            fields,
            constructors,
            scope_constructors,
            dependencies,
        } = self;

        let fields = if let Some(fields) = fields {
            quote! { #fields }
        } else {
            quote!(;)
        };

        let constructors = if constructors.is_empty() {
            quote!()
        } else {
            quote!( { #constructors } )
        };

        let scope_constructors = if scope_constructors.is_empty() {
            quote!()
        } else {
            quote!( { #scope_constructors } )
        };

        // TODO: make new pub
        tokens.extend(quote! {
            #(#attrs)*
            struct #self_ty #lifetime_generic #fields

            impl #lifetime_generic #self_ty #lifetime_generic {
                fn new() -> Self {
                    Self #constructors
                }

                pub fn new_scope(&self) -> Self {
                    Self #scope_constructors
                }

                #(#dependencies)*
            }
        });
    }
}

impl ToTokens for Dependency {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let Self {
            attrs,
            block,
            constness,
            asyncness,
            unsafety,
            abi,
            fn_token,
            ident,
            generics,
            paren_token,
            inputs,
            variadic,
            ty,
            create_asyncness,
            create_ident,
            create_ty,
            is_managed,
            dependencies,
        } = self;

        // Do the same thing `syn` does for the paren_token
        let mut params = TokenStream::new();

        paren_token.surround(&mut params, |tokens| {
            inputs.to_tokens(tokens);
            if let Some(variadic) = &variadic {
                if !inputs.empty_or_trailing() {
                    <Token![,]>::default().to_tokens(tokens);
                }
                variadic.to_tokens(tokens);
            }
        });

        let (create_dependencies, dependency_params): (Vec<_>, Vec<_>) = dependencies
            .iter()
            .map(|child_dependency| {
                let ChildDependency {
                    ident,
                    awaitness,
                    is_ref,
                } = child_dependency;

                let awaitness = awaitness.map(|awaitness| quote! { .#awaitness });

                let create_stmt = quote! {
                    let #ident = self.#ident()#awaitness;
                };

                let param = if *is_ref {
                    quote! { &#ident }
                } else {
                    quote! { #ident }
                };

                (create_stmt, param)
            })
            .unzip();

        let create_awaitness = if create_asyncness.is_some() {
            Some(quote! { .await })
        } else {
            None
        };

        let mut create_call = quote! {
            self.#create_ident(#(#dependency_params),*)#create_awaitness
        };

        if *is_managed {
            create_call = quote! {
                self.#ident.get_or_init(|| #create_call)
            };
        }

        tokens.extend(quote!(
            #constness #create_asyncness #unsafety #abi #fn_token #create_ident #generics #params -> #create_ty #block

            #(#attrs)*
            pub #constness #asyncness #unsafety #abi #fn_token #ident #generics(&self) -> #ty {
                #(#create_dependencies)*

                #create_call
            }
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use pretty_assertions::assert_eq;
    use syn::parse_quote;

    use crate::processing::{self, Lifetime};

    use super::*;

    #[test]
    fn from_processing_container() {
        let config = Rc::new(RefCell::new(processing::Dependency {
            attrs: vec![],
            sig: parse_quote! {
                async fn config(&self) -> Config
            },
            block: parse_quote!({ Config::new().await }),
            is_async: true,
            is_boxed: false,
            lifetime: Lifetime::Singleton,
            ty: parse_quote! { Config },
            dependencies: vec![],
        }));
        let container = processing::Container {
            attrs: vec![],
            self_ty: parse_quote! { Container },
            dependencies: vec![
                config.clone(),
                Rc::new(RefCell::new(processing::Dependency {
                    attrs: vec![],
                    sig: parse_quote! {
                        fn service(&self, config: &Config) -> Service
                    },
                    block: parse_quote!({ Service::new(config) }),
                    is_async: true,
                    is_boxed: false,
                    lifetime: Lifetime::Transient,
                    ty: parse_quote! { Service },
                    dependencies: vec![processing::ChildDependency {
                        inner: config,
                        is_ref: true,
                    }],
                })),
            ],
        };
        let container: super::Container = container.into();

        let expected = super::Container {
            attrs: vec![],
            self_ty: parse_quote! { Container },
            lifetime_generic: None,
            fields: Some(parse_quote! {
                {
                    config: std::rc::Rc<std::cell::OnceCell<Config>>
                }
            }),
            constructors: parse_quote!( config: Default::default() ),
            scope_constructors: parse_quote!( config: self.config.clone() ),
            dependencies: vec![
                Dependency {
                    attrs: vec![],
                    block: parse_quote!({ Config::new().await }),
                    constness: None,
                    asyncness: Some(parse_quote!(async)),
                    unsafety: None,
                    abi: None,
                    fn_token: parse_quote!(fn),
                    ident: parse_quote!(config),
                    generics: Default::default(),
                    paren_token: Default::default(),
                    inputs: parse_quote!(&self),
                    variadic: None,
                    ty: parse_quote!(&Config),
                    create_asyncness: Some(parse_quote!(async)),
                    create_ident: parse_quote!(create_config),
                    create_ty: parse_quote!(Config),
                    is_managed: true,
                    dependencies: vec![],
                },
                Dependency {
                    attrs: vec![],
                    block: parse_quote!({ Service::new(config) }),
                    constness: None,
                    asyncness: Some(parse_quote!(async)),
                    unsafety: None,
                    abi: None,
                    fn_token: parse_quote!(fn),
                    ident: parse_quote!(service),
                    generics: Default::default(),
                    paren_token: Default::default(),
                    inputs: parse_quote!(&self, config: &Config),
                    variadic: None,
                    ty: parse_quote!(Service),
                    create_asyncness: None,
                    create_ident: parse_quote!(create_service),
                    create_ty: parse_quote!(Service),
                    is_managed: false,
                    dependencies: vec![ChildDependency {
                        ident: parse_quote!(config),
                        awaitness: Some(parse_quote!(await)),
                        is_ref: true,
                    }],
                },
            ],
        };

        assert_eq!(container, expected);
    }

    #[test]
    fn from_processing_dependency() {
        let dependency = processing::Dependency {
            attrs: vec![],
            sig: parse_quote! {
                fn db(&self) -> Box<dyn DB>
            },
            block: parse_quote!({ Box::new(Sqlite::new()) }),
            is_async: false,
            is_boxed: true,
            lifetime: Lifetime::Scoped,
            ty: parse_quote! { dyn DB },
            dependencies: vec![],
        };
        let dependency: Dependency = dependency.into();

        let expected = Dependency {
            attrs: vec![],
            block: parse_quote!({ Box::new(Sqlite::new()) }),
            constness: None,
            asyncness: None,
            unsafety: None,
            abi: None,
            fn_token: parse_quote!(fn),
            ident: parse_quote!(db),
            generics: Default::default(),
            paren_token: Default::default(),
            inputs: parse_quote!(&self),
            variadic: None,
            ty: parse_quote!(&std::boxed::Box<dyn DB + '_>),
            create_asyncness: None,
            create_ident: parse_quote!(create_db),
            create_ty: parse_quote!(std::boxed::Box<dyn DB + '_>),
            is_managed: true,
            dependencies: vec![],
        };

        assert_eq!(dependency, expected);
    }
}
