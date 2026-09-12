use super::*;
// ── TokenizerTrainer ────────────────────────────────────────────────────────
// Real corpus-driven tokenizer training: classic BPE and WordPiece.
//
// Both trainers consume raw text, pre-tokenize into words on
// whitespace/punctuation boundaries (ASCII-lowercased to mirror the
// `MlWordpieceTokenizer` normalization used by `ml_wordpiece_encode`), and
// iteratively merge the best adjacent symbol pair until `vocab_size` is
// reached or the frequency threshold below is hit.
//
// Determinism: ties in pair selection are resolved by the lexicographically
// smallest `(left, right)` pair, so training the same corpus twice yields the
// identical vocabulary with stable ids.
//
// Zero-integration contract: the learned vocabulary is serialized through the
// SAME inline `token:id` spec format consumed by `ml_parse_wordpiece_vocab`
// (and therefore by `spectra.std.ml.tokenizer_wordpiece`, encode/decode, and
// the `tokenizer_type=wordpiece / tokenizer_version=v1` artifact path). BPE
// additionally exports a `##`-prefixed twin for every token so the existing
// longest-match WordPiece encoder can consume pure-BPE vocabs.

/// Minimum weighted frequency for a candidate pair to be merged. This is the
/// "limiar de frequência" early-stop for both trainers.
pub(crate) const ML_TOKENIZER_TRAIN_MIN_PAIR_FREQUENCY: usize = 2;

pub(crate) const ML_TOKENIZER_TRAIN_UNK: &str = "[UNK]";
pub(crate) const ML_TOKENIZER_TRAIN_CONTINUATION: &str = "##";

/// Word frequencies over maximal alphanumeric runs. Whitespace and
/// punctuation act as delimiters; ASCII letters are lowercased to match the
/// encoder normalization (`lowercase = true`, edge-punctuation trimming makes
/// punctuation-delimited words equivalent).
pub(crate) fn ml_training_word_frequencies(corpus: &str) -> HashMap<String, usize> {
    let mut freqs: HashMap<String, usize> = HashMap::new();
    let mut current = String::new();
    for ch in corpus.chars() {
        if ch.is_alphanumeric() {
            if ch.is_ascii_uppercase() {
                current.push(ch.to_ascii_lowercase());
            } else {
                current.push(ch);
            }
        } else if !current.is_empty() {
            *freqs.entry(std::mem::take(&mut current)).or_insert(0) += 1;
        }
    }
    if !current.is_empty() {
        *freqs.entry(current).or_insert(0) += 1;
    }
    freqs
}

/// Unique single-character base symbols of the corpus, sorted
/// lexicographically. Every base char enters the initial vocabulary (both as
/// the plain form and the `##` continuation form) so any position of any
/// corpus word is representable.
pub(crate) fn ml_training_base_symbols(word_freqs: &HashMap<String, usize>) -> Vec<String> {
    let mut chars: Vec<char> = word_freqs
        .keys()
        .flat_map(|word| word.chars())
        .collect::<Vec<_>>();
    chars.sort_unstable();
    chars.dedup();
    chars.into_iter().map(String::from).collect()
}

pub(crate) fn ml_training_initial_symbols(word: &str) -> Vec<String> {
    word.chars()
        .enumerate()
        .map(|(index, ch)| {
            if index == 0 {
                ch.to_string()
            } else {
                format!("{}{}", ML_TOKENIZER_TRAIN_CONTINUATION, ch)
            }
        })
        .collect()
}

