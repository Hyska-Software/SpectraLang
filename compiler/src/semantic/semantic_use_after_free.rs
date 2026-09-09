use super::*;

// Flow-sensitive, function-local use-after-free tracking (E034).
//
// Scope and contract (documented design):
// - Tracking is LOCAL to one function body (no interprocedural analysis).
// - A binding enters the frame when declared (`let`, pattern binding, or
//   parameter) and is marked `Freed` when passed as the handle argument of a
//   resource-release call (`free`/`*_free*` family) or covered by a
//   `free_all`-family call.
// - Any subsequent read of a Freed binding (identifier use in any expression
//   position: argument, receiver, condition, return, index...) emits coded
//   E034 "use after free of 'x'" with a hint pointing at the free site line.
// - Reassignment (`x = <new value>`) or rebinding (`let x = ...`) revives the
//   binding to Alive.
// - Branches merge conservatively to avoid false positives: after an
//   if/elif/else chain with an `else` arm, a binding stays Freed only when it
//   was still Freed at the end of EVERY arm; without `else`, frees inside the
//   conditional arms never leak out (the branch may not have executed).
// - Loop bodies and match/switch arms are analyzed with an isolated snapshot:
//   frees inside them are caught within the same linear pass, but never leak
//   past the construct (a loop may run zero times; switch/match arms may not
//   cover every path).
// - Free-all calls mark every *live* tracked binding whose type belongs to the
//   arena family of the call: `tensor.free_all()`/`free_all()` -> Tensor,
//   `list_free_all()` -> List, `map_free_all()` -> Map, `set_free_all()` ->
//   Set. Handles of unrelated families (e.g. ml int-handles) are unaffected,
//   so handles obtained after another variable's free_all stay Alive.
//
// Sanctioned exception (release-state introspection): `value_kind` deliberately
// accepts released handles so programs can observe release state; reading a
// freed handle through it does NOT emit E034.

/// Resource-release functions whose first argument is the released handle.
const UAF_DIRECT_FREE_FUNCTIONS: &[&str] = &[
    "free",
    "tensor_free",
    "value_free",
    "notification_free",
    "artifact_free",
    "builder_free",
    "iterator_free",
    "list_free",
    "map_free",
    "set_free",
];

/// Arena-wide release calls mapped to the handle family they release.
fn uaf_free_all_family(name: &str) -> Option<&'static str> {
    match name {
        "free_all" | "tensor_free_all" => Some("Tensor"),
        "list_free_all" => Some("List"),
        "map_free_all" => Some("Map"),
        "set_free_all" => Some("Set"),
        _ => None,
    }
}

/// Release-state introspection readers that deliberately accept released handles.
const UAF_INTROSPECTION_READERS: &[&str] = &["value_kind"];


#[derive(Debug, Clone, Default)]
pub(crate) struct UafFrame {
    /// Bindings currently known to be released: name -> line of the free.
    /// Absence means Alive.
    freed: HashMap<String, usize>,
    /// Tracked bindings with their current types (for free_all families).
    bindings: HashMap<String, Type>,
}

impl UafFrame {
    fn type_in_family(ty: &Type, family: &str) -> bool {
        match ty {
            Type::Tensor { .. } => family == "Tensor",
            Type::Struct { name } | Type::Applied { name, .. } => {
                (family == "List"
                    && (name == "List" || name.starts_with("List_")))
                    || (family == "Map" && (name == "Map" || name.starts_with("Map_")))
                    || (family == "Set" && (name == "Set" || name.starts_with("Set_")))
                    || (family == "Iterator"
                        && (name == "Iterator" || name.starts_with("Iterator_")))
            }
            _ => false,
        }
    }

    /// Conservative branch merge: a binding stays Freed after the construct
    /// only when it was Freed at the end of every analyzed branch arm.
    pub(crate) fn merge(base: &UafFrame, arms: &[UafFrame]) -> UafFrame {
        let mut merged = UafFrame {
            freed: HashMap::new(),
            bindings: base.bindings.clone(),
        };
        for arm in arms {
            for (name, ty) in &arm.bindings {
                merged.bindings.insert(name.clone(), ty.clone());
            }
        }
        if let Some(first) = arms.first() {
            for (name, line) in &first.freed {
                if arms[1..].iter().all(|arm| arm.freed.contains_key(name)) {
                    merged.freed.insert(name.clone(), *line);
                }
            }
        }
        merged
    }
}

