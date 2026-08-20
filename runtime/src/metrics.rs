//! Small, deterministic Prometheus text-exposition registry.
//!
//! The registry owns metric definitions and series values.  Updates take a
//! short mutex and rendering snapshots the values, so scraping never holds a
//! lock while formatting or while application code runs.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

const MAX_LABELS: usize = 8;
const MAX_LABEL_VALUE_BYTES: usize = 128;
const MAX_SERIES: usize = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MetricKind {
    Counter,
    Gauge,
    Histogram,
}

#[derive(Debug)]
struct MetricDef {
    kind: MetricKind,
    help: String,
    labels: Vec<String>,
    buckets: Vec<f64>,
}

#[derive(Clone, Debug)]
enum MetricValue {
    Scalar(f64),
    Histogram {
        counts: Vec<u64>,
        sum: f64,
        count: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct SeriesKey {
    name: String,
    labels: Vec<(String, String)>,
}

struct Inner {
    definitions: Mutex<BTreeMap<String, MetricDef>>,
    values: Mutex<BTreeMap<SeriesKey, MetricValue>>,
}

#[derive(Clone)]
pub struct MetricsRegistry {
    inner: Arc<Inner>,
}

#[derive(Clone)]
pub struct Counter {
    registry: MetricsRegistry,
    name: String,
}

#[derive(Clone)]
pub struct Gauge {
    registry: MetricsRegistry,
    name: String,
}

#[derive(Clone)]
pub struct Histogram {
    registry: MetricsRegistry,
    name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetricsError {
    InvalidName(String),
    InvalidHelp,
    DuplicateMetric(String),
    IncompatibleMetric(String),
    InvalidLabels(String),
    UnknownMetric(String),
    InvalidValue,
    CardinalityLimit,
    SeriesNotFound,
}

impl fmt::Display for MetricsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(n) => write!(f, "invalid metric name: {n}"),
            Self::InvalidHelp => write!(f, "metric help is empty or invalid"),
            Self::DuplicateMetric(n) => write!(f, "metric already registered: {n}"),
            Self::IncompatibleMetric(n) => write!(f, "metric definition is incompatible: {n}"),
            Self::InvalidLabels(message) => write!(f, "invalid metric labels: {message}"),
            Self::UnknownMetric(n) => write!(f, "unknown metric: {n}"),
            Self::InvalidValue => write!(
                f,
                "metric value must be finite and non-negative where required"
            ),
            Self::CardinalityLimit => write!(f, "metric series cardinality limit exceeded"),
            Self::SeriesNotFound => write!(f, "metric series does not exist"),
        }
    }
}
impl std::error::Error for MetricsError {}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                definitions: Mutex::new(BTreeMap::new()),
                values: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    pub fn register_counter(
        &self,
        name: impl Into<String>,
        help: impl Into<String>,
        labels: &[&str],
    ) -> Result<Counter, MetricsError> {
        let name = name.into();
        self.register(
            name.clone(),
            help.into(),
            MetricKind::Counter,
            labels,
            Vec::new(),
        )?;
        Ok(Counter {
            registry: self.clone(),
            name,
        })
    }

    pub fn register_gauge(
        &self,
        name: impl Into<String>,
        help: impl Into<String>,
        labels: &[&str],
    ) -> Result<Gauge, MetricsError> {
        let name = name.into();
        self.register(
            name.clone(),
            help.into(),
            MetricKind::Gauge,
            labels,
            Vec::new(),
        )?;
        Ok(Gauge {
            registry: self.clone(),
            name,
        })
    }

    pub fn register_histogram(
        &self,
        name: impl Into<String>,
        help: impl Into<String>,
        buckets: &[f64],
        labels: &[&str],
    ) -> Result<Histogram, MetricsError> {
        if buckets.is_empty()
            || buckets
                .iter()
                .any(|value| !value.is_finite() || *value < 0.0)
            || buckets.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(MetricsError::InvalidValue);
        }
        let name = name.into();
        self.register(
            name.clone(),
            help.into(),
            MetricKind::Histogram,
            labels,
            buckets.to_vec(),
        )?;
        Ok(Histogram {
            registry: self.clone(),
            name,
        })
    }

    fn register(
        &self,
        name: String,
        help: String,
        kind: MetricKind,
        labels: &[&str],
        buckets: Vec<f64>,
    ) -> Result<(), MetricsError> {
        validate_name(&name)?;
        if help.trim().is_empty() || help.contains('\n') {
            return Err(MetricsError::InvalidHelp);
        }
        let label_names = validate_label_names(labels)?;
        let mut definitions = self.inner.definitions.lock().unwrap();
        if let Some(existing) = definitions.get(&name) {
            if existing.kind == kind
                && existing.help == help
                && existing.labels == label_names
                && existing.buckets == buckets
            {
                return Ok(());
            }
            return Err(MetricsError::DuplicateMetric(name));
        }
        definitions.insert(
            name,
            MetricDef {
                kind,
                help,
                labels: label_names,
                buckets,
            },
        );
        Ok(())
    }

    pub fn counter_inc(
        &self,
        name: &str,
        labels: &[(&str, &str)],
        amount: f64,
    ) -> Result<(), MetricsError> {
        self.update_scalar(name, labels, amount, true)
    }
    pub fn gauge_set(
        &self,
        name: &str,
        labels: &[(&str, &str)],
        value: f64,
    ) -> Result<(), MetricsError> {
        self.update_scalar(name, labels, value, false)
    }
    pub fn histogram_observe(
        &self,
        name: &str,
        labels: &[(&str, &str)],
        value: f64,
    ) -> Result<(), MetricsError> {
        if !value.is_finite() || value < 0.0 {
            return Err(MetricsError::InvalidValue);
        }
        let key = self.series_key(name, labels)?;
        let definitions = self.inner.definitions.lock().unwrap();
        let definition = definitions
            .get(name)
            .ok_or_else(|| MetricsError::UnknownMetric(name.into()))?;
        if definition.kind != MetricKind::Histogram {
            return Err(MetricsError::IncompatibleMetric(name.into()));
        }
        let buckets = definition.buckets.clone();
        drop(definitions);
        let mut values = self.inner.values.lock().unwrap();
        let entry = values.entry(key).or_insert_with(|| MetricValue::Histogram {
            counts: vec![0; buckets.len()],
            sum: 0.0,
            count: 0,
        });
        let MetricValue::Histogram { counts, sum, count } = entry else {
            return Err(MetricsError::IncompatibleMetric(name.into()));
        };
        for (index, bucket) in buckets.iter().enumerate() {
            if value <= *bucket {
                counts[index] = counts[index].saturating_add(1);
            }
        }
        *sum += value;
        *count = count.saturating_add(1);
        Ok(())
    }

    fn update_scalar(
        &self,
        name: &str,
        labels: &[(&str, &str)],
        value: f64,
        add: bool,
    ) -> Result<(), MetricsError> {
        if !value.is_finite() || (add && value < 0.0) {
            return Err(MetricsError::InvalidValue);
        }
        let key = self.series_key(name, labels)?;
        let definitions = self.inner.definitions.lock().unwrap();
        let definition = definitions
            .get(name)
            .ok_or_else(|| MetricsError::UnknownMetric(name.into()))?;
        if definition.kind == MetricKind::Histogram
            || (!add && definition.kind != MetricKind::Gauge)
            || (add && definition.kind != MetricKind::Counter)
        {
            return Err(MetricsError::IncompatibleMetric(name.into()));
        }
        drop(definitions);
        let mut values = self.inner.values.lock().unwrap();
        if !values.contains_key(&key) && values.len() >= MAX_SERIES {
            return Err(MetricsError::CardinalityLimit);
        }
        let entry = values.entry(key).or_insert(MetricValue::Scalar(0.0));
        let MetricValue::Scalar(current) = entry else {
            return Err(MetricsError::IncompatibleMetric(name.into()));
        };
        if add {
            *current += value;
        } else {
            *current = value;
        }
        Ok(())
    }

    fn series_key(&self, name: &str, labels: &[(&str, &str)]) -> Result<SeriesKey, MetricsError> {
        let definitions = self.inner.definitions.lock().unwrap();
        let definition = definitions
            .get(name)
            .ok_or_else(|| MetricsError::UnknownMetric(name.into()))?;
        if labels.len() != definition.labels.len() {
            return Err(MetricsError::InvalidLabels("label count mismatch".into()));
        }
        let mut result = Vec::with_capacity(labels.len());
        for (expected, supplied) in definition.labels.iter().zip(labels.iter()) {
            if expected != supplied.0 {
                return Err(MetricsError::InvalidLabels(
                    "label order or name mismatch".into(),
                ));
            }
            validate_label_value(supplied.1)?;
            result.push((expected.clone(), supplied.1.to_string()));
        }
        Ok(SeriesKey {
            name: name.into(),
            labels: result,
        })
    }

    pub fn render_prometheus(&self) -> String {
        let definitions = self.inner.definitions.lock().unwrap();
        let values = self.inner.values.lock().unwrap();
        let mut output = String::new();
        for (name, definition) in definitions.iter() {
            output.push_str(&format!(
                "# HELP {name} {}\n# TYPE {name} {}\n",
                escape_help(&definition.help),
                match definition.kind {
                    MetricKind::Counter => "counter",
                    MetricKind::Gauge => "gauge",
                    MetricKind::Histogram => "histogram",
                }
            ));
            let series = values.iter().filter(|(key, _)| key.name == *name);
            for (key, value) in series {
                let labels = render_labels(&key.labels);
                match (definition.kind, value) {
                    (MetricKind::Counter | MetricKind::Gauge, MetricValue::Scalar(number)) => {
                        output.push_str(&format!("{name}{labels} {number}\n"))
                    }
                    (MetricKind::Histogram, MetricValue::Histogram { counts, sum, count }) => {
                        for (index, bucket) in definition.buckets.iter().enumerate() {
                            output.push_str(&format!(
                                "{name}_bucket{} {}\n",
                                append_label(&labels, "le", &format_number(*bucket)),
                                counts[index]
                            ));
                        }
                        output.push_str(&format!(
                            "{name}_bucket{} {}\n",
                            append_label(&labels, "le", "+Inf"),
                            count
                        ));
                        output.push_str(&format!(
                            "{name}_sum{labels} {sum}\n{name}_count{labels} {count}\n"
                        ));
                    }
                    _ => {}
                }
            }
        }
        output
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}
impl Counter {
    pub fn inc(&self, amount: f64) -> Result<(), MetricsError> {
        self.registry.counter_inc(&self.name, &[], amount)
    }
    pub fn inc_labels(&self, labels: &[(&str, &str)], amount: f64) -> Result<(), MetricsError> {
        self.registry.counter_inc(&self.name, labels, amount)
    }
}
impl Gauge {
    pub fn set(&self, value: f64) -> Result<(), MetricsError> {
        self.registry.gauge_set(&self.name, &[], value)
    }
    pub fn set_labels(&self, labels: &[(&str, &str)], value: f64) -> Result<(), MetricsError> {
        self.registry.gauge_set(&self.name, labels, value)
    }
}
impl Histogram {
    pub fn observe(&self, value: f64) -> Result<(), MetricsError> {
        self.registry.histogram_observe(&self.name, &[], value)
    }
    pub fn observe_labels(&self, labels: &[(&str, &str)], value: f64) -> Result<(), MetricsError> {
        self.registry.histogram_observe(&self.name, labels, value)
    }
}

