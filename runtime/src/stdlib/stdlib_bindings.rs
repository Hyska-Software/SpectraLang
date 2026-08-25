const MATH_ABS: &str = "spectra.std.math.abs";
const MATH_MIN: &str = "spectra.std.math.min";
const MATH_MAX: &str = "spectra.std.math.max";
const MATH_CLAMP: &str = "spectra.std.math.clamp";
const MATH_SQRT_F: &str = "spectra.std.math.sqrt_f";
const MATH_POW_F: &str = "spectra.std.math.pow_f";
const MATH_FLOOR_F: &str = "spectra.std.math.floor_f";
const MATH_CEIL_F: &str = "spectra.std.math.ceil_f";
const MATH_ROUND_F: &str = "spectra.std.math.round_f";

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

#[derive(Default)]
struct StringValueRegistry {
    values: HashMap<SpectraHostValue, String>,
}

fn string_value_registry() -> &'static Mutex<StringValueRegistry> {
    static REGISTRY: OnceLock<Mutex<StringValueRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(StringValueRegistry::default()))
}

fn register_string_value(pointer: SpectraHostValue, value: &str) {
    if pointer == 0 {
        return;
    }
    lock_unpoisoned(string_value_registry())
        .values
        .insert(pointer, value.to_string());
}

pub(crate) fn forget_string_value(pointer: usize) {
    if pointer == 0 {
        return;
    }
    lock_unpoisoned(string_value_registry())
        .values
        .remove(&(pointer as SpectraHostValue));
}

pub(crate) fn clear_string_values() {
    lock_unpoisoned(string_value_registry()).values.clear();
}

fn registered_string_value(value: SpectraHostValue) -> Option<String> {
    lock_unpoisoned(string_value_registry())
        .values
        .get(&value)
        .cloned()
}

#[derive(Clone, Debug)]
enum CollectionKey {
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
    fn raw_value(&self) -> SpectraHostValue {
        match self {
            Self::Scalar(value) => *value,
            // String keys retain their original pointer for iteration. Their
            // equality/hash identity is the owned text above.
            Self::String { raw, .. } => *raw,
        }
    }
}

fn collection_key(value: SpectraHostValue) -> CollectionKey {
    registered_string_value(value)
        .map(|text| CollectionKey::String { value: text, raw: value })
        .unwrap_or(CollectionKey::Scalar(value))
}

fn collection_values_equal(left: SpectraHostValue, right: SpectraHostValue) -> bool {
    collection_key(left) == collection_key(right)
}

const IO_PRINT: &str = "spectra.std.io.print";
const IO_PRINTLN: &str = "spectra.std.io.println";
const IO_FLUSH: &str = "spectra.std.io.flush";
const IO_EPRINT: &str = "spectra.std.io.eprint";
const IO_EPRINTLN: &str = "spectra.std.io.eprintln";
const IO_READ_LINE: &str = "spectra.std.io.read_line";

// ── std.math (novos) ─────────────────────────────────────────────────────────
const MATH_SIN_F: &str = "spectra.std.math.sin_f";
const MATH_COS_F: &str = "spectra.std.math.cos_f";
const MATH_TAN_F: &str = "spectra.std.math.tan_f";
const MATH_LOG_F: &str = "spectra.std.math.log_f";
const MATH_LOG2_F: &str = "spectra.std.math.log2_f";
const MATH_LOG10_F: &str = "spectra.std.math.log10_f";
const MATH_ATAN2_F: &str = "spectra.std.math.atan2_f";
const MATH_PI: &str = "spectra.std.math.pi";
const MATH_E_CONST: &str = "spectra.std.math.e_const";

// ── std.string ──────────────────────────────────────────────────────────────
const STR_LEN: &str = "spectra.std.string.len";
const STR_CONTAINS: &str = "spectra.std.string.contains";
const STR_TO_UPPER: &str = "spectra.std.string.to_upper";
const STR_TO_LOWER: &str = "spectra.std.string.to_lower";
const STR_TRIM: &str = "spectra.std.string.trim";
const STR_STARTS_WITH: &str = "spectra.std.string.starts_with";
const STR_ENDS_WITH: &str = "spectra.std.string.ends_with";
const STR_EQ: &str = "spectra.std.string.eq";
const STR_CONCAT: &str = "spectra.std.string.concat";
const STR_REPEAT: &str = "spectra.std.string.repeat_str";
const STR_BUILDER_NEW: &str = "spectra.std.string.builder_new";
const STR_BUILDER_PUSH: &str = "spectra.std.string.builder_push";
const STR_BUILDER_LEN: &str = "spectra.std.string.builder_len";
const STR_BUILDER_FINISH: &str = "spectra.std.string.builder_finish";
const STR_BUILDER_FREE: &str = "spectra.std.string.builder_free";
const STR_CHAR_AT: &str = "spectra.std.string.char_at";
const STR_SUBSTRING: &str = "spectra.std.string.substring";
const STR_REPLACE: &str = "spectra.std.string.replace";
const STR_INDEX_OF: &str = "spectra.std.string.index_of";
const STR_SPLIT_FIRST: &str = "spectra.std.string.split_first";
const STR_SPLIT_LAST: &str = "spectra.std.string.split_last";
const STR_IS_EMPTY: &str = "spectra.std.string.is_empty";
const STR_COUNT: &str = "spectra.std.string.count_occurrences";

// ── std.convert ─────────────────────────────────────────────────────────────
const CONV_INT_TO_STRING: &str = "spectra.std.convert.int_to_string";
const CONV_FLOAT_TO_STRING: &str = "spectra.std.convert.float_to_string";
const CONV_BOOL_TO_STRING: &str = "spectra.std.convert.bool_to_string";
const CONV_STRING_TO_INT: &str = "spectra.std.convert.string_to_int";
const CONV_STRING_TO_FLOAT: &str = "spectra.std.convert.string_to_float";
const CONV_INT_TO_FLOAT: &str = "spectra.std.convert.int_to_float";
const CONV_FLOAT_TO_INT: &str = "spectra.std.convert.float_to_int";
const CONV_STRING_TO_INT_OR: &str = "spectra.std.convert.string_to_int_or";
const CONV_STRING_TO_FLOAT_OR: &str = "spectra.std.convert.string_to_float_or";
const CONV_STRING_TO_BOOL: &str = "spectra.std.convert.string_to_bool";
const CONV_BOOL_TO_INT: &str = "spectra.std.convert.bool_to_int";

