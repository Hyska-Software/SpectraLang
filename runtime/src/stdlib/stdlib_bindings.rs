use super::*;
pub(crate) const MATH_ABS: &str = "spectra.std.math.abs";
pub(crate) const MATH_MIN: &str = "spectra.std.math.min";
pub(crate) const MATH_MAX: &str = "spectra.std.math.max";
pub(crate) const MATH_CLAMP: &str = "spectra.std.math.clamp";
pub(crate) const MATH_SQRT_F: &str = "spectra.std.math.sqrt_f";
pub(crate) const MATH_POW_F: &str = "spectra.std.math.pow_f";
pub(crate) const MATH_FLOOR_F: &str = "spectra.std.math.floor_f";
pub(crate) const MATH_CEIL_F: &str = "spectra.std.math.ceil_f";
pub(crate) const MATH_ROUND_F: &str = "spectra.std.math.round_f";

pub(crate) fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}


#[derive(Clone, Debug)]
pub(crate) enum CollectionKey {
    Scalar(SpectraHostValue),
    String {
        value: String,
        raw: SpectraHostValue,
    },
}

impl PartialEq for CollectionKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Scalar(left), Self::Scalar(right)) => left == right,
            (Self::String { value: left, .. }, Self::String { value: right, .. }) => left == right,
            _ => false,
        }
    }
}

impl Eq for CollectionKey {}

impl Hash for CollectionKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Scalar(value) => {
                0_u8.hash(state);
                value.hash(state);
            }
            Self::String { value, .. } => {
                1_u8.hash(state);
                value.hash(state);
            }
        }
    }
}

impl CollectionKey {
    pub(crate) fn raw_value(&self) -> SpectraHostValue {
        match self {
            Self::Scalar(value) => *value,
            // String keys retain their original pointer for iteration. Their
            // equality/hash identity is the owned text above.
            Self::String { raw, .. } => *raw,
        }
    }
}

pub(crate) fn collection_key(value: SpectraHostValue) -> CollectionKey {
    match unsafe { try_read_packed_string(value) } {
        Some(text) => CollectionKey::String { value: text, raw: value },
        None => CollectionKey::Scalar(value),
    }
}

pub(crate) fn collection_values_equal(left: SpectraHostValue, right: SpectraHostValue) -> bool {
    collection_key(left) == collection_key(right)
}

pub(crate) const IO_PRINT: &str = "spectra.std.io.print";
pub(crate) const IO_PRINTLN: &str = "spectra.std.io.println";
pub(crate) const IO_FLUSH: &str = "spectra.std.io.flush";
pub(crate) const IO_EPRINT: &str = "spectra.std.io.eprint";
pub(crate) const IO_EPRINTLN: &str = "spectra.std.io.eprintln";
pub(crate) const IO_READ_LINE: &str = "spectra.std.io.read_line";

// ── std.math (novos) ─────────────────────────────────────────────────────────
pub(crate) const MATH_SIN_F: &str = "spectra.std.math.sin_f";
pub(crate) const MATH_COS_F: &str = "spectra.std.math.cos_f";
pub(crate) const MATH_TAN_F: &str = "spectra.std.math.tan_f";
pub(crate) const MATH_LOG_F: &str = "spectra.std.math.log_f";
pub(crate) const MATH_LOG2_F: &str = "spectra.std.math.log2_f";
pub(crate) const MATH_LOG10_F: &str = "spectra.std.math.log10_f";
pub(crate) const MATH_ATAN2_F: &str = "spectra.std.math.atan2_f";
pub(crate) const MATH_PI: &str = "spectra.std.math.pi";
pub(crate) const MATH_E_CONST: &str = "spectra.std.math.e_const";

// ── std.string ──────────────────────────────────────────────────────────────
pub(crate) const STR_LEN: &str = "spectra.std.string.len";
pub(crate) const STR_CONTAINS: &str = "spectra.std.string.contains";
pub(crate) const STR_TO_UPPER: &str = "spectra.std.string.to_upper";
pub(crate) const STR_TO_LOWER: &str = "spectra.std.string.to_lower";
pub(crate) const STR_TRIM: &str = "spectra.std.string.trim";
pub(crate) const STR_STARTS_WITH: &str = "spectra.std.string.starts_with";
pub(crate) const STR_ENDS_WITH: &str = "spectra.std.string.ends_with";
pub(crate) const STR_EQ: &str = "spectra.std.string.eq";
pub(crate) const STR_CONCAT: &str = "spectra.std.string.concat";
pub(crate) const STR_REPEAT: &str = "spectra.std.string.repeat_str";
pub(crate) const STR_BUILDER_NEW: &str = "spectra.std.string.builder_new";
pub(crate) const STR_BUILDER_PUSH: &str = "spectra.std.string.builder_push";
pub(crate) const STR_BUILDER_LEN: &str = "spectra.std.string.builder_len";
pub(crate) const STR_BUILDER_FINISH: &str = "spectra.std.string.builder_finish";
pub(crate) const STR_BUILDER_FREE: &str = "spectra.std.string.builder_free";
pub(crate) const STR_CHAR_AT: &str = "spectra.std.string.char_at";
pub(crate) const STR_SUBSTRING: &str = "spectra.std.string.substring";
pub(crate) const STR_REPLACE: &str = "spectra.std.string.replace";
pub(crate) const STR_INDEX_OF: &str = "spectra.std.string.index_of";
pub(crate) const STR_SPLIT_FIRST: &str = "spectra.std.string.split_first";
pub(crate) const STR_SPLIT_LAST: &str = "spectra.std.string.split_last";
pub(crate) const STR_IS_EMPTY: &str = "spectra.std.string.is_empty";
pub(crate) const STR_COUNT: &str = "spectra.std.string.count_occurrences";

// ── std.convert ─────────────────────────────────────────────────────────────
pub(crate) const CONV_INT_TO_STRING: &str = "spectra.std.convert.int_to_string";
pub(crate) const CONV_FLOAT_TO_STRING: &str = "spectra.std.convert.float_to_string";
pub(crate) const CONV_BOOL_TO_STRING: &str = "spectra.std.convert.bool_to_string";
pub(crate) const CONV_STRING_TO_INT: &str = "spectra.std.convert.string_to_int";
pub(crate) const CONV_STRING_TO_FLOAT: &str = "spectra.std.convert.string_to_float";
pub(crate) const CONV_INT_TO_FLOAT: &str = "spectra.std.convert.int_to_float";
pub(crate) const CONV_FLOAT_TO_INT: &str = "spectra.std.convert.float_to_int";
pub(crate) const CONV_STRING_TO_INT_OR: &str = "spectra.std.convert.string_to_int_or";
pub(crate) const CONV_STRING_TO_FLOAT_OR: &str = "spectra.std.convert.string_to_float_or";
pub(crate) const CONV_STRING_TO_BOOL: &str = "spectra.std.convert.string_to_bool";
pub(crate) const CONV_BOOL_TO_INT: &str = "spectra.std.convert.bool_to_int";

// ── std.random ───────────────────────────────────────────────────────────────
pub(crate) const RAND_SEED: &str = "spectra.std.random.random_seed";
pub(crate) const RAND_INT: &str = "spectra.std.random.random_int";
pub(crate) const RAND_FLOAT: &str = "spectra.std.random.random_float";
pub(crate) const RAND_BOOL: &str = "spectra.std.random.random_bool";

/// Type tags for the polymorphic io.print host call.
/// Args are pairs: (type_tag: i64, value: i64).
pub(crate) const _PRINT_TAG_INT: SpectraHostValue = 0;
pub(crate) const PRINT_TAG_STR: SpectraHostValue = 1;
pub(crate) const PRINT_TAG_BOOL: SpectraHostValue = 2;
pub(crate) const PRINT_TAG_FLOAT: SpectraHostValue = 3;

