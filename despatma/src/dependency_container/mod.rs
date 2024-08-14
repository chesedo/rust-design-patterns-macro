use std::{cell::RefCell, rc::Rc};

use indexmap::IndexMap;
use proc_macro2::TokenStream;
use proc_macro_error::emit_error;
use quote::{quote, ToTokens};
use syn::{Attribute, Block, Ident, ImplItem, ItemImpl, Signature, Token, Type};

use self::visitor::{CheckWiring, FixAsyncTree, ImplTraitButRegisteredConcrete, Visit, VisitMut};

mod visitor;

#[cfg_attr(test, derive(Eq, PartialEq, Debug))]
pub struct Container {
    attrs: Vec<Attribute>,
    self_ty: Type,
    dependencies: IndexMap<Ident, Rc<RefCell<Dependency>>>,
}

#[cfg_attr(test, derive(Eq, PartialEq, Debug))]
struct Dependency {
    attrs: Vec<Attribute>,
    sig: Signature,
    block: Block,
    is_async: bool,
    dependencies: Vec<ChildDependency>,
}

impl Dependency {
    fn create_dependency_fn(&self) -> (Ident, TokenStream) {
        let Self {
            attrs: _,
            sig,
            block,
            is_async: _,
            dependencies: _,
        } = self;
        let Signature {
            constness,
            asyncness,
            unsafety,
            abi,
            fn_token,
            ident,
            generics,
            paren_token: _,
            inputs,
            variadic,
            output,
        } = sig;

        let create_ident = Ident::new(&format!("create_{}", ident), ident.span());

        let create_fn = quote! {
            #constness #asyncness #unsafety #abi #fn_token #create_ident #generics (#inputs, #variadic) #output #block
        };

        (create_ident, create_fn)
    }

    fn dependency_fn(
        &self,
        create_ident: Ident,
        container_dependencies: &IndexMap<Ident, Rc<RefCell<Dependency>>>,
    ) -> TokenStream {
        let Dependency {
            attrs,
            sig,
            block: _,
            dependencies,
            is_async,
        } = self;
        let Signature {
            constness,
            asyncness,
            unsafety,
            abi,
            fn_token,
            ident,
            generics,
            paren_token: _,
            inputs: _,
            variadic: _,
            output,
        } = sig;

        let (create_dependencies, dependency_params): (Vec<_>, Vec<_>) = dependencies
            .iter()
            .map(|dep| {
                let ChildDependency { ident, is_ref } = dep;

                // The dependency might not exist if it was mispelt since we still try our best to generate the code
                let is_async = container_dependencies
                    .get(ident)
                    .map(|d| d.borrow().is_async)
                    .unwrap_or_default();

                let param = if *is_ref {
                    quote! { &#ident }
                } else {
                    quote! { #ident }
                };

                let awai = if is_async {
                    Some(quote! { .await })
                } else {
                    None
                };

                (
                    quote! {
                        let #ident = self.#ident()#awai;
                    },
                    param,
                )
            })
            .unzip();

        let pub_asyncness = if *is_async {
            Some(<Token![async]>::default())
        } else {
            None
        };
        let pub_await = if asyncness.is_some() {
            Some(quote! { .await })
        } else {
            None
        };

        quote! {
            #(#attrs)*
            pub #constness #pub_asyncness #unsafety #abi #fn_token #ident #generics(&self) #output {
                #(#create_dependencies)*

                self.#create_ident(#(#dependency_params),*)#pub_await
            }
        }
    }
}

#[cfg_attr(test, derive(Eq, PartialEq, Debug))]
struct ChildDependency {
    ident: Ident,
    is_ref: bool,
}

impl Container {
    pub fn from_item_impl(item_impl: ItemImpl) -> Self {
        let dependencies = item_impl
            .items
            .into_iter()
            .filter_map(|item| match item {
                ImplItem::Fn(item_fn) => {
                    let dependencies = item_fn
                        .sig
                        .inputs
                        .iter()
                        .filter_map(|input| match input {
                            syn::FnArg::Receiver(_) => None,
                            syn::FnArg::Typed(pat_type) => {
                                let ident = match pat_type.pat.as_ref() {
                                    syn::Pat::Ident(pat_ident) => pat_ident.ident.clone(),
                                    pat => {
                                        emit_error!(pat, "This argument type is not supported");
                                        return None;
                                    }
                                };
                                Some(ChildDependency {
                                    ident,
                                    is_ref: matches!(pat_type.ty.as_ref(), Type::Reference(_)),
                                })
                            }
                        })
                        .collect();

                    Some((
                        item_fn.sig.ident.clone(),
                        Rc::new(RefCell::new(Dependency {
                            attrs: item_fn.attrs,
                            is_async: item_fn.sig.asyncness.is_some(),
                            sig: item_fn.sig,
                            block: item_fn.block,
                            dependencies,
                        })),
                    ))
                }
                impl_item => {
                    emit_error!(impl_item, "This impl item is not supported");
                    None
                }
            })
            .collect();

        Self {
            attrs: item_impl.attrs,
            self_ty: item_impl.self_ty.as_ref().clone(),
            dependencies,
        }
    }