// ── std.random ───────────────────────────────────────────────────────────────
const RAND_SEED: &str = "spectra.std.random.random_seed";
const RAND_INT: &str = "spectra.std.random.random_int";
const RAND_FLOAT: &str = "spectra.std.random.random_float";
const RAND_BOOL: &str = "spectra.std.random.random_bool";

/// Type tags for the polymorphic io.print host call.
/// Args are pairs: (type_tag: i64, value: i64).
const _PRINT_TAG_INT: SpectraHostValue = 0;
const PRINT_TAG_STR: SpectraHostValue = 1;
const PRINT_TAG_BOOL: SpectraHostValue = 2;
const PRINT_TAG_FLOAT: SpectraHostValue = 3;

const LIST_NEW: &str = spectra_contract::STD_COLLECTIONS_LIST_NEW_BINDING;
const LIST_PUSH: &str = "spectra.std.collections.list_push";
const LIST_LEN: &str = "spectra.std.collections.list_len";
const LIST_GET: &str = spectra_contract::STD_COLLECTIONS_LIST_GET_BINDING;
const LIST_GET_OPTION: &str = spectra_contract::STD_COLLECTIONS_LIST_GET_OPTION_BINDING;
const LIST_GET_COMPAT: &str = spectra_contract::STD_COMPAT_COLLECTIONS_LIST_GET_BINDING;
const LIST_SET: &str = "spectra.std.collections.list_set";
const LIST_CONTAINS: &str = "spectra.std.collections.list_contains";
const LIST_CLEAR: &str = "spectra.std.collections.list_clear";
const LIST_FREE: &str = "spectra.std.collections.list_free";
const LIST_FREE_ALL: &str = "spectra.std.collections.list_free_all";
const LIST_POP: &str = spectra_contract::STD_COLLECTIONS_LIST_POP_BINDING;
const LIST_POP_FRONT: &str = spectra_contract::STD_COLLECTIONS_LIST_POP_FRONT_BINDING;
const LIST_POP_OPTION: &str = "spectra.std.collections.list_pop_option";
const LIST_POP_FRONT_OPTION: &str = "spectra.std.collections.list_pop_front_option";
const LIST_POP_COMPAT: &str = spectra_contract::STD_COMPAT_COLLECTIONS_LIST_POP_BINDING;
const LIST_POP_FRONT_COMPAT: &str = spectra_contract::STD_COMPAT_COLLECTIONS_LIST_POP_FRONT_BINDING;
const LIST_INSERT_AT: &str = "spectra.std.collections.list_insert_at";
const LIST_REMOVE_AT: &str = spectra_contract::STD_COLLECTIONS_LIST_REMOVE_AT_BINDING;
const LIST_REMOVE_AT_OPTION: &str = spectra_contract::STD_COLLECTIONS_LIST_REMOVE_AT_OPTION_BINDING;
const LIST_REMOVE_AT_COMPAT: &str = spectra_contract::STD_COMPAT_COLLECTIONS_LIST_REMOVE_AT_BINDING;
const LIST_INDEX_OF: &str = "spectra.std.collections.list_index_of";
const LIST_SORT: &str = "spectra.std.collections.list_sort";

// ── std.collections higher-order functions ──────────────────────────────────
const LIST_MAP: &str = "spectra.std.collections.list_map";
const LIST_FILTER: &str = "spectra.std.collections.list_filter";
const LIST_REDUCE: &str = "spectra.std.collections.list_reduce";
const LIST_SORT_BY: &str = "spectra.std.collections.list_sort_by";

// ── std.fs ───────────────────────────────────────────────────────────────────
const FS_READ: &str = spectra_contract::STD_FS_FS_READ_BINDING;
const FS_WRITE: &str = spectra_contract::STD_FS_FS_WRITE_BINDING;
const FS_APPEND: &str = spectra_contract::STD_FS_FS_APPEND_BINDING;
const FS_EXISTS: &str = spectra_contract::STD_FS_FS_EXISTS_BINDING;
const FS_REMOVE: &str = spectra_contract::STD_FS_FS_REMOVE_BINDING;
const FS_CREATE_DIR_ALL: &str = spectra_contract::STD_FS_CREATE_DIR_ALL_BINDING;
const FS_REMOVE_DIR: &str = spectra_contract::STD_FS_REMOVE_DIR_BINDING;
const FS_RENAME: &str = spectra_contract::STD_FS_RENAME_BINDING;
const FS_COPY: &str = spectra_contract::STD_FS_COPY_BINDING;
const FS_READ_DIR: &str = spectra_contract::STD_FS_READ_DIR_BINDING;
const FS_READ_COMPAT: &str = spectra_contract::STD_COMPAT_FS_FS_READ_BINDING;
const FS_WRITE_COMPAT: &str = spectra_contract::STD_COMPAT_FS_FS_WRITE_BINDING;
const FS_APPEND_COMPAT: &str = spectra_contract::STD_COMPAT_FS_FS_APPEND_BINDING;
const FS_EXISTS_COMPAT: &str = spectra_contract::STD_COMPAT_FS_FS_EXISTS_BINDING;
const FS_REMOVE_COMPAT: &str = spectra_contract::STD_COMPAT_FS_FS_REMOVE_BINDING;

// ── std.env ──────────────────────────────────────────────────────────────────
const ENV_GET: &str = spectra_contract::STD_ENV_ENV_GET_BINDING;
const ENV_GET_OPTION: &str = spectra_contract::STD_ENV_ENV_GET_OPTION_BINDING;
const ENV_SET: &str = "spectra.std.env.env_set";
const ENV_ARGS_COUNT: &str = "spectra.std.env.env_args_count";
const ENV_ARG: &str = spectra_contract::STD_ENV_ENV_ARG_BINDING;
const ENV_ARG_OPTION: &str = spectra_contract::STD_ENV_ENV_ARG_OPTION_BINDING;
const ENV_GET_COMPAT: &str = spectra_contract::STD_COMPAT_ENV_ENV_GET_BINDING;
const ENV_ARG_COMPAT: &str = spectra_contract::STD_COMPAT_ENV_ENV_ARG_BINDING;

