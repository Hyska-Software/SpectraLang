use super::dialect::Dialect;
use super::error::QueryError;
use crate::sqlite::SqliteValue;
use std::marker::PhantomData;

pub trait SqlType: Clone + Send + Sync + 'static {}
#[derive(Debug, Clone, Copy)]
pub struct Integer;
#[derive(Debug, Clone, Copy)]
pub struct Real;
#[derive(Debug, Clone, Copy)]
pub struct Text;
#[derive(Debug, Clone, Copy)]
pub struct Boolean;
#[derive(Debug, Clone, Copy)]
pub struct Blob;
#[derive(Debug, Clone, Copy)]
pub struct Null;
impl SqlType for Integer {}
impl SqlType for Real {}
impl SqlType for Text {}
impl SqlType for Boolean {}
impl SqlType for Blob {}
impl SqlType for Null {}

#[derive(Debug, Clone)]
pub struct Column<T: SqlType> {
    name: String,
    marker: PhantomData<T>,
}

impl<T: SqlType> Column<T> {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            marker: PhantomData,
        }
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn reference(&self) -> ColumnRef {
        ColumnRef(self.name.clone())
    }
    pub fn expr(&self) -> Expr<T> {
        Expr::Column(self.clone())
    }
    pub fn equals(&self, value: Value<T>) -> Predicate {
        Predicate::eq(self.expr(), value.expr())
    }
    pub fn not_equals(&self, value: Value<T>) -> Predicate {
        Predicate::ne(self.expr(), value.expr())
    }
    pub fn gt(&self, value: Value<T>) -> Predicate {
        Predicate::gt(self.expr(), value.expr())
    }
    pub fn ge(&self, value: Value<T>) -> Predicate {
        Predicate::ge(self.expr(), value.expr())
    }
    pub fn lt(&self, value: Value<T>) -> Predicate {
        Predicate::lt(self.expr(), value.expr())
    }
    pub fn le(&self, value: Value<T>) -> Predicate {
        Predicate::le(self.expr(), value.expr())
    }
    pub fn like(&self, pattern: impl Into<String>) -> Predicate {
        Predicate::like(self.expr(), pattern)
    }
    pub fn like_exact(&self, pattern: impl Into<String>) -> Predicate {
        Predicate::like_exact(self.expr(), pattern)
    }
    pub fn in_list(&self, values: &[Value<T>]) -> Result<Predicate, QueryError> {
        Predicate::in_list(self.expr(), values)
    }
}