pub(crate) const LIST_NEW: &str = spectra_contract::STD_COLLECTIONS_LIST_NEW_BINDING;
pub(crate) const LIST_PUSH: &str = "spectra.std.collections.list_push";
pub(crate) const LIST_LEN: &str = "spectra.std.collections.list_len";
pub(crate) const LIST_GET: &str = spectra_contract::STD_COLLECTIONS_LIST_GET_BINDING;
pub(crate) const LIST_GET_OPTION: &str = spectra_contract::STD_COLLECTIONS_LIST_GET_OPTION_BINDING;
pub(crate) const LIST_GET_COMPAT: &str = spectra_contract::STD_COMPAT_COLLECTIONS_LIST_GET_BINDING;
pub(crate) const LIST_SET: &str = "spectra.std.collections.list_set";
pub(crate) const LIST_CONTAINS: &str = "spectra.std.collections.list_contains";
pub(crate) const LIST_CLEAR: &str = "spectra.std.collections.list_clear";
pub(crate) const LIST_FREE: &str = "spectra.std.collections.list_free";
pub(crate) const LIST_FREE_ALL: &str = "spectra.std.collections.list_free_all";
pub(crate) const LIST_POP: &str = spectra_contract::STD_COLLECTIONS_LIST_POP_BINDING;
pub(crate) const LIST_POP_FRONT: &str = spectra_contract::STD_COLLECTIONS_LIST_POP_FRONT_BINDING;
pub(crate) const LIST_POP_OPTION: &str = "spectra.std.collections.list_pop_option";
pub(crate) const LIST_POP_FRONT_OPTION: &str = "spectra.std.collections.list_pop_front_option";
pub(crate) const LIST_POP_COMPAT: &str = spectra_contract::STD_COMPAT_COLLECTIONS_LIST_POP_BINDING;
pub(crate) const LIST_POP_FRONT_COMPAT: &str = spectra_contract::STD_COMPAT_COLLECTIONS_LIST_POP_FRONT_BINDING;
pub(crate) const LIST_INSERT_AT: &str = "spectra.std.collections.list_insert_at";
pub(crate) const LIST_REMOVE_AT: &str = spectra_contract::STD_COLLECTIONS_LIST_REMOVE_AT_BINDING;
pub(crate) const LIST_REMOVE_AT_OPTION: &str = spectra_contract::STD_COLLECTIONS_LIST_REMOVE_AT_OPTION_BINDING;
pub(crate) const LIST_REMOVE_AT_COMPAT: &str = spectra_contract::STD_COMPAT_COLLECTIONS_LIST_REMOVE_AT_BINDING;
pub(crate) const LIST_INDEX_OF: &str = "spectra.std.collections.list_index_of";
pub(crate) const LIST_SORT: &str = "spectra.std.collections.list_sort";

// ── std.collections higher-order functions ──────────────────────────────────
pub(crate) const LIST_MAP: &str = "spectra.std.collections.list_map";
pub(crate) const LIST_FILTER: &str = "spectra.std.collections.list_filter";
pub(crate) const LIST_REDUCE: &str = "spectra.std.collections.list_reduce";
pub(crate) const LIST_SORT_BY: &str = "spectra.std.collections.list_sort_by";

// ── std.fs ───────────────────────────────────────────────────────────────────
pub(crate) const FS_READ: &str = spectra_contract::STD_FS_FS_READ_BINDING;
pub(crate) const FS_WRITE: &str = spectra_contract::STD_FS_FS_WRITE_BINDING;
pub(crate) const FS_APPEND: &str = spectra_contract::STD_FS_FS_APPEND_BINDING;
pub(crate) const FS_EXISTS: &str = spectra_contract::STD_FS_FS_EXISTS_BINDING;
pub(crate) const FS_REMOVE: &str = spectra_contract::STD_FS_FS_REMOVE_BINDING;
pub(crate) const FS_CREATE_DIR_ALL: &str = spectra_contract::STD_FS_CREATE_DIR_ALL_BINDING;
pub(crate) const FS_REMOVE_DIR: &str = spectra_contract::STD_FS_REMOVE_DIR_BINDING;
pub(crate) const FS_RENAME: &str = spectra_contract::STD_FS_RENAME_BINDING;
pub(crate) const FS_COPY: &str = spectra_contract::STD_FS_COPY_BINDING;
pub(crate) const FS_READ_DIR: &str = spectra_contract::STD_FS_READ_DIR_BINDING;
pub(crate) const FS_READ_COMPAT: &str = spectra_contract::STD_COMPAT_FS_FS_READ_BINDING;
pub(crate) const FS_WRITE_COMPAT: &str = spectra_contract::STD_COMPAT_FS_FS_WRITE_BINDING;
pub(crate) const FS_APPEND_COMPAT: &str = spectra_contract::STD_COMPAT_FS_FS_APPEND_BINDING;
pub(crate) const FS_EXISTS_COMPAT: &str = spectra_contract::STD_COMPAT_FS_FS_EXISTS_BINDING;
pub(crate) const FS_REMOVE_COMPAT: &str = spectra_contract::STD_COMPAT_FS_FS_REMOVE_BINDING;

// ── std.env ──────────────────────────────────────────────────────────────────
pub(crate) const ENV_GET: &str = spectra_contract::STD_ENV_ENV_GET_BINDING;
pub(crate) const ENV_GET_OPTION: &str = spectra_contract::STD_ENV_ENV_GET_OPTION_BINDING;
pub(crate) const ENV_SET: &str = "spectra.std.env.env_set";
pub(crate) const ENV_ARGS_COUNT: &str = "spectra.std.env.env_args_count";
pub(crate) const ENV_ARG: &str = spectra_contract::STD_ENV_ENV_ARG_BINDING;
pub(crate) const ENV_ARG_OPTION: &str = spectra_contract::STD_ENV_ENV_ARG_OPTION_BINDING;
pub(crate) const ENV_GET_COMPAT: &str = spectra_contract::STD_COMPAT_ENV_ENV_GET_BINDING;
pub(crate) const ENV_ARG_COMPAT: &str = spectra_contract::STD_COMPAT_ENV_ENV_ARG_BINDING;

// ── std.string (novos) ───────────────────────────────────────────────────────
pub(crate) const STR_SPLIT_BY: &str = "spectra.std.string.split_by";
pub(crate) const STR_PAD_LEFT: &str = "spectra.std.string.pad_left";
pub(crate) const STR_PAD_RIGHT: &str = "spectra.std.string.pad_right";
pub(crate) const STR_REVERSE: &str = "spectra.std.string.reverse_str";

// ── std.math (novos) ─────────────────────────────────────────────────────────
pub(crate) const MATH_SIGN: &str = "spectra.std.math.sign";
pub(crate) const MATH_GCD: &str = "spectra.std.math.gcd";
pub(crate) const MATH_LCM: &str = "spectra.std.math.lcm";
pub(crate) const MATH_IS_NAN_F: &str = "spectra.std.math.is_nan_f";
pub(crate) const MATH_IS_INFINITE_F: &str = "spectra.std.math.is_infinite_f";
pub(crate) const MATH_ABS_F: &str = "spectra.std.math.abs_f";

// ── std.char ─────────────────────────────────────────────────────────────────
pub(crate) const CHAR_IS_ALPHA: &str = "spectra.std.char.is_alpha";
pub(crate) const CHAR_IS_DIGIT: &str = "spectra.std.char.is_digit_char";
pub(crate) const CHAR_IS_WHITESPACE: &str = "spectra.std.char.is_whitespace_char";
pub(crate) const CHAR_IS_UPPER: &str = "spectra.std.char.is_upper_char";
pub(crate) const CHAR_IS_LOWER: &str = "spectra.std.char.is_lower_char";
pub(crate) const CHAR_TO_UPPER: &str = "spectra.std.char.to_upper_char";
pub(crate) const CHAR_TO_LOWER: &str = "spectra.std.char.to_lower_char";
pub(crate) const CHAR_IS_ALPHANUMERIC: &str = "spectra.std.char.is_alphanumeric";

