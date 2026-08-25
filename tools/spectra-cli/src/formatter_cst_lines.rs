    fn build_lines(source: &str, tokens: &[Token]) -> Result<Vec<CstLine>, ()> {
        let mut lines = Vec::new();
        let mut current = LineAccumulator::default();
        let mut previous_end = 0usize;

        for token in tokens {
            if matches!(token.kind, TokenKind::EndOfFile) {
                break;
            }

            let start = token.span.start;
            let end = token.span.end;

            if start > end || end > source.len() {
                return Err(());
            }

            let trivia = &source[previous_end..start];
            process_trivia(trivia, &mut lines, &mut current);

            let lexeme = source[start..end].to_string();
            current.push_token(LineToken {
                kind: token.kind.clone(),
                lexeme,
            });

            previous_end = end;
        }

        let trailing = &source[previous_end..];
        process_trivia(trailing, &mut lines, &mut current);
        current.finish_into(&mut lines);

        Ok(lines)
    }

    fn process_trivia(text: &str, lines: &mut Vec<CstLine>, current: &mut LineAccumulator) {
        let mut chars = text.chars().peekable();
        let mut suppress_blank_line = false;

        while let Some(ch) = chars.next() {
            match ch {
                '\r' => {}
                '\n' => {
                    if suppress_blank_line {
                        suppress_blank_line = false;
                    } else {
                        current.consume_into_blank(lines);
                    }
                }
                '/' if matches!(chars.peek(), Some('/')) => {
                    chars.next();
                    let is_doc = matches!(chars.peek(), Some('/'));
                    let mut comment = String::from("//");
                    if is_doc {
                        comment.push('/');
                        chars.next();
                    }
                    while let Some(&next) = chars.peek() {
                        if next == '\n' {
                            break;
                        }
                        comment.push(next);
                        chars.next();
                    }
                    let trimmed = comment.trim_end().to_string();
                    if is_doc {
                        current.finish_into(lines);
                        lines.push(CstLine::DocComment(sanitize_doc_comment(&trimmed)));
                        suppress_blank_line = true;
                    } else {
                        current.push_comment(trimmed);
                        suppress_blank_line = false;
                    }
                }
                _ => {}
            }
        }
    }

    fn sanitize_doc_comment(comment: &str) -> String {
        let trimmed = comment.trim_start();
        let without_prefix = trimmed.strip_prefix("///").unwrap_or(trimmed);
        let payload = without_prefix.trim();
        if payload.is_empty() {
            "///".to_string()
        } else {
            format!("/// {}", payload)
        }
    }

    fn build_line_text(elements: &[LineElement]) -> String {
        let mut result = String::new();
        let mut pending_space = false;
        let mut prev_token: Option<&LineToken> = None;
        let mut brace_kinds = Vec::new();

        for (index, element) in elements.iter().enumerate() {
            match element {
                LineElement::Token(token) => {
                    let next_token = next_token(elements, index + 1);
                    let literal_close = if matches!(token.kind, TokenKind::Symbol('}')) {
                        brace_kinds.pop().unwrap_or(false)
                    } else {
                        false
                    };
                    apply_token(
                        &mut result,
                        &mut pending_space,
                        token,
                        prev_token,
                        next_token,
                        literal_close,
                    );
                    if matches!(token.kind, TokenKind::Symbol('{')) {
                        brace_kinds.push(is_struct_literal_open(elements, index, prev_token));
                    }
                    prev_token = Some(token);
                }
                LineElement::Comment(comment) => {
                    if !result.is_empty() && !result.ends_with(' ') {
                        result.push(' ');
                    }
                    result.push_str(comment);
                    break;
                }
            }
        }

        result.trim_end().to_string()
    }

    fn next_token(elements: &[LineElement], start: usize) -> Option<&LineToken> {
        elements
            .get(start..)?
            .iter()
            .find_map(|element| match element {
                LineElement::Token(token) => Some(token),
                _ => None,
            })
    }

    fn apply_token(
        result: &mut String,
        pending_space: &mut bool,
        token: &LineToken,
        prev: Option<&LineToken>,
        next: Option<&LineToken>,
        literal_close: bool,
    ) {
        let decision = space_before_decision(token, prev, next, literal_close);
        let mut insert_space = match decision {
            SpaceDecision::Force => true,
            SpaceDecision::Suppress => false,
            SpaceDecision::Inherit => *pending_space,
        };
        *pending_space = false;

        if (disallow_space_before(token) && !literal_close)
            || matches!(decision, SpaceDecision::Suppress)
        {
            insert_space = false;
            while result.ends_with(' ') {
                result.pop();
            }
        }

        if insert_space && !result.is_empty() && !result.ends_with(' ') {
            result.push(' ');
        }

        result.push_str(&token.lexeme);

        if should_force_space_after(token, prev, next) {
            *pending_space = true;
        }
    }

    #[derive(Clone, Copy)]
    enum SpaceDecision {
        Inherit,
        Force,
        Suppress,
    }

    fn disallow_space_before(token: &LineToken) -> bool {
        matches!(
            token.kind,
            TokenKind::Symbol(ch) if matches!(ch, ',' | ';' | ')' | ']' | '}' | '.' | ':')
        )
    }

    fn space_before_decision(
        token: &LineToken,
        prev: Option<&LineToken>,
        _next: Option<&LineToken>,
        literal_close: bool,
    ) -> SpaceDecision {
        match &token.kind {
            TokenKind::Symbol('(') => {
                if let Some(prev) = prev {
                    if let TokenKind::Keyword(keyword) = &prev.kind {
                        if keyword_requires_paren_space(keyword) {
                            return SpaceDecision::Force;
                        }
                    }
                    if matches!(
                        prev.kind,
                        TokenKind::Identifier(_)
                            | TokenKind::Number(_)
                            | TokenKind::StringLiteral(_)
                            | TokenKind::Symbol(')' | ']' | '}')
                    ) {
                        return SpaceDecision::Suppress;
                    }
                }
                SpaceDecision::Suppress
            }
            TokenKind::Symbol('[') => {
                if matches!(
                    prev.map(|token| &token.kind),
                    Some(TokenKind::Keyword(Keyword::In))
                ) {
                    SpaceDecision::Force
                } else {
                    SpaceDecision::Suppress
                }
            }
            TokenKind::Symbol('{') => {
                if prev.is_some() {
                    SpaceDecision::Force
                } else {
                    SpaceDecision::Inherit
                }
            }
            TokenKind::Symbol('-') | TokenKind::Symbol('!') => {
                if is_unary_operator(token, prev) {
                    SpaceDecision::Suppress
                } else {
                    SpaceDecision::Force
                }
            }
            TokenKind::Symbol(ch) if is_binary_symbol(*ch) => SpaceDecision::Force,
            TokenKind::Operator(_) => {
                if is_unary_operator(token, prev) {
                    SpaceDecision::Suppress
                } else {
                    SpaceDecision::Force
                }
            }
            TokenKind::Keyword(_) => {
                if prev.is_some() {
                    SpaceDecision::Force
                } else {
                    SpaceDecision::Inherit
                }
            }
            TokenKind::Identifier(_) | TokenKind::Number(_) | TokenKind::StringLiteral(_) => {
                if let Some(prev_token) = prev {
                    if requires_space_between(prev_token) {
                        SpaceDecision::Force
                    } else {
                        SpaceDecision::Inherit
                    }
                } else {
                    SpaceDecision::Inherit
                }
            }
            TokenKind::Symbol(':') => SpaceDecision::Suppress,
            TokenKind::Symbol('.') | TokenKind::Symbol(',') | TokenKind::Symbol(';') => {
                SpaceDecision::Suppress
            }
            TokenKind::Symbol(')') | TokenKind::Symbol(']') | TokenKind::Symbol('}') => {
                if literal_close {
                    SpaceDecision::Force
                } else {
                    SpaceDecision::Suppress
                }
            }
            _ => SpaceDecision::Inherit,
        }
    }

    fn is_binary_symbol(ch: char) -> bool {
        matches!(ch, '=' | '+' | '*' | '/' | '%' | '<' | '>' | '&' | '|')
    }

    fn is_struct_literal_open(
        elements: &[LineElement],
        index: usize,
        prev: Option<&LineToken>,
    ) -> bool {
        let Some(prev) = prev else {
            return false;
        };

        if matches!(prev.kind, TokenKind::Symbol(')')) {
            return false;
        }

        if matches!(
            prev.kind,
            TokenKind::Keyword(
                Keyword::If
                    | Keyword::Else
                    | Keyword::For
                    | Keyword::While
                    | Keyword::Loop
                    | Keyword::Match
                    | Keyword::Switch
                    | Keyword::Impl
                    | Keyword::Trait
                    | Keyword::Record
                    | Keyword::Struct
                    | Keyword::Enum
                    | Keyword::Func
                    | Keyword::Fn
                    | Keyword::Async
            )
        ) {
            return false;
        }

        if !matches!(prev.kind, TokenKind::Identifier(_)) {
            return false;
        }

        let preceding = elements[..index.saturating_sub(1)]
            .iter()
            .rev()
            .find_map(|element| match element {
            LineElement::Token(token) => Some(token),
            LineElement::Comment(_) => None,
        });

        matches!(
            preceding.map(|token| &token.kind),
            None
                | Some(TokenKind::Symbol('=' | ',' | ':' | '(' | '['))
                | Some(TokenKind::Keyword(Keyword::Return | Keyword::Then))
                | Some(TokenKind::Keyword(Keyword::Record | Keyword::Struct | Keyword::Enum))
        )
    }

    fn requires_space_between(token: &LineToken) -> bool {
        matches!(
            token.kind,
            TokenKind::Identifier(_)
                | TokenKind::Number(_)
                | TokenKind::StringLiteral(_)
                | TokenKind::Keyword(_)
                | TokenKind::Symbol(')' | ']' | '}')
        )
    }

    fn should_force_space_after(
        token: &LineToken,
        prev: Option<&LineToken>,
        next: Option<&LineToken>,
    ) -> bool {
        match &token.kind {
            TokenKind::Operator(_) => !is_unary_operator(token, prev),
            TokenKind::Symbol(ch) if is_binary_symbol(*ch) => !is_unary_operator(token, prev),
            TokenKind::Symbol('-') => !is_unary_operator(token, prev),
            TokenKind::Symbol(',') | TokenKind::Symbol(';') => true,
            TokenKind::Symbol(':') => {
                if let Some(next_token) = next {
                    !matches!(prev.map(|token| &token.kind), Some(TokenKind::Symbol(':')))
                        && !matches!(next_token.kind, TokenKind::Symbol(':'))
                } else {
                    false
                }
            }
            TokenKind::Keyword(keyword) => keyword_requires_space_after(keyword),
            TokenKind::Symbol('{') => true,
            _ => false,
        }
    }

    fn is_unary_operator(token: &LineToken, prev: Option<&LineToken>) -> bool {
        match &token.kind {
            TokenKind::Symbol('-') | TokenKind::Symbol('!') => previous_allows_unary(prev),
            _ => false,
        }
    }

    fn previous_allows_unary(prev: Option<&LineToken>) -> bool {
        match prev {
            None => true,
            Some(token) => match &token.kind {
                TokenKind::Symbol(
                    '(' | '[' | '{' | '=' | ',' | ':' | '+' | '-' | '*' | '/' | '%',
                ) => true,
                TokenKind::Operator(_) => true,
                TokenKind::Keyword(keyword) => keyword_expects_expression(keyword),
                _ => false,
            },
        }
    }

    fn keyword_requires_paren_space(keyword: &Keyword) -> bool {
        matches!(
            keyword,
            Keyword::If
                | Keyword::While
                | Keyword::For
                | Keyword::Switch
                | Keyword::Match
                | Keyword::Unless
        )
    }

    fn keyword_requires_space_after(keyword: &Keyword) -> bool {
        matches!(
            keyword,
            Keyword::Func
                | Keyword::Fn
                | Keyword::Let
                | Keyword::Import
                | Keyword::From
                | Keyword::If
                | Keyword::Else
                | Keyword::When
                | Keyword::Then
                | Keyword::Otherwise
                | Keyword::While
                | Keyword::For
                | Keyword::In
                | Keyword::Match
                | Keyword::Switch
                | Keyword::Unless
                | Keyword::Return
                | Keyword::Returns
                | Keyword::Record
                | Keyword::Struct
                | Keyword::Enum
                | Keyword::Impl
                | Keyword::Trait
                | Keyword::Class
                | Keyword::Public
                | Keyword::Pub
                | Keyword::Mut
                | Keyword::Async
                | Keyword::AndWord
                | Keyword::OrWord
                | Keyword::NotWord
        )
    }

    fn keyword_expects_expression(keyword: &Keyword) -> bool {
        matches!(
            keyword,
            Keyword::Return
                | Keyword::If
                | Keyword::Else
                | Keyword::While
                | Keyword::For
                | Keyword::Match
                | Keyword::Switch
                | Keyword::Unless
                | Keyword::Case
        )
    }
