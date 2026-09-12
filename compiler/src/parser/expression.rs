use crate::{
    ast::{BinaryOperator, Expression, ExpressionKind, FStringPart, LambdaParam, UnaryOperator},
    token::{Keyword, Operator, TokenKind},
};

use super::Parser;

#[path = "expression_precedence.rs"]
mod expression_precedence;

#[path = "expression_primary.rs"]
mod expression_primary;

#[path = "expression_patterns.rs"]
mod expression_patterns;