// ── std.time ─────────────────────────────────────────────────────────────────
pub(crate) const TIME_NOW_MILLIS: &str = "spectra.std.time.time_now_millis";
pub(crate) const TIME_NOW_SECS: &str = "spectra.std.time.time_now_secs";
pub(crate) const TIME_SLEEP_MS: &str = "spectra.std.time.sleep_ms";
pub(crate) const TIME_MONOTONIC_MILLIS: &str = "spectra.std.time.monotonic_millis";
pub(crate) const TIME_MONOTONIC_NANOS: &str = "spectra.std.time.monotonic_nanos";
pub(crate) const TIME_DURATION_MS: &str = "spectra.std.time.duration_ms";
pub(crate) const TIME_DURATION_SECS: &str = "spectra.std.time.duration_secs";
pub(crate) const TIME_DURATION_MILLIS: &str = "spectra.std.time.duration_millis";
pub(crate) const TIME_DURATION_SECS_VALUE: &str = "spectra.std.time.duration_secs_value";
pub(crate) const TIME_DURATION_ADD: &str = "spectra.std.time.duration_add";
pub(crate) const TIME_DURATION_SUB: &str = "spectra.std.time.duration_sub";
pub(crate) const TIME_INSTANT_NOW: &str = "spectra.std.time.instant_now";
pub(crate) const TIME_INSTANT_ELAPSED_MS: &str = "spectra.std.time.instant_elapsed_ms";
pub(crate) const TIME_INSTANT_ADD: &str = "spectra.std.time.instant_add";
pub(crate) const TIME_INSTANT_HAS_ELAPSED: &str = "spectra.std.time.instant_has_elapsed";
pub(crate) const TIME_SLEEP: &str = "spectra.std.time.sleep";
pub(crate) const TIME_UNIX_TO_UTC: &str = "spectra.std.time.unix_to_utc";
pub(crate) const TIME_UTC_YEAR: &str = "spectra.std.time.utc_year";
pub(crate) const TIME_UTC_MONTH: &str = "spectra.std.time.utc_month";
pub(crate) const TIME_UTC_DAY: &str = "spectra.std.time.utc_day";
pub(crate) const TIME_UTC_HOUR: &str = "spectra.std.time.utc_hour";
pub(crate) const TIME_UTC_MINUTE: &str = "spectra.std.time.utc_minute";
pub(crate) const TIME_UTC_SECOND: &str = "spectra.std.time.utc_second";

// ── std.range ────────────────────────────────────────────────────────────────
pub(crate) const RANGE_CREATE: &str = "spectra.std.range.create";
pub(crate) const RANGE_LEN: &str = "spectra.std.range.len";
pub(crate) const RANGE_AT: &str = "spectra.std.range.at";
pub(crate) const RANGE_EQ: &str = "spectra.std.range.eq";
pub(crate) const RANGE_START: &str = "spectra.std.range.start";
pub(crate) const RANGE_END: &str = "spectra.std.range.end";
pub(crate) const RANGE_IS_INCLUSIVE: &str = "spectra.std.range.is_inclusive";
pub(crate) const RANGE_ITER: &str = "spectra.std.range.iter";

// ── std.tensor ──────────────────────────────────────────────────────────────
pub(crate) const TENSOR_ZEROS: &str = "spectra.std.tensor.zeros";
pub(crate) const TENSOR_ONES: &str = "spectra.std.tensor.ones";
pub(crate) const TENSOR_FULL: &str = "spectra.std.tensor.full";
pub(crate) const TENSOR_FULL_F: &str = "spectra.std.tensor.full_f";
pub(crate) const TENSOR_LITERAL: &str = "spectra.std.tensor.literal";
pub(crate) const TENSOR_LITERAL_F: &str = "spectra.std.tensor.literal_f";
pub(crate) const TENSOR_LITERAL2: &str = "spectra.std.tensor.literal2";
pub(crate) const TENSOR_LITERAL2_F: &str = "spectra.std.tensor.literal2_f";
pub(crate) const TENSOR_ARANGE: &str = "spectra.std.tensor.arange";
pub(crate) const TENSOR_ZEROS2: &str = "spectra.std.tensor.zeros2";
pub(crate) const TENSOR_ONES2: &str = "spectra.std.tensor.ones2";
pub(crate) const TENSOR_FULL2: &str = "spectra.std.tensor.full2";
pub(crate) const TENSOR_FULL2_F: &str = "spectra.std.tensor.full2_f";
pub(crate) const TENSOR_LEN: &str = "spectra.std.tensor.len";
pub(crate) const TENSOR_RANK: &str = "spectra.std.tensor.rank";
pub(crate) const TENSOR_DIM: &str = "spectra.std.tensor.dim";
pub(crate) const TENSOR_ROWS: &str = "spectra.std.tensor.rows";
pub(crate) const TENSOR_COLS: &str = "spectra.std.tensor.cols";
pub(crate) const TENSOR_IS_VALID: &str = "spectra.std.tensor.is_valid";
pub(crate) const TENSOR_GET: &str = "spectra.std.tensor.get";
pub(crate) const TENSOR_GET_F: &str = "spectra.std.tensor.get_f";
pub(crate) const TENSOR_SET: &str = "spectra.std.tensor.set";
pub(crate) const TENSOR_SET_F: &str = "spectra.std.tensor.set_f";
pub(crate) const TENSOR_GET2: &str = "spectra.std.tensor.get2";
pub(crate) const TENSOR_GET2_F: &str = "spectra.std.tensor.get2_f";
pub(crate) const TENSOR_SET2: &str = "spectra.std.tensor.set2";
pub(crate) const TENSOR_SET2_F: &str = "spectra.std.tensor.set2_f";
pub(crate) const TENSOR_RESHAPE: &str = "spectra.std.tensor.reshape";
pub(crate) const TENSOR_FLATTEN: &str = "spectra.std.tensor.flatten";
pub(crate) const TENSOR_PERMUTE: &str = "spectra.std.tensor.permute";
pub(crate) const TENSOR_SLICE: &str = "spectra.std.tensor.slice";
pub(crate) const TENSOR_CONCAT: &str = "spectra.std.tensor.concat";
pub(crate) const TENSOR_STACK: &str = "spectra.std.tensor.stack";
pub(crate) const TENSOR_ADD: &str = "spectra.std.tensor.add";
pub(crate) const TENSOR_SUB: &str = "spectra.std.tensor.sub";
pub(crate) const TENSOR_MUL: &str = "spectra.std.tensor.mul";
pub(crate) const TENSOR_DIV: &str = "spectra.std.tensor.div";
pub(crate) const TENSOR_SUM: &str = "spectra.std.tensor.sum";
pub(crate) const TENSOR_SUM_F: &str = "spectra.std.tensor.sum_f";
pub(crate) const TENSOR_SUM_T: &str = "spectra.std.tensor.sum_t";
pub(crate) const TENSOR_MEAN_F: &str = "spectra.std.tensor.mean_f";
pub(crate) const TENSOR_MEAN_T: &str = "spectra.std.tensor.mean_t";
pub(crate) const TENSOR_MAX: &str = "spectra.std.tensor.max";
pub(crate) const TENSOR_MIN: &str = "spectra.std.tensor.min";
pub(crate) const TENSOR_ARGMAX: &str = "spectra.std.tensor.argmax";
pub(crate) const TENSOR_MATMUL: &str = "spectra.std.tensor.matmul";
pub(crate) const TENSOR_MATMUL_BATCHED: &str = "spectra.std.tensor.matmul_batched";
pub(crate) const TENSOR_TRANSPOSE: &str = "spectra.std.tensor.transpose";
pub(crate) const TENSOR_DOT: &str = "spectra.std.tensor.dot";
pub(crate) const TENSOR_DOT_T: &str = "spectra.std.tensor.dot_t";
pub(crate) const TENSOR_NEG: &str = "spectra.std.tensor.neg";
pub(crate) const TENSOR_EXP_F: &str = "spectra.std.tensor.exp_f";
pub(crate) const TENSOR_LOG_F: &str = "spectra.std.tensor.log_f";
pub(crate) const TENSOR_SQRT_F: &str = "spectra.std.tensor.sqrt_f";
pub(crate) const TENSOR_RELU: &str = "spectra.std.tensor.relu";
pub(crate) const TENSOR_SIGMOID_F: &str = "spectra.std.tensor.sigmoid_f";
pub(crate) const TENSOR_TANH_F: &str = "spectra.std.tensor.tanh_f";
pub(crate) const TENSOR_SEED: &str = "spectra.std.tensor.seed";
pub(crate) const TENSOR_UNIFORM: &str = "spectra.std.tensor.uniform";
pub(crate) const TENSOR_UNIFORM_F: &str = "spectra.std.tensor.uniform_f";
pub(crate) const TENSOR_NORMAL_F: &str = "spectra.std.tensor.normal_f";
pub(crate) const TENSOR_BERNOULLI: &str = "spectra.std.tensor.bernoulli";
pub(crate) const TENSOR_CATEGORICAL: &str = "spectra.std.tensor.categorical";
pub(crate) const TENSOR_SET_DETERMINISTIC_MODE: &str = "spectra.std.tensor.set_deterministic_mode";
pub(crate) const TENSOR_DETERMINISTIC_MODE: &str = "spectra.std.tensor.deterministic_mode";
pub(crate) const TENSOR_TOLERANCE_ABS: &str = "spectra.std.tensor.tolerance_abs";
pub(crate) const TENSOR_TOLERANCE_REL: &str = "spectra.std.tensor.tolerance_rel";
pub(crate) const TENSOR_DEVICE: &str = "spectra.std.tensor.device";
pub(crate) const TENSOR_DEVICE_AVAILABLE: &str = "spectra.std.tensor.device_available";
pub(crate) const TENSOR_DEVICE_STATUS: &str = "spectra.std.tensor.device_status";
pub(crate) const TENSOR_TO_DEVICE: &str = "spectra.std.tensor.to_device";
pub(crate) const TENSOR_CPU: &str = "spectra.std.tensor.cpu";
pub(crate) const TENSOR_SYNC: &str = "spectra.std.tensor.sync";
pub(crate) const TENSOR_PRECISION: &str = "spectra.std.tensor.precision";
pub(crate) const TENSOR_TO_PRECISION: &str = "spectra.std.tensor.to_precision";
pub(crate) const TENSOR_STATS_ALLOCATIONS: &str = "spectra.std.tensor.stats_allocations";
pub(crate) const TENSOR_STATS_ACTIVE: &str = "spectra.std.tensor.stats_active";
pub(crate) const TENSOR_STATS_PEAK_BYTES: &str = "spectra.std.tensor.stats_peak_bytes";
pub(crate) const TENSOR_STATS_REUSED_BUFFERS: &str = "spectra.std.tensor.stats_reused_buffers";
pub(crate) const TENSOR_STATS_POOL_HITS: &str = "spectra.std.tensor.stats_pool_hits";
pub(crate) const TENSOR_STATS_POOL_MISSES: &str = "spectra.std.tensor.stats_pool_misses";
pub(crate) const TENSOR_STATS_ACTIVE_BYTES: &str = "spectra.std.tensor.stats_active_bytes";
pub(crate) const TENSOR_STATS_SCRATCH_REUSES: &str = "spectra.std.tensor.stats_scratch_reuses";
pub(crate) const TENSOR_KERNEL_STRATEGY: &str = "spectra.std.tensor.kernel_strategy";
pub(crate) const TENSOR_STATS_KERNEL_OPS: &str = "spectra.std.tensor.stats_kernel_ops";
pub(crate) const TENSOR_STATS_KERNEL_ELEMENTS: &str = "spectra.std.tensor.stats_kernel_elements";
pub(crate) const TENSOR_STATS_DEVICE_TRANSFERS: &str = "spectra.std.tensor.stats_device_transfers";
pub(crate) const TENSOR_STATS_GPU_KERNEL_OPS: &str = "spectra.std.tensor.stats_gpu_kernel_ops";
#[cfg(feature = "gpu")]
pub(crate) const TENSOR_STATS_DEVICE_POOL_HITS: &str = "spectra.std.tensor.stats_device_pool_hits";
#[cfg(feature = "gpu")]
pub(crate) const TENSOR_STATS_DEVICE_POOL_MISSES: &str = "spectra.std.tensor.stats_device_pool_misses";
#[cfg(feature = "gpu")]
pub(crate) const TENSOR_STATS_DEVICE_POOL_BYTES_RESIDENT: &str =
    "spectra.std.tensor.stats_device_pool_bytes_resident";
