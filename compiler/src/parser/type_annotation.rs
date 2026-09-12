use crate::{
    ast::{TypeAnnotation, TypeAnnotationKind},
    span::span_union,
    token::{Keyword, TokenKind},
};

use super::Parser;

impl Parser {
    pub(super) fn parse_type_annotation(&mut self) -> Result<TypeAnnotation, ()> {
        let start_span = self.current().span;

        // Dynamic trait object: dyn TraitName
        if matches!(&self.current().kind, TokenKind::Keyword(Keyword::Dyn)) {
            self.advance(); // consume 'dyn'
            let (trait_name, end_span) =
                self.consume_identifier("Expected trait name after 'dyn'")?;
            let mut auto_traits = Vec::new();
            let mut final_span = end_span;
            while self.check_symbol('+') {
                self.advance();
                let (bound, bound_span) =
                    self.consume_identifier("Expected trait bound after '+' in dyn type")?;
                auto_traits.push(bound);
                final_span = bound_span;
            }
            return Ok(TypeAnnotation {
                kind: TypeAnnotationKind::DynTrait {
                    trait_name,
                    auto_traits,
                },
                span: span_union(start_span, final_span),
            });
        }

        // Function type: func(T1, T2) returns ReturnType
        if self.check_function_keyword() {
            self.advance(); // consume 'func'
            self.consume_symbol('(', "Expected '(' after 'func' in function type")?;
            let mut params = Vec::new();
            while !self.check_symbol(')') && !self.is_at_end() {
                params.push(self.parse_type_annotation()?);
                if self.check_symbol(',') {
                    self.advance();
                } else {
                    break;
                }
            }
            self.consume_symbol(')', "Expected ')' after function parameter types")?;
            let return_type = if self.check_keyword(Keyword::Returns) {
                self.advance(); // consume 'returns'
                self.parse_type_annotation()?
            } else {
                TypeAnnotation {
                    kind: TypeAnnotationKind::Simple {
                        segments: vec!["unit".to_string()],
                    },
                    span: start_span,
                }
            };
            let end_span = self
                .tokens
                .get(self.position.saturating_sub(1))
                .map(|t| t.span)
                .unwrap_or(start_span);
            return Ok(TypeAnnotation {
                kind: TypeAnnotationKind::Function {
                    params,
                    return_type: Box::new(return_type),
                },
                span: span_union(start_span, end_span),
            });
        }

        // Check for Self type
        if self.check_keyword(crate::token::Keyword::SelfType) {
            let end_span = self.current().span;
            self.advance();
            return Ok(TypeAnnotation {
                kind: TypeAnnotationKind::Simple {
                    segments: vec!["Self".to_string()],
                },
                span: span_union(start_span, end_span),
            });
        }

        // Check if it's an array type: [int], [string], etc.
        if self.check_symbol('[') {
            self.advance(); // consume '['
            let element_type = if !self.check_symbol(']') {
                self.parse_type_annotation()?
            } else {
                TypeAnnotation {
                    kind: TypeAnnotationKind::Simple {
                        segments: vec!["unknown".to_string()],
                    },
                    span: start_span,
                }
            };
            self.consume_symbol(']', "Expected ']' after array element type")?;
            let end_span = self
                .tokens
                .get(self.position.saturating_sub(1))
                .map(|t| t.span)
                .unwrap_or(start_span);
            return Ok(TypeAnnotation {
                kind: TypeAnnotationKind::Generic {
                    name: "array".to_string(),
                    type_args: vec![element_type],
                },
                span: span_union(start_span, end_span),
            });
        }

        // Check if it's a tuple type: (int, string, ...)
        if self.check_symbol('(') {
            self.advance(); // consume '('

            let mut elements = Vec::new();

            // Empty tuple type: ()
            if self.check_symbol(')') {
                let end_span = self.current().span;
                self.advance();
                return Ok(TypeAnnotation {
                    kind: TypeAnnotationKind::Tuple { elements },
                    span: span_union(start_span, end_span),
                });
            }

            // Parse first element
            elements.push(self.parse_type_annotation()?);

            // Parse remaining elements
            while self.check_symbol(',') {
                self.advance(); // consume ','

                // Allow trailing comma
                if self.check_symbol(')') {
                    break;
                }

                elements.push(self.parse_type_annotation()?);
            }

            let end_span = self.current().span;
            self.consume_symbol(')', "Expected ')' after tuple type")?;

            return Ok(TypeAnnotation {
                kind: TypeAnnotationKind::Tuple { elements },
                span: span_union(start_span, end_span),
            });
        }

        // Parse simple type path like: Vec, std.collections.HashMap, etc.
        let (first_segment, _) = self.consume_identifier("Expected type name")?;
        let mut segments = vec![first_segment];

        // Handle qualified types (e.g., std.collections.HashMap)
        while self.check_symbol('.') {
            self.advance(); // consume '.'
            let (segment, _) = self.consume_identifier("Expected identifier after '.'")?;
            segments.push(segment);
        }

        // Consume generic type arguments if present: Option<int>, Result<int, string>
        // Store them properly in the Generic variant so the lowering can resolve
        // the monomorphized enum type (Option_int, Result_int_string, etc.).
        if self.check_symbol('<') && self.looks_like_type_args_in_annotation() {
            self.advance(); // consume '<'
            let mut type_args: Vec<TypeAnnotation> = Vec::new();
            loop {
                if self.check_symbol('>') {
                    break;
                }
                if self.is_at_end() {
                    break;
                }
                type_args.push(self.parse_type_annotation()?);
                if !self.check_symbol(',') {
                    break;
                }
                self.advance(); // consume ','
            }
            self.consume_symbol('>', "Expected '>' after type arguments")?;

            // Collapse qualified name back to single-segment name for Generic
            let name = segments.join(".");
            let end_span = self
                .tokens
                .get(self.position.saturating_sub(1))
                .map(|t| t.span)
                .unwrap_or(start_span);
            return Ok(TypeAnnotation {
                kind: TypeAnnotationKind::Generic { name, type_args },
                span: span_union(start_span, end_span),
            });
        }

        let end_span = self
            .tokens
            .get(self.position.saturating_sub(1))
            .map(|t| t.span)
            .unwrap_or(start_span);

        Ok(TypeAnnotation {
            kind: TypeAnnotationKind::Simple { segments },
            span: span_union(start_span, end_span),
        })
    }