impl SemanticAnalyzer {
    // -- frame lifecycle ----------------------------------------------------

    pub(crate) fn uaf_enter_function(&mut self) -> UafFrame {
        let previous = self.uaf_frame.take().unwrap_or_default();
        self.uaf_frame = Some(UafFrame::default());
        previous
    }

    pub(crate) fn uaf_snapshot(&self) -> UafFrame {
        self.uaf_frame.clone().unwrap_or_default()
    }

    pub(crate) fn uaf_restore(&mut self, saved: UafFrame) {
        self.uaf_frame = Some(saved);
    }

    pub(crate) fn uaf_merge_branches(&mut self, base: &UafFrame, arms: Vec<UafFrame>) {
        if let Some(frame) = self.uaf_frame.as_mut() {
            *frame = UafFrame::merge(base, &arms);
        }
    }

    // -- state transitions --------------------------------------------------

    /// Declare/rebind `name`: revives it to Alive and records its type.
    pub(crate) fn uaf_on_bind(&mut self, name: &str, ty: &Type) {
        if let Some(frame) = self.uaf_frame.as_mut() {
            frame.freed.remove(name);
            frame.bindings.insert(name.to_string(), ty.clone());
        }
    }

    /// Mark `name` as released; the span identifies the freeing call site.
    fn uaf_mark_freed(&mut self, name: &str, span: Span) {
        if let Some(frame) = self.uaf_frame.as_mut() {
            frame
                .freed
                .insert(name.to_string(), span.start_location.line);
        }
    }

    /// Mark every live tracked binding whose type belongs to `family`.
    fn uaf_mark_family_freed(&mut self, family: &str, span: Span) {
        let line = span.start_location.line;
        if let Some(frame) = self.uaf_frame.as_mut() {
            let released: Vec<String> = frame
                .bindings
                .iter()
                .filter(|(name, ty)| {
                    UafFrame::type_in_family(ty, family) && !frame.freed.contains_key(*name)
                })
                .map(|(name, _)| name.clone())
                .collect();
            for name in released {
                frame.freed.insert(name, line);
            }
        }
    }

    /// Report E034 when reading a released binding.
    pub(crate) fn uaf_check_use(&mut self, name: &str, span: Span) {
        if self.uaf_suspend_use_checks > 0 {
            return;
        }
        let free_line = match self.uaf_frame.as_ref().and_then(|f| f.freed.get(name)) {
            Some(line) => *line,
            None => return,
        };
        // Only report while the binding is actually visible; stale entries from
        // sibling scopes must not surface here.
        if self.lookup_symbol(name).is_none() {
            return;
        }
        let message = format!("use after free of '{}'", name);
        let hint = format!(
            "'{}' was released by a resource-free call at line {}; reassign or recreate the handle before using it again",
            name, free_line
        );
        self.error_coded_with_hint("E034", message, span, hint);
    }

    // -- call classification ------------------------------------------------

    /// Last path segment of a callee (handles bare and dotted namespace calls).
    fn uaf_callee_last_segment(callee: &Expression) -> Option<String> {
        namespace_path(callee)
            .map(|path| {
                path.rsplit('.')
                    .next()
                    .unwrap_or(&path)
                    .to_string()
            })
    }

    pub(crate) fn uaf_callee_is_introspection_reader(&self, callee: &Expression) -> bool {
        Self::uaf_callee_last_segment(callee)
            .map(|name| UAF_INTROSPECTION_READERS.contains(&name.as_str()))
            .unwrap_or(false)
    }

