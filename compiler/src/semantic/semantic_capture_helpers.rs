impl SemanticAnalyzer {
    fn collect_pattern_names_for_closure(pattern: &Pattern, names: &mut HashSet<String>) {
        match pattern {
            Pattern::Identifier(name) => {
                names.insert(name.clone());
            }
            Pattern::Tuple(items) => {
                for item in items {
                    Self::collect_pattern_names_for_closure(item, names);
                }
            }
            Pattern::Struct { fields, .. } => {
                for (_, pattern) in fields {
                    Self::collect_pattern_names_for_closure(pattern, names);
                }
            }
            Pattern::EnumVariant {
                data, struct_data, ..
            } => {
                if let Some(items) = data {
                    for item in items {
                        Self::collect_pattern_names_for_closure(item, names);
                    }
                }
                if let Some(fields) = struct_data {
                    for (_, item) in fields {
                        Self::collect_pattern_names_for_closure(item, names);
                    }
                }
            }
            Pattern::Or(patterns) => {
                for pattern in patterns {
                    Self::collect_pattern_names_for_closure(pattern, names);
                }
            }
            Pattern::Wildcard | Pattern::Literal(_) => {}
        }
    }

}