#[cfg(feature = "gpu")]
pub(crate) const TENSOR_STORAGE_DEVICE: &str = "spectra.std.tensor.storage_device";
pub(crate) const TENSOR_STATS_CPU_FALLBACKS: &str = "spectra.std.tensor.stats_cpu_fallbacks";
pub(crate) const TENSOR_STATS_GPU_ERRORS: &str = "spectra.std.tensor.stats_gpu_errors";
pub(crate) const TENSOR_STATS_DEVICE_RESIDENT: &str = "spectra.std.tensor.stats_device_resident_tensors";
pub(crate) const TENSOR_STATS_GPU_BACKWARD_OPS: &str = "spectra.std.tensor.stats_gpu_backward_ops";
pub(crate) const TENSOR_STATS_GRAPH_NODES: &str = "spectra.std.tensor.stats_graph_nodes";
pub(crate) const TENSOR_STATS_LIFETIME_RECORDS: &str = "spectra.std.tensor.stats_lifetime_records";
pub(crate) const TENSOR_STATS_RELEASED_LIFETIMES: &str = "spectra.std.tensor.stats_released_lifetimes";
pub(crate) const TENSOR_STATS_ALLOCATION_SITES: &str = "spectra.std.tensor.stats_allocation_sites";
pub(crate) const TENSOR_STATS_REUSE_RATE_PER_MILLE: &str = "spectra.std.tensor.stats_reuse_rate_per_mille";
pub(crate) const TENSOR_MEMORY_REPORT: &str = "spectra.std.tensor.memory_report";
pub(crate) const TENSOR_RESET_STATS: &str = "spectra.std.tensor.reset_stats";
pub(crate) const TENSOR_REQUIRES_GRAD: &str = "spectra.std.tensor.requires_grad";
pub(crate) const TENSOR_BACKWARD: &str = "spectra.std.tensor.backward";
pub(crate) const TENSOR_GRAD: &str = "spectra.std.tensor.grad";
pub(crate) const TENSOR_ZERO_GRAD: &str = "spectra.std.tensor.zero_grad";
pub(crate) const TENSOR_SET_GRAD_ENABLED: &str = "spectra.std.tensor.set_grad_enabled";
pub(crate) const TENSOR_GRAD_ENABLED: &str = "spectra.std.tensor.grad_enabled";
pub(crate) const TENSOR_FREE: &str = "spectra.std.tensor.free";
pub(crate) const TENSOR_FREE_ALL: &str = "spectra.std.tensor.free_all";
pub(crate) const TENSOR_REFILL: &str = "spectra.std.tensor.refill";