/// Classic BPE over the pre-tokenized corpus. Returns the ordered vocabulary
/// (starting with `[UNK]`) or `None` for an empty corpus or a `vocab_size`
/// too small for the mandatory base inventory.
///
/// Per iteration: count adjacent-symbol pairs weighted by word frequency,
/// merge the most frequent pair (ties → lexicographically smallest pair),
/// repeat while the pair frequency is at least
/// `ML_TOKENIZER_TRAIN_MIN_PAIR_FREQUENCY` and the exported vocabulary still
/// has room. Cost: `O(merges × corpus_symbols)`.
pub(crate) fn ml_train_bpe_vocab(corpus: &str, vocab_size: usize) -> Option<Vec<String>> {
    let word_freqs = ml_training_word_frequencies(corpus);
    if word_freqs.is_empty() || vocab_size < 2 {
        return None;
    }
    let base = ml_training_base_symbols(&word_freqs);
    // Exported size = [UNK] + plain form + ## form of every plain token.
    if vocab_size < 1 + 2 * base.len() {
        return None;
    }
    let mut words: Vec<(Vec<String>, usize)> = word_freqs
        .iter()
        .map(|(word, freq)| (word.chars().map(String::from).collect(), *freq))
        .collect();
    let mut merges: Vec<String> = Vec::new();
    loop {
        // Room check: one more merge adds two exported entries.
        if 1 + 2 * (base.len() + merges.len()) + 2 > vocab_size {
            break;
        }
        let mut pairs: HashMap<(String, String), usize> = HashMap::new();
        for (symbols, freq) in &words {
            for window in symbols.windows(2) {
                *pairs
                    .entry((window[0].clone(), window[1].clone()))
                    .or_insert(0) += freq;
            }
        }
        // Maximize frequency; break ties by lexicographically smallest pair.
        let mut best: Option<(&(String, String), usize)> = None;
        for (pair, count) in &pairs {
            if *count < ML_TOKENIZER_TRAIN_MIN_PAIR_FREQUENCY {
                continue;
            }
            let better = match best {
                None => true,
                Some((best_pair, best_count)) => {
                    *count > best_count || (*count == best_count && pair < best_pair)
                }
            };
            if better {
                best = Some((pair, *count));
            }
        }
        let Some(((left, right), _)) = best else {
            break;
        };
        let merged = format!("{}{}", left, right);
        for (symbols, _) in &mut words {
            let mut index = 0;
            while index + 1 < symbols.len() {
                if symbols[index] == *left && symbols[index + 1] == *right {
                    symbols[index] = merged.clone();
                    symbols.remove(index + 1);
                }
                index += 1;
            }
        }
        merges.push(merged);
    }
    let mut vocab = vec![ML_TOKENIZER_TRAIN_UNK.to_string()];
    for token in base.iter().chain(merges.iter()) {
        vocab.push(token.clone());
        vocab.push(format!("{}{}", ML_TOKENIZER_TRAIN_CONTINUATION, token));
    }
    Some(vocab)
}

/// WordPiece training: same iterative mesh as BPE but the pair score is
/// `freq(pair) / (freq(left) * freq(right))` (implication likelihood), ties
/// broken by the lexicographically smallest pair, subject to the same
/// absolute-frequency threshold. Learned tokens keep the `##` prefix of their
/// left operand, so continuation pieces come out naturally marked.
/// Cost: `O(merges × corpus_symbols)`.
pub(crate) fn ml_train_wordpiece_vocab(corpus: &str, vocab_size: usize) -> Option<Vec<String>> {
    let word_freqs = ml_training_word_frequencies(corpus);
    if word_freqs.is_empty() || vocab_size < 2 {
        return None;
    }
    let base = ml_training_base_symbols(&word_freqs);
    if vocab_size < 1 + 2 * base.len() {
        return None;
    }
    let mut words: Vec<(Vec<String>, usize)> = word_freqs
        .iter()
        .map(|(word, freq)| (ml_training_initial_symbols(word), *freq))
        .collect();
    let mut learned: Vec<String> = Vec::new();
    loop {
        // Learned tokens already carry their own ## marking: one entry each.
        if 1 + 2 * base.len() + learned.len() + 1 > vocab_size {
            break;
        }
        let mut piece_counts: HashMap<&str, usize> = HashMap::new();
        let mut pair_counts: HashMap<(&str, &str), usize> = HashMap::new();
        for (symbols, freq) in &words {
            for symbol in symbols {
                *piece_counts.entry(symbol.as_str()).or_insert(0) += freq;
            }
            for window in symbols.windows(2) {
                *pair_counts
                    .entry((window[0].as_str(), window[1].as_str()))
                    .or_insert(0) += freq;
            }
        }
        let mut best: Option<(&(&str, &str), f64)> = None;
        for (pair, count) in &pair_counts {
            if *count < ML_TOKENIZER_TRAIN_MIN_PAIR_FREQUENCY {
                continue;
            }
            let left = piece_counts[pair.0];
            let right = piece_counts[pair.1];
            if left == 0 || right == 0 {
                continue;
            }
            let score = *count as f64 / (left * right) as f64;
            let better = match best {
                None => true,
                Some((best_pair, best_score)) => {
                    score > best_score || (score == best_score && *pair < *best_pair)
                }
            };
            if better {
                best = Some((pair, score));
            }
        }
        let Some(((left, right), _)) = best else {
            break;
        };
        let (left, right) = (left.to_string(), right.to_string());
        let right_content = right
            .strip_prefix(ML_TOKENIZER_TRAIN_CONTINUATION)
            .unwrap_or(right.as_str());
        let merged = format!("{}{}", left, right_content);
        for (symbols, _) in &mut words {
            let mut index = 0;
            while index + 1 < symbols.len() {
                if symbols[index] == left && symbols[index + 1] == right {
                    symbols[index] = merged.clone();
                    symbols.remove(index + 1);
                }
                index += 1;
            }
        }
        learned.push(merged);
    }
    let mut vocab = vec![ML_TOKENIZER_TRAIN_UNK.to_string()];
    for token in &base {
        vocab.push(token.clone());
        vocab.push(format!("{}{}", ML_TOKENIZER_TRAIN_CONTINUATION, token));
    }
    vocab.extend(learned);
    Some(vocab)
}