// ── std.string (novos) ───────────────────────────────────────────────────────
const STR_SPLIT_BY: &str = "spectra.std.string.split_by";
const STR_PAD_LEFT: &str = "spectra.std.string.pad_left";
const STR_PAD_RIGHT: &str = "spectra.std.string.pad_right";
const STR_REVERSE: &str = "spectra.std.string.reverse_str";

// ── std.math (novos) ─────────────────────────────────────────────────────────
const MATH_SIGN: &str = "spectra.std.math.sign";
const MATH_GCD: &str = "spectra.std.math.gcd";
const MATH_LCM: &str = "spectra.std.math.lcm";
const MATH_IS_NAN_F: &str = "spectra.std.math.is_nan_f";
const MATH_IS_INFINITE_F: &str = "spectra.std.math.is_infinite_f";
const MATH_ABS_F: &str = "spectra.std.math.abs_f";

// ── std.char ─────────────────────────────────────────────────────────────────
const CHAR_IS_ALPHA: &str = "spectra.std.char.is_alpha";
const CHAR_IS_DIGIT: &str = "spectra.std.char.is_digit_char";
const CHAR_IS_WHITESPACE: &str = "spectra.std.char.is_whitespace_char";
const CHAR_IS_UPPER: &str = "spectra.std.char.is_upper_char";
const CHAR_IS_LOWER: &str = "spectra.std.char.is_lower_char";
const CHAR_TO_UPPER: &str = "spectra.std.char.to_upper_char";
const CHAR_TO_LOWER: &str = "spectra.std.char.to_lower_char";
const CHAR_IS_ALPHANUMERIC: &str = "spectra.std.char.is_alphanumeric";

// ── std.time ─────────────────────────────────────────────────────────────────
const TIME_NOW_MILLIS: &str = "spectra.std.time.time_now_millis";
const TIME_NOW_SECS: &str = "spectra.std.time.time_now_secs";
const TIME_SLEEP_MS: &str = "spectra.std.time.sleep_ms";
const TIME_MONOTONIC_MILLIS: &str = "spectra.std.time.monotonic_millis";
const TIME_MONOTONIC_NANOS: &str = "spectra.std.time.monotonic_nanos";
const TIME_DURATION_MS: &str = "spectra.std.time.duration_ms";
const TIME_DURATION_SECS: &str = "spectra.std.time.duration_secs";
const TIME_DURATION_MILLIS: &str = "spectra.std.time.duration_millis";
const TIME_DURATION_SECS_VALUE: &str = "spectra.std.time.duration_secs_value";
const TIME_DURATION_ADD: &str = "spectra.std.time.duration_add";
const TIME_DURATION_SUB: &str = "spectra.std.time.duration_sub";
const TIME_INSTANT_NOW: &str = "spectra.std.time.instant_now";
const TIME_INSTANT_ELAPSED_MS: &str = "spectra.std.time.instant_elapsed_ms";
const TIME_INSTANT_ADD: &str = "spectra.std.time.instant_add";
const TIME_INSTANT_HAS_ELAPSED: &str = "spectra.std.time.instant_has_elapsed";
const TIME_SLEEP: &str = "spectra.std.time.sleep";
const TIME_UNIX_TO_UTC: &str = "spectra.std.time.unix_to_utc";
const TIME_UTC_YEAR: &str = "spectra.std.time.utc_year";
const TIME_UTC_MONTH: &str = "spectra.std.time.utc_month";
const TIME_UTC_DAY: &str = "spectra.std.time.utc_day";
const TIME_UTC_HOUR: &str = "spectra.std.time.utc_hour";
const TIME_UTC_MINUTE: &str = "spectra.std.time.utc_minute";
const TIME_UTC_SECOND: &str = "spectra.std.time.utc_second";

// ── std.range ────────────────────────────────────────────────────────────────
const RANGE_CREATE: &str = "spectra.std.range.create";
const RANGE_LEN: &str = "spectra.std.range.len";
const RANGE_AT: &str = "spectra.std.range.at";
const RANGE_EQ: &str = "spectra.std.range.eq";
const RANGE_START: &str = "spectra.std.range.start";
const RANGE_END: &str = "spectra.std.range.end";
const RANGE_IS_INCLUSIVE: &str = "spectra.std.range.is_inclusive";
const RANGE_ITER: &str = "spectra.std.range.iter";