    pub fn validate(&self) {
        let mut wiring_visitor = CheckWiring::new(self.dependencies.keys().cloned().collect());

        wiring_visitor.visit_container(self);
        wiring_visitor.emit_errors();

        let mut rpit_requesting_concrete =
            ImplTraitButRegisteredConcrete::new(self.dependencies.clone());
        rpit_requesting_concrete.visit_container(self);
        rpit_requesting_concrete.emit_errors();
    }

    pub fn update(&mut self) {
        let mut async_visitor = FixAsyncTree::new(self.dependencies.clone());
        async_visitor.visit_container_mut(self);
    }
}

impl ToTokens for Container {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let self_attrs = &self.attrs;
        let self_ty = &self.self_ty;
        let self_dependencies = &self.dependencies;

        let dependencies = self_dependencies.values().map(|dep| {
            let dep_ref = dep.borrow();

            let (create_ident, create_dependency_fn) = dep_ref.create_dependency_fn();
            let dependency_fn = dep_ref.dependency_fn(create_ident, self_dependencies);

            quote! {
                #create_dependency_fn

                #dependency_fn
            }
        });

        tokens.extend(quote! {
            #(#self_attrs)*
            struct #self_ty;

            impl #self_ty {
                fn new() -> Self {
                    Self
                }

                #(#dependencies)*
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod inputs {
        use syn::parse_quote;

        use super::*;
        use pretty_assertions::assert_eq;

        #[test]
        fn simple() {
            let container = Container::from_item_impl(parse_quote!(
                impl DependencyContainer {
                    fn config(&self) -> Config {
                        Config
                    }
                }
            ));
            let expected = Container {
                attrs: vec![],
                self_ty: parse_quote!(DependencyContainer),
                dependencies: IndexMap::from_iter(vec![(
                    parse_quote!(config),
                    Rc::new(RefCell::new(Dependency {
                        attrs: vec![],
                        sig: parse_quote!(fn config(&self) -> Config),
                        block: parse_quote!({ Config }),
                        is_async: false,
                        dependencies: vec![],
                    })),
                )]),
            };

            assert_eq!(container, expected);
        }

        #[test]
        fn with_dependency() {
            let container = Container::from_item_impl(parse_quote!(
                impl Dependencies {
                    fn service(&self, config: Config) -> Service {
                        Service
                    }
                }
            ));
            let expected = Container {
                attrs: vec![],
                self_ty: parse_quote!(Dependencies),
                dependencies: IndexMap::from_iter(vec![(
                    parse_quote!(service),
                    Rc::new(RefCell::new(Dependency {
                        attrs: vec![],
                        sig: parse_quote!(fn service(&self, config: Config) -> Service),
                        block: parse_quote!({ Service }),
                        is_async: false,
                        dependencies: vec![ChildDependency {
                            ident: parse_quote!(config),
                            is_ref: false,
                        }],
                    })),
                )]),
            };

            assert_eq!(container, expected);
        }

        #[test]
        fn with_ref_dependency() {
            let container = Container::from_item_impl(parse_quote!(
                impl Dependencies {
                    fn service(&self, config: &Config) -> Service {
                        Service
                    }
                }
            ));
            let expected = Container {
                attrs: vec![],
                self_ty: parse_quote!(Dependencies),
                dependencies: IndexMap::from_iter(vec![(
                    parse_quote!(service),
                    Rc::new(RefCell::new(Dependency {
                        attrs: vec![],
                        sig: parse_quote!(fn service(&self, config: &Config) -> Service),
                        block: parse_quote!({ Service }),
                        is_async: false,
                        dependencies: vec![ChildDependency {
                            ident: parse_quote!(config),
                            is_ref: true,
                        }],
                    })),
                )]),
            };

            assert_eq!(container, expected);
        }

        #[test]
        fn with_async_dependency() {
            let container = Container::from_item_impl(parse_quote!(
                impl Dependencies {
                    fn service(&self, config: Config) -> Service {
                        Service
                    }
                    async fn config(&self) -> Config {
                        Config
                    }
                }
            ));
            let expected = Container {
                attrs: vec![],
                self_ty: parse_quote!(Dependencies),
                dependencies: IndexMap::from_iter(vec![
                    (
                        parse_quote!(service),
                        Rc::new(RefCell::new(Dependency {
                            attrs: vec![],
                            sig: parse_quote!(fn service(&self, config: Config) -> Service),
                            block: parse_quote!({ Service }),
                            is_async: false,
                            dependencies: vec![ChildDependency {
                                ident: parse_quote!(config),
                                is_ref: false,
                            }],
                        })),
                    ),
                    (
                        parse_quote!(config),
                        Rc::new(RefCell::new(Dependency {
                            attrs: vec![],
                            sig: parse_quote!(async fn config(&self) -> Config),
                            block: parse_quote!({ Config }),
                            is_async: true,
                            dependencies: vec![],
                        })),
                    ),
                ]),
            };

            assert_eq!(container, expected);
        }
    }
}