// ── std.ml ──────────────────────────────────────────────────────────────────
pub(crate) const ML_MODULE_NEW: &str = "spectra.std.ml.module_new";
pub(crate) const ML_MODULE_ADD_PARAMETER: &str = "spectra.std.ml.module_add_parameter";
pub(crate) const ML_MODULE_PARAMETER_COUNT: &str = "spectra.std.ml.module_parameter_count";
pub(crate) const ML_MODULE_PARAMETER: &str = "spectra.std.ml.module_parameter";
pub(crate) const ML_MODULE_SET_TRAINING: &str = "spectra.std.ml.module_set_training";
pub(crate) const ML_MODULE_IS_TRAINING: &str = "spectra.std.ml.module_is_training";
pub(crate) const ML_LINEAR: &str = "spectra.std.ml.linear";
pub(crate) const ML_CONV2D: &str = "spectra.std.ml.conv2d";
pub(crate) const ML_DROPOUT: &str = "spectra.std.ml.dropout";
pub(crate) const ML_MAX_POOL2D: &str = "spectra.std.ml.max_pool2d";
pub(crate) const ML_MSE_LOSS: &str = "spectra.std.ml.mse_loss";
pub(crate) const ML_BCE_LOSS: &str = "spectra.std.ml.bce_loss";
pub(crate) const ML_CROSS_ENTROPY_LOSS: &str = "spectra.std.ml.cross_entropy_loss";
pub(crate) const ML_NLL_LOSS: &str = "spectra.std.ml.nll_loss";
pub(crate) const ML_SGD_STEP: &str = "spectra.std.ml.sgd_step";
pub(crate) const ML_SGD_MOMENTUM_STEP: &str = "spectra.std.ml.sgd_momentum_step";
pub(crate) const ML_ADAM_STEP: &str = "spectra.std.ml.adam_step";
pub(crate) const ML_ADAMW_STEP: &str = "spectra.std.ml.adamw_step";
pub(crate) const ML_EXP_LR: &str = "spectra.std.ml.exp_lr";
pub(crate) const ML_UNSCALE_GRAD: &str = "spectra.std.ml.unscale_grad";
pub(crate) const ML_DATASET_FROM_TENSORS: &str = "spectra.std.ml.dataset_from_tensors";
pub(crate) const ML_DATASET_FROM_CSV: &str = "spectra.std.ml.dataset_from_csv";
pub(crate) const ML_DATASET_FROM_JSONL: &str = "spectra.std.ml.dataset_from_jsonl";
pub(crate) const ML_DATASET_FROM_NPY: &str = "spectra.std.ml.dataset_from_npy";
pub(crate) const ML_DATASET_FROM_DIRECTORY: &str = "spectra.std.ml.dataset_from_directory";
pub(crate) const ML_DATASET_LEN: &str = "spectra.std.ml.dataset_len";
pub(crate) const ML_DATASET_MAP_FEATURES: &str = "spectra.std.ml.dataset_map_features";
pub(crate) const ML_DATASET_FILTER_LABEL_MIN: &str = "spectra.std.ml.dataset_filter_label_min";
pub(crate) const ML_DATASET_TRAIN_SPLIT: &str = "spectra.std.ml.dataset_train_split";
pub(crate) const ML_DATASET_TEST_SPLIT: &str = "spectra.std.ml.dataset_test_split";
pub(crate) const ML_DATALOADER_NEW: &str = "spectra.std.ml.dataloader_new";
pub(crate) const ML_DATALOADER_BATCH_COUNT: &str = "spectra.std.ml.dataloader_batch_count";
pub(crate) const ML_DATALOADER_BATCH_FEATURES: &str = "spectra.std.ml.dataloader_batch_features";
pub(crate) const ML_DATALOADER_BATCH_LABELS: &str = "spectra.std.ml.dataloader_batch_labels";
pub(crate) const ML_DATAFRAME_FROM_CSV: &str = "spectra.std.ml.dataframe_from_csv";
pub(crate) const ML_DATAFRAME_ROWS: &str = "spectra.std.ml.dataframe_rows";
pub(crate) const ML_DATAFRAME_COLS: &str = "spectra.std.ml.dataframe_cols";
pub(crate) const ML_DATAFRAME_COLUMN: &str = "spectra.std.ml.dataframe_column";
pub(crate) const ML_EXPERIMENT_START: &str = "spectra.std.ml.experiment_start";
pub(crate) const ML_EXPERIMENT_SET_CONFIG: &str = "spectra.std.ml.experiment_set_config";
pub(crate) const ML_EXPERIMENT_LOG_METRIC: &str = "spectra.std.ml.experiment_log_metric";
pub(crate) const ML_EXPERIMENT_LOG_ARTIFACT: &str = "spectra.std.ml.experiment_log_artifact";
pub(crate) const ML_EXPERIMENT_SET_LOCKFILE: &str = "spectra.std.ml.experiment_set_lockfile";
pub(crate) const ML_EXPERIMENT_SET_MODEL_OUTPUT: &str = "spectra.std.ml.experiment_set_model_output";
pub(crate) const ML_EXPERIMENT_FINISH: &str = "spectra.std.ml.experiment_finish";
pub(crate) const ML_EXPERIMENT_MANIFEST_PATH: &str = "spectra.std.ml.experiment_manifest_path";
pub(crate) const ML_EXPERIMENT_REPRO_COMMAND: &str = "spectra.std.ml.experiment_repro_command";
pub(crate) const ML_EXPERIMENT_COMPARE_MANIFESTS: &str = "spectra.std.ml.experiment_compare_manifests";
pub(crate) const ML_DISTRIBUTED_SESSION_START: &str = "spectra.std.ml.distributed_session_start";
pub(crate) const ML_DISTRIBUTED_GLOBAL_STEP: &str = "spectra.std.ml.distributed_global_step";
pub(crate) const ML_DISTRIBUTED_WORKER_STEP_COUNT: &str = "spectra.std.ml.distributed_worker_step_count";
pub(crate) const ML_DISTRIBUTED_CHECKPOINT_SAVE: &str = "spectra.std.ml.distributed_checkpoint_save";
pub(crate) const ML_DISTRIBUTED_RESUME: &str = "spectra.std.ml.distributed_resume";
pub(crate) const ML_DISTRIBUTED_SUMMARY: &str = "spectra.std.ml.distributed_summary";
pub(crate) const ML_ONNX_EXPORT: &str = "spectra.std.ml.onnx_export";
pub(crate) const ML_ONNX_EXPORT_WEIGHTS: &str = "spectra.std.ml.onnx_export_weights";
pub(crate) const ML_ONNX_IMPORT_SUMMARY: &str = "spectra.std.ml.onnx_import_summary";
pub(crate) const ML_ONNX_VALIDATE: &str = "spectra.std.ml.onnx_validate";
pub(crate) const ML_ONNX_ROUNDTRIP: &str = "spectra.std.ml.onnx_roundtrip";
pub(crate) const ML_ONNX_SESSION_FROM_BYTES: &str = "spectra.std.ml.onnx_session_from_bytes";
pub(crate) const ML_ONNX_RUN: &str = "spectra.std.ml.onnx_run";
// ── OnnxMultiInput ──
pub(crate) const ML_ONNX_RUN_MULTI: &str = "spectra.std.ml.onnx_run_multi";
pub(crate) const ML_ONNX_SESSION_FREE: &str = "spectra.std.ml.onnx_session_free";
pub(crate) const ML_EMBEDDING_LOOKUP: &str = "spectra.std.ml.embedding_lookup";
pub(crate) const ML_POSITIONAL_ENCODING: &str = "spectra.std.ml.positional_encoding";
pub(crate) const ML_LAYER_NORM: &str = "spectra.std.ml.layer_norm";
pub(crate) const ML_GELU: &str = "spectra.std.ml.gelu";
pub(crate) const ML_SWIGLU: &str = "spectra.std.ml.swiglu";
pub(crate) const ML_ATTENTION: &str = "spectra.std.ml.attention";
pub(crate) const ML_KV_CACHE_NEW: &str = "spectra.std.ml.kv_cache_new";
pub(crate) const ML_KV_CACHE_APPEND: &str = "spectra.std.ml.kv_cache_append";
pub(crate) const ML_KV_CACHE_KEYS: &str = "spectra.std.ml.kv_cache_keys";
pub(crate) const ML_KV_CACHE_VALUES: &str = "spectra.std.ml.kv_cache_values";
pub(crate) const ML_KV_CACHE_LEN: &str = "spectra.std.ml.kv_cache_len";
pub(crate) const ML_LOGITS_SAMPLE: &str = "spectra.std.ml.logits_sample";
pub(crate) const ML_LOGITS_SAMPLE_SEEDED: &str = "spectra.std.ml.logits_sample_seeded";
pub(crate) const ML_TOKENIZER_WORDPIECE: &str = "spectra.std.ml.tokenizer_wordpiece";
pub(crate) const ML_TOKENIZER_LOAD: &str = "spectra.std.ml.tokenizer_load";
pub(crate) const ML_TOKENIZER_ENCODE: &str = "spectra.std.ml.tokenizer_encode";
pub(crate) const ML_TOKENIZER_DECODE: &str = "spectra.std.ml.tokenizer_decode";
pub(crate) const ML_TEXT_EMBED: &str = "spectra.std.ml.text_embed";
pub(crate) const ML_EMBEDDING_LOAD: &str = "spectra.std.ml.embedding_load";
pub(crate) const ML_VECTOR_INDEX_NEW: &str = "spectra.std.ml.vector_index_new";
pub(crate) const ML_VECTOR_INDEX_INSERT: &str = "spectra.std.ml.vector_index_insert";
pub(crate) const ML_VECTOR_INDEX_QUERY: &str = "spectra.std.ml.vector_index_query";
pub(crate) const ML_VECTOR_INDEX_PERSIST: &str = "spectra.std.ml.vector_index_persist";
pub(crate) const ML_VECTOR_INDEX_LOAD: &str = "spectra.std.ml.vector_index_load";
pub(crate) const ML_VECTOR_INDEX_SET_METADATA: &str = "spectra.std.ml.vector_index_set_metadata";
pub(crate) const ML_VECTOR_INDEX_METRICS: &str = "spectra.std.ml.vector_index_metrics";
pub(crate) const ML_RAG_CHUNK_TEXT: &str = "spectra.std.ml.rag_chunk_text";
pub(crate) const ML_RAG_BUILD_PROMPT: &str = "spectra.std.ml.rag_build_prompt";
pub(crate) const ML_RAG_EVALUATE_ANSWER: &str = "spectra.std.ml.rag_evaluate_answer";
pub(crate) const ML_METRICS_CLASSIFICATION: &str = "spectra.std.ml.metrics_classification";
pub(crate) const ML_METRICS_REGRESSION: &str = "spectra.std.ml.metrics_regression";
pub(crate) const ML_METRICS_RANKING: &str = "spectra.std.ml.metrics_ranking";
pub(crate) const ML_METRICS_GENERATION: &str = "spectra.std.ml.metrics_generation";
pub(crate) const ML_SERVING_METRICS: &str = "spectra.std.ml.serving_metrics";
pub(crate) const ML_EVALUATION_REPORT: &str = "spectra.std.ml.evaluation_report";
pub(crate) const ML_ARTIFACT_NEW: &str = "spectra.std.ml.artifact_new";
pub(crate) const ML_ARTIFACT_SET_METADATA: &str = "spectra.std.ml.artifact_set_metadata";
pub(crate) const ML_ARTIFACT_ADD_TENSOR: &str = "spectra.std.ml.artifact_add_tensor";
pub(crate) const ML_ARTIFACT_SAVE: &str = "spectra.std.ml.artifact_save";
pub(crate) const ML_ARTIFACT_LOAD: &str = "spectra.std.ml.artifact_load";
pub(crate) const ML_ARTIFACT_TENSOR: &str = "spectra.std.ml.artifact_tensor";
pub(crate) const ML_ARTIFACT_METADATA: &str = "spectra.std.ml.artifact_metadata";
pub(crate) const ML_ARTIFACT_VALIDATE: &str = "spectra.std.ml.artifact_validate";
pub(crate) const ML_ARTIFACT_FREE: &str = "spectra.std.ml.artifact_free";