// ── std.tensor ──────────────────────────────────────────────────────────────
const TENSOR_ZEROS: &str = "spectra.std.tensor.zeros";
const TENSOR_ONES: &str = "spectra.std.tensor.ones";
const TENSOR_FULL: &str = "spectra.std.tensor.full";
const TENSOR_FULL_F: &str = "spectra.std.tensor.full_f";
const TENSOR_LITERAL: &str = "spectra.std.tensor.literal";
const TENSOR_LITERAL_F: &str = "spectra.std.tensor.literal_f";
const TENSOR_LITERAL2: &str = "spectra.std.tensor.literal2";
const TENSOR_LITERAL2_F: &str = "spectra.std.tensor.literal2_f";
const TENSOR_ARANGE: &str = "spectra.std.tensor.arange";
const TENSOR_ZEROS2: &str = "spectra.std.tensor.zeros2";
const TENSOR_ONES2: &str = "spectra.std.tensor.ones2";
const TENSOR_FULL2: &str = "spectra.std.tensor.full2";
const TENSOR_FULL2_F: &str = "spectra.std.tensor.full2_f";
const TENSOR_LEN: &str = "spectra.std.tensor.len";
const TENSOR_RANK: &str = "spectra.std.tensor.rank";
const TENSOR_DIM: &str = "spectra.std.tensor.dim";
const TENSOR_ROWS: &str = "spectra.std.tensor.rows";
const TENSOR_COLS: &str = "spectra.std.tensor.cols";
const TENSOR_IS_VALID: &str = "spectra.std.tensor.is_valid";
const TENSOR_GET: &str = "spectra.std.tensor.get";
const TENSOR_GET_F: &str = "spectra.std.tensor.get_f";
const TENSOR_SET: &str = "spectra.std.tensor.set";
const TENSOR_SET_F: &str = "spectra.std.tensor.set_f";
const TENSOR_GET2: &str = "spectra.std.tensor.get2";
const TENSOR_GET2_F: &str = "spectra.std.tensor.get2_f";
const TENSOR_SET2: &str = "spectra.std.tensor.set2";
const TENSOR_SET2_F: &str = "spectra.std.tensor.set2_f";
const TENSOR_RESHAPE: &str = "spectra.std.tensor.reshape";
const TENSOR_FLATTEN: &str = "spectra.std.tensor.flatten";
const TENSOR_PERMUTE: &str = "spectra.std.tensor.permute";
const TENSOR_SLICE: &str = "spectra.std.tensor.slice";
const TENSOR_CONCAT: &str = "spectra.std.tensor.concat";
const TENSOR_STACK: &str = "spectra.std.tensor.stack";
const TENSOR_ADD: &str = "spectra.std.tensor.add";
const TENSOR_SUB: &str = "spectra.std.tensor.sub";
const TENSOR_MUL: &str = "spectra.std.tensor.mul";
const TENSOR_DIV: &str = "spectra.std.tensor.div";
const TENSOR_SUM: &str = "spectra.std.tensor.sum";
const TENSOR_SUM_F: &str = "spectra.std.tensor.sum_f";
const TENSOR_SUM_T: &str = "spectra.std.tensor.sum_t";
const TENSOR_MEAN_F: &str = "spectra.std.tensor.mean_f";
const TENSOR_MEAN_T: &str = "spectra.std.tensor.mean_t";
const TENSOR_MAX: &str = "spectra.std.tensor.max";
const TENSOR_MIN: &str = "spectra.std.tensor.min";
const TENSOR_ARGMAX: &str = "spectra.std.tensor.argmax";
const TENSOR_MATMUL: &str = "spectra.std.tensor.matmul";
const TENSOR_MATMUL_BATCHED: &str = "spectra.std.tensor.matmul_batched";
const TENSOR_TRANSPOSE: &str = "spectra.std.tensor.transpose";
const TENSOR_DOT: &str = "spectra.std.tensor.dot";
const TENSOR_DOT_T: &str = "spectra.std.tensor.dot_t";
const TENSOR_NEG: &str = "spectra.std.tensor.neg";
const TENSOR_EXP_F: &str = "spectra.std.tensor.exp_f";
const TENSOR_LOG_F: &str = "spectra.std.tensor.log_f";
const TENSOR_SQRT_F: &str = "spectra.std.tensor.sqrt_f";
const TENSOR_RELU: &str = "spectra.std.tensor.relu";
const TENSOR_SIGMOID_F: &str = "spectra.std.tensor.sigmoid_f";
const TENSOR_TANH_F: &str = "spectra.std.tensor.tanh_f";
const TENSOR_SEED: &str = "spectra.std.tensor.seed";
const TENSOR_UNIFORM: &str = "spectra.std.tensor.uniform";
const TENSOR_UNIFORM_F: &str = "spectra.std.tensor.uniform_f";
const TENSOR_NORMAL_F: &str = "spectra.std.tensor.normal_f";
const TENSOR_BERNOULLI: &str = "spectra.std.tensor.bernoulli";
const TENSOR_CATEGORICAL: &str = "spectra.std.tensor.categorical";
const TENSOR_SET_DETERMINISTIC_MODE: &str = "spectra.std.tensor.set_deterministic_mode";
const TENSOR_DETERMINISTIC_MODE: &str = "spectra.std.tensor.deterministic_mode";
const TENSOR_TOLERANCE_ABS: &str = "spectra.std.tensor.tolerance_abs";
const TENSOR_TOLERANCE_REL: &str = "spectra.std.tensor.tolerance_rel";
const TENSOR_DEVICE: &str = "spectra.std.tensor.device";
const TENSOR_DEVICE_AVAILABLE: &str = "spectra.std.tensor.device_available";
const TENSOR_DEVICE_STATUS: &str = "spectra.std.tensor.device_status";
const TENSOR_TO_DEVICE: &str = "spectra.std.tensor.to_device";
const TENSOR_CPU: &str = "spectra.std.tensor.cpu";
const TENSOR_SYNC: &str = "spectra.std.tensor.sync";
const TENSOR_PRECISION: &str = "spectra.std.tensor.precision";
const TENSOR_TO_PRECISION: &str = "spectra.std.tensor.to_precision";
const TENSOR_STATS_ALLOCATIONS: &str = "spectra.std.tensor.stats_allocations";
const TENSOR_STATS_ACTIVE: &str = "spectra.std.tensor.stats_active";
const TENSOR_STATS_PEAK_BYTES: &str = "spectra.std.tensor.stats_peak_bytes";
const TENSOR_STATS_REUSED_BUFFERS: &str = "spectra.std.tensor.stats_reused_buffers";
const TENSOR_STATS_POOL_HITS: &str = "spectra.std.tensor.stats_pool_hits";
const TENSOR_STATS_POOL_MISSES: &str = "spectra.std.tensor.stats_pool_misses";
const TENSOR_STATS_ACTIVE_BYTES: &str = "spectra.std.tensor.stats_active_bytes";
const TENSOR_STATS_SCRATCH_REUSES: &str = "spectra.std.tensor.stats_scratch_reuses";
const TENSOR_KERNEL_STRATEGY: &str = "spectra.std.tensor.kernel_strategy";
const TENSOR_STATS_KERNEL_OPS: &str = "spectra.std.tensor.stats_kernel_ops";
const TENSOR_STATS_KERNEL_ELEMENTS: &str = "spectra.std.tensor.stats_kernel_elements";
const TENSOR_STATS_DEVICE_TRANSFERS: &str = "spectra.std.tensor.stats_device_transfers";
const TENSOR_STATS_GPU_KERNEL_OPS: &str = "spectra.std.tensor.stats_gpu_kernel_ops";
#[cfg(feature = "gpu")]
const TENSOR_STATS_DEVICE_POOL_HITS: &str = "spectra.std.tensor.stats_device_pool_hits";
#[cfg(feature = "gpu")]
const TENSOR_STATS_DEVICE_POOL_MISSES: &str = "spectra.std.tensor.stats_device_pool_misses";
#[cfg(feature = "gpu")]
const TENSOR_STATS_DEVICE_POOL_BYTES_RESIDENT: &str =
    "spectra.std.tensor.stats_device_pool_bytes_resident";
