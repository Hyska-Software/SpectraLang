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

#[derive(Debug, Clone)]
pub enum Predicate {
    Compare {
        operator: &'static str,
        left: Box<AnyExpr>,
        right: Box<AnyExpr>,
    },
    And(Vec<Predicate>),
    Or(Vec<Predicate>),
    Not(Box<Predicate>),
}
#[derive(Debug, Clone)]
pub enum AnyExpr {
    Column(String),
    Param(SqliteValue),
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

#[derive(Debug, Clone)]
pub struct Select {
    table: String,
    columns: Vec<String>,
    predicate: Option<Predicate>,
    order: Option<(String, Order)>,
    limit: Option<i64>,
    offset: Option<i64>,
}
impl Select {
    pub fn from(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            columns: Vec::new(),
            predicate: None,
            order: None,
            limit: None,
            offset: None,
        }
    }
    pub fn columns<T: SqlType>(mut self, columns: &[Column<T>]) -> Self {
        self.columns.extend(columns.iter().map(|c| c.name.clone()));
        self
    }
    pub fn columns_named(mut self, columns: &[ColumnRef]) -> Self {
        self.columns.extend(columns.iter().map(|c| c.0.clone()));
        self
    }
    pub fn where_(mut self, predicate: Predicate) -> Self {
        self.predicate = Some(predicate);
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
        let projection = if self.columns.is_empty() {
            "*".to_owned()
        } else {
            self.columns
                .iter()
                .map(|c| d.quote_identifier(c))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ")
        };
        let mut params = Vec::new();
        let mut sql = format!("SELECT {projection} FROM {table}");
        if let Some(p) = &self.predicate {
            sql.push_str(" WHERE ");
            render_predicate(p, d, &mut params, &mut sql)?;
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
        self.predicate = Some(predicate);
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
            render_predicate(p, d, &mut params, &mut sql)?;
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
        self.predicate = Some(predicate);
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
        render_predicate(p, d, &mut params, &mut sql)?;
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
    }
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
