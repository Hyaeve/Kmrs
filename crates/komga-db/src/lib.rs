//! Persistence layer: Flyway-compatible migrator, connection pool, UDF/collation, DAOs.

pub mod dao;
pub mod error;
mod java_migrations;
pub mod migrate;
pub mod pool;
pub mod search_sql;
pub mod udf;

pub use error::{Error, Result};
pub use migrate::{Migrator, Placeholders};

/// Migration list for the main database (database.sqlite): 86 SQL + 5 Java ports.
pub fn main_migrations() -> Vec<migrate::Migration> {
    let sql: &[migrate::SqlMigration] = include!(concat!(env!("OUT_DIR"), "/migrations_main.rs"));
    let mut migrations: Vec<migrate::Migration> = sql
        .iter()
        .map(|m| {
            migrate::Migration::Sql(migrate::SqlMigration {
                file_name: m.file_name,
                sql: m.sql,
            })
        })
        .collect();
    migrations.extend(java_migrations::java_migrations());
    migrations
}

/// Migration list for the tasks database (tasks.sqlite).
pub fn tasks_migrations() -> Vec<migrate::Migration> {
    let sql: &[migrate::SqlMigration] = include!(concat!(env!("OUT_DIR"), "/migrations_tasks.rs"));
    sql.iter()
        .map(|m| {
            migrate::Migration::Sql(migrate::SqlMigration {
                file_name: m.file_name,
                sql: m.sql,
            })
        })
        .collect()
}