#[cfg(feature = "gpu")]
const TENSOR_STORAGE_DEVICE: &str = "spectra.std.tensor.storage_device";
const TENSOR_STATS_CPU_FALLBACKS: &str = "spectra.std.tensor.stats_cpu_fallbacks";
const TENSOR_STATS_GPU_ERRORS: &str = "spectra.std.tensor.stats_gpu_errors";
const TENSOR_STATS_DEVICE_RESIDENT: &str = "spectra.std.tensor.stats_device_resident_tensors";
const TENSOR_STATS_GPU_BACKWARD_OPS: &str = "spectra.std.tensor.stats_gpu_backward_ops";
const TENSOR_STATS_GRAPH_NODES: &str = "spectra.std.tensor.stats_graph_nodes";
const TENSOR_STATS_LIFETIME_RECORDS: &str = "spectra.std.tensor.stats_lifetime_records";
const TENSOR_STATS_RELEASED_LIFETIMES: &str = "spectra.std.tensor.stats_released_lifetimes";
const TENSOR_STATS_ALLOCATION_SITES: &str = "spectra.std.tensor.stats_allocation_sites";
const TENSOR_STATS_REUSE_RATE_PER_MILLE: &str = "spectra.std.tensor.stats_reuse_rate_per_mille";
const TENSOR_MEMORY_REPORT: &str = "spectra.std.tensor.memory_report";
const TENSOR_RESET_STATS: &str = "spectra.std.tensor.reset_stats";
const TENSOR_REQUIRES_GRAD: &str = "spectra.std.tensor.requires_grad";
const TENSOR_BACKWARD: &str = "spectra.std.tensor.backward";
const TENSOR_GRAD: &str = "spectra.std.tensor.grad";
const TENSOR_ZERO_GRAD: &str = "spectra.std.tensor.zero_grad";
const TENSOR_SET_GRAD_ENABLED: &str = "spectra.std.tensor.set_grad_enabled";
const TENSOR_GRAD_ENABLED: &str = "spectra.std.tensor.grad_enabled";
const TENSOR_FREE: &str = "spectra.std.tensor.free";
const TENSOR_FREE_ALL: &str = "spectra.std.tensor.free_all";
const TENSOR_REFILL: &str = "spectra.std.tensor.refill";