pub(crate) const CONCURRENT_TASK_SPAWN: &str = "spectra.std.concurrent.task_spawn";
pub(crate) const CONCURRENT_TASK_SPAWN_FN: &str = "spectra.std.concurrent.task_spawn_fn";
pub(crate) const CONCURRENT_TASK_JOIN: &str = "spectra.std.concurrent.task_join";
pub(crate) const CONCURRENT_TASK_SPAWN_JOIN: &str = "spectra.std.concurrent.task_spawn_join";
pub(crate) const CONCURRENT_TASK_SPAWN_BATCH: &str = "spectra.std.concurrent.task_spawn_batch";
pub(crate) const CONCURRENT_TASK_JOIN_BATCH_SUM: &str = "spectra.std.concurrent.task_join_batch_sum";
pub(crate) const CONCURRENT_TASK_IS_DONE: &str = "spectra.std.concurrent.task_is_done";
pub(crate) const CONCURRENT_CHANNEL_NEW: &str = "spectra.std.concurrent.channel_new";
pub(crate) const CONCURRENT_CHANNEL_SEND: &str = "spectra.std.concurrent.channel_send";
pub(crate) const CONCURRENT_CHANNEL_RECV: &str = "spectra.std.concurrent.channel_recv";
pub(crate) const CONCURRENT_CHANNEL_LEN: &str = "spectra.std.concurrent.channel_len";
pub(crate) const CONCURRENT_CHANNEL_CLOSE: &str = "spectra.std.concurrent.channel_close";
pub(crate) const CONCURRENT_COUNTER_NEW: &str = "spectra.std.concurrent.counter_new";
pub(crate) const CONCURRENT_COUNTER_ADD: &str = "spectra.std.concurrent.counter_add";
pub(crate) const CONCURRENT_COUNTER_GET: &str = "spectra.std.concurrent.counter_get";
pub(crate) const CONCURRENT_PIPELINE_SUM: &str = "spectra.std.concurrent.pipeline_sum";
pub(crate) const CONCURRENT_STATS_TASKS_SPAWNED: &str = "spectra.std.concurrent.stats_tasks_spawned";
pub(crate) const CONCURRENT_STATS_CHANNELS: &str = "spectra.std.concurrent.stats_channels";
pub(crate) const CONCURRENT_RESET: &str = "spectra.std.concurrent.reset";

pub(crate) struct ConcurrentDiagnostics {
    pub(crate) fused_fast_abi_calls: AtomicU64,
    pub(crate) spawn_fast_abi_calls: AtomicU64,
    pub(crate) join_fast_abi_calls: AtomicU64,
    pub(crate) reset_fast_abi_calls: AtomicU64,
    pub(crate) locks_acquired: AtomicU64,
    pub(crate) slots_created: AtomicU64,
    pub(crate) tasks_counted: AtomicU64,
    pub(crate) tasks_created: AtomicU64,
    pub(crate) tasks_executed: AtomicU64,
    pub(crate) task_polls: AtomicU64,
    pub(crate) task_wakeups: AtomicU64,
    pub(crate) task_joins: AtomicU64,
    pub(crate) batch_spawn_fast_abi_calls: AtomicU64,
    pub(crate) batch_join_fast_abi_calls: AtomicU64,
    pub(crate) batches_created: AtomicU64,
    pub(crate) batches_joined: AtomicU64,
    pub(crate) tasks_cancelled: AtomicU64,
    pub(crate) tasks_failed: AtomicU64,
    pub(crate) pending_tasks: AtomicU64,
    pub(crate) max_pending_tasks: AtomicU64,
    pub(crate) scheduler_ns: AtomicU64,
    pub(crate) execution_ns: AtomicU64,
}

impl ConcurrentDiagnostics {
    pub(crate) const fn new() -> Self {
        Self {
            fused_fast_abi_calls: AtomicU64::new(0),
            spawn_fast_abi_calls: AtomicU64::new(0),
            join_fast_abi_calls: AtomicU64::new(0),
            reset_fast_abi_calls: AtomicU64::new(0),
            locks_acquired: AtomicU64::new(0),
            slots_created: AtomicU64::new(0),
            tasks_counted: AtomicU64::new(0),
            tasks_created: AtomicU64::new(0),
            tasks_executed: AtomicU64::new(0),
            task_polls: AtomicU64::new(0),
            task_wakeups: AtomicU64::new(0),
            task_joins: AtomicU64::new(0),
            batch_spawn_fast_abi_calls: AtomicU64::new(0),
            batch_join_fast_abi_calls: AtomicU64::new(0),
            batches_created: AtomicU64::new(0),
            batches_joined: AtomicU64::new(0),
            tasks_cancelled: AtomicU64::new(0),
            tasks_failed: AtomicU64::new(0),
            pending_tasks: AtomicU64::new(0),
            max_pending_tasks: AtomicU64::new(0),
            scheduler_ns: AtomicU64::new(0),
            execution_ns: AtomicU64::new(0),
        }
    }
}

pub(crate) fn concurrent_diagnostics() -> Option<&'static ConcurrentDiagnostics> {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    static DATA: ConcurrentDiagnostics = ConcurrentDiagnostics::new();
    if *ENABLED
        .get_or_init(|| std::env::var("SPECTRA_CONCURRENT_DIAGNOSTICS").as_deref() == Ok("1"))
    {
        Some(&DATA)
    } else {
        None
    }
}

