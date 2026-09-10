use spectra_db::query::{
    Aggregate, Boolean, Column, Delete, Insert, Integer, JoinKind, Order, Predicate, Query, Real,
    Select, SqliteDialect, Text, Update, Value,
};
use spectra_db::query::{PostgresDialect, QueryError};
use spectra_db::sqlite::{open_pool, SqliteConnection, SqliteValue};
use spectra_db::PoolConfig;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Same coarse-tick collision-proofing as migrations_integration.rs: nanos
/// alone can repeat across parallel test threads sharing one database file.
static DATABASE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn database() -> SqliteConnection {
    let path = std::env::temp_dir().join(format!(
        "spectra-r2502-{}-{}-{}.sqlite",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::process::id(),
        DATABASE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let connection = SqliteConnection::open(&path, std::time::Duration::from_secs(1)).unwrap();
    connection.execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL, score REAL NOT NULL, active INTEGER NOT NULL);").unwrap();
    connection
}

#[test]
fn compiles_parameterized_crud_with_deterministic_placeholders() {
    let id = Column::<Integer>::new("id");
    let name = Column::<Text>::new("name");
    let score = Column::<Real>::new("score");
    let dialect = SqliteDialect;

    let insert = Insert::into("items")
        .set(id.clone(), Value::integer(7))
        .set(name.clone(), Value::text("O'Reilly"))
        .set(score.clone(), Value::real(9.5));
    let compiled = insert.compile(&dialect).unwrap();
    assert_eq!(
        compiled.sql,
        "INSERT INTO \"items\" (\"id\", \"name\", \"score\") VALUES (?1, ?2, ?3)"
    );
    assert_eq!(
        compiled.params,
        vec![
            SqliteValue::Integer(7),
            SqliteValue::Text("O'Reilly".into()),
            SqliteValue::Real(9.5)
        ]
    );

    let select = Select::from("items")
        .columns_named(&[id.reference(), name.reference(), score.reference()])
        .where_(id.equals(Value::integer(7)))
        .order_by(score, Order::Desc)
        .limit(1);
    assert_eq!(select.compile(&dialect).unwrap().sql, "SELECT \"id\", \"name\", \"score\" FROM \"items\" WHERE \"id\" = ?1 ORDER BY \"score\" DESC LIMIT 1");

    let update = Update::table("items")
        .set(name.clone(), Value::text("updated"))
        .where_(id.equals(Value::integer(7)));
    assert_eq!(
        update.compile(&dialect).unwrap().sql,
        "UPDATE \"items\" SET \"name\" = ?1 WHERE \"id\" = ?2"
    );
    let delete = Delete::from("items").where_(Predicate::eq(id.expr(), Value::integer(7).expr()));
    assert_eq!(
        delete.compile(&dialect).unwrap().sql,
        "DELETE FROM \"items\" WHERE \"id\" = ?1"
    );
}

#[test]
fn executes_real_sqlite_crud_and_preserves_parameters() {
    let connection = database();
    let id = Column::<Integer>::new("id");
    let name = Column::<Text>::new("name");
    let score = Column::<Real>::new("score");
    let active = Column::<Boolean>::new("active");
    let dialect = SqliteDialect;

    connection
        .execute_query(
            Insert::into("items")
                .set(id.clone(), Value::integer(1))
                .set(name.clone(), Value::text("first"))
                .set(score.clone(), Value::real(1.5))
                .set(active, Value::boolean(true))
                .compile(&dialect)
                .unwrap(),
        )
        .unwrap();
    let result = connection
        .execute_query(
            Select::from("items")
                .columns_named(&[id.reference(), name.reference(), score.reference()])
                .where_(id.equals(Value::integer(1)))
                .compile(&dialect)
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        result.rows,
        vec![vec![
            SqliteValue::Integer(1),
            SqliteValue::Text("first".into()),
            SqliteValue::Real(1.5)
        ]]
    );

    let updated = connection
        .execute_query(
            Update::table("items")
                .set(name.clone(), Value::text("changed"))
                .where_(id.equals(Value::integer(1)))
                .compile(&dialect)
                .unwrap(),
        )
        .unwrap();
    assert_eq!(updated.affected_rows, 1);
    let deleted = connection
        .execute_query(
            Delete::from("items")
                .where_(id.equals(Value::integer(1)))
                .compile(&dialect)
                .unwrap(),
        )
        .unwrap();
    assert_eq!(deleted.affected_rows, 1);
}

#[test]
fn rejects_invalid_identifiers_and_unscoped_writes() {
    let id = Column::<Integer>::new("id");
    let dialect = SqliteDialect;
    assert!(Select::from("").compile(&dialect).is_err());
    assert!(Update::table("items")
        .set(id.clone(), Value::integer(1))
        .compile(&dialect)
        .is_err());
    assert!(Delete::from("items").compile(&dialect).is_err());
    assert!(Select::from("items").limit(-1).compile(&dialect).is_err());
    assert!(Select::from("bad\0name").compile(&dialect).is_err());
}

#[test]
fn executes_compiled_queries_through_the_shared_pool() {
    let path = std::env::temp_dir().join(format!(
        "spectra-r2502-pool-{}.sqlite",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let seed = SqliteConnection::open(&path, std::time::Duration::from_secs(1)).unwrap();
    seed.execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL, score REAL NOT NULL, active INTEGER NOT NULL);").unwrap();
    let pool = open_pool(
        &path,
        PoolConfig {
            min_size: 1,
            max_size: 2,
            ..PoolConfig::default()
        },
    )
    .unwrap();
    let lease = pool.acquire_blocking().unwrap();
    let id = Column::<Integer>::new("id");
    let name = Column::<Text>::new("name");
    let score = Column::<Real>::new("score");
    let active = Column::<Boolean>::new("active");
    let inserted = lease
        .connection()
        .unwrap()
        .execute_query(
            Insert::into("items")
                .set(id.clone(), Value::integer(10))
                .set(name, Value::text("pooled"))
                .set(score, Value::real(3.0))
                .set(active, Value::boolean(true))
                .compile(&SqliteDialect)
                .unwrap(),
        )
        .unwrap();
    assert_eq!(inserted.affected_rows, 1);
    lease.release().unwrap();
    pool.shutdown().unwrap();
}

#[test]
fn compiles_join_group_by_having_in_deterministic_clause_order() {
    let pg = PostgresDialect;
    let lite = SqliteDialect;

    let status = Column::<Text>::new("orders.status");
    let order_customer = Column::<Integer>::new("orders.customer_id");
    let total = Column::<Real>::new("orders.total");
    let customer_id = Column::<Integer>::new("customers.id");
    let region = Column::<Text>::new("customers.region");

    let select = Select::from("orders")
        .columns(std::slice::from_ref(&status))
        .add_join(
            JoinKind::Inner,
            "customers",
            Predicate::eq(order_customer.expr(), customer_id.expr()),
        )
        .aggregate(Aggregate::count_all())
        .aliased_aggregate(Aggregate::sum(total.clone()), "total_revenue")
        .where_(status.not_equals(Value::text("draft")))
        .group_by(std::slice::from_ref(&region))
        .having(Aggregate::sum(total).ge(Value::real(250.0)))
        .order_by(region, Order::Desc)
        .limit(10)
        .offset(20);

    assert_eq!(
        select.compile(&lite).unwrap().sql,
        "SELECT \"orders\".\"status\", COUNT(*), SUM(\"orders\".\"total\") AS \"total_revenue\" \
         FROM \"orders\" INNER JOIN \"customers\" ON \"orders\".\"customer_id\" = \"customers\".\"id\" \
         WHERE \"orders\".\"status\" <> ?1 GROUP BY \"customers\".\"region\" \
         HAVING SUM(\"orders\".\"total\") >= ?2 ORDER BY \"customers\".\"region\" DESC \
         LIMIT 10 OFFSET 20"
    );
    assert_eq!(
        select.compile(&pg).unwrap().sql,
        "SELECT \"orders\".\"status\", COUNT(*), SUM(\"orders\".\"total\") AS \"total_revenue\" \
         FROM \"orders\" INNER JOIN \"customers\" ON \"orders\".\"customer_id\" = \"customers\".\"id\" \
         WHERE \"orders\".\"status\" <> $1 GROUP BY \"customers\".\"region\" \
         HAVING SUM(\"orders\".\"total\") >= $2 ORDER BY \"customers\".\"region\" DESC \
         LIMIT 10 OFFSET 20"
    );

    let compiled = select.compile(&pg).unwrap();
    assert_eq!(
        compiled.params,
        vec![
            SqliteValue::Text("draft".to_owned()),
            SqliteValue::Real(250.0),
        ]
    );
}

#[test]
fn compiles_left_joins_before_the_where_clause() {
    let user_id = Column::<Integer>::new("users.id");
    let profile_user = Column::<Integer>::new("profiles.user_id");
    let compiled = Select::from("users")
        .add_join(
            JoinKind::Left,
            "profiles",
            Predicate::eq(user_id.expr(), profile_user.expr()),
        )
        .compile(&PostgresDialect)
        .unwrap();
    assert_eq!(
        compiled.sql,
        "SELECT * FROM \"users\" LEFT JOIN \"profiles\" ON \"users\".\"id\" = \"profiles\".\"user_id\""
    );
    assert!(compiled.params.is_empty());
}

#[test]
fn expands_where_in_placeholders_deterministically_per_dialect() {
    let id = Column::<Integer>::new("items.id");
    let name = Column::<Text>::new("name");
    let values = [
        Value::integer(1),
        Value::integer(2),
        Value::integer(3),
        Value::integer(5),
    ];

    let select = Select::from("items")
        .where_(name.equals(Value::text("pinned")))
        .where_in(id.clone(), &values)
        .unwrap()
        .order_by(id.clone(), Order::Asc);

    let lite = select.compile(&SqliteDialect).unwrap();
    assert_eq!(
        lite.sql,
        "SELECT * FROM \"items\" WHERE \"name\" = ?1 AND \"items\".\"id\" IN (?2, ?3, ?4, ?5) ORDER BY \"items\".\"id\" ASC"
    );
    assert_eq!(
        lite.params,
        vec![
            SqliteValue::Text("pinned".to_owned()),
            SqliteValue::Integer(1),
            SqliteValue::Integer(2),
            SqliteValue::Integer(3),
            SqliteValue::Integer(5),
        ]
    );

    let pg = select.compile(&PostgresDialect).unwrap();
    assert_eq!(
        pg.sql,
        "SELECT * FROM \"items\" WHERE \"name\" = $1 AND \"items\".\"id\" IN ($2, $3, $4, $5) ORDER BY \"items\".\"id\" ASC"
    );
    assert_eq!(pg.params.len(), 5);
}

#[test]
fn rejects_empty_where_in_lists() {
    let id = Column::<Integer>::new("id");
    let result = Select::from("items").where_in(id, &[]);
    match result {
        Err(QueryError::InvalidParameter(message)) => {
            assert_eq!(message, "empty IN list");
        }
        other => panic!("expected empty IN list error, got {other:?}"),
    }
}

#[test]
fn binds_like_patterns_and_escape_characters_as_parameters() {
    let name = Column::<Text>::new("name");

    // Wildcard pattern kept verbatim; the escape character travels as a parameter.
    let wildcards = Select::from("items")
        .where_(name.clone().like("50%_off"))
        .compile(&SqliteDialect)
        .unwrap();
    assert_eq!(
        wildcards.sql,
        "SELECT * FROM \"items\" WHERE \"name\" LIKE ?1 ESCAPE ?2"
    );
    assert_eq!(
        wildcards.params,
        vec![
            SqliteValue::Text("50%_off".to_owned()),
            SqliteValue::Text("\\".to_owned()),
        ]
    );

    // Same query compiled against Postgres keeps ordered numbered placeholders.
    let pg_wildcards = Select::from("items")
        .where_(name.clone().like("%_x"))
        .compile(&PostgresDialect)
        .unwrap();
    assert_eq!(
        pg_wildcards.sql,
        "SELECT * FROM \"items\" WHERE \"name\" LIKE $1 ESCAPE $2"
    );

    // Exact matching pre-escapes %, _, and \ before binding.
    let exact = Predicate::like_exact(name.expr(), "50%_of\\f");
    let escaped_pattern = Select::from("items")
        .where_(exact)
        .compile(&PostgresDialect)
        .unwrap();
    assert_eq!(
        escaped_pattern.sql,
        "SELECT * FROM \"items\" WHERE \"name\" LIKE $1 ESCAPE $2"
    );
    assert_eq!(
        escaped_pattern.params[0],
        SqliteValue::Text("50\\%\\_of\\\\f".to_owned())
    );
}

#[test]
fn handles_order_by_limit_offset_edge_cases() {
    let score = Column::<Real>::new("score");

    // Negative limits are rejected everywhere.
    let negative = Select::from("items")
        .order_by(score.clone(), Order::Desc)
        .limit(-1);
    assert!(matches!(
        negative.compile(&SqliteDialect),
        Err(QueryError::NegativeLimit)
    ));

    // SQLite requires a LIMIT before OFFSET: an unbounded LIMIT is inserted.
    let offset_only_sqlite = Select::from("items")
        .offset(15)
        .compile(&SqliteDialect)
        .unwrap();
    assert_eq!(
        offset_only_sqlite.sql,
        "SELECT * FROM \"items\" LIMIT -1 OFFSET 15"
    );

    // Postgres accepts a bare OFFSET.
    let offset_only_pg = Select::from("items")
        .offset(15)
        .compile(&PostgresDialect)
        .unwrap();
    assert_eq!(offset_only_pg.sql, "SELECT * FROM \"items\" OFFSET 15");

    // Combined ORDER BY, LIMIT and OFFSET keep their mandatory ordering.
    let combined = Select::from("items")
        .order_by(score, Order::Desc)
        .offset(10)
        .limit(5);
    assert_eq!(
        combined.compile(&SqliteDialect).unwrap().sql,
        "SELECT * FROM \"items\" ORDER BY \"score\" DESC LIMIT 5 OFFSET 10"
    );
}