// ── std.ml ──────────────────────────────────────────────────────────────────
const ML_MODULE_NEW: &str = "spectra.std.ml.module_new";
const ML_MODULE_ADD_PARAMETER: &str = "spectra.std.ml.module_add_parameter";
const ML_MODULE_PARAMETER_COUNT: &str = "spectra.std.ml.module_parameter_count";
const ML_MODULE_PARAMETER: &str = "spectra.std.ml.module_parameter";
const ML_MODULE_SET_TRAINING: &str = "spectra.std.ml.module_set_training";
const ML_MODULE_IS_TRAINING: &str = "spectra.std.ml.module_is_training";
const ML_LINEAR: &str = "spectra.std.ml.linear";
const ML_CONV2D: &str = "spectra.std.ml.conv2d";
const ML_DROPOUT: &str = "spectra.std.ml.dropout";
const ML_MAX_POOL2D: &str = "spectra.std.ml.max_pool2d";
const ML_MSE_LOSS: &str = "spectra.std.ml.mse_loss";
const ML_BCE_LOSS: &str = "spectra.std.ml.bce_loss";
const ML_CROSS_ENTROPY_LOSS: &str = "spectra.std.ml.cross_entropy_loss";
const ML_NLL_LOSS: &str = "spectra.std.ml.nll_loss";
const ML_SGD_STEP: &str = "spectra.std.ml.sgd_step";
const ML_SGD_MOMENTUM_STEP: &str = "spectra.std.ml.sgd_momentum_step";
const ML_ADAM_STEP: &str = "spectra.std.ml.adam_step";
const ML_ADAMW_STEP: &str = "spectra.std.ml.adamw_step";
const ML_EXP_LR: &str = "spectra.std.ml.exp_lr";
const ML_UNSCALE_GRAD: &str = "spectra.std.ml.unscale_grad";
const ML_DATASET_FROM_TENSORS: &str = "spectra.std.ml.dataset_from_tensors";
const ML_DATASET_FROM_CSV: &str = "spectra.std.ml.dataset_from_csv";
const ML_DATASET_FROM_JSONL: &str = "spectra.std.ml.dataset_from_jsonl";
const ML_DATASET_FROM_NPY: &str = "spectra.std.ml.dataset_from_npy";
const ML_DATASET_FROM_DIRECTORY: &str = "spectra.std.ml.dataset_from_directory";
const ML_DATASET_LEN: &str = "spectra.std.ml.dataset_len";
const ML_DATASET_MAP_FEATURES: &str = "spectra.std.ml.dataset_map_features";
const ML_DATASET_FILTER_LABEL_MIN: &str = "spectra.std.ml.dataset_filter_label_min";
const ML_DATASET_TRAIN_SPLIT: &str = "spectra.std.ml.dataset_train_split";
const ML_DATASET_TEST_SPLIT: &str = "spectra.std.ml.dataset_test_split";
const ML_DATALOADER_NEW: &str = "spectra.std.ml.dataloader_new";
const ML_DATALOADER_BATCH_COUNT: &str = "spectra.std.ml.dataloader_batch_count";
const ML_DATALOADER_BATCH_FEATURES: &str = "spectra.std.ml.dataloader_batch_features";
const ML_DATALOADER_BATCH_LABELS: &str = "spectra.std.ml.dataloader_batch_labels";
const ML_DATAFRAME_FROM_CSV: &str = "spectra.std.ml.dataframe_from_csv";
const ML_DATAFRAME_ROWS: &str = "spectra.std.ml.dataframe_rows";
const ML_DATAFRAME_COLS: &str = "spectra.std.ml.dataframe_cols";
const ML_DATAFRAME_COLUMN: &str = "spectra.std.ml.dataframe_column";
const ML_EXPERIMENT_START: &str = "spectra.std.ml.experiment_start";
const ML_EXPERIMENT_SET_CONFIG: &str = "spectra.std.ml.experiment_set_config";
const ML_EXPERIMENT_LOG_METRIC: &str = "spectra.std.ml.experiment_log_metric";
const ML_EXPERIMENT_LOG_ARTIFACT: &str = "spectra.std.ml.experiment_log_artifact";
const ML_EXPERIMENT_SET_LOCKFILE: &str = "spectra.std.ml.experiment_set_lockfile";
const ML_EXPERIMENT_SET_MODEL_OUTPUT: &str = "spectra.std.ml.experiment_set_model_output";
const ML_EXPERIMENT_FINISH: &str = "spectra.std.ml.experiment_finish";
const ML_EXPERIMENT_MANIFEST_PATH: &str = "spectra.std.ml.experiment_manifest_path";
const ML_EXPERIMENT_REPRO_COMMAND: &str = "spectra.std.ml.experiment_repro_command";
const ML_EXPERIMENT_COMPARE_MANIFESTS: &str = "spectra.std.ml.experiment_compare_manifests";
const ML_DISTRIBUTED_SESSION_START: &str = "spectra.std.ml.distributed_session_start";
const ML_DISTRIBUTED_WORKER_STEP: &str = "spectra.std.ml.distributed_worker_step";
const ML_DISTRIBUTED_GLOBAL_STEP: &str = "spectra.std.ml.distributed_global_step";
const ML_DISTRIBUTED_WORKER_STEP_COUNT: &str = "spectra.std.ml.distributed_worker_step_count";
const ML_DISTRIBUTED_CHECKPOINT_SAVE: &str = "spectra.std.ml.distributed_checkpoint_save";
const ML_DISTRIBUTED_RESUME: &str = "spectra.std.ml.distributed_resume";
const ML_DISTRIBUTED_SUMMARY: &str = "spectra.std.ml.distributed_summary";
const ML_ONNX_EXPORT: &str = "spectra.std.ml.onnx_export";
const ML_ONNX_IMPORT_SUMMARY: &str = "spectra.std.ml.onnx_import_summary";
const ML_ONNX_VALIDATE: &str = "spectra.std.ml.onnx_validate";
const ML_ONNX_ROUNDTRIP: &str = "spectra.std.ml.onnx_roundtrip";
const ML_ONNX_SESSION_FROM_BYTES: &str = "spectra.std.ml.onnx_session_from_bytes";
const ML_ONNX_RUN: &str = "spectra.std.ml.onnx_run";
const ML_ONNX_SESSION_FREE: &str = "spectra.std.ml.onnx_session_free";
const ML_EMBEDDING_LOOKUP: &str = "spectra.std.ml.embedding_lookup";
const ML_POSITIONAL_ENCODING: &str = "spectra.std.ml.positional_encoding";
const ML_LAYER_NORM: &str = "spectra.std.ml.layer_norm";
const ML_GELU: &str = "spectra.std.ml.gelu";
const ML_SWIGLU: &str = "spectra.std.ml.swiglu";
const ML_ATTENTION: &str = "spectra.std.ml.attention";
const ML_KV_CACHE_NEW: &str = "spectra.std.ml.kv_cache_new";
const ML_KV_CACHE_APPEND: &str = "spectra.std.ml.kv_cache_append";
const ML_KV_CACHE_KEYS: &str = "spectra.std.ml.kv_cache_keys";
const ML_KV_CACHE_VALUES: &str = "spectra.std.ml.kv_cache_values";
const ML_KV_CACHE_LEN: &str = "spectra.std.ml.kv_cache_len";
const ML_LOGITS_SAMPLE: &str = "spectra.std.ml.logits_sample";
const ML_TOKENIZER_WORDPIECE: &str = "spectra.std.ml.tokenizer_wordpiece";
const ML_TOKENIZER_LOAD: &str = "spectra.std.ml.tokenizer_load";
const ML_TOKENIZER_ENCODE: &str = "spectra.std.ml.tokenizer_encode";
const ML_TOKENIZER_DECODE: &str = "spectra.std.ml.tokenizer_decode";
const ML_TEXT_EMBED: &str = "spectra.std.ml.text_embed";
const ML_EMBEDDING_LOAD: &str = "spectra.std.ml.embedding_load";
const ML_VECTOR_INDEX_NEW: &str = "spectra.std.ml.vector_index_new";
const ML_VECTOR_INDEX_INSERT: &str = "spectra.std.ml.vector_index_insert";
const ML_VECTOR_INDEX_QUERY: &str = "spectra.std.ml.vector_index_query";
const ML_VECTOR_INDEX_PERSIST: &str = "spectra.std.ml.vector_index_persist";
const ML_VECTOR_INDEX_LOAD: &str = "spectra.std.ml.vector_index_load";
const ML_VECTOR_INDEX_SET_METADATA: &str = "spectra.std.ml.vector_index_set_metadata";
const ML_VECTOR_INDEX_METRICS: &str = "spectra.std.ml.vector_index_metrics";
const ML_RAG_CHUNK_TEXT: &str = "spectra.std.ml.rag_chunk_text";
const ML_RAG_BUILD_PROMPT: &str = "spectra.std.ml.rag_build_prompt";
const ML_RAG_EVALUATE_ANSWER: &str = "spectra.std.ml.rag_evaluate_answer";
const ML_METRICS_CLASSIFICATION: &str = "spectra.std.ml.metrics_classification";
const ML_METRICS_REGRESSION: &str = "spectra.std.ml.metrics_regression";
const ML_METRICS_RANKING: &str = "spectra.std.ml.metrics_ranking";
const ML_METRICS_GENERATION: &str = "spectra.std.ml.metrics_generation";
const ML_SERVING_METRICS: &str = "spectra.std.ml.serving_metrics";
const ML_EVALUATION_REPORT: &str = "spectra.std.ml.evaluation_report";
const ML_ARTIFACT_NEW: &str = "spectra.std.ml.artifact_new";
const ML_ARTIFACT_SET_METADATA: &str = "spectra.std.ml.artifact_set_metadata";
const ML_ARTIFACT_ADD_TENSOR: &str = "spectra.std.ml.artifact_add_tensor";
const ML_ARTIFACT_SAVE: &str = "spectra.std.ml.artifact_save";
const ML_ARTIFACT_LOAD: &str = "spectra.std.ml.artifact_load";
const ML_ARTIFACT_TENSOR: &str = "spectra.std.ml.artifact_tensor";
const ML_ARTIFACT_METADATA: &str = "spectra.std.ml.artifact_metadata";
const ML_ARTIFACT_VALIDATE: &str = "spectra.std.ml.artifact_validate";
const ML_ARTIFACT_FREE: &str = "spectra.std.ml.artifact_free";

