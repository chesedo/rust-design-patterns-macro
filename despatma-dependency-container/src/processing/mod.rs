use std::{cell::RefCell, rc::Rc};

use proc_macro2::Span;
use syn::{parse_quote, Attribute, Block, ImplItemFn, ReturnType, Signature, Type};

use crate::input;

use self::visitor::{
    AddWildcardLifetime, ErrorVisitorMut, ExtractAsync, ExtractBoxType, ExtractLifetime,
    ImplTraitButRegisteredConcrete, ImplTraitFields, LinkDependencies, OwningManagedDependency,
    SetHasExplicitLifetime, UnsupportedRegisteredTypes, VisitableMut, WrapBoxType,
};

mod visitor;

#[cfg_attr(test, derive(Eq, PartialEq, Debug))]
pub struct Container {
    pub(crate) attrs: Vec<Attribute>,
    pub(crate) self_ty: Type,
    pub(crate) dependencies: Vec<Rc<RefCell<Dependency>>>,
}

#[derive(Clone)]
#[cfg_attr(test, derive(Eq, PartialEq, Debug))]
pub struct Dependency {
    pub(crate) attrs: Vec<Attribute>,
    pub(crate) sig: Signature,
    pub(crate) block: Block,
    pub(crate) is_async: bool,
    pub(crate) is_boxed: bool,
    pub(crate) has_explicit_lifetime: bool,
    pub(crate) lifetime: Lifetime,
    pub(crate) ty: Type,
    pub(crate) create_ty: Type,
    pub(crate) field_ty: Option<Type>,
    pub(crate) dependencies: Vec<ChildDependency>,
}

#[derive(Clone)]
#[cfg_attr(test, derive(Eq, PartialEq, Debug))]
pub struct ChildDependency {
    pub(crate) inner: Rc<RefCell<Dependency>>,
    pub(crate) ty: Type,
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug))]
pub enum Lifetime {
    Transient,
    Scoped(Span),
    Singleton(Span),
}

impl PartialEq for Lifetime {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Transient, Self::Transient)
                | (Self::Scoped(_), Self::Scoped(_))
                | (Self::Singleton(_), Self::Singleton(_))
        )
    }
}

impl Eq for Lifetime {}

impl Lifetime {
    pub fn is_managed(&self) -> bool {
        matches!(self, Lifetime::Singleton(_) | Lifetime::Scoped(_))
    }
}

impl From<input::Container> for Container {
    fn from(input: input::Container) -> Self {
        let input::Container {
            attrs,
            self_ty,
            dependencies,
        } = input;

        let dependencies = dependencies
            .into_iter()
            .map(Dependency::from)
            .map(RefCell::from)
            .map(Rc::new)
            .collect();

        Self {
            attrs,
            self_ty,
            dependencies,
        }
    }
}

impl From<ImplItemFn> for Dependency {
    fn from(impl_item_fn: ImplItemFn) -> Self {
        let ImplItemFn {
            attrs,
            vis: _,
            defaultness: _,
            sig,
            block,
        } = impl_item_fn;

        let ty = match &sig.output {
            ReturnType::Type(_, ty) => ty.as_ref().clone(),
            ReturnType::Default => parse_quote! { () },
        };

        let create_ty = ty.clone();

        Self {
            attrs,
            sig,
            block,
            is_async: false,
            is_boxed: false,
            has_explicit_lifetime: false,
            lifetime: Lifetime::Transient,
            ty,
            create_ty,
            field_ty: None,
            dependencies: vec![],
        }
    }
}

impl Container {
    pub fn process(&mut self) {
        self.process_visitor::<ExtractLifetime>();
        self.process_visitor::<LinkDependencies>();

        // Needs lifetimes to be extracted first
        self.process_visitor::<ImplTraitFields>();

        // Needs dependencies to be linked first
        // But types should not be changed yet
        self.process_visitor::<ImplTraitButRegisteredConcrete>();

        // Needs lifetimes to be extracted and dependencies to be linked
        self.process_visitor::<OwningManagedDependency>();

        // Needs dependencies to be linked first
        self.process_visitor::<ExtractAsync>();

        self.process_visitor::<ExtractBoxType>();
        self.process_visitor::<UnsupportedRegisteredTypes>();

        // Needs lifetimes to be extracted and boxes to be extracted
        self.process_visitor::<SetHasExplicitLifetime>();

        // Needs dependencies to be linked and lifetimes to be extracted
        // But boxes should not be wrapped yet
        self.process_visitor::<AddWildcardLifetime>();

        // Needs has_explicit_lifetime to be set
        self.process_visitor::<WrapBoxType>();
    }

    fn process_visitor<V: ErrorVisitorMut>(&mut self) {
        let mut visitor = V::new();

        self.apply_mut(&mut visitor);

        visitor.emit_errors();
    }
}
