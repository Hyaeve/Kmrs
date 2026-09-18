//! Unified error type for komga-db.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    #[error(transparent)]
    Pool(#[from] r2d2::Error),
    #[error("invalid datetime in database: {0}")]
    Datetime(String),
    #[error("invalid enum value in database: {0}")]
    EnumValue(String),
    #[error("migration failed: {0}")]
    Migrate(#[from] crate::migrate::MigrateError),
}

pub type Result<T> = std::result::Result<T, Error>;
