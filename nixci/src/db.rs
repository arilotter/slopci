use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;
use std::str::FromStr;

pub async fn setup(database_url: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(database_url)?
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;

    migrate(&pool).await?;

    Ok(pool)
}

async fn migrate(pool: &SqlitePool) -> Result<()> {
    let sql = include_str!("../migrations/001_initial.sql");
    sqlx::raw_sql(sql).execute(pool).await?;
    Ok(())
}