const CONCURRENT_TASK_SPAWN: &str = "spectra.std.concurrent.task_spawn";
const CONCURRENT_TASK_JOIN: &str = "spectra.std.concurrent.task_join";
const CONCURRENT_TASK_SPAWN_JOIN: &str = "spectra.std.concurrent.task_spawn_join";
const CONCURRENT_TASK_SPAWN_BATCH: &str = "spectra.std.concurrent.task_spawn_batch";
const CONCURRENT_TASK_JOIN_BATCH_SUM: &str = "spectra.std.concurrent.task_join_batch_sum";
const CONCURRENT_TASK_IS_DONE: &str = "spectra.std.concurrent.task_is_done";
const CONCURRENT_CHANNEL_NEW: &str = "spectra.std.concurrent.channel_new";
const CONCURRENT_CHANNEL_SEND: &str = "spectra.std.concurrent.channel_send";
const CONCURRENT_CHANNEL_RECV: &str = "spectra.std.concurrent.channel_recv";
const CONCURRENT_CHANNEL_LEN: &str = "spectra.std.concurrent.channel_len";
const CONCURRENT_CHANNEL_CLOSE: &str = "spectra.std.concurrent.channel_close";
const CONCURRENT_COUNTER_NEW: &str = "spectra.std.concurrent.counter_new";
const CONCURRENT_COUNTER_ADD: &str = "spectra.std.concurrent.counter_add";
const CONCURRENT_COUNTER_GET: &str = "spectra.std.concurrent.counter_get";
const CONCURRENT_PIPELINE_SUM: &str = "spectra.std.concurrent.pipeline_sum";
const CONCURRENT_STATS_TASKS_SPAWNED: &str = "spectra.std.concurrent.stats_tasks_spawned";
const CONCURRENT_STATS_CHANNELS: &str = "spectra.std.concurrent.stats_channels";
const CONCURRENT_RESET: &str = "spectra.std.concurrent.reset";

struct ConcurrentDiagnostics {
    fused_fast_abi_calls: AtomicU64,
    spawn_fast_abi_calls: AtomicU64,
    join_fast_abi_calls: AtomicU64,
    reset_fast_abi_calls: AtomicU64,
    locks_acquired: AtomicU64,
    slots_created: AtomicU64,
    tasks_counted: AtomicU64,
    tasks_created: AtomicU64,
    tasks_executed: AtomicU64,
    task_polls: AtomicU64,
    task_wakeups: AtomicU64,
    task_joins: AtomicU64,
    batch_spawn_fast_abi_calls: AtomicU64,
    batch_join_fast_abi_calls: AtomicU64,
    batches_created: AtomicU64,
    batches_joined: AtomicU64,
    tasks_cancelled: AtomicU64,
    tasks_failed: AtomicU64,
    pending_tasks: AtomicU64,
    max_pending_tasks: AtomicU64,
    scheduler_ns: AtomicU64,
    execution_ns: AtomicU64,
}