/// Serialize a vocabulary into the exact inline spec format understood by
/// `ml_parse_wordpiece_vocab`: one `token:id` line per token, ids dense from 0.
pub(crate) fn ml_training_vocab_spec(vocab: &[String]) -> String {
    vocab
        .iter()
        .enumerate()
        .map(|(id, token)| format!("{}:{}\n", token, id))
        .collect()
}

pub(crate) extern "C" fn std_ml_train_bpe(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(corpus) = read_spectra_string(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[1] < 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(vocab) = ml_train_bpe_vocab(&corpus, args[1] as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(tokenizer) = ml_parse_wordpiece_vocab(&ml_training_vocab_spec(&vocab)) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let handle = with_ml_registry(|registry| registry.tokenizers.insert(tokenizer));
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

pub(crate) extern "C" fn std_ml_train_wordpiece(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 2) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(corpus) = read_spectra_string(args[0]) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        if args[1] < 2 {
            return HOST_STATUS_INVALID_ARGUMENT;
        }
        let Some(vocab) = ml_train_wordpiece_vocab(&corpus, args[1] as usize) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(tokenizer) = ml_parse_wordpiece_vocab(&ml_training_vocab_spec(&vocab)) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let handle = with_ml_registry(|registry| registry.tokenizers.insert(tokenizer));
        tensor_result(ctx_ref, handle as SpectraHostValue)
    }
}

/// Export the inline `token:id` spec of ANY tokenizer handle (trained, inline,
/// or artifact-loaded), enabling save-to-artifact through the standard hosts.
pub(crate) extern "C" fn std_ml_tokenizer_vocab(ctx: *mut SpectraHostCallContext) -> i32 {
    unsafe {
        let Ok((ctx_ref, args)) = ml_args(ctx, 1) else {
            return HOST_STATUS_INVALID_ARGUMENT;
        };
        let Some(spec) = with_ml_registry(|registry| {
            registry
                .tokenizers
                .get(&(args[0] as usize))
                .map(|tokenizer| -> String {
                    let mut ids: Vec<i64> = tokenizer.id_to_token.keys().copied().collect();
                    ids.sort_unstable();
                    ids.iter()
                        .map(|id| format!("{}:{}\n", tokenizer.id_to_token[id], id))
                        .collect()
                })
        }) else {
            return HOST_STATUS_NOT_FOUND;
        };
        tensor_result(ctx_ref, alloc_spectra_string(&spec))
    }
}
