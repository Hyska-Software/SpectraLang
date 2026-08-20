use crate::{
    ast::{BinaryOperator, Expression, ExpressionKind, FStringPart, LambdaParam, UnaryOperator},
    token::{Keyword, Operator, TokenKind},
};

use super::Parser;

include!("expression_precedence.rs");

include!("expression_primary.rs");

include!("expression_patterns.rs");
