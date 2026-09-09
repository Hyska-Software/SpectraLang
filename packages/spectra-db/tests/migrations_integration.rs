use spectra_db::migrations::{discover, SqliteMigrator};
use spectra_db::sqlite::{SqliteConnection, SqliteStatement, SqliteValue, StepResult};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::sync::atomic::{AtomicU64, Ordering};

/// Process-wide fixture sequence. Wall-clock nanos alone can repeat within a
/// single coarse timer tick when tests run on parallel threads, handing two
/// fixtures the same directory and mixing their migration files (observed as
/// spurious DB2503_NAME_MISMATCH). pid + sequence makes collisions impossible.
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    migrations: PathBuf,
    database: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "spectra-r2503-{}-{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::process::id(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let migrations = root.join("migrations");
        fs::create_dir_all(&migrations).unwrap();
        let database = root.join("database.sqlite");
        Self {
            root,
            migrations,
            database,
        }
    }
    fn migration(&self, file: &str, sql: &str) {
        fs::write(self.migrations.join(file), sql).unwrap();
    }
    fn connection(&self) -> SqliteConnection {
        SqliteConnection::open(&self.database, Duration::from_millis(500)).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn seed(fixture: &Fixture) {
    fixture.migration(
        "0001_create_users.up.sql",
        "CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT NOT NULL);\r\n",
    );
    fixture.migration("0001_create_users.down.sql", "DROP TABLE users;\r\n");
    fixture.migration(
        "0002_add_email.up.sql",
        "ALTER TABLE users ADD COLUMN email TEXT;",
    );
    fixture.migration(
        "0002_add_email.down.sql",
        "ALTER TABLE users DROP COLUMN email;",
    );
}

fn scalar_text(connection: &SqliteConnection, sql: &str) -> String {
    let mut statement = SqliteStatement::prepare(connection.clone(), sql).unwrap();
    assert_eq!(statement.step().unwrap(), StepResult::Row);
    let value = statement.column_value(0).unwrap();
    statement.finalize().unwrap();
    match value {
        SqliteValue::Text(value) => value,
        other => panic!("unexpected value: {other:?}"),
    }
}

#[test]
fn discovers_normalizes_and_checksums_migrations_deterministically() {
    let fixture = Fixture::new();
    seed(&fixture);
    let migrations = discover(&fixture.migrations).unwrap();
    assert_eq!(
        migrations
            .iter()
            .map(|migration| migration.version)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(
        migrations[0].up_sql,
        "CREATE TABLE users(id INTEGER PRIMARY KEY, name TEXT NOT NULL);\n"
    );
    assert_eq!(
        migrations[0].checksum,
        discover(&fixture.migrations).unwrap()[0].checksum
    );
}

#[test]
fn applies_in_order_records_tracking_and_is_idempotent() {
    let fixture = Fixture::new();
    seed(&fixture);
    let migrator =
        SqliteMigrator::from_directory(fixture.connection(), &fixture.migrations).unwrap();
    assert_eq!(migrator.status().unwrap().pending.len(), 2);
    assert_eq!(migrator.migrate().unwrap().len(), 2);
    assert_eq!(migrator.migrate().unwrap().len(), 0);
    let status = migrator.status().unwrap();
    assert_eq!(status.applied.len(), 2);
    assert!(status.drift.is_empty());
    assert_eq!(
        scalar_text(
            &fixture.connection(),
            "SELECT name FROM sqlite_master WHERE type='table' AND name='users'"
        ),
        "users"
    );
}

#[test]
fn rollback_reverts_latest_versions_and_checksum_drift_blocks_changes() {
    let fixture = Fixture::new();
    seed(&fixture);
    let migrator =
        SqliteMigrator::from_directory(fixture.connection(), &fixture.migrations).unwrap();
    migrator.migrate().unwrap();
    assert_eq!(migrator.rollback(1).unwrap()[0].version, 2);
    assert_eq!(migrator.status().unwrap().applied.len(), 1);
    fs::write(
        fixture.migrations.join("0001_create_users.up.sql"),
        "CREATE TABLE changed(id INTEGER);\n",
    )
    .unwrap();
    let drift = SqliteMigrator::from_directory(fixture.connection(), &fixture.migrations)
        .unwrap()
        .migrate();
    assert_eq!(drift.unwrap_err().code, "DB2503_CHECKSUM_MISMATCH");
}

#[test]
fn failed_migration_is_atomic_and_invalid_pairs_are_rejected() {
    let fixture = Fixture::new();
    fixture.migration("0001_ok.up.sql", "CREATE TABLE ok(id INTEGER);");
    fixture.migration("0001_ok.down.sql", "DROP TABLE ok;");
    fixture.migration("0002_broken.up.sql", "THIS IS NOT SQL;");
    fixture.migration("0002_broken.down.sql", "DROP TABLE missing;");
    fixture.migration("0003_never.up.sql", "CREATE TABLE never(id INTEGER);");
    fixture.migration("0003_never.down.sql", "DROP TABLE never;");
    let migrator =
        SqliteMigrator::from_directory(fixture.connection(), &fixture.migrations).unwrap();
    assert!(migrator.migrate().is_err());
    let status = migrator.status().unwrap();
    assert_eq!(status.applied.len(), 1);
    assert!(status
        .pending
        .iter()
        .any(|migration| migration.version == 2));
    let invalid = Fixture::new();
    invalid.migration("0001_orphan.up.sql", "CREATE TABLE orphan(id INTEGER);");
    assert_eq!(
        discover(&invalid.migrations).unwrap_err().code,
        "DB2503_MISSING_PAIR"
    );
}

#[test]
fn missing_directory_is_rejected_before_database_mutation() {
    let fixture = Fixture::new();
    let missing = fixture.migrations.join("missing");
    let error = match SqliteMigrator::from_directory(fixture.connection(), &missing) {
        Ok(_) => panic!("missing migration directory was accepted"),
        Err(error) => error,
    };
    assert_eq!(error.code, "DB2503_DIRECTORY");
    assert!(fixture.database.exists());
    let probe = SqliteStatement::prepare(
        fixture.connection(),
        "SELECT COUNT(*) FROM _spectra_migrations",
    );
    assert!(probe.is_err());
}