impl ConcurrentDiagnostics {
    const fn new() -> Self {
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

fn concurrent_diagnostics() -> Option<&'static ConcurrentDiagnostics> {
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

const ASYNC_TASK_READY: &str = "spectra.async.task.ready";
const ASYNC_TASK_READY_BATCH: &str = "spectra.async.task.ready_batch";
const ASYNC_TASK_BATCH_CHECKSUM: &str = "spectra.async.task.batch_checksum";
const ASYNC_TASK_POLL: &str = "spectra.async.task.poll";
const ASYNC_TASK_RESULT: &str = "spectra.async.task.result";
const ASYNC_TASK_BLOCK_ON: &str = "spectra.async.task.block_on";
const ASYNC_TASK_WAIT: &str = "spectra.async.task.wait";
const ASYNC_TASK_JOIN: &str = "spectra.async.task.join";
const ASYNC_TASK_JOIN_STATUS: &str = "spectra.async.task.join_status";
const ASYNC_TASK_CANCEL: &str = "spectra.async.task.cancel";
const ASYNC_TASK_IS_CANCELLED: &str = "spectra.async.task.is_cancelled";
const ASYNC_TASK_CANCEL_HANDLE: &str = "spectra.async.task.cancel_handle";
const ASYNC_TASK_WITH_TIMEOUT: &str = "spectra.async.task.with_timeout";
const ASYNC_TASK_FAIL: &str = "spectra.async.task.fail";
const ASYNC_TASK_JOIN_ORDER: &str = "spectra.async.task.join_order";
const ASYNC_TASK_RESET: &str = "spectra.async.task.reset";
const ASYNC_CANCEL_HANDLE_CANCEL: &str = "spectra.async.cancel_handle.cancel";
const ASYNC_SCHEDULER_ADVANCE_TIME: &str = "spectra.async.scheduler.advance_time";
const ASYNC_SCOPE_NEW: &str = "spectra.async.scope.new";
const ASYNC_SCOPE_CHILD: &str = "spectra.async.scope.child";
const ASYNC_SCOPE_ATTACH: &str = "spectra.async.scope.attach";
const ASYNC_SCOPE_SPAWN_READY: &str = "spectra.async.scope.spawn_ready";
const ASYNC_SCOPE_CANCEL: &str = "spectra.async.scope.cancel";
const ASYNC_SCOPE_JOIN: &str = "spectra.async.scope.join";
const ASYNC_SCOPE_JOINED_COUNT: &str = "spectra.async.scope.joined_count";
const ASYNC_SCOPE_FAILURES: &str = "spectra.async.scope.failures";
const ASYNC_STREAM_NEW: &str = "spectra.async.stream.new";
const ASYNC_STREAM_PUSH: &str = "spectra.async.stream.push";
const ASYNC_STREAM_DONE: &str = "spectra.async.stream.done";
const ASYNC_STREAM_NEXT: &str = "spectra.async.stream.next";
const ASYNC_STREAM_NEXT_STATUS: &str = "spectra.async.stream.next_status";
const ASYNC_STREAM_CANCEL: &str = "spectra.async.stream.cancel";
const ASYNC_STREAM_LEN: &str = "spectra.async.stream.len";
const ASYNC_STREAM_CAPACITY: &str = "spectra.async.stream.capacity";
const ASYNC_STREAM_MAP: &str = "spectra.async.stream.map";
const ASYNC_STREAM_FILTER: &str = "spectra.async.stream.filter";
const ASYNC_STREAM_FOLD: &str = "spectra.async.stream.fold";
const ASYNC_STREAM_TAKE: &str = "spectra.async.stream.take";
const ASYNC_STREAM_SKIP: &str = "spectra.async.stream.skip";
const ASYNC_STREAM_CHUNKS: &str = "spectra.async.stream.chunks";
const ASYNC_STREAM_FUSE: &str = "spectra.async.stream.fuse";
const ASYNC_FS_READ: &str = "spectra.async.fs.read_async";
const ASYNC_FS_WRITE: &str = "spectra.async.fs.write_async";
const ASYNC_TCP_LISTEN: &str = "spectra.async.tcp.listen";
const ASYNC_TCP_LISTENER_PORT: &str = "spectra.async.tcp.listener_port";
const ASYNC_TCP_CONNECT: &str = "spectra.async.tcp.connect_async";
const ASYNC_TCP_ACCEPT: &str = "spectra.async.tcp.accept_async";
const ASYNC_TCP_READ: &str = "spectra.async.tcp.read_async";
const ASYNC_TCP_WRITE: &str = "spectra.async.tcp.write_async";
const ASYNC_TCP_CLOSE: &str = "spectra.async.tcp.close";
const ASYNC_UDP_BIND: &str = "spectra.async.udp.bind";
const ASYNC_UDP_PORT: &str = "spectra.async.udp.port";
const ASYNC_UDP_SEND_TO: &str = "spectra.async.udp.send_to_async";
const ASYNC_UDP_RECV: &str = "spectra.async.udp.recv_async";
const ASYNC_UDP_CLOSE: &str = "spectra.async.udp.close";
const ASYNC_CHANNEL_NEW: &str = "spectra.async.channel.new";
const ASYNC_CHANNEL_SEND: &str = "spectra.async.channel.send";
const ASYNC_CHANNEL_RECV: &str = "spectra.async.channel.recv";
const ASYNC_CHANNEL_CLOSE: &str = "spectra.async.channel.close";
const ASYNC_CHANNEL_LEN: &str = "spectra.async.channel.len";

const ASYNC_REACTOR_BACKEND: &str = "spectra.async.reactor.backend";
const ASYNC_REACTOR_WAKE: &str = "spectra.async.reactor.wake";
const ASYNC_REACTOR_TIMER: &str = "spectra.async.reactor.timer";
const ASYNC_REACTOR_IO_REGISTER: &str = "spectra.async.reactor.io_register";
const ASYNC_REACTOR_IO_NOTIFY: &str = "spectra.async.reactor.io_notify";
const ASYNC_REACTOR_POLL: &str = "spectra.async.reactor.poll";
const ASYNC_REACTOR_LAST_KIND: &str = "spectra.async.reactor.last_kind";
const ASYNC_REACTOR_LAST_READINESS: &str = "spectra.async.reactor.last_readiness";
const ASYNC_REACTOR_STATS_QUEUED: &str = "spectra.async.reactor.stats_queued";
const ASYNC_REACTOR_STATS_TASK_WAKEUPS: &str = "spectra.async.reactor.stats_task_wakeups";
const ASYNC_REACTOR_STATS_TIMER_EVENTS: &str = "spectra.async.reactor.stats_timer_events";
const ASYNC_REACTOR_STATS_IO_EVENTS: &str = "spectra.async.reactor.stats_io_events";
const ASYNC_REACTOR_STATS_IO_REGISTRATIONS: &str = "spectra.async.reactor.stats_io_registrations";
const ASYNC_REACTOR_RESET: &str = "spectra.async.reactor.reset";

const SERVE_SERVER_NEW: &str = "spectra.std.serve.server_new";
const SERVE_SERVER_WARMUP: &str = "spectra.std.serve.server_warmup";
const SERVE_SERVER_IS_WARM: &str = "spectra.std.serve.server_is_warm";
const SERVE_SERVER_ENQUEUE: &str = "spectra.std.serve.server_enqueue";
const SERVE_SERVER_CANCEL: &str = "spectra.std.serve.server_cancel";
const SERVE_SERVER_PROCESS_BATCH: &str = "spectra.std.serve.server_process_batch";
const SERVE_SERVER_RESULT: &str = "spectra.std.serve.server_result";
const SERVE_SERVER_PENDING: &str = "spectra.std.serve.server_pending";
const SERVE_SERVER_SET_TIMEOUT: &str = "spectra.std.serve.server_set_timeout";
const SERVE_SERVER_RESIDENT_MODEL: &str = "spectra.std.serve.server_resident_model";
const SERVE_SERVER_BENCHMARK: &str = "spectra.std.serve.server_benchmark";
const SERVE_SERVER_SET_INPUT_POLICY: &str = "spectra.std.serve.server_set_input_policy";
const SERVE_SERVER_SET_OUTPUT_POLICY: &str = "spectra.std.serve.server_set_output_policy";
const SERVE_SERVER_SET_RATE_LIMIT: &str = "spectra.std.serve.server_set_rate_limit";
const SERVE_SERVER_SET_FALLBACK: &str = "spectra.std.serve.server_set_fallback";
const SERVE_SERVER_LAST_DIAGNOSTIC: &str = "spectra.std.serve.server_last_diagnostic";
const SERVE_SERVER_AUDIT_LOG: &str = "spectra.std.serve.server_audit_log";
const SERVE_SERVER_SET_MODEL_VERSION: &str = "spectra.std.serve.server_set_model_version";
const SERVE_SERVER_MONITORING_SNAPSHOT: &str = "spectra.std.serve.server_monitoring_snapshot";
const SERVE_SERVER_DISTRIBUTION_SUMMARY: &str = "spectra.std.serve.server_distribution_summary";
const SERVE_DRIFT_CHECK: &str = "spectra.std.serve.drift_check";
const SERVE_EXPORT_MONITORING: &str = "spectra.std.serve.export_monitoring";
const SERVE_RESET: &str = "spectra.std.serve.reset";

// ── std.io (novos) ───────────────────────────────────────────────────────────
const IO_INPUT: &str = "spectra.std.io.input";
const NUMERIC_CHECKED_F32: &str = "spectra.std.numeric.checked_f32";