    pub(crate) fn uaf_after_call_analysis(&mut self, callee: &Expression, arguments: &[Expression], span: Span) {
        let Some(name) = Self::uaf_callee_last_segment(callee) else {
            return;
        };
        if UAF_DIRECT_FREE_FUNCTIONS.contains(&name.as_str()) {
            if arguments.len() == 1 {
                if let ExpressionKind::Identifier(handle) = &arguments[0].kind {
                    self.uaf_mark_freed(handle, arguments[0].span);
                }
            }
            return;
        }
        if let Some(family) = uaf_free_all_family(&name) {
            if arguments.is_empty() {
                self.uaf_mark_family_freed(family, span);
            }
        }
    }

    /// Same classification for method-form releases (`tensor.free(x)` /
    /// `tensor.free_all()`), where the receiver is a module namespace.
    pub(crate) fn uaf_after_method_call_analysis(
        &mut self,
        method_name: &str,
        arguments: &[Expression],
        span: Span,
    ) {
        match method_name {
            "free" if arguments.len() == 1 => {
                if let ExpressionKind::Identifier(handle) = &arguments[0].kind {
                    self.uaf_mark_freed(handle, arguments[0].span);
                }
            }
            "free_all" if arguments.is_empty() => {
                // std.tensor.free_all() releases the tensor arena; collection
                // arenas expose their own `*_free_all` plain functions.
                self.uaf_mark_family_freed("Tensor", span);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod use_after_free_tests {
    use crate::{CompilationOptions, CompilationPipeline, CompilerError};

    pub(crate) fn compile(source: &str) -> Result<(), Vec<CompilerError>> {
        let mut pipeline = CompilationPipeline::new(CompilationOptions::default());
        pipeline.compile(source, "use_after_free.spectra").map(|_| ())
    }

    #[test]
    fn e034_reports_read_after_free() {
        let source = r#"
            module use_after_free_simple

            import std.tensor as tensor

            public func main() returns int {
                let handle = tensor.arange(1, 4, 1)
                tensor.free(handle)
                return tensor.rank(handle)
            }
        "#;
        let errors = compile(source).expect_err("use after free must be rejected");
        assert!(
            errors.iter().any(|error| matches!(
                error,
                CompilerError::Semantic(semantic)
                    if semantic.code.as_deref() == Some("E034")
                        && semantic.message.contains("use after free of 'handle'")
            )),
            "expected coded E034: {errors:?}"
        );
        assert!(
            !errors.iter().any(|error| matches!(
                error,
                CompilerError::Semantic(semantic) if semantic.code.as_deref() == Some("E001")
            )),
            "no unrelated diagnostics expected: {errors:?}"
        );
    }

    #[test]
    fn free_then_reassign_compiles() {
        let source = r#"
            module free_then_reassign_ok

            import std.tensor as tensor

            public func main() returns int {
                let handle = tensor.arange(1, 4, 1)
                tensor.free(handle)
                handle = tensor.arange(1, 4, 1)
                let rank = tensor.rank(handle)
                tensor.free(handle)
                tensor.free_all()
                return rank
            }
        "#;
        compile(source).expect("reassignment after free must revive the binding");
    }

    #[test]
    fn conditional_frees_merge_conservatively() {
        let source = r#"
            module conditional_merge

            import std.tensor as tensor

            public func main() returns int {
                let flag = true
                let handle = tensor.arange(1, 4, 1)
                if flag {
                    tensor.free(handle)
                    handle = tensor.arange(1, 4, 1)
                } else {
                    tensor.free(handle)
                    handle = tensor.arange(1, 4, 1)
                }
                tensor.free(handle)

                let other = tensor.arange(1, 4, 1)
                if flag {
                    tensor.free(other)
                }
                other = tensor.arange(1, 4, 1)
                tensor.free(other)
                tensor.free_all()
                return 0
            }
        "#;
        compile(source).expect("branch merge must not produce false positives");
    }

    #[test]
    fn option_reads_on_live_handles_compile_clean() {
        let source = r#"
            module use_after_free_live_reads

            import std.collections as collections
            import std.option as option

            public func main() returns int {
                let first = collections.list_new()
                collections.list_push(first, 10)
                let maybe_first = collections.list_get(first, 0)
                if option.is_none(maybe_first) {
                    return 1
                }
                collections.list_free(first)
                return 0
            }
        "#;
        compile(source).expect("absence-safe reads on live handles must compile");
    }
}
