# Akane

A self-hosted video streaming platform with HLS encoding, Cloudflare R2 storage, and a modern admin dashboard.

## Features

- **Video Upload & Processing**: Upload videos with automatic HLS encoding at multiple resolutions (1080p, 720p, 480p, 360p)
- **Cloudflare R2 Storage**: Store video segments and thumbnails on R2 for fast, cost-effective delivery
- **Hardware Encoding Support**: NVIDIA (h264_nvenc), AMD/Intel VAAPI (h264_vaapi), Intel QuickSync (h264_qsv), or CPU (libx264)
- **Subtitle Support**: Extract and serve ASS/SSA/SRT subtitles from MKV files with libass rendering
- **Font Attachments**: Extract embedded fonts from MKV files for proper subtitle rendering
- **Chapter Support**: Extract and display video chapters from container metadata
- **Analytics**: Real-time viewer tracking with ClickHouse for historical analytics
- **Admin Dashboard**: Next.js 16 web UI for video management, uploads, and analytics
- **Chunked Uploads**: Support for large file uploads with progress tracking
- **Processing Queue**: Background video encoding with concurrent job limits

## Tech Stack

### Backend (Rust)
- **Axum** - Web framework
- **SQLx** - SQLite database for video metadata
- **ClickHouse** - Analytics storage for view tracking
- **AWS SDK** - R2/S3 compatible storage
- **FFmpeg** - Video encoding and metadata extraction

### Frontend (Next.js)
- **Next.js 16** with App Router
- **React 19**
- **Tailwind CSS 4** + DaisyUI 5
- **TypeScript**

## Prerequisites

- Rust (2024 edition)
- FFmpeg with encoding support
- Bun (for web UI)
- Cloudflare R2 bucket
- ClickHouse (optional, for analytics)

## Configuration

Copy `config.yml.example` to `config.yml` and configure:

```yaml
server:
  host: "0.0.0.0"
  port: 3000
  secret_key: "your-secret-key"
  admin_password: "your-admin-password"
  max_concurrent_encodes: 1
  max_concurrent_uploads: 30

r2:
  endpoint: "https://<accountid>.r2.cloudflarestorage.com"
  bucket: "your-bucket"
  access_key_id: "your-access-key"
  secret_access_key: "your-secret-key"
  public_base_url: "https://your-domain.com/"

video:
  encoder: "libx264"  # or h264_nvenc, h264_vaapi, h264_qsv

clickhouse:
  url: "http://localhost:8123"
  user: "default"
  password: ""
  database: "default"
```

## Installation

### Backend

```bash
# Build the Rust backend
cargo build --release

# Run the server
./target/release/akane
```

### Web UI

```bash
cd akane-webui

# Install dependencies
bun install

# Development
bun run dev

# Production build
bun run build
```

## API Endpoints

### Public
- `GET /player/{id}` - Embedded video player with libass subtitle rendering
- `GET /hls/{id}/{file}` - HLS segments and playlists
- `GET /api/videos/{id}/subtitles` - List available subtitles
- `GET /api/videos/{id}/subtitles/{track}` - Get subtitle file
- `GET /api/videos/{id}/attachments` - List font attachments
- `GET /api/videos/{id}/chapters` - Get video chapters
- `GET /api/analytics/realtime` - SSE stream for real-time viewers
- `GET /api/analytics/history` - Historical view data
- `GET /api/progress/{upload_id}` - Upload/encoding progress

### Protected (requires Bearer token)
- `POST /api/upload` - Upload video file
- `POST /api/upload/chunk` - Chunked upload
- `POST /api/upload/finalize` - Finalize chunked upload
- `GET /api/videos` - List videos with pagination/filtering
- `PUT /api/videos/{id}` - Update video metadata
- `DELETE /api/videos` - Delete videos
- `GET /api/queues` - List processing queue
- `DELETE /api/queues/{id}` - Cancel queued item

## Database

SQLite is used for video metadata with migrations in `migrations/`:
- Videos table with FTS5 search
- Subtitles and attachments metadata
- Chapters table