#[derive(Debug, Clone)]
pub struct Value<T: SqlType> {
    value: SqliteValue,
    marker: PhantomData<T>,
}
impl<T: SqlType> Value<T> {
    pub fn expr(&self) -> Expr<T> {
        Expr::Param(self.value.clone(), PhantomData)
    }
}
impl Value<Integer> {
    pub fn integer(value: i64) -> Self {
        Self {
            value: SqliteValue::Integer(value),
            marker: PhantomData,
        }
    }
}
impl Value<Real> {
    pub fn real(value: f64) -> Self {
        Self {
            value: SqliteValue::Real(value),
            marker: PhantomData,
        }
    }
}
impl Value<Text> {
    pub fn text(value: impl Into<String>) -> Self {
        Self {
            value: SqliteValue::Text(value.into()),
            marker: PhantomData,
        }
    }
}
impl Value<Boolean> {
    pub fn boolean(value: bool) -> Self {
        Self {
            value: SqliteValue::Integer(value as i64),
            marker: PhantomData,
        }
    }
}
impl Value<Blob> {
    pub fn blob(value: Vec<u8>) -> Self {
        Self {
            value: SqliteValue::Blob(value),
            marker: PhantomData,
        }
    }
}
impl Value<Null> {
    pub fn null() -> Self {
        Self {
            value: SqliteValue::Null,
            marker: PhantomData,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Expr<T: SqlType> {
    Column(Column<T>),
    Param(SqliteValue, PhantomData<T>),
}

/// Argument accepted by aggregate functions.
#[derive(Debug, Clone)]
pub enum AggregateArgument {
    /// `*`, valid for `COUNT(*)`.
    Star,
    /// A column reference or a bound parameter.
    Expr(Box<AnyExpr>),
}

impl From<&str> for AggregateArgument {
    fn from(name: &str) -> Self {
        Self::Expr(Box::new(AnyExpr::Column(name.to_owned())))
    }
}

impl<T: SqlType> From<Column<T>> for AggregateArgument {
    fn from(column: Column<T>) -> Self {
        Self::Expr(Box::new(AnyExpr::Column(column.name)))
    }
}
#[derive(Debug, Clone)]
pub struct Aggregate {
    function: &'static str,
    argument: AggregateArgument,
}

impl Aggregate {
    fn new(function: &'static str, argument: impl Into<AggregateArgument>) -> Self {
        Self {
            function,
            argument: argument.into(),
        }
    }
    /// `COUNT(<expression>)`.
    pub fn count(argument: impl Into<AggregateArgument>) -> Self {
        Self::new("COUNT", argument)
    }
    /// `COUNT(*)`.
    pub fn count_all() -> Self {
        Self {
            function: "COUNT",
            argument: AggregateArgument::Star,
        }
    }
    /// `SUM(<expression>)`.
    pub fn sum(argument: impl Into<AggregateArgument>) -> Self {
        Self::new("SUM", argument)
    }
    /// `AVG(<expression>)`.
    pub fn avg(argument: impl Into<AggregateArgument>) -> Self {
        Self::new("AVG", argument)
    }
    /// `MIN(<expression>)`.
    pub fn min(argument: impl Into<AggregateArgument>) -> Self {
        Self::new("MIN", argument)
    }
    /// `MAX(<expression>)`.
    pub fn max(argument: impl Into<AggregateArgument>) -> Self {
        Self::new("MAX", argument)
    }
    pub fn equals(self, right: impl Into<AnyExpr>) -> Predicate {
        Predicate::eq(AnyExpr::from(self), right)
    }
    pub fn not_equals(self, right: impl Into<AnyExpr>) -> Predicate {
        Predicate::ne(AnyExpr::from(self), right)
    }
    pub fn gt(self, right: impl Into<AnyExpr>) -> Predicate {
        Predicate::gt(AnyExpr::from(self), right)
    }
    pub fn ge(self, right: impl Into<AnyExpr>) -> Predicate {
        Predicate::ge(AnyExpr::from(self), right)
    }
    pub fn lt(self, right: impl Into<AnyExpr>) -> Predicate {
        Predicate::lt(AnyExpr::from(self), right)
    }
    pub fn le(self, right: impl Into<AnyExpr>) -> Predicate {
        Predicate::le(AnyExpr::from(self), right)
    }
}

#[derive(Debug, Clone)]
pub enum Predicate {
    Compare {
        operator: &'static str,
        left: Box<AnyExpr>,
        right: Box<AnyExpr>,
    },
    InList {
        expr: Box<AnyExpr>,
        values: Vec<SqliteValue>,
    },
    Like {
        expr: Box<AnyExpr>,
        pattern: SqliteValue,
    },
    And(Vec<Predicate>),
    Or(Vec<Predicate>),
    Not(Box<Predicate>),
}

/// Escape character emitted alongside every `LIKE` predicate.
///
/// Bound as a parameter so neither the pattern nor the escape character ever
/// reach the SQL text itself.
pub const LIKE_ESCAPE_CHARACTER: &str = "\\";

/// Escapes `%`, `_`, and the escape character so `pattern` matches literally.
pub fn escape_like_pattern(pattern: &str) -> String {
    let mut escaped = String::with_capacity(pattern.len());
    for character in pattern.chars() {
        if matches!(character, '%' | '_' | '\\') {
            escaped.push_str(LIKE_ESCAPE_CHARACTER);
        }
        escaped.push(character);
    }
    escaped
}

#[derive(Debug, Clone)]
pub enum AnyExpr {
    Column(String),
    Param(SqliteValue),
    Aggregate {
        function: &'static str,
        argument: Box<AggregateArgument>,
    },
}
#[derive(Debug, Clone)]
pub struct ColumnRef(pub(crate) String);
impl<T: SqlType> From<Expr<T>> for AnyExpr {
    fn from(expr: Expr<T>) -> Self {
        match expr {
            Expr::Column(c) => Self::Column(c.name),
            Expr::Param(v, _) => Self::Param(v),
        }
    }
}
impl From<Aggregate> for AnyExpr {
    fn from(aggregate: Aggregate) -> Self {
        Self::Aggregate {
            function: aggregate.function,
            argument: Box::new(aggregate.argument),
        }
    }
}
impl<T: SqlType> From<Value<T>> for AnyExpr {
    fn from(value: Value<T>) -> Self {
        Self::Param(value.value)
    }
}
impl Predicate {
    fn compare(op: &'static str, left: impl Into<AnyExpr>, right: impl Into<AnyExpr>) -> Self {
        Self::Compare {
            operator: op,
            left: Box::new(left.into()),
            right: Box::new(right.into()),
        }
    }
    pub fn eq(left: impl Into<AnyExpr>, right: impl Into<AnyExpr>) -> Self {
        Self::compare("=", left, right)
    }
    pub fn ne(left: impl Into<AnyExpr>, right: impl Into<AnyExpr>) -> Self {
        Self::compare("<>", left, right)
    }
    pub fn gt(left: impl Into<AnyExpr>, right: impl Into<AnyExpr>) -> Self {
        Self::compare(">", left, right)
    }
    pub fn ge(left: impl Into<AnyExpr>, right: impl Into<AnyExpr>) -> Self {
        Self::compare(">=", left, right)
    }
    pub fn lt(left: impl Into<AnyExpr>, right: impl Into<AnyExpr>) -> Self {
        Self::compare("<", left, right)
    }
    pub fn le(left: impl Into<AnyExpr>, right: impl Into<AnyExpr>) -> Self {
        Self::compare("<=", left, right)
    }
    /// `expr IN (?, ?, ...)`.
    ///
    /// Parameters are appended in slice order. An empty `values` slice is
    /// rejected because an empty IN list is not valid SQL in either dialect;
    /// callers must decide between skipping the clause or using a sentinel.
    pub fn in_list<T: SqlType>(
        expr: impl Into<AnyExpr>,
        values: &[Value<T>],
    ) -> Result<Self, QueryError> {
        if values.is_empty() {
            return Err(QueryError::InvalidParameter("empty IN list".into()));
        }
        Ok(Self::InList {
            expr: Box::new(expr.into()),
            values: values.iter().map(|v| v.value.clone()).collect(),
        })
    }
    /// `expr LIKE ? ESCAPE ?` with the user's wildcard pattern kept verbatim.
    pub fn like(expr: impl Into<AnyExpr>, pattern: impl Into<String>) -> Self {
        Self::Like {
            expr: Box::new(expr.into()),
            pattern: SqliteValue::Text(pattern.into()),
        }
    }
    /// `expr LIKE ? ESCAPE ?` where `%` and `_` inside the given text are
    /// escaped, producing an exact-substring match instead of a pattern match.
    pub fn like_exact(expr: impl Into<AnyExpr>, pattern: impl Into<String>) -> Self {
        Self::like(expr, escape_like_pattern(&pattern.into()))
    }
    pub fn and(self, other: Predicate) -> Self {
        Self::And(vec![self, other])
    }
    pub fn or(self, other: Predicate) -> Self {
        Self::Or(vec![self, other])
    }
    #[allow(clippy::should_implement_trait)]
    pub fn not(self) -> Self {
        Self::Not(Box::new(self))
    }
}

impl std::ops::Not for Predicate {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self::Not(Box::new(self))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Order {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
    Left,
}

#[derive(Debug, Clone)]
enum ProjectionItem {
    Column(String),
    Aggregate {
        aggregate: Aggregate,
        alias: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct Select {
    table: String,
    projections: Vec<ProjectionItem>,
    joins: Vec<(JoinKind, String, Predicate)>,
    predicate: Option<Predicate>,
    group_by: Vec<String>,
    having: Option<Predicate>,
    order: Option<(String, Order)>,
    limit: Option<i64>,
    offset: Option<i64>,
}

/// Combines accumulated predicates with AND while avoiding redundant nesting.
fn combine_predicate(current: Option<Predicate>, next: Predicate) -> Predicate {
    match current {
        Some(existing) => Predicate::And(vec![existing, next]),
        None => next,
    }
}

impl Select {
    pub fn from(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            projections: Vec::new(),
            joins: Vec::new(),
            predicate: None,
            group_by: Vec::new(),
            having: None,
            order: None,
            limit: None,
            offset: None,
        }
    }
    pub fn columns<T: SqlType>(mut self, columns: &[Column<T>]) -> Self {
        self.projections.extend(
            columns
                .iter()
                .map(|c| ProjectionItem::Column(c.name.clone())),
        );
        self
    }
    pub fn columns_named(mut self, columns: &[ColumnRef]) -> Self {
        self.projections
            .extend(columns.iter().map(|c| ProjectionItem::Column(c.0.clone())));
        self
    }
    /// Appends an aggregate to the projection list, preserving call order
    /// relative to `columns`/`columns_named`.
    pub fn aggregate(mut self, aggregate: Aggregate) -> Self {
        self.projections.push(ProjectionItem::Aggregate {
            aggregate,
            alias: None,
        });
        self
    }
    /// Like `aggregate`, but renames the computed field with `AS "<alias>"`.
    pub fn aliased_aggregate(mut self, aggregate: Aggregate, alias: impl Into<String>) -> Self {
        self.projections.push(ProjectionItem::Aggregate {
            aggregate,
            alias: Some(alias.into()),
        });
        self
    }
    /// Adds `JOIN "<table>" ON <on>`. Repeated calls preserve join order.
    pub fn add_join(mut self, kind: JoinKind, table: impl Into<String>, on: Predicate) -> Self {
        self.joins.push((kind, table.into(), on));
        self
    }
    /// Adds a predicate; repeated calls are combined with AND.
    pub fn where_(mut self, predicate: Predicate) -> Self {
        self.predicate = Some(combine_predicate(self.predicate.take(), predicate));
        self
    }
    /// Adds `column IN (...one parameter per value...)`; repeats combine with AND.
    pub fn where_in<T: SqlType>(
        self,
        column: Column<T>,
        values: &[Value<T>],
    ) -> Result<Self, QueryError> {
        let predicate = column.in_list(values)?;
        Ok(self.where_(predicate))
    }
    /// Adds a verbatim `LIKE` with an escape-character clause; repeats combine with AND.
    pub fn where_like(self, column: Column<Text>, pattern: impl Into<String>) -> Self {
        self.where_(column.like(pattern))
    }
    /// Appends columns to the GROUP BY clause; repeated calls accumulate.
    pub fn group_by<T: SqlType>(mut self, columns: &[Column<T>]) -> Self {
        self.group_by.extend(columns.iter().map(|c| c.name.clone()));
        self
    }
    /// Appends columns to the GROUP BY clause by reference name (allows
    /// qualified `table.column` entries).
    pub fn group_by_named(mut self, columns: &[ColumnRef]) -> Self {
        self.group_by.extend(columns.iter().map(|c| c.0.clone()));
        self
    }
    /// Adds a HAVING condition; repeated calls are combined with AND.
    pub fn having(mut self, predicate: Predicate) -> Self {
        self.having = Some(combine_predicate(self.having.take(), predicate));
        self
    }
    pub fn order_by<T: SqlType>(mut self, column: Column<T>, order: Order) -> Self {
        self.order = Some((column.name, order));
        self
    }
    pub fn limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }
    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = Some(offset);
        self
    }
    pub(crate) fn compile_select<D: Dialect>(
        &self,
        d: &D,
    ) -> Result<super::CompiledQuery, QueryError> {
        let table = d.quote_identifier(&self.table)?;
        let mut params = Vec::new();
        let mut projections = Vec::with_capacity(self.projections.len());
        for item in &self.projections {
            match item {
                ProjectionItem::Column(column) => projections.push(d.quote_identifier(column)?),
                ProjectionItem::Aggregate { aggregate, alias } => {
                    let mut text = render_expr(&AnyExpr::from(aggregate.clone()), d, &mut params)?;
                    if let Some(alias) = alias {
                        text.push_str(" AS ");
                        text.push_str(&d.quote_identifier(alias)?);
                    }
                    projections.push(text);
                }
            }
        }
        let projection = if projections.is_empty() {
            "*".to_owned()
        } else {
            projections.join(", ")
        };
        let mut sql = format!("SELECT {projection} FROM {table}");
        for (kind, joined_table, on) in &self.joins {
            let keyword = match kind {
                JoinKind::Inner => "INNER JOIN",
                JoinKind::Left => "LEFT JOIN",
            };
            sql.push(' ');
            sql.push_str(keyword);
            sql.push(' ');
            sql.push_str(&d.quote_identifier(joined_table)?);
            sql.push_str(" ON ");
            render_predicate(on, d, &mut params, &mut sql)?;
        }
        if let Some(p) = &self.predicate {
            sql.push_str(" WHERE ");
            render_predicate_root(p, d, &mut params, &mut sql)?;
        }
        if !self.group_by.is_empty() {
            sql.push_str(" GROUP BY ");
            let names = self
                .group_by
                .iter()
                .map(|c| d.quote_identifier(c))
                .collect::<Result<Vec<_>, QueryError>>()?
                .join(", ");
            sql.push_str(&names);
        }
        if let Some(h) = &self.having {
            sql.push_str(" HAVING ");
            render_predicate_root(h, d, &mut params, &mut sql)?;
        }
        if let Some((column, order)) = &self.order {
            sql.push_str(" ORDER BY ");
            sql.push_str(&d.quote_identifier(column)?);
            sql.push_str(if matches!(order, Order::Asc) {
                " ASC"
            } else {
                " DESC"
            });
        }
        if let Some(limit) = self.limit {
            if limit < 0 {
                return Err(QueryError::NegativeLimit);
            }
            sql.push_str(" LIMIT ");
            sql.push_str(&limit.to_string());
        }
        if let Some(offset) = self.offset {
            if offset < 0 {
                return Err(QueryError::NegativeOffset);
            }
            if self.limit.is_none() && !d.supports_offset_without_limit() {
                // SQLite requires a LIMIT before OFFSET; -1 means unbounded.
                sql.push_str(" LIMIT ");
                sql.push_str(d.unbounded_limit());
            }
            sql.push_str(" OFFSET ");
            sql.push_str(&offset.to_string());
        }
        Ok(super::CompiledQuery { sql, params })
    }
}

#[derive(Debug, Clone)]
pub struct Insert {
    table: String,
    assignments: Vec<(String, SqliteValue)>,
}
impl Insert {
    pub fn into(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            assignments: Vec::new(),
        }
    }
    pub fn set<T: SqlType>(mut self, column: Column<T>, value: Value<T>) -> Self {
        self.assignments.push((column.name, value.value));
        self
    }
    pub(crate) fn compile_insert<D: Dialect>(
        &self,
        d: &D,
    ) -> Result<super::CompiledQuery, QueryError> {
        if self.assignments.is_empty() {
            return Err(QueryError::EmptyQuery("insert"));
        }
        let mut seen = std::collections::HashSet::new();
        let mut params = Vec::new();
        let mut cols = Vec::new();
        for (column, value) in &self.assignments {
            if !seen.insert(column) {
                return Err(QueryError::DuplicateColumn(column.clone()));
            }
            cols.push(d.quote_identifier(column)?);
            params.push(value.clone());
        }
        let placeholders = (1..=params.len())
            .map(|i| d.placeholder(i))
            .collect::<Vec<_>>()
            .join(", ");
        Ok(super::CompiledQuery {
            sql: format!(
                "INSERT INTO {} ({}) VALUES ({placeholders})",
                d.quote_identifier(&self.table)?,
                cols.join(", ")
            ),
            params,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Update {
    table: String,
    assignments: Vec<(String, SqliteValue)>,
    predicate: Option<Predicate>,
}
impl Update {
    pub fn table(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            assignments: Vec::new(),
            predicate: None,
        }
    }
    pub fn set<T: SqlType>(mut self, column: Column<T>, value: Value<T>) -> Self {
        self.assignments.push((column.name, value.value));
        self
    }
    pub fn where_(mut self, predicate: Predicate) -> Self {
        self.predicate = Some(combine_predicate(self.predicate.take(), predicate));
        self
    }
    pub(crate) fn compile_update<D: Dialect>(
        &self,
        d: &D,
    ) -> Result<super::CompiledQuery, QueryError> {
        if self.assignments.is_empty() {
            return Err(QueryError::MissingAssignments);
        }
        let mut params = Vec::new();
        let assignments = self
            .assignments
            .iter()
            .map(|(c, v)| {
                params.push(v.clone());
                Ok(format!(
                    "{} = {}",
                    d.quote_identifier(c)?,
                    d.placeholder(params.len())
                ))
            })
            .collect::<Result<Vec<_>, QueryError>>()?
            .join(", ");
        let mut sql = format!(
            "UPDATE {} SET {assignments}",
            d.quote_identifier(&self.table)?
        );
        if let Some(p) = &self.predicate {
            sql.push_str(" WHERE ");
            render_predicate_root(p, d, &mut params, &mut sql)?;
        } else {
            return Err(QueryError::MissingPredicate);
        }
        Ok(super::CompiledQuery { sql, params })
    }
}

#[derive(Debug, Clone)]
pub struct Delete {
    table: String,
    predicate: Option<Predicate>,
}
impl Delete {
    pub fn from(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            predicate: None,
        }
    }
    pub fn where_(mut self, predicate: Predicate) -> Self {
        self.predicate = Some(combine_predicate(self.predicate.take(), predicate));
        self
    }
    pub(crate) fn compile_delete<D: Dialect>(
        &self,
        d: &D,
    ) -> Result<super::CompiledQuery, QueryError> {
        let Some(p) = &self.predicate else {
            return Err(QueryError::MissingPredicate);
        };
        let mut params = Vec::new();
        let mut sql = format!("DELETE FROM {} WHERE ", d.quote_identifier(&self.table)?);
        render_predicate_root(p, d, &mut params, &mut sql)?;
        Ok(super::CompiledQuery { sql, params })
    }
}

pub trait Query {
    type Output;
    fn compile<D: Dialect>(&self, dialect: &D) -> Result<super::CompiledQuery, QueryError>;
}

fn render_expr<D: Dialect>(
    expr: &AnyExpr,
    d: &D,
    params: &mut Vec<SqliteValue>,
) -> Result<String, QueryError> {
    match expr {
        AnyExpr::Column(name) => d.quote_identifier(name),
        AnyExpr::Param(value) => {
            params.push(value.clone());
            Ok(d.placeholder(params.len()))
        }
        AnyExpr::Aggregate { function, argument } => {
            let inner = match argument.as_ref() {
                AggregateArgument::Star => "*".to_owned(),
                AggregateArgument::Expr(inner) => render_expr(inner, d, params)?,
            };
            Ok(format!("{function}({inner})"))
        }
    }
}
/// Renders a clause-level predicate.
///
/// Identical to `render_predicate` except that a top-level `AND`/`OR` chain
/// renders flat without enclosing parentheses, which keeps chained
/// `where_`/`having` calls readable; nested groups keep their parentheses.
fn render_predicate_root<D: Dialect>(
    predicate: &Predicate,
    d: &D,
    params: &mut Vec<SqliteValue>,
    sql: &mut String,
) -> Result<(), QueryError> {
    let items = match predicate {
        Predicate::And(items) | Predicate::Or(items) => Some(items),
        _ => None,
    };
    if let Some(items) = items {
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                sql.push_str(if matches!(predicate, Predicate::And(_)) {
                    " AND "
                } else {
                    " OR "
                });
            }
            render_predicate(item, d, params, sql)?;
        }
    } else {
        render_predicate(predicate, d, params, sql)?;
    }
    Ok(())
}
fn render_predicate<D: Dialect>(
    predicate: &Predicate,
    d: &D,
    params: &mut Vec<SqliteValue>,
    sql: &mut String,
) -> Result<(), QueryError> {
    match predicate {
        Predicate::Compare {
            operator,
            left,
            right,
        } => {
            sql.push_str(&render_expr(left, d, params)?);
            sql.push(' ');
            sql.push_str(operator);
            sql.push(' ');
            sql.push_str(&render_expr(right, d, params)?);
        }
        Predicate::InList { expr, values } => {
            if values.is_empty() {
                return Err(QueryError::InvalidParameter("empty IN list".into()));
            }
            sql.push_str(&render_expr(expr, d, params)?);
            sql.push_str(" IN (");
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    sql.push_str(", ");
                }
                params.push(value.clone());
                sql.push_str(&d.placeholder(params.len()));
            }
            sql.push(')');
        }
        Predicate::Like { expr, pattern } => {
            sql.push_str(&render_expr(expr, d, params)?);
            sql.push_str(" LIKE ");
            params.push(pattern.clone());
            sql.push_str(&d.placeholder(params.len()));
            sql.push_str(" ESCAPE ");
            params.push(SqliteValue::Text(LIKE_ESCAPE_CHARACTER.to_owned()));
            sql.push_str(&d.placeholder(params.len()));
        }
        Predicate::And(items) | Predicate::Or(items) => {
            if items.is_empty() {
                return Err(QueryError::InvalidParameter(
                    "empty boolean predicate".into(),
                ));
            }
            sql.push('(');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    sql.push_str(if matches!(predicate, Predicate::And(_)) {
                        " AND "
                    } else {
                        " OR "
                    });
                }
                render_predicate(item, d, params, sql)?;
            }
            sql.push(')');
        }
        Predicate::Not(item) => {
            sql.push_str("NOT (");
            render_predicate(item, d, params, sql)?;
            sql.push(')');
        }
    }
    Ok(())
}
