use crate::database::{get_attachments_for_video, get_chapters_for_video, get_subtitles_for_video};
use crate::handlers::common::{generate_token, internal_err, minify_js, verify_token};
use crate::types::AppState;

use axum::{
    body::Body,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use futures::StreamExt;
use std::net::SocketAddr;

pub async fn get_player(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Use the same IP extraction logic as get_hls_file for token consistency
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|xff| xff.split(',').next().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| addr.ip().to_string());
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Generate token (view is now tracked on first play, not page load)
    let token = generate_token(&id, &state.config.server.secret_key, &ip, user_agent);

    // Fetch all content data server-side to generate optimized JS
    let subtitles = get_subtitles_for_video(&state.db_pool, &id)
        .await
        .unwrap_or_default();
    let attachments = get_attachments_for_video(&state.db_pool, &id)
        .await
        .unwrap_or_default();
    let chapters = get_chapters_for_video(&state.db_pool, &id)
        .await
        .unwrap_or_default();

    let has_subtitles = !subtitles.is_empty();
    let has_fonts = !attachments.is_empty();
    let has_chapters = !chapters.is_empty();

    // Build subtitle data for JavaScript
    let subtitle_js = if has_subtitles {
        let subtitle_config: Vec<String> = subtitles
            .iter()
            .map(|sub| {
                let name = sub
                    .title
                    .clone()
                    .or_else(|| sub.language.clone())
                    .unwrap_or_else(|| format!("Track {}", sub.track_index));
                let escaped_name =
                    serde_json::to_string(&name).unwrap_or_else(|_| r#""""#.to_string());
                let ext = match sub.codec.as_str() {
                    "ass" | "ssa" => "ass",
                    "subrip" | "srt" => "srt",
                    "hdmv_pgs_subtitle" | "pgssub" => "sup",
                    "dvd_subtitle" | "dvdsub" => "sub",
                    _ => "ass",
                };
                let codec_escaped = serde_json::to_string(&sub.codec).unwrap_or_else(|_| r#""""#.to_string());
                format!(
                    r#"{{ name: {}, url: "/api/videos/{}/subtitles/{}.{}", codec: {}, default: {} }}"#,
                    escaped_name, id, sub.track_index, ext, codec_escaped, sub.is_default
                )
            })
            .collect();
        format!("const subtitles = [{}];", subtitle_config.join(", "))
    } else {
        "const subtitles = [];".to_string()
    };

    // Build font URLs for JASSUB (only if fonts exist)
    let fonts_js = if has_fonts {
        let font_urls: Vec<String> = attachments
            .iter()
            .map(|att| format!("/api/videos/{}/attachments/{}", id, att.filename))
            .collect();
        let json = serde_json::to_string(&font_urls).unwrap_or_else(|_| "[]".to_string());
        format!("const fonts = {};", json)
    } else {
        "const fonts = [];".to_string()
    };

    // Build chapters array (only if chapters exist)
    let chapters_js = if has_chapters {
        let chapter_config: Vec<String> = chapters
            .iter()
            .filter(|ch| ch.start_time >= 0.0 && ch.end_time > ch.start_time)
            .map(|ch| {
                format!(
                    r#"{{ start: {}, end: {}, title: {} }}"#,
                    ch.start_time,
                    ch.end_time,
                    serde_json::to_string(&ch.title).unwrap_or_else(|_| r#""""#.to_string())
                )
            })
            .collect();
        if chapter_config.is_empty() {
            "const chapters = [];".to_string()
        } else {
            format!("const chapters = [{}];", chapter_config.join(", "))
        }
    } else {
        "const chapters = [];".to_string()
    };

    let js_code = format!(
        r#"
        const videoId = '{video_id}';
        {subtitle_js}
        {fonts_js}
        {chapters_js}
        const thumbnailUrl = '/hls/{video_id}/thumbnail.jpg';
        const spriteUrl = '/hls/{video_id}/sprites.jpg';
        const spriteColumns = 10;
        const spriteRows = 10;
        const spriteWidth = 160;
        const spriteHeight = 90;
        let spriteInterval = 10;
        
        let player = null;
        let video = null;
        let jassub = null;
        let bitmapRenderer = null;
        let viewTracked = false;
        let heartbeatStarted = false;
        let currentSubtitle = null;
        let subtitlesEnabled = true;

        // Subtitle type detection
        function needsJassub(codec) {{
            return ['ass','ssa','subrip','srt'].includes(codec?.toLowerCase());
        }}
        function isPgsSubtitle(codec) {{
            return ['hdmv_pgs_subtitle','pgssub'].includes(codec?.toLowerCase());
        }}
        function isVobSubSubtitle(codec) {{
            return ['dvd_subtitle','dvdsub'].includes(codec?.toLowerCase());
        }}

        // Format time as HH:MM:SS or MM:SS
        function formatTime(seconds) {{
            if (!isFinite(seconds)) return '0:00';
            const h = Math.floor(seconds / 3600);
            const m = Math.floor((seconds % 3600) / 60);
            const s = Math.floor(seconds % 60);
            if (h > 0) return h + ':' + m.toString().padStart(2,'0') + ':' + s.toString().padStart(2,'0');
            return m + ':' + s.toString().padStart(2,'0');
        }}

        async function init() {{
            video = document.getElementById('video');
            const playBtn = document.getElementById('playBtn');
            const progress = document.getElementById('progress');
            const progressBar = document.getElementById('progressBar');
            const buffered = document.getElementById('buffered');
            const currentTimeEl = document.getElementById('currentTime');
            const durationEl = document.getElementById('duration');
            const volumeSlider = document.getElementById('volumeSlider');
            const volumeBtn = document.getElementById('volumeBtn');
            const fullscreenBtn = document.getElementById('fullscreenBtn');
            const qualityBtn = document.getElementById('qualityBtn');
            const qualityMenu = document.getElementById('qualityMenu');
            const audioBtn = document.getElementById('audioBtn');
            const audioMenu = document.getElementById('audioMenu');
            const subtitleBtn = document.getElementById('subtitleBtn');
            const subtitleMenu = document.getElementById('subtitleMenu');
            const controls = document.getElementById('controls');
            const container = document.getElementById('container');
            const loading = document.getElementById('loading');
            const scrubHandle = document.getElementById('scrubHandle');
            const preview = document.getElementById('preview');
            const previewImage = document.getElementById('previewImage');
            const previewTime = document.getElementById('previewTime');

            if (thumbnailUrl && video && container) {{
                video.poster = thumbnailUrl;
                container.style.backgroundImage = `url(${{thumbnailUrl}})`;
                container.classList.add('has-thumb');
            }}

            const setLoading = (isLoading) => {{
                if (!loading) return;
                if (isLoading) {{
                    loading.classList.remove('hide');
                }} else {{
                    loading.classList.add('hide');
                }}
            }};

            const updateSpriteInterval = () => {{
                if (!video || !isFinite(video.duration) || video.duration <= 0) return;
                const frames = spriteColumns * spriteRows;
                spriteInterval = Math.max(video.duration / frames, 0.1);
            }};

            const hidePreview = () => {{
                if (preview) preview.classList.remove('show');
            }};

            const showPreview = (clientX) => {{
                if (!spriteUrl || !preview || !previewImage || !previewTime || !progress || !video || !isFinite(video.duration)) return;
                const rect = progress.getBoundingClientRect();
                const pct = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
                const time = pct * (video.duration || 0);
                const frames = spriteColumns * spriteRows;
                const interval = spriteInterval > 0 ? spriteInterval : Math.max((video.duration || 0) / frames, 0.1);
                const idx = Math.min(frames - 1, Math.floor(time / interval));
                const col = idx % spriteColumns;
                const row = Math.floor(idx / spriteColumns);

                previewImage.style.backgroundImage = 'url(' + spriteUrl + ')';
                previewImage.style.backgroundSize = (spriteColumns * spriteWidth) + 'px ' + (spriteRows * spriteHeight) + 'px';
                previewImage.style.backgroundPosition = '-' + (col * spriteWidth) + 'px -' + (row * spriteHeight) + 'px';
                previewTime.textContent = formatTime(time);

                const minX = 60;
                const maxX = rect.width - 60;
                const x = Math.min(maxX, Math.max(minX, clientX - rect.left));
                preview.style.left = x + 'px';
                preview.classList.add('show');
            }};

            const updateBufferedBar = () => {{
                if (!buffered || !video) return;
                const duration = video.duration || 0;
                if (!duration) {{
                    buffered.style.width = '0%';
                    return;
                }}

                let end = 0;

                // Prefer Shaka's buffered info when available
                if (player && typeof player.getBufferedInfo === 'function') {{
                    const info = player.getBufferedInfo();
                    const shakaRanges = info && info.total;
                    if (shakaRanges && shakaRanges.length) {{
                        end = shakaRanges[shakaRanges.length - 1].end;
                    }}
                }}

                // Fallback to media element buffered ranges
                if (!end) {{
                    const ranges = video.buffered;
                    if (ranges && typeof ranges.length === 'number' && ranges.length > 0) {{
                        end = ranges.end(ranges.length - 1);
                    }}
                }}

                const pct = Math.max(0, Math.min(1, end / duration)) * 100;
                buffered.style.width = pct + '%';
            }};

            // Initialize Shaka Player
            shaka.polyfill.installAll();
            if (!shaka.Player.isBrowserSupported()) {{
                console.error('Shaka Player not supported');
                return;
            }}
            player = new shaka.Player();
            await player.attach(video);
            player.configure({{
                streaming: {{
                    bufferingGoal: 30,
                    rebufferingGoal: 2,
                    bufferBehind: 30
                }}
            }});

            // Load HLS stream
            try {{
                await player.load('/hls/{video_id}/index.m3u8');
                setLoading(false);
                updateBufferedBar();
            }} catch (e) {{
                console.error('Failed to load video:', e);
            }}

            // Shaka buffering events keep spinner in sync with buffering state
            player.addEventListener('buffering', (event) => {{
                setLoading(!!event.buffering);
                updateBufferedBar();
            }});

            const updateOrientation = () => {{
                if (!video.videoWidth || !video.videoHeight) return;
                const portrait = video.videoHeight > video.videoWidth;
                if (portrait) {{
                    container.dataset.orientation = 'portrait';
                }} else {{
                    container.dataset.orientation = 'landscape';
                }}
            }};

            video.onloadedmetadata = () => {{
                updateOrientation();
                updateSpriteInterval();
            }};
            window.addEventListener('resize', updateOrientation);
            document.onfullscreenchange = () => {{
                updateOrientation();
                if (document.fullscreenElement) {{
                    fullscreenBtn.innerHTML = exitFsIcon;
                }} else {{
                    fullscreenBtn.innerHTML = fsIcon;
                }}
            }};

            // Initialize subtitle if available
            if (subtitles.length > 0) {{
                const defaultSub = subtitles.find(s => s.default) || subtitles[0];
                await loadSubtitle(defaultSub);
                buildSubtitleMenu();
            }}

            // Build quality menu
            buildQualityMenu();
            buildAudioMenu();

            // Play/Pause
            playBtn.onclick = togglePlay;
            video.onclick = togglePlay;

            // Double-tap (mobile) seek +/-5s
            let lastTapTime = 0;
            let suppressClick = false;
            video.addEventListener('touchend', (e) => {{
                const now = Date.now();
                const delta = now - lastTapTime;
                lastTapTime = now;
                if (delta > 350) return; // not a double tap

                const touch = e.changedTouches[0];
                const rect = video.getBoundingClientRect();
                const mid = rect.left + rect.width / 2;
                if (touch.clientX < mid) {{
                    video.currentTime = Math.max(0, video.currentTime - 5);
                }} else {{
                    video.currentTime = Math.min(video.duration || video.currentTime + 5, video.currentTime + 5);
                }}
                suppressClick = true;
                setTimeout(() => suppressClick = false, 300);
                e.preventDefault();
            }});

            // Double-click (desktop) toggles fullscreen
            video.ondblclick = (e) => {{
                e.preventDefault();
                fullscreenBtn.click();
            }};

            function togglePlay() {{
                if (suppressClick) return;
                if (video.paused) {{
                    video.play();
                }} else {{
                    video.pause();
                }}
            }}

            video.onplay = () => {{
                playBtn.innerHTML = pauseIcon;
                if (!viewTracked) {{
                    viewTracked = true;
                    fetch('/api/videos/{video_id}/view', {{ method: 'POST' }});
                }}
                if (!heartbeatStarted) {{
                    heartbeatStarted = true;
                    startHeartbeat();
                }}
                setLoading(false);
            }};
            video.onpause = () => {{ playBtn.innerHTML = playIcon; }};
            video.onwaiting = () => setLoading(true);
            video.onplaying = () => setLoading(false);
            video.oncanplay = () => setLoading(false);

            // Progress bar
            video.ontimeupdate = () => {{
                if (video.duration) {{
                    const pct = (video.currentTime / video.duration) * 100;
                    progressBar.style.width = pct + '%';
                    if (scrubHandle) scrubHandle.style.left = pct + '%';
                    currentTimeEl.textContent = formatTime(video.currentTime);
                }}
                updateBufferedBar();
            }};
            video.ondurationchange = () => {{
                durationEl.textContent = formatTime(video.duration);
                updateBufferedBar();
                updateSpriteInterval();
            }};
            video.onprogress = updateBufferedBar;
            progress.onmousemove = (e) => showPreview(e.clientX);
            progress.onmouseleave = hidePreview;
            progress.ontouchmove = (e) => {{
                if (e.touches && e.touches[0]) showPreview(e.touches[0].clientX);
            }};
            progress.ontouchend = hidePreview;
            progress.onclick = (e) => {{
                const rect = progress.getBoundingClientRect();
                const pct = (e.clientX - rect.left) / rect.width;
                video.currentTime = pct * video.duration;
            }};

            // Volume
            volumeSlider.oninput = () => {{
                video.volume = volumeSlider.value;
                video.muted = video.volume === 0;
                updateVolumeIcon();
            }};
            volumeBtn.onclick = () => {{
                video.muted = !video.muted;
                updateVolumeIcon();
            }};
            function updateVolumeIcon() {{
                if (video.muted || video.volume === 0) {{
                    volumeBtn.innerHTML = muteIcon;
                }} else {{
                    volumeBtn.innerHTML = volumeIcon;
                }}
            }}

            // Fullscreen
            fullscreenBtn.onclick = () => {{
                if (document.fullscreenElement) {{
                    document.exitFullscreen();
                }} else {{
                    container.requestFullscreen();
                }}
            }};

            // Menus
            qualityBtn.onclick = (e) => {{
                e.stopPropagation();
                if (subtitleMenu) subtitleMenu.classList.remove('show');
                if (audioMenu) audioMenu.classList.remove('show');
                qualityMenu.classList.toggle('show');
            }};
            if (audioBtn && audioMenu) {{
                audioBtn.onclick = (e) => {{
                    e.stopPropagation();
                    qualityMenu.classList.remove('show');
                    if (subtitleMenu) subtitleMenu.classList.remove('show');
                    audioMenu.classList.toggle('show');
                }};
            }}
            if (subtitleBtn && subtitleMenu) {{
                subtitleBtn.onclick = (e) => {{
                    e.stopPropagation();
                    qualityMenu.classList.remove('show');
                    if (audioMenu) audioMenu.classList.remove('show');
                    subtitleMenu.classList.toggle('show');
                }};
            }}
            document.onclick = () => {{
                qualityMenu.classList.remove('show');
                if (audioMenu) audioMenu.classList.remove('show');
                if (subtitleMenu) subtitleMenu.classList.remove('show');
            }};

            // Keyboard shortcuts
            document.onkeydown = (e) => {{
                if (e.target.tagName === 'INPUT') return;
                switch(e.key.toLowerCase()) {{
                    case ' ':
                    case 'k':
                        e.preventDefault();
                        togglePlay();
                        break;
                    case 'f':
                        e.preventDefault();
                        fullscreenBtn.click();
                        break;
                    case 'm':
                        e.preventDefault();
                        video.muted = !video.muted;
                        updateVolumeIcon();
                        break;
                    case 'arrowleft':
                        e.preventDefault();
                        video.currentTime = Math.max(0, video.currentTime - 5);
                        break;
                    case 'arrowright':
                        e.preventDefault();
                        video.currentTime = Math.min(video.duration, video.currentTime + 5);
                        break;
                    case 'j':
                        e.preventDefault();
                        video.currentTime = Math.max(0, video.currentTime - 10);
                        break;
                    case 'l':
                        e.preventDefault();
                        video.currentTime = Math.min(video.duration, video.currentTime + 10);
                        break;
                }}
            }};

            // Auto-hide controls
            let hideTimeout;
            container.onmousemove = () => {{
                controls.classList.add('show');
                clearTimeout(hideTimeout);
                hideTimeout = setTimeout(() => {{
                    if (!video.paused) controls.classList.remove('show');
                }}, 3000);
            }};
            container.onmouseleave = () => {{
                if (!video.paused) controls.classList.remove('show');
            }};
            controls.classList.add('show');
        }}

        function buildQualityMenu() {{
            const menu = document.getElementById('qualityMenu');
            const tracks = player.getVariantTracks();
            const heights = [...new Set(tracks.map(t => t.height))].sort((a,b) => b - a);
            
            menu.innerHTML = '<div class="menu-item" data-value="-1">Auto</div>';
            heights.forEach(h => {{
                menu.innerHTML += '<div class="menu-item" data-value="' + h + '">' + h + 'p</div>';
            }});

            menu.querySelectorAll('.menu-item').forEach(item => {{
                item.onclick = (e) => {{
                    e.stopPropagation();
                    const val = parseInt(item.dataset.value);
                    if (val === -1) {{
                        player.configure({{ abr: {{ enabled: true }} }});
                    }} else {{
                        player.configure({{ abr: {{ enabled: false }} }});
                        const track = tracks.find(t => t.height === val);
                        if (track) player.selectVariantTrack(track, true);
                    }}
                    menu.querySelectorAll('.menu-item').forEach(i => i.classList.remove('active'));
                    item.classList.add('active');
                    menu.classList.remove('show');
                }};
            }});
            menu.querySelector('.menu-item').classList.add('active');
        }}

        function buildAudioMenu() {{
            const menu = document.getElementById('audioMenu');
            const wrap = document.getElementById('audioWrap');
            if (!menu || !wrap) return;

            const tracks = player.getVariantTracks();
            const audioMap = new Map();
            tracks.forEach(t => {{
                const key = `${{t.language || 'und'}}:${{t.audioChannelsCount || t.channelsCount || ''}}:${{t.roles?.[0] || ''}}`;
                if (!audioMap.has(key)) {{
                    audioMap.set(key, {{
                        language: t.language || '',
                        role: (t.roles && t.roles[0]) || '',
                        label: t.label || t.language || 'Unknown',
                        channels: t.audioChannelsCount || t.channelsCount || null
                    }});
                }}
            }});

            const audioTracks = Array.from(audioMap.values());
            if (audioTracks.length <= 1) {{
                wrap.style.display = 'none';
                return;
            }} else {{
                wrap.style.display = '';
            }}

            menu.innerHTML = '';
            audioTracks.forEach((track, idx) => {{
                const channelLabel = track.channels ? ' (' + track.channels + 'ch)' : '';
                menu.innerHTML += `<div class="menu-item" data-lang="${{track.language}}" data-role="${{track.role}}" data-idx="${{idx}}">${{track.label}}${{channelLabel}}</div>`;
            }});

            const items = menu.querySelectorAll('.menu-item');
            items.forEach((item) => {{
                item.onclick = (e) => {{
                    e.stopPropagation();
                    const lang = item.dataset.lang || '';
                    const role = item.dataset.role || '';
                    player.selectAudioLanguage(lang, role || undefined);
                    items.forEach(i => i.classList.remove('active'));
                    item.classList.add('active');
                    menu.classList.remove('show');
                }};
            }});

            items[0]?.classList.add('active');
        }}

        function buildSubtitleMenu() {{
            const menu = document.getElementById('subtitleMenu');
            menu.innerHTML = '<div class="menu-item" data-idx="-1">Off</div>';
            subtitles.forEach((sub, idx) => {{
                menu.innerHTML += '<div class="menu-item" data-idx="' + idx + '">' + sub.name + '</div>';
            }});

            menu.querySelectorAll('.menu-item').forEach(item => {{
                item.onclick = async (e) => {{
                    e.stopPropagation();
                    const idx = parseInt(item.dataset.idx);
                    menu.querySelectorAll('.menu-item').forEach(i => i.classList.remove('active'));
                    item.classList.add('active');
                    if (idx === -1) {{
                        destroySubtitleRenderer();
                        currentSubtitle = null;
                    }} else {{
                        await loadSubtitle(subtitles[idx]);
                    }}
                    menu.classList.remove('show');
                }};
            }});
            // Select default
            const defIdx = subtitles.findIndex(s => s.default);
            const activeIdx = defIdx >= 0 ? defIdx : 0;
            menu.querySelectorAll('.menu-item')[activeIdx + 1]?.classList.add('active');
        }}

        function destroySubtitleRenderer() {{
            if (jassub) {{ try {{ jassub.destroy(); }} catch {{}} jassub = null; }}
            if (bitmapRenderer) {{ try {{ bitmapRenderer.destroy(); }} catch {{}} bitmapRenderer = null; }}
        }}

        async function loadSubtitle(sub) {{
            destroySubtitleRenderer();
            currentSubtitle = sub;

            if (needsJassub(sub.codec)) {{
                try {{
                    jassub = new JASSUB({{
                        video: video,
                        subUrl: sub.url,
                        workerUrl: '/jassub/jassub-worker.js',
                        wasmUrl: '/jassub/jassub-worker.wasm',
                        fonts: fonts,
                        fallbackFont: 'Arial',
                    }});
                }} catch (e) {{ console.error('JASSUB error:', e); }}
            }} else if (isPgsSubtitle(sub.codec)) {{
                try {{
                    const libbitsub = await import('/libbitsub/libbitsub.js');
                    await libbitsub.default();
                    bitmapRenderer = new libbitsub.PgsRenderer({{
                        video: video,
                        subUrl: sub.url,
                        workerUrl: '/libbitsub/libbitsub.js'
                    }});
                }} catch (e) {{ console.error('PGS renderer error:', e); }}
            }} else if (isVobSubSubtitle(sub.codec)) {{
                try {{
                    const libbitsub = await import('/libbitsub/libbitsub.js');
                    await libbitsub.default();
                    const idxUrl = sub.url.replace(/\\.sub$/, '.idx');
                    bitmapRenderer = new libbitsub.VobSubRenderer({{
                        video: video,
                        subUrl: sub.url,
                        idxUrl: idxUrl
                    }});
                }} catch (e) {{ console.error('VobSub renderer error:', e); }}
            }}
        }}

        function startHeartbeat() {{
            fetch('/api/videos/{video_id}/heartbeat', {{ method: 'POST' }});
            setInterval(() => {{
                fetch('/api/videos/{video_id}/heartbeat', {{ method: 'POST' }});
            }}, 10000);
        }}

        // SVG Icons
        const playIcon = '<svg viewBox="0 0 24 24"><path fill="currentColor" d="M8 5v14l11-7z"/></svg>';
        const pauseIcon = '<svg viewBox="0 0 24 24"><path fill="currentColor" d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/></svg>';
        const volumeIcon = '<svg viewBox="0 0 24 24"><path fill="currentColor" d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z"/></svg>';
        const muteIcon = '<svg viewBox="0 0 24 24"><path fill="currentColor" d="M16.5 12c0-1.77-1.02-3.29-2.5-4.03v2.21l2.45 2.45c.03-.2.05-.41.05-.63zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51C20.63 14.91 21 13.5 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3L3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25c-.67.52-1.42.93-2.25 1.18v2.06c1.38-.31 2.63-.95 3.69-1.81L19.73 21 21 19.73l-9-9L4.27 3zM12 4L9.91 6.09 12 8.18V4z"/></svg>';
        const fsIcon = '<svg viewBox="0 0 24 24"><path fill="currentColor" d="M7 14H5v5h5v-2H7v-3zm-2-4h2V7h3V5H5v5zm12 7h-3v2h5v-5h-2v3zM14 5v2h3v3h2V5h-5z"/></svg>';
        const exitFsIcon = '<svg viewBox="0 0 24 24"><path fill="currentColor" d="M5 16h3v3h2v-5H5v2zm3-8H5v2h5V5H8v3zm6 11h2v-3h3v-2h-5v5zm2-11V5h-2v5h5V8h-3z"/></svg>';

        document.addEventListener('DOMContentLoaded', init);
        "#,
        video_id = id,
        subtitle_js = subtitle_js,
        fonts_js = fonts_js,
        chapters_js = chapters_js,
    );

    // Minify JS
    let minified_js = minify_js(&js_code);

    // Build HTML with Shaka Player and custom skin
    let mut scripts = vec![
        r#"<script src="https://cdn.jsdelivr.net/npm/shaka-player/dist/shaka-player.compiled.min.js"></script>"#,
    ];

    if has_subtitles {
        scripts.push(
            r#"<script src="https://cdn.jsdelivr.net/npm/jassub/dist/jassub.umd.js"></script>"#,
        );
    }

    let scripts_html = scripts.join("\n    ");

    let subtitle_controls = if has_subtitles {
        r#"
            <div class="menu-wrap">
                <button id="subtitleBtn" class="ctrl-btn" aria-label="Subtitles"><svg viewBox="0 0 24 24"><path fill="currentColor" d="M20 4H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2zm0 14H4V6h16v12zM6 10h2v2H6zm0 4h8v2H6zm10 0h2v2h-2zm-6-4h8v2h-8z"/></svg></button>
                <div id="subtitleMenu" class="menu"></div>
            </div>
        "#.to_string()
    } else {
        String::new()
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Video Player</title>
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{ background: #000; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; }}
        #container {{ position: relative; width: 100%; height: 100vh; background: #000; overflow: hidden; display: flex; align-items: center; justify-content: center; }}
        #container.has-thumb {{ background-size: cover; background-position: center; background-repeat: no-repeat; }}
        #video {{ width: 100%; height: 100%; object-fit: contain; max-width: 100%; max-height: 100%; }}
        #container[data-orientation="portrait"] #video {{ width: auto; height: 100%; max-width: 100%; }}
        #container[data-orientation="landscape"] #video {{ width: 100%; height: auto; max-height: 100%; }}
        #loading {{ position: absolute; inset: 0; background: transparent; display: flex; flex-direction: column; gap: 12px; align-items: center; justify-content: center; color: #fff; font-size: 17px; letter-spacing: 0.3px; transition: opacity 0.25s ease, visibility 0.25s ease; z-index: 3; }}
        #loading.hide {{ opacity: 0; visibility: hidden; }}
        .spinner {{ width: 44px; height: 44px; border: 3px solid rgba(255,255,255,0.14); border-top-color: #e50914; border-radius: 50%; animation: spin 1s linear infinite; }}
        @keyframes spin {{ from {{ transform: rotate(0deg); }} to {{ transform: rotate(360deg); }} }}
        
        /* Custom Controls */
        #controls {{
            position: absolute; bottom: 0; left: 0; right: 0;
            background: linear-gradient(transparent, rgba(0,0,0,0.92));
            padding: 62px 22px 16px; display: flex; flex-wrap: wrap;
            align-items: center; gap: 12px; opacity: 0; transition: opacity 0.3s;
        }}
        #controls.show {{ opacity: 1; }}
        
        /* Progress Bar */
        #progress {{ flex: 100%; height: 6px; background: rgba(255,255,255,0.16); cursor: pointer; border-radius: 999px; position: relative; overflow: visible; }}
        #progressTrack {{ position: absolute; inset: 0; background: linear-gradient(90deg, rgba(255,255,255,0.08), rgba(255,255,255,0.16)); }}
        #buffered {{ position: absolute; top: 0; left: 0; height: 100%; background: rgba(255,255,255,0.35); border-radius: 999px; }}
        #progressBar {{ position: absolute; top: 0; left: 0; height: 100%; background: #e50914; border-radius: 999px; box-shadow: 0 0 12px rgba(229,9,20,0.45); }}
        #scrubHandle {{ position: absolute; top: 50%; width: 14px; height: 14px; background: #e50914; border-radius: 50%; transform: translate(-50%, -50%); box-shadow: 0 4px 14px rgba(229,9,20,0.45); }}
        #progress:hover {{ height: 8px; }}
        .preview {{ position: absolute; bottom: 110%; left: 0; transform: translateX(-50%); background: rgba(12,12,12,0.92); border: 1px solid rgba(255,255,255,0.1); border-radius: 10px; box-shadow: 0 10px 24px rgba(0,0,0,0.55); display: none; overflow: hidden; pointer-events: none; z-index: 5; }}
        .preview.show {{ display: block; }}
        #previewImage {{ width: 160px; height: 90px; background-repeat: no-repeat; background-color: #0f0f0f; }}
        #previewTime {{ padding: 6px 10px; color: #fff; text-align: center; font-size: 12px; letter-spacing: 0.3px; border-top: 1px solid rgba(255,255,255,0.08); background: rgba(0,0,0,0.65); }}
        
        /* Buttons */
        .ctrl-btn {{ background: none; border: none; color: #fff; width: 36px; height: 36px; cursor: pointer; padding: 6px; display: flex; align-items: center; justify-content: center; border-radius: 10px; transition: background 0.15s ease; }}
        .ctrl-btn:hover {{ background: rgba(255,255,255,0.1); }}
        .ctrl-btn svg {{ width: 24px; height: 24px; }}
        
        /* Time Display */
        #time {{ color: #fff; font-size: 13px; white-space: nowrap; letter-spacing: 0.2px; }}
        
        /* Volume */
        #volumeWrap {{ display: flex; align-items: center; }}
        #volumeSlider {{ width: 0; opacity: 0; transition: all 0.2s; height: 4px; cursor: pointer; accent-color: #e50914; }}
        #volumeWrap:hover #volumeSlider {{ width: 90px; opacity: 1; margin-left: 10px; }}
        
        /* Spacer */
        .spacer {{ flex: 1; }}
        
        /* Menus */
        .menu-wrap {{ position: relative; }}
        .menu {{ position: absolute; bottom: 100%; right: 0; background: rgba(24,24,24,0.96); border: 1px solid rgba(255,255,255,0.08); border-radius: 10px; padding: 8px 0; min-width: 140px; display: none; box-shadow: 0 12px 28px rgba(0,0,0,0.45); }}
        .menu.show {{ display: block; }}
        .menu-item {{ padding: 10px 16px; color: #fff; font-size: 14px; cursor: pointer; display: flex; align-items: center; gap: 8px; }}
        .menu-item:hover {{ background: rgba(255,255,255,0.08); }}
        .menu-item.active {{ color: #e50914; }}
    </style>
</head>
<body>
    <div id="container">
        <video id="video" autoplay playsinline></video>
        <div id="loading">
            <div class="spinner"></div>
        </div>
        <div id="controls">
            <div id="progress">
                <div id="preview" class="preview">
                    <div id="previewImage"></div>
                    <div id="previewTime"></div>
                </div>
                <div id="progressTrack"></div>
                <div id="buffered"></div>
                <div id="progressBar"></div>
                <div id="scrubHandle"></div>
            </div>
            <button id="playBtn" class="ctrl-btn"><svg viewBox="0 0 24 24"><path fill="currentColor" d="M8 5v14l11-7z"/></svg></button>
            <div id="volumeWrap">
                <button id="volumeBtn" class="ctrl-btn"><svg viewBox="0 0 24 24"><path fill="currentColor" d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z"/></svg></button>
                <input type="range" id="volumeSlider" min="0" max="1" step="0.1" value="1">
            </div>
            <div id="time"><span id="currentTime">0:00</span> / <span id="duration">0:00</span></div>
            <div class="spacer"></div>
            <div class="menu-wrap">
                <button id="qualityBtn" class="ctrl-btn" aria-label="Quality"><svg viewBox="0 0 24 24"><path fill="currentColor" d="M19.14 12.94c.04-.31.06-.63.06-.94s-.02-.63-.06-.94l2.03-1.58a.5.5 0 00.12-.64l-1.92-3.32a.5.5 0 00-.61-.22l-2.39.96a7.007 7.007 0 00-1.63-.94l-.36-2.54A.5.5 0 0013.88 2h-3.76a.5.5 0 00-.5.42l-.36 2.54c-.59.23-1.14.54-1.63.94l-2.39-.96a.5.5 0 00-.61.22L2.71 9.1a.5.5 0 00.12.64l2.03 1.58c-.04.31-.06.63-.06.94s.02.63.06.94l-2.03 1.58a.5.5 0 00-.12.64l1.92 3.32c.14.24.43.34.68.22l2.39-.96c.49.4 1.04.72 1.63.94l.36 2.54c.04.26.26.46.5.46h3.76c.25 0 .46-.2.5-.46l.36-2.54c.59-.23 1.14-.54 1.63-.94l2.39.96c.25.1.54.01.68-.22l1.92-3.32a.5.5 0 00-.12-.64l-2.03-1.58zM12 15.5A3.5 3.5 0 1115.5 12 3.5 3.5 0 0112 15.5z"/></svg></button>
                <div id="qualityMenu" class="menu"></div>
            </div>
            <div class="menu-wrap" id="audioWrap">
                <button id="audioBtn" class="ctrl-btn" aria-label="Audio Tracks"><svg viewBox="0 0 24 24"><path fill="currentColor" d="M12 3a9 9 0 100 18 9 9 0 000-18zm0 2a7 7 0 110 14 7 7 0 010-14zm-1 3v8.5a3.5 3.5 0 11-2-3.163V8h3z"/></svg></button>
                <div id="audioMenu" class="menu"></div>
            </div>
            {subtitle_controls}
            <button id="fullscreenBtn" class="ctrl-btn"><svg viewBox="0 0 24 24"><path fill="currentColor" d="M7 14H5v5h5v-2H7v-3zm-2-4h2V7h3V5H5v5zm12 7h-3v2h5v-5h-2v3zM14 5v2h3v3h2V5h-5z"/></svg></button>
        </div>
    </div>
    {scripts_html}
    <script>{minified_js}</script>
</body>
</html>"#,
        scripts_html = scripts_html,
        subtitle_controls = subtitle_controls,
        minified_js = minified_js,
    );

    // Determine cookie attributes based on protocol
    let is_https = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|proto| proto == "https")
        .unwrap_or(false);

    let cookie_attr = if is_https {
        "SameSite=None; Secure"
    } else {
        "SameSite=Lax"
    };

    // Set cookie
    let cookie = format!(
        "token={}; Path=/; HttpOnly; Max-Age=3600; {}",
        token, cookie_attr
    );

    ([(header::SET_COOKIE, cookie)], Html(html))
}

pub async fn get_hls_file(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path((id, file)): Path<(String, String)>,
) -> Result<Response, (StatusCode, String)> {
    let key = format!("{}/{}", id, file);

    // Verify token for HLS files (.m3u8, .ts)
    if file.ends_with(".m3u8") || file.ends_with(".ts") {
        // Extract token from Cookie header
        let cookie_header = headers
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let mut token = "";
        for cookie in cookie_header.split(';') {
            let cookie = cookie.trim();
            if let Some(val) = cookie.strip_prefix("token=") {
                token = val;
                break;
            }
        }

        // Try to get the real client IP from X-Forwarded-For header
        let ip = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|xff| xff.split(',').next().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| addr.ip().to_string());

        // Extract User-Agent header
        let user_agent = headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !verify_token(&id, token, &state.config.server.secret_key, &ip, user_agent) {
            return Err((
                StatusCode::FORBIDDEN,
                "Access denied: Invalid or expired token".to_string(),
            ));
        }
    }

    // Fetch content from S3
    let content = state
        .s3
        .get_object()
        .bucket(&state.config.r2.bucket)
        .key(&key)
        .send()
        .await
        .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

    let reader = content.body.into_async_read();
    let stream = tokio_util::io::ReaderStream::new(reader);
    let body_stream = stream.map(|result| result.map_err(std::io::Error::other));
    let body = Body::from_stream(body_stream);

    // Determine Content-Type
    let content_type = if file.ends_with(".m3u8") {
        "application/vnd.apple.mpegurl"
    } else if file.ends_with(".ts") {
        "video/mp2t"
    } else if file.ends_with(".jpg") || file.ends_with(".jpeg") {
        "image/jpeg"
    } else {
        "application/octet-stream"
    };

    Ok(([(header::CONTENT_TYPE, content_type)], body).into_response())
}
