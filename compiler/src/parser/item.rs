use crate::{
    ast::{
        Attribute, AttributeArgument, Block, ConstDecl, Function, FunctionParam, ImplBlock, Item,
        Method, Parameter, StaticDecl, TraitDeclaration, TraitImpl, TraitMethod, TypeAlias,
        TypeAnnotation, TypeParameter, Visibility,
    },
    span::{span_union, Span},
    token::{Keyword, TokenKind},
};
use std::collections::{HashMap, HashSet};

use super::{ParameterSignature, Parser, TraitMethodSignature, TypePattern};

#[path = "item_dispatch.rs"]
mod item_dispatch;

#[path = "item_declarations.rs"]
mod item_declarations;

#[path = "item_impl.rs"]
mod item_impl;

#[path = "item_traits.rs"]
mod item_traits;

#[path = "item_signatures.rs"]
mod item_signatures;

#[path = "item_aliases.rs"]
mod item_aliases;