pub fn global() -> MetricsRegistry {
    static GLOBAL: OnceLock<MetricsRegistry> = OnceLock::new();
    GLOBAL.get_or_init(MetricsRegistry::new).clone()
}

fn validate_name(name: &str) -> Result<(), MetricsError> {
    if name.is_empty()
        || !name.chars().enumerate().all(|(index, ch)| {
            if index == 0 {
                ch.is_ascii_alphabetic() || ch == '_' || ch == ':'
            } else {
                ch.is_ascii_alphanumeric() || ch == '_' || ch == ':'
            }
        })
    {
        Err(MetricsError::InvalidName(name.into()))
    } else {
        Ok(())
    }
}
fn validate_label_names(labels: &[&str]) -> Result<Vec<String>, MetricsError> {
    if labels.len() > MAX_LABELS {
        return Err(MetricsError::InvalidLabels("too many labels".into()));
    }
    let mut result = Vec::new();
    for label in labels {
        validate_name(label)?;
        if label.ends_with("_total")
            || matches!(
                *label,
                "url" | "path" | "query" | "body" | "token" | "user_id"
            )
        {
            return Err(MetricsError::InvalidLabels(
                "high-cardinality label is forbidden".into(),
            ));
        }
        if result.iter().any(|existing| existing == label) {
            return Err(MetricsError::InvalidLabels("duplicate label".into()));
        }
        result.push((*label).into());
    }
    Ok(result)
}
fn validate_label_value(value: &str) -> Result<(), MetricsError> {
    if value.is_empty()
        || value.len() > MAX_LABEL_VALUE_BYTES
        || value
            .chars()
            .any(|ch| ch == '\n' || ch == '\r' || ch.is_control())
    {
        Err(MetricsError::InvalidLabels("invalid label value".into()))
    } else if value.to_ascii_lowercase().contains("password")
        || value.to_ascii_lowercase().contains("secret")
        || value.to_ascii_lowercase().contains("token")
    {
        Err(MetricsError::InvalidLabels("sensitive label value".into()))
    } else {
        Ok(())
    }
}
fn escape_help(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\n', "\\n")
}
fn escape_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
fn render_labels(labels: &[(String, String)]) -> String {
    if labels.is_empty() {
        String::new()
    } else {
        format!(
            "{{{}}}",
            labels
                .iter()
                .map(|(name, value)| format!("{name}=\"{}\"", escape_label(value)))
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}
fn append_label(existing: &str, name: &str, value: &str) -> String {
    if existing.is_empty() {
        format!("{{{name}=\"{value}\"}}")
    } else {
        format!("{},{name}=\"{value}\"}}", existing.trim_end_matches('}'))
    }
}
fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}