pub fn concurrent_diagnostics_report_json() -> Option<String> {
    let data = concurrent_diagnostics()?;
    Some(format!(
        "{{\"fused_fast_abi_calls\":{},\"spawn_fast_abi_calls\":{},\"join_fast_abi_calls\":{},\"batch_spawn_fast_abi_calls\":{},\"batch_join_fast_abi_calls\":{},\"reset_fast_abi_calls\":{},\"locks_acquired\":{},\"slots_created\":{},\"tasks_counted\":{},\"tasks_created\":{},\"tasks_executed\":{},\"task_polls\":{},\"task_wakeups\":{},\"task_joins\":{},\"batches_created\":{},\"batches_joined\":{},\"tasks_cancelled\":{},\"tasks_failed\":{},\"pending_tasks\":{},\"max_pending_tasks\":{},\"scheduler_ns\":{},\"execution_ns\":{},\"executor_workers\":{}}}",
        data.fused_fast_abi_calls.load(Ordering::Relaxed),
        data.spawn_fast_abi_calls.load(Ordering::Relaxed),
        data.join_fast_abi_calls.load(Ordering::Relaxed),
        data.batch_spawn_fast_abi_calls.load(Ordering::Relaxed),
        data.batch_join_fast_abi_calls.load(Ordering::Relaxed),
        data.reset_fast_abi_calls.load(Ordering::Relaxed),
        data.locks_acquired.load(Ordering::Relaxed),
        data.slots_created.load(Ordering::Relaxed),
        data.tasks_counted.load(Ordering::Relaxed),
        data.tasks_created.load(Ordering::Relaxed),
        data.tasks_executed.load(Ordering::Relaxed),
        data.task_polls.load(Ordering::Relaxed),
        data.task_wakeups.load(Ordering::Relaxed),
        data.task_joins.load(Ordering::Relaxed),
        data.batches_created.load(Ordering::Relaxed),
        data.batches_joined.load(Ordering::Relaxed),
        data.tasks_cancelled.load(Ordering::Relaxed),
        data.tasks_failed.load(Ordering::Relaxed),
        data.pending_tasks.load(Ordering::Relaxed),
        data.max_pending_tasks.load(Ordering::Relaxed),
        data.scheduler_ns.load(Ordering::Relaxed),
        data.execution_ns.load(Ordering::Relaxed),
        concurrent_executor().workers,
    ))
}

pub(crate) const ASYNC_TASK_READY: &str = "spectra.async.task.ready";
pub(crate) const ASYNC_TASK_READY_BATCH: &str = "spectra.async.task.ready_batch";
pub(crate) const ASYNC_TASK_BATCH_CHECKSUM: &str = "spectra.async.task.batch_checksum";
pub(crate) const ASYNC_TASK_POLL: &str = "spectra.async.task.poll";
pub(crate) const ASYNC_TASK_RESULT: &str = "spectra.async.task.result";
pub(crate) const ASYNC_TASK_BLOCK_ON: &str = "spectra.async.task.block_on";
pub(crate) const ASYNC_TASK_WAIT: &str = "spectra.async.task.wait";
pub(crate) const ASYNC_TASK_JOIN: &str = "spectra.async.task.join";
pub(crate) const ASYNC_TASK_JOIN_STATUS: &str = "spectra.async.task.join_status";
pub(crate) const ASYNC_TASK_CANCEL: &str = "spectra.async.task.cancel";
pub(crate) const ASYNC_TASK_IS_CANCELLED: &str = "spectra.async.task.is_cancelled";
pub(crate) const ASYNC_TASK_CANCEL_HANDLE: &str = "spectra.async.task.cancel_handle";
pub(crate) const ASYNC_TASK_WITH_TIMEOUT: &str = "spectra.async.task.with_timeout";
pub(crate) const ASYNC_TASK_FAIL: &str = "spectra.async.task.fail";
pub(crate) const ASYNC_TASK_JOIN_ORDER: &str = "spectra.async.task.join_order";
pub(crate) const ASYNC_TASK_RESET: &str = "spectra.async.task.reset";
pub(crate) const ASYNC_CANCEL_HANDLE_CANCEL: &str = "spectra.async.cancel_handle.cancel";
pub(crate) const ASYNC_SCHEDULER_ADVANCE_TIME: &str = "spectra.async.scheduler.advance_time";
pub(crate) const ASYNC_SCOPE_NEW: &str = "spectra.async.scope.new";
pub(crate) const ASYNC_SCOPE_CHILD: &str = "spectra.async.scope.child";
pub(crate) const ASYNC_SCOPE_ATTACH: &str = "spectra.async.scope.attach";
pub(crate) const ASYNC_SCOPE_SPAWN_READY: &str = "spectra.async.scope.spawn_ready";
pub(crate) const ASYNC_SCOPE_CANCEL: &str = "spectra.async.scope.cancel";
pub(crate) const ASYNC_SCOPE_JOIN: &str = "spectra.async.scope.join";
pub(crate) const ASYNC_SCOPE_JOINED_COUNT: &str = "spectra.async.scope.joined_count";
pub(crate) const ASYNC_SCOPE_FAILURES: &str = "spectra.async.scope.failures";
pub(crate) const ASYNC_STREAM_NEW: &str = "spectra.async.stream.new";
pub(crate) const ASYNC_STREAM_PUSH: &str = "spectra.async.stream.push";
pub(crate) const ASYNC_STREAM_DONE: &str = "spectra.async.stream.done";
pub(crate) const ASYNC_STREAM_NEXT: &str = "spectra.async.stream.next";
pub(crate) const ASYNC_STREAM_NEXT_STATUS: &str = "spectra.async.stream.next_status";
pub(crate) const ASYNC_STREAM_CANCEL: &str = "spectra.async.stream.cancel";
pub(crate) const ASYNC_STREAM_LEN: &str = "spectra.async.stream.len";
pub(crate) const ASYNC_STREAM_CAPACITY: &str = "spectra.async.stream.capacity";
pub(crate) const ASYNC_STREAM_MAP: &str = "spectra.async.stream.map";
pub(crate) const ASYNC_STREAM_FILTER: &str = "spectra.async.stream.filter";
pub(crate) const ASYNC_STREAM_FOLD: &str = "spectra.async.stream.fold";
pub(crate) const ASYNC_STREAM_TAKE: &str = "spectra.async.stream.take";
pub(crate) const ASYNC_STREAM_SKIP: &str = "spectra.async.stream.skip";
pub(crate) const ASYNC_STREAM_CHUNKS: &str = "spectra.async.stream.chunks";
pub(crate) const ASYNC_STREAM_FUSE: &str = "spectra.async.stream.fuse";
pub(crate) const ASYNC_FS_READ: &str = "spectra.async.fs.read_async";
pub(crate) const ASYNC_FS_WRITE: &str = "spectra.async.fs.write_async";
pub(crate) const ASYNC_TCP_LISTEN: &str = "spectra.async.tcp.listen";
pub(crate) const ASYNC_TCP_LISTENER_PORT: &str = "spectra.async.tcp.listener_port";
pub(crate) const ASYNC_TCP_CONNECT: &str = "spectra.async.tcp.connect_async";
pub(crate) const ASYNC_TCP_ACCEPT: &str = "spectra.async.tcp.accept_async";
pub(crate) const ASYNC_TCP_READ: &str = "spectra.async.tcp.read_async";
pub(crate) const ASYNC_TCP_WRITE: &str = "spectra.async.tcp.write_async";
pub(crate) const ASYNC_TCP_CLOSE: &str = "spectra.async.tcp.close";
pub(crate) const ASYNC_UDP_BIND: &str = "spectra.async.udp.bind";
pub(crate) const ASYNC_UDP_PORT: &str = "spectra.async.udp.port";
pub(crate) const ASYNC_UDP_SEND_TO: &str = "spectra.async.udp.send_to_async";
pub(crate) const ASYNC_UDP_RECV: &str = "spectra.async.udp.recv_async";
pub(crate) const ASYNC_UDP_CLOSE: &str = "spectra.async.udp.close";
pub(crate) const ASYNC_CHANNEL_NEW: &str = "spectra.async.channel.new";
pub(crate) const ASYNC_CHANNEL_SEND: &str = "spectra.async.channel.send";
pub(crate) const ASYNC_CHANNEL_RECV: &str = "spectra.async.channel.recv";
pub(crate) const ASYNC_CHANNEL_CLOSE: &str = "spectra.async.channel.close";
pub(crate) const ASYNC_CHANNEL_LEN: &str = "spectra.async.channel.len";

