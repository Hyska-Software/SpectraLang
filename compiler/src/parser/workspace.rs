use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{
    ast::Module,
    error::{LexError, ParseError},
    lexer::Lexer,
};

use super::Parser;

#[derive(Debug)]
struct CachedModule {
    hash: u64,
    outcome: CachedOutcome,
}

#[derive(Debug, Clone)]
enum CachedOutcome {
    /// The cached module is wrapped in `Arc` so cache hits never need a deep clone.
    Success(Arc<Module>),
    Lexical(Vec<LexError>),
    Parse(Vec<ParseError>),
}

#[derive(Debug, Clone)]
pub struct ModuleParseSuccess {
    pub module: Module,
    pub reused: bool,
    pub lexing_duration: Duration,
    pub parsing_duration: Duration,
}

#[derive(Debug, Clone)]
pub enum ModuleParseError {
    Lexical(Vec<LexError>),
    Parse(Vec<ParseError>),
}

pub struct ModuleLoader {
    cache: HashMap<String, CachedModule>,
}

impl Default for ModuleLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl ModuleLoader {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn invalidate(&mut self, module_id: &str) {
        self.cache.remove(module_id);
    }

    pub fn parse_module(
        &mut self,
        module_id: &str,
        source: &str,
    ) -> Result<ModuleParseSuccess, ModuleParseError> {
        let hash = Self::compute_hash(source);

        if let Some(entry) = self.cache.get(module_id) {
            if entry.hash == hash {
                return match &entry.outcome {
                    CachedOutcome::Success(arc) => Ok(ModuleParseSuccess {
                        // Arc::clone is O(1); callers that need ownership get a
                        // deep clone here, but the cache itself never has to.
                        module: (**arc).clone(),
                        reused: true,
                        lexing_duration: Duration::default(),
                        parsing_duration: Duration::default(),
                    }),
                    CachedOutcome::Lexical(errors) => {
                        Err(ModuleParseError::Lexical(errors.clone()))
                    }
                    CachedOutcome::Parse(errors) => Err(ModuleParseError::Parse(errors.clone())),
                };
            }
        }

        let lex_start = Instant::now();
        let tokens = match Lexer::new(source).tokenize() {
            Ok(tokens) => tokens,
            Err(errors) => {
                let cloned = errors.clone();
                self.cache.insert(
                    module_id.to_string(),
                    CachedModule {
                        hash,
                        outcome: CachedOutcome::Lexical(cloned),
                    },
                );
                return Err(ModuleParseError::Lexical(errors));
            }
        };
        let lex_duration = lex_start.elapsed();

        let parse_start = Instant::now();
        let result = Parser::new(tokens).parse();
        let parse_duration = parse_start.elapsed();

        match result {
            Ok(module) => {
                // Wrap in Arc so future cache hits are cheap (no deep clone needed
                // until the caller actually needs an owned value).
                let arc = Arc::new(module);
                self.cache.insert(
                    module_id.to_string(),
                    CachedModule {
                        hash,
                        outcome: CachedOutcome::Success(Arc::clone(&arc)),
                    },
                );

                Ok(ModuleParseSuccess {
                    module: (*arc).clone(),
                    reused: false,
                    lexing_duration: lex_duration,
                    parsing_duration: parse_duration,
                })
            }
            Err(errors) => {
                let cloned = errors.clone();
                self.cache.insert(
                    module_id.to_string(),
                    CachedModule {
                        hash,
                        outcome: CachedOutcome::Parse(cloned),
                    },
                );
                Err(ModuleParseError::Parse(errors))
            }
        }
    }

    fn compute_hash(source: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caches_successful_parse() {
        let mut loader = ModuleLoader::new();
        let source = "\n            module demo\n            func main() {}\n        ";

        let first = loader
            .parse_module("demo", source)
            .expect("first parse should succeed");
        assert!(!first.reused);

        let second = loader
            .parse_module("demo", source)
            .expect("second parse should reuse cache");
        assert!(second.reused);
    }

    #[test]
    fn reparses_when_source_changes() {
        let mut loader = ModuleLoader::new();

        let original = "\n            module demo\n            func main() {}\n        ";

        loader
            .parse_module("demo", original)
            .expect("initial parse should succeed");

        let modified = "\n            module demo\n            func main() { let x = 1 }\n        ";

        let result = loader
            .parse_module("demo", modified)
            .expect("modified source should reparse successfully");
        assert!(!result.reused, "modified source must trigger reparse");
    }

}
