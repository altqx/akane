use crate::config::ClickHouseConfig;
use anyhow::{Context, Result};
use clickhouse::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::info;

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct ViewRow {
    pub video_id: String,
    pub ip_address: String,
    pub user_agent: String,
    pub created_at: u32, // DateTime in ClickHouse is stored as u32 Unix timestamp
}

pub fn initialize_client(config: &ClickHouseConfig) -> Client {
    Client::default()
        .with_url(&config.url)
        .with_user(&config.user)
        .with_password(&config.password)
        .with_database(&config.database)
}

pub async fn create_schema(client: &Client, config: &ClickHouseConfig) -> Result<()> {
    info!("Initializing ClickHouse schema...");

    let temp_client = Client::default()
        .with_url(&config.url)
        .with_user(&config.user)
        .with_password(&config.password);
    
    temp_client
        .query(&format!("CREATE DATABASE IF NOT EXISTS `{}`", config.database))
        .execute()
        .await
        .context("Failed to create database in ClickHouse")?;
    
    info!("ClickHouse database `{}` ready", config.database);

    // Create views table
    client
        .query(
            "CREATE TABLE IF NOT EXISTS views (
                video_id String,
                ip_address String,
                user_agent String,
                created_at DateTime
            ) ENGINE = MergeTree()
            ORDER BY (video_id, created_at)",
        )
        .execute()
        .await
        .context("Failed to create views table in ClickHouse")?;

    info!("ClickHouse schema initialized successfully");
    Ok(())
}

pub async fn insert_view(
    client: &Client,
    video_id: &str,
    ip: &str,
    user_agent: &str,
) -> Result<()> {
    let row = ViewRow {
        video_id: video_id.to_string(),
        ip_address: ip.to_string(),
        user_agent: user_agent.to_string(),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as u32,
    };

    let mut insert = client.insert("views")?;
    insert.write(&row).await?;
    insert
        .end()
        .await
        .context("Failed to insert view into ClickHouse")?;

    Ok(())
}

#[derive(Debug, Deserialize, clickhouse::Row)]
pub struct ViewCount {
    pub video_id: String,
    pub count: u64,
}

pub async fn get_view_counts(
    client: &Client,
    video_ids: &[String],
) -> Result<HashMap<String, i64>> {
    if video_ids.is_empty() {
        return Ok(HashMap::new());
    }

    // Build the IN clause with placeholders for each video_id
    let placeholders: Vec<String> = video_ids.iter().map(|id| format!("'{}'", id.replace('\''  , "''"))).collect();
    let query = format!(
        "SELECT video_id, count(*) as count FROM views WHERE video_id IN ({}) GROUP BY video_id",
        placeholders.join(", ")
    );

    let mut cursor = client.query(&query).fetch::<ViewCount>()?;

    let mut counts = HashMap::new();

    while let Some(row) = cursor.next().await? {
        counts.insert(row.video_id, row.count as i64);
    }

    Ok(counts)
}

#[derive(Debug, Deserialize, clickhouse::Row, Serialize)]
pub struct HistoryItem {
    pub date: String,
    pub count: u64,
}

pub async fn get_analytics_history(client: &Client) -> Result<Vec<HistoryItem>> {
    let query = "
        SELECT 
            formatDateTime(created_at, '%Y-%m-%d') as date, 
            count(*) as count 
        FROM views 
        GROUP BY date 
        ORDER BY date DESC 
        LIMIT 30
    ";

    let mut cursor = client.query(query).fetch::<HistoryItem>()?;
    let mut history = Vec::new();

    while let Some(row) = cursor.next().await? {
        history.push(row);
    }

    Ok(history)
}
