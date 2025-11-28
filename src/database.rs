use crate::types::{VideoDto, VideoQuery};
use anyhow::{Context, Result};
use sqlx::{Sqlite, SqlitePool, migrate::MigrateDatabase};
use std::collections::HashMap;
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

#[derive(sqlx::FromRow)]
struct VideoRow {
    id: String,
    name: String,
    tags: String,
    available_resolutions: String,
    duration: i64,
    thumbnail_key: String,
    created_at: String,
}

pub async fn count_videos(db_pool: &SqlitePool, filters: &VideoQuery) -> Result<i64> {
    let name = filters.name.as_ref().map(|s| s.to_lowercase());
    let tag = filters.tag.as_ref();

    let count = match (name.as_ref(), tag) {
        (None, None) => {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) as count FROM videos")
                .fetch_one(db_pool)
                .await?
        }
        (Some(name), None) => {
            let safe_name = name.replace("\"", "");
            let pattern = format!("name:\"{}\"*", safe_name);
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) as count FROM videos_fts WHERE videos_fts MATCH ?",
            )
            .bind(pattern)
            .fetch_one(db_pool)
            .await?
        }
        (None, Some(tag)) => {
            let safe_tag = tag.replace("\"", "");
            let pattern = format!("tags:\"{}\"", safe_tag);
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) as count FROM videos_fts WHERE videos_fts MATCH ?")
                .bind(pattern)
                .fetch_one(db_pool)
                .await?
        }
        (Some(name), Some(tag)) => {
            let safe_name = name.replace("\"", "");
            let safe_tag = tag.replace("\"", "");
            let pattern = format!("name:\"{}\"* AND tags:\"{}\"", safe_name, safe_tag);
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) as count FROM videos_fts WHERE videos_fts MATCH ?",
            )
            .bind(pattern)
            .fetch_one(db_pool)
            .await?
        }
    };

    Ok(count)
}

pub async fn list_videos(
    db_pool: &SqlitePool,
    filters: &VideoQuery,
    page: u32,
    page_size: u32,
    public_base_url: &str,
    view_counts: &HashMap<String, i64>,
) -> Result<Vec<VideoDto>> {
    let page = if page == 0 { 1 } else { page };
    let page_size = page_size.clamp(1, 100);

    let limit = page_size as i64;
    let offset = ((page - 1) * page_size) as i64;

    let name = filters.name.as_ref().map(|s| s.to_lowercase());
    let tag = filters.tag.as_ref();

    let rows: Vec<VideoRow> = match (name.as_ref(), tag) {
         (None, None) => {
             sqlx::query_as::<_, VideoRow>(
                 "SELECT id, name, tags, available_resolutions, duration, thumbnail_key, entrypoint, created_at \
                  FROM videos \
                  ORDER BY datetime(created_at) DESC \
                  LIMIT ? OFFSET ?",
             )
             .bind(limit)
             .bind(offset)
             .fetch_all(db_pool)
             .await?
         }
         (Some(name), None) => {
             let safe_name = name.replace("\"", "");
             let pattern = format!("name:\"{}\"*", safe_name);
             sqlx::query_as::<_, VideoRow>(
                 "SELECT v.id, v.name, v.tags, v.available_resolutions, v.duration, v.thumbnail_key, v.entrypoint, v.created_at \
                  FROM videos v \
                  JOIN videos_fts f ON v.id = f.id \
                  WHERE f.videos_fts MATCH ? \
                  ORDER BY datetime(v.created_at) DESC \
                  LIMIT ? OFFSET ?",
             )
             .bind(pattern)
             .bind(limit)
             .bind(offset)
             .fetch_all(db_pool)
             .await?
         }
         (None, Some(tag)) => {
             let safe_tag = tag.replace("\"", "");
             let pattern = format!("tags:\"{}\"", safe_tag);
             sqlx::query_as::<_, VideoRow>(
                 "SELECT v.id, v.name, v.tags, v.available_resolutions, v.duration, v.thumbnail_key, v.entrypoint, v.created_at \
                  FROM videos v \
                  JOIN videos_fts f ON v.id = f.id \
                  WHERE f.videos_fts MATCH ? \
                  ORDER BY datetime(v.created_at) DESC \
                  LIMIT ? OFFSET ?",
             )
             .bind(pattern)
             .bind(limit)
             .bind(offset)
             .fetch_all(db_pool)
             .await?
         }
         (Some(name), Some(tag)) => {
             let safe_name = name.replace("\"", "");
             let safe_tag = tag.replace("\"", "");
             let pattern = format!("name:\"{}\"* AND tags:\"{}\"", safe_name, safe_tag);
             sqlx::query_as::<_, VideoRow>(
                 "SELECT v.id, v.name, v.tags, v.available_resolutions, v.duration, v.thumbnail_key, v.entrypoint, v.created_at \
                  FROM videos v \
                  JOIN videos_fts f ON v.id = f.id \
                  WHERE f.videos_fts MATCH ? \
                  ORDER BY datetime(v.created_at) DESC \
                  LIMIT ? OFFSET ?",
             )
             .bind(pattern)
             .bind(limit)
             .bind(offset)
             .fetch_all(db_pool)
             .await?
         }
     };

    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let tags: Vec<String> =
            serde_json::from_str(&row.tags).context("Failed to parse tags JSON from database")?;
        let resolutions: Vec<String> = serde_json::from_str(&row.available_resolutions)
            .context("Failed to parse available_resolutions JSON from database")?;

        let base = public_base_url.trim_end_matches('/');
        let thumbnail_url = format!("{}/{}", base, row.thumbnail_key);
        // Return player URL instead of direct HLS URL
        let player_url = format!("/player/{}", row.id);

        let view_count = *view_counts.get(&row.id).unwrap_or(&0);

        result.push(VideoDto {
            id: row.id,
            name: row.name,
            tags,
            available_resolutions: resolutions,
            duration: row.duration as u32,
            thumbnail_url,
            player_url,
            view_count,
            created_at: row.created_at,
        });
    }

    Ok(result)
}

#[derive(sqlx::FromRow, serde::Serialize)]
pub struct VideoSummary {
    pub id: String,
    pub name: String,
    #[sqlx(default)]
    pub view_count: i64,
    pub created_at: String,
    pub thumbnail_key: String,
}

pub async fn get_all_videos_summary(
    db_pool: &SqlitePool,
    view_counts: &HashMap<String, i64>,
    limit: Option<i64>,
) -> Result<Vec<VideoSummary>> {
    let query = if let Some(l) = limit {
        format!("SELECT id, name, created_at, thumbnail_key \
         FROM videos \
         ORDER BY datetime(created_at) DESC \
         LIMIT {}", l)
    } else {
        "SELECT id, name, created_at, thumbnail_key \
         FROM videos \
         ORDER BY datetime(created_at) DESC".to_string()
    };

    let rows = sqlx::query_as::<_, VideoSummary>(&query)
    .fetch_all(db_pool)
    .await?;

    // Update view counts from ClickHouse data
    let rows = rows
        .into_iter()
        .map(|mut row| {
            if let Some(&count) = view_counts.get(&row.id) {
                row.view_count = count;
            }
            row
        })
        .collect();

    Ok(rows)
}
