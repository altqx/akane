use anyhow::{Context, Result};
use sqlx::{Sqlite, SqlitePool, migrate::MigrateDatabase};
use tracing::info;
use crate::types::{VideoDto, VideoQuery};
 
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
     entrypoint: String,
     created_at: String,
 }
 
 pub async fn count_videos(
     db_pool: &SqlitePool,
     filters: &VideoQuery,
 ) -> Result<i64> {
     let name = filters.name.as_ref().map(|s| s.to_lowercase());
     let tag = filters.tag.as_ref();
 
     let count = match (name.as_ref(), tag) {
         (None, None) => {
             sqlx::query_scalar::<_, i64>(
                 "SELECT COUNT(*) as count FROM videos",
             )
             .fetch_one(db_pool)
             .await?
         }
         (Some(name), None) => {
             let pattern = format!("%{}%", name);
             sqlx::query_scalar::<_, i64>(
                 "SELECT COUNT(*) as count FROM videos WHERE LOWER(name) LIKE ?",
             )
             .bind(pattern)
             .fetch_one(db_pool)
             .await?
         }
         (None, Some(tag)) => {
             let pattern = format!("%{}%", tag);
             sqlx::query_scalar::<_, i64>(
                 "SELECT COUNT(*) as count FROM videos WHERE tags LIKE ?",
             )
             .bind(pattern)
             .fetch_one(db_pool)
             .await?
         }
         (Some(name), Some(tag)) => {
             let name_pattern = format!("%{}%", name);
             let tag_pattern = format!("%{}%", tag);
             sqlx::query_scalar::<_, i64>(
                 "SELECT COUNT(*) as count FROM videos WHERE LOWER(name) LIKE ? AND tags LIKE ?",
             )
             .bind(name_pattern)
             .bind(tag_pattern)
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
             let pattern = format!("%{}%", name);
             sqlx::query_as::<_, VideoRow>(
                 "SELECT id, name, tags, available_resolutions, duration, thumbnail_key, entrypoint, created_at \
                  FROM videos \
                  WHERE LOWER(name) LIKE ? \
                  ORDER BY datetime(created_at) DESC \
                  LIMIT ? OFFSET ?",
             )
             .bind(pattern)
             .bind(limit)
             .bind(offset)
             .fetch_all(db_pool)
             .await?
         }
         (None, Some(tag)) => {
             let pattern = format!("%{}%", tag);
             sqlx::query_as::<_, VideoRow>(
                 "SELECT id, name, tags, available_resolutions, duration, thumbnail_key, entrypoint, created_at \
                  FROM videos \
                  WHERE tags LIKE ? \
                  ORDER BY datetime(created_at) DESC \
                  LIMIT ? OFFSET ?",
             )
             .bind(pattern)
             .bind(limit)
             .bind(offset)
             .fetch_all(db_pool)
             .await?
         }
         (Some(name), Some(tag)) => {
             let name_pattern = format!("%{}%", name);
             let tag_pattern = format!("%{}%", tag);
             sqlx::query_as::<_, VideoRow>(
                 "SELECT id, name, tags, available_resolutions, duration, thumbnail_key, entrypoint, created_at \
                  FROM videos \
                  WHERE LOWER(name) LIKE ? AND tags LIKE ? \
                  ORDER BY datetime(created_at) DESC \
                  LIMIT ? OFFSET ?",
             )
             .bind(name_pattern)
             .bind(tag_pattern)
             .bind(limit)
             .bind(offset)
             .fetch_all(db_pool)
             .await?
         }
     };
 
     let mut result = Vec::with_capacity(rows.len());
     for row in rows {
         let tags: Vec<String> = serde_json::from_str(&row.tags)
             .context("Failed to parse tags JSON from database")?;
         let resolutions: Vec<String> = serde_json::from_str(&row.available_resolutions)
             .context("Failed to parse available_resolutions JSON from database")?;
 
         let base = public_base_url.trim_end_matches('/');
         let thumbnail_url = format!("{}/{}", base, row.thumbnail_key);
         let playlist_url = format!("{}/{}", base, row.entrypoint);
 
         result.push(VideoDto {
             id: row.id,
             name: row.name,
             tags,
             available_resolutions: resolutions,
             duration: row.duration as u32,
             thumbnail_url,
             playlist_url,
             created_at: row.created_at,
         });
     }
 
     Ok(result)
 }
