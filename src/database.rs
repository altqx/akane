use anyhow::{Context, Result};
use sqlx::{Sqlite, SqlitePool, migrate::MigrateDatabase};
use tracing::info;

pub async fn initialize_database(database_url: &str) -> Result<SqlitePool> {
    if !Sqlite::database_exists(database_url).await.unwrap_or(false) {
        info!("Creating database: {}", database_url);
        Sqlite::create_database(database_url)
            .await
            .context("Failed to create database")?;
    }

    let db_pool = SqlitePool::connect(database_url)
        .await
        .context("Failed to connect to database")?;

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&db_pool)
        .await
        .context("Failed to run migrations")?;

    info!("Database initialized successfully");

    Ok(db_pool)
}

pub async fn save_video(
    db_pool: &SqlitePool,
    video_id: &str,
    video_name: &str,
    tags: &[String],
    available_resolutions: &[String],
    duration: u32,
    thumbnail_key: &str,
    entrypoint: &str,
) -> Result<()> {
    let tags_json = serde_json::to_string(tags)?;
    let resolutions_json = serde_json::to_string(available_resolutions)?;

    sqlx
        ::query(
            "INSERT INTO videos (id, name, tags, available_resolutions, duration, thumbnail_key, entrypoint) VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(video_id)
        .bind(video_name)
        .bind(&tags_json)
        .bind(&resolutions_json)
        .bind(duration as i64)
        .bind(thumbnail_key)
        .bind(entrypoint)
        .execute(db_pool).await?;

    info!(
        "Video saved to database: id={}, name={}",
        video_id, video_name
    );

    Ok(())
}