pub(crate) const ASYNC_REACTOR_BACKEND: &str = "spectra.async.reactor.backend";
pub(crate) const ASYNC_REACTOR_WAKE: &str = "spectra.async.reactor.wake";
pub(crate) const ASYNC_REACTOR_TIMER: &str = "spectra.async.reactor.timer";
/// Synthetic-I/O host names. Gated like their wrappers (`async_network_reactor.rs`,
/// `registration.rs`): production I/O uses `register_source`, so these names exist
/// only in test builds.
#[cfg(test)]
pub(crate) const ASYNC_REACTOR_IO_REGISTER: &str = "spectra.async.reactor.io_register";
#[cfg(test)]
pub(crate) const ASYNC_REACTOR_IO_NOTIFY: &str = "spectra.async.reactor.io_notify";
pub(crate) const ASYNC_REACTOR_POLL: &str = "spectra.async.reactor.poll";
pub(crate) const ASYNC_REACTOR_LAST_KIND: &str = "spectra.async.reactor.last_kind";
pub(crate) const ASYNC_REACTOR_LAST_READINESS: &str = "spectra.async.reactor.last_readiness";
pub(crate) const ASYNC_REACTOR_STATS_QUEUED: &str = "spectra.async.reactor.stats_queued";
pub(crate) const ASYNC_REACTOR_STATS_TASK_WAKEUPS: &str = "spectra.async.reactor.stats_task_wakeups";
pub(crate) const ASYNC_REACTOR_STATS_TIMER_EVENTS: &str = "spectra.async.reactor.stats_timer_events";
pub(crate) const ASYNC_REACTOR_STATS_IO_EVENTS: &str = "spectra.async.reactor.stats_io_events";
pub(crate) const ASYNC_REACTOR_STATS_IO_REGISTRATIONS: &str = "spectra.async.reactor.stats_io_registrations";
pub(crate) const ASYNC_REACTOR_RESET: &str = "spectra.async.reactor.reset";

pub(crate) const SERVE_SERVER_NEW: &str = "spectra.std.serve.server_new";
pub(crate) const SERVE_SERVER_WARMUP: &str = "spectra.std.serve.server_warmup";
pub(crate) const SERVE_SERVER_IS_WARM: &str = "spectra.std.serve.server_is_warm";
pub(crate) const SERVE_SERVER_ENQUEUE: &str = "spectra.std.serve.server_enqueue";
pub(crate) const SERVE_SERVER_CANCEL: &str = "spectra.std.serve.server_cancel";
pub(crate) const SERVE_SERVER_PROCESS_BATCH: &str = "spectra.std.serve.server_process_batch";
pub(crate) const SERVE_SERVER_RESULT: &str = "spectra.std.serve.server_result";
pub(crate) const SERVE_SERVER_PENDING: &str = "spectra.std.serve.server_pending";
pub(crate) const SERVE_SERVER_SET_TIMEOUT: &str = "spectra.std.serve.server_set_timeout";
pub(crate) const SERVE_SERVER_RESIDENT_MODEL: &str = "spectra.std.serve.server_resident_model";
pub(crate) const SERVE_SERVER_BENCHMARK: &str = "spectra.std.serve.server_benchmark";
pub(crate) const SERVE_SERVER_SET_INPUT_POLICY: &str = "spectra.std.serve.server_set_input_policy";
pub(crate) const SERVE_SERVER_SET_OUTPUT_POLICY: &str = "spectra.std.serve.server_set_output_policy";
pub(crate) const SERVE_SERVER_SET_RATE_LIMIT: &str = "spectra.std.serve.server_set_rate_limit";
pub(crate) const SERVE_SERVER_SET_FALLBACK: &str = "spectra.std.serve.server_set_fallback";
pub(crate) const SERVE_SERVER_LAST_DIAGNOSTIC: &str = "spectra.std.serve.server_last_diagnostic";
pub(crate) const SERVE_SERVER_AUDIT_LOG: &str = "spectra.std.serve.server_audit_log";
pub(crate) const SERVE_SERVER_SET_MODEL_VERSION: &str = "spectra.std.serve.server_set_model_version";
pub(crate) const SERVE_SERVER_MONITORING_SNAPSHOT: &str = "spectra.std.serve.server_monitoring_snapshot";
pub(crate) const SERVE_SERVER_DISTRIBUTION_SUMMARY: &str = "spectra.std.serve.server_distribution_summary";
pub(crate) const SERVE_DRIFT_CHECK: &str = "spectra.std.serve.drift_check";
pub(crate) const SERVE_EXPORT_MONITORING: &str = "spectra.std.serve.export_monitoring";
pub(crate) const SERVE_RESET: &str = "spectra.std.serve.reset";

// ── std.io (novos) ───────────────────────────────────────────────────────────
pub(crate) const IO_INPUT: &str = "spectra.std.io.input";
pub(crate) const NUMERIC_CHECKED_F32: &str = "spectra.std.numeric.checked_f32";

// ── TokenizerTrainer ─────────────────────────────────────────────────────────
pub(crate) const ML_TOKENIZER_TRAIN_BPE: &str = "spectra.std.ml.train_bpe";
pub(crate) const ML_TOKENIZER_TRAIN_WORDPIECE: &str = "spectra.std.ml.train_wordpiece";
pub(crate) const ML_TOKENIZER_VOCAB: &str = "spectra.std.ml.tokenizer_vocab";

// ── StatsEmbed ───────────────────────────────────────────────────────────────
pub(crate) const ML_TEXT_EMBED_MODEL: &str = "spectra.std.ml.text_embed_model";
pub(crate) const ML_TEXT_EMBED_MODEL_SESSION: &str = "spectra.std.ml.text_embed_model_session";

// ── DistTCP ──────────────────────────────────────────────────────────────────
pub(crate) const ML_DISTRIBUTED_TRAIN_MULTITHREAD: &str = "spectra.std.ml.distributed_train_multithread";
pub(crate) const ML_DISTRIBUTED_TRAIN_TCP: &str = "spectra.std.ml.distributed_train_tcp";
pub(crate) const ML_DISTRIBUTED_TRAIN_DATASET_MULTITHREAD: &str = "spectra.std.ml.distributed_train_dataset_multithread";
pub(crate) const ML_DISTRIBUTED_TRAIN_DATASET_TCP: &str = "spectra.std.ml.distributed_train_dataset_tcp";

// ── ServeReal ────────────────────────────────────────────────────────────────
pub(crate) const SERVE_SERVER_REGISTER_MODEL_LINEAR: &str =
    "spectra.std.serve.server_register_model_linear";
pub(crate) const SERVE_SERVER_REGISTER_MODEL_ONNX: &str =
    "spectra.std.serve.server_register_model_onnx";
pub(crate) const SERVE_SERVER_RESULT_VECTOR: &str = "spectra.std.serve.server_result_vector";

// ── ServeHttp ── (APPEND-ONLY: novos bindings abaixo desta linha)
pub(crate) const SERVE_HTTP_START: &str = "spectra.std.serve.http_start";
pub(crate) const SERVE_HTTP_STOP: &str = "spectra.std.serve.http_stop";

// ── ServeMultiModel ── (APPEND-ONLY: novos bindings abaixo desta linha)
pub(crate) const SERVE_SERVER_REGISTER_NAMED_MODEL_LINEAR: &str =
    "spectra.std.serve.server_register_named_model_linear";
pub(crate) const SERVE_SERVER_REGISTER_NAMED_MODEL_ONNX: &str =
    "spectra.std.serve.server_register_named_model_onnx";
pub(crate) const SERVE_SERVER_INFER: &str = "spectra.std.serve.server_infer";

// ── RagGenerate ──────────────────────────────────────────────────────────────
pub(crate) const ML_GENERATE: &str = "spectra.std.ml.generate";

// ── RagGenerateEx ── (APPEND-ONLY: novos bindings abaixo desta linha)
pub(crate) const ML_GENERATE_EX: &str = "spectra.std.ml.generate_ex";
