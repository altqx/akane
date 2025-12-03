# Akane Copilot Instructions

## Project Overview

Akane is a self-hosted video streaming platform with two main components:
- **Rust backend** (`src/`): Axum web server handling video processing, HLS encoding, R2 storage, and analytics
- **Next.js admin UI** (`akane-webui/`): React 19 + Next.js 16 dashboard for video management

## Architecture

### Data Flow
1. Videos uploaded via chunked upload API → temp storage → FFmpeg HLS encoding → Cloudflare R2
2. Metadata stored in SQLite (`videos.db`), analytics in ClickHouse
3. Player page (`/player/{id}`) serves ArtPlayer with dynamically-included plugins based on content features

### Key State Management
- `AppState` (`src/types.rs`): Central state with S3 client, DB pool, progress tracking, semaphores
- `ffmpeg_semaphore`: Limits concurrent encodes (configurable via `max_concurrent_encodes`)
- `UploadContext` (`akane-webui/context/`): React context for upload queue state

## Development Commands

```bash
# Backend (from repo root)
cargo build --release
./target/release/akane  # Requires config.yml

# Frontend (from akane-webui/)
bun install
bun run dev  # Runs on port 3001, proxies API to :3000
bun run build  # Outputs to ../webui/ for static serving
```

## Code Patterns

### Backend Handler Pattern
Handlers in `src/handlers.rs` follow this structure:
```rust
pub async fn handler_name(
    State(state): State<AppState>,
    Path(id): Path<String>,  // or other extractors
) -> impl IntoResponse { ... }
```

### Protected vs Public Routes
- **Protected** (require `Authorization: Bearer {admin_password}`): `/api/upload`, `/api/videos`, `/api/queues`
- **Public**: `/api/videos/{id}/heartbeat`, `/api/analytics/*`, `/player/{id}`, `/hls/{id}/*`

### Video Processing Pipeline
1. `upload_video`/`upload_chunk` → temp file
2. `encode_to_hls` (in `src/video.rs`) → multi-resolution HLS with FFmpeg
3. `upload_hls_to_r2` (in `src/storage.rs`) → parallel upload to R2
4. `save_video` + optional `save_subtitle`/`save_chapter`/`save_attachment`

### Frontend Component Patterns
- Use DaisyUI 5 classes (`btn`, `card`, `badge`, etc.)
- Custom components wrap DaisyUI: `Button.tsx`, `Input.tsx`
- All pages are client components (`'use client'`) due to auth wrapper

## File Organization

| Path | Purpose |
|------|---------|
| `src/main.rs` | Route definitions, middleware setup |
| `src/handlers.rs` | All API handlers (~2200 lines) |
| `src/video.rs` | FFmpeg operations, metadata extraction |
| `src/database.rs` | SQLite queries with sqlx |
| `src/clickhouse.rs` | Analytics tracking |
| `migrations/` | SQLite schema migrations (auto-run on startup) |
| `akane-webui/app/` | Next.js App Router pages |
| `akane-webui/components/` | Reusable UI components |

## Configuration

Config loaded from `config.yml` (see `config.yml.example`):
- `video.encoder`: `libx264`, `h264_nvenc`, `h264_vaapi`, or `h264_qsv`
- `server.max_concurrent_encodes`: Semaphore limit for FFmpeg jobs
- `r2.public_base_url`: CDN URL for video delivery

## Testing & Deployment

- No test suite currently; test manually via admin UI
- Deploy: `cargo build --release`, then use `deploy.sh` (PM2-based)
- Static UI served from `webui/` directory by Rust backend at `/admin-webui`

## Common Tasks

**Adding a new API endpoint:**
1. Add handler in `src/handlers.rs`
2. Register route in `src/main.rs` (protected or public router)
3. Add types to `src/types.rs` if needed

**Adding a new DB field:**
1. Create migration in `migrations/` with timestamp prefix
2. Update relevant queries in `src/database.rs`
3. Update `VideoDto` or related types in `src/types.rs`

**Adding a frontend page:**
1. Create `akane-webui/app/{route}/page.tsx`
2. Add nav link in `Navbar.tsx`
