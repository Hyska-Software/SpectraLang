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

include!("item_dispatch.rs");

include!("item_declarations.rs");

include!("item_impl.rs");

include!("item_traits.rs");

include!("item_signatures.rs");

include!("item_aliases.rs");