    /// Returns `true` when the current `<` token appears to open generic type arguments
    /// rather than being a less-than comparison operator.
    ///
    /// Heuristic: scan forward to find the matching `>`, then check that the
    /// token immediately after it is one that can legally follow a type annotation
    /// (`{`, `=`, `,`, `)`, `;`, `->`, or EOF).
    fn looks_like_type_args_in_annotation(&self) -> bool {
        if !self.check_symbol('<') {
            return false;
        }
        let mut i = self.position + 1;
        let mut depth = 1usize;
        while i < self.tokens.len() {
            match &self.tokens[i].kind {
                TokenKind::Symbol('<') => depth += 1,
                TokenKind::Symbol('>') => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        // Check what follows the closing '>'
                        let after = i + 1;
                        if after >= self.tokens.len() {
                            return true; // end of input — assume type args
                        }
                        return matches!(
                            self.tokens[after].kind,
                            TokenKind::Symbol('{')      // function body / struct literal
                            | TokenKind::Symbol('=')    // let binding
                            | TokenKind::Symbol(',')    // parameter separator
                            | TokenKind::Symbol(')')    // end of parameter list / tuple
                            | TokenKind::Keyword(Keyword::Returns) // function return type
                            | TokenKind::Symbol('[') // array index after type
                            | TokenKind::Symbol('>') // nested generic closing delimiter
                        );
                    }
                }
                // These inside the brackets are not valid in a type arg list
                TokenKind::Symbol('{') => return false,
                TokenKind::Symbol(';') => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }
}
