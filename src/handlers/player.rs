use crate::database::{get_attachments_for_video, get_chapters_for_video, get_subtitles_for_video};
use crate::handlers::common::{generate_token, internal_err, minify_js_simple, verify_token};
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
    let has_multiple_subtitles = subtitles.len() > 1;
    let has_fonts = !attachments.is_empty();
    let has_chapters = !chapters.is_empty();

    // Build subtitle configuration for ArtPlayer (only if subtitles exist)
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
                let default = if sub.is_default { "true" } else { "false" };
                // Add file extension based on codec for libass to detect ASS files
                let ext = match sub.codec.as_str() {
                    "ass" | "ssa" => "ass",
                    "subrip" | "srt" => "srt",
                    _ => "ass",
                };
                format!(
                    r#"{{ name: {}, url: "/api/videos/{}/subtitles/{}.{}", default: {} }}"#,
                    escaped_name, id, sub.track_index, ext, default
                )
            })
            .collect();
        format!("const subtitles = [{}];", subtitle_config.join(", "))
    } else {
        String::new()
    };

    // Build font URLs for libass (only if fonts exist)
    let fonts_js = if has_fonts {
        let font_urls: Vec<String> = attachments
            .iter()
            .map(|att| format!("/api/videos/{}/attachments/{}", id, att.filename))
            .collect();
        let json = serde_json::to_string(&font_urls).unwrap_or_else(|_| "[]".to_string());
        format!("const fonts = {};", json)
    } else {
        String::new()
    };

    // Build chapters array (only if chapters exist) - filter invalid time points
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
            String::new()
        } else {
            format!("const chapters = [{}];", chapter_config.join(", "))
        }
    } else {
        String::new()
    };

    // Build plugins array - only include what's needed
    let mut plugins = vec![
        r#"artplayerPluginHlsControl({
                quality: {
                    control: true,
                    setting: true,
                    getName: (level) => level.height + 'P',
                    title: 'Quality',
                    auto: 'Auto',
                },
            })"#
        .to_string(),
    ];

    plugins.push("artplayerPluginAutoThumbnail({ width: 160, number: 100 })".to_string());

    // Only add chapter plugin if we have valid chapters
    let has_valid_chapters = has_chapters
        && chapters
            .iter()
            .any(|ch| ch.start_time >= 0.0 && ch.end_time > ch.start_time);
    if has_valid_chapters {
        plugins.push("artplayerPluginChapter({ chapters: chapters })".to_string());
    }

    let plugins_js = plugins.join(",\n            ");

    // Build JASSUB initialization code (only if subtitles exist)
    let default_sub = subtitles
        .iter()
        .find(|s| s.is_default)
        .or_else(|| subtitles.first());
    let jassub_init_js = if let Some(sub) = default_sub {
        let ext = match sub.codec.as_str() {
            "ass" | "ssa" => "ass",
            "subrip" | "srt" => "srt",
            _ => "ass",
        };
        let fonts_array = if has_fonts { "fonts" } else { "[]" };
        let default_sub_url = format!("/api/videos/{}/subtitles/{}.{}", id, sub.track_index, ext);

        // Build subtitle selector if multiple subtitles exist
        let subtitle_selector = if has_multiple_subtitles {
            r#"
                // Add subtitle selector to settings
                art.setting.add({
                    name: 'subtitle',
                    html: 'Subtitle',
                    tooltip: subtitles.find(s => s.default)?.name || subtitles[0]?.name || 'None',
                    selector: [
                        { html: 'Off', value: 'off' },
                        ...subtitles.map(s => ({ html: s.name, url: s.url, default: s.default }))
                    ],
                    onSelect: function(item) {
                        if (item.value === 'off') {
                            window.subtitlesEnabled = false;
                            updateToggleButton();
                            if (window.jassub) {
                                window.jassub.freeTrack();
                            }
                        } else if (item.url) {
                            window.subtitlesEnabled = true;
                            window.currentSubUrl = item.url;
                            updateToggleButton();
                            if (window.jassub) {
                                window.jassub.setTrackByUrl(item.url);
                            }
                        }
                        return item.html;
                    },
                });"#
                .to_string()
        } else {
            String::new()
        };

        format!(
            r#"
            // Initialize JASSUB for ASS subtitle rendering after Artplayer is ready
            art.on('ready', function() {{
                // Subtitle state management
                window.subtitlesEnabled = true;
                window.currentSubUrl = '{default_sub_url}';
                
                function updateToggleButton() {{
                    const toggleEl = document.querySelector('.art-control-subtitle-toggle');
                    if (toggleEl) {{
                        toggleEl.style.opacity = window.subtitlesEnabled ? '1' : '0.5';
                    }}
                }}

                console.log('Artplayer ready, initializing JASSUB...');
                console.log('Video element:', art.video);
                console.log('subUrl:', window.currentSubUrl);
                console.log('fonts:', {fonts_array});
                
                try {{
                    window.jassub = new JASSUB({{
                        video: art.video,
                        subUrl: window.currentSubUrl,
                        workerUrl: '/jassub/jassub-worker.js',
                        wasmUrl: '/jassub/jassub-worker.wasm',
                        fonts: {fonts_array},
                        fallbackFont: 'Arial',
                    }});
                    console.log('JASSUB initialized:', window.jassub);
                }} catch (e) {{
                    console.error('JASSUB initialization error:', e);
                }}

                // Add subtitle toggle button to controls
                art.controls.add({{
                    name: 'subtitle-toggle',
                    position: 'right',
                    index: 10,
                    html: '<svg xmlns="http://www.w3.org/2000/svg" width="22" height="22" viewBox="0 0 24 24" fill="currentColor"><path d="M20 4H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2zm0 14H4V6h16v12zM6 10h2v2H6zm0 4h8v2H6zm10 0h2v2h-2zm-6-4h8v2h-8z"/></svg>',
                    tooltip: 'Toggle Subtitles',
                    style: {{ color: '#fff' }},
                    click: function() {{
                        window.subtitlesEnabled = !window.subtitlesEnabled;
                        updateToggleButton();
                        if (window.jassub) {{
                            if (window.subtitlesEnabled && window.currentSubUrl) {{
                                window.jassub.setTrackByUrl(window.currentSubUrl);
                            }} else {{
                                window.jassub.freeTrack();
                            }}
                        }}
                    }},
                }});
                {subtitle_selector}
            }});"#,
            default_sub_url = default_sub_url,
            fonts_array = fonts_array,
            subtitle_selector = subtitle_selector
        )
    } else {
        String::new()
    };

    let js_code = format!(
        r#"
        let viewTracked = false;
        let heartbeatStarted = false;
        let art = null;
        {subtitle_js}
        {fonts_js}
        {chapters_js}

        function init() {{
            // Get saved settings from Artplayer's localStorage
            let savedSettings = {{}};
            try {{
                savedSettings = JSON.parse(localStorage.getItem('artplayer_settings')) || {{}};
            }} catch {{}}
            const savedQualityLevel = savedSettings.qualityLevel;
            const savedPlaybackRate = savedSettings.playbackRate;
            
            art = new Artplayer({{
                container: '#artplayer',
                url: '/hls/{video_id}/index.m3u8',
                type: 'm3u8',
                autoplay: true,
                autoSize: false,
                autoMini: false,
                loop: false,
                flip: true,
                playbackRate: true,
                aspectRatio: true,
                setting: true,
                hotkey: true,
                pip: true,
                mutex: true,
                fullscreen: true,
                fullscreenWeb: true,
                subtitleOffset: true,
                miniProgressBar: true,
                localVideo: false,
                localSubtitle: false,
                volume: 1,
                isLive: false,
                muted: false,
                autoPlayback: true,
                airplay: true,
                theme: '#ff0000',
                lang: 'en',
                moreVideoAttr: {{
                    crossOrigin: 'anonymous',
                }},
                plugins: [
            {plugins_js}
                ],
                customType: {{
                    m3u8: function playM3u8(video, url, art) {{
                        if (Hls.isSupported()) {{
                            if (art.hls) art.hls.destroy();
                            const hls = new Hls();
                            hls.loadSource(url);
                            hls.attachMedia(video);
                            art.hls = hls;
                            art.on('destroy', () => hls.destroy());
                            
                            // Restore saved quality level after HLS manifest is loaded
                            hls.on(Hls.Events.MANIFEST_PARSED, function() {{
                                if (savedQualityLevel !== undefined && savedQualityLevel >= -1 && savedQualityLevel < hls.levels.length) {{
                                    hls.currentLevel = savedQualityLevel;
                                }}
                            }});
                            
                            // Save quality level when changed
                            hls.on(Hls.Events.LEVEL_SWITCHED, function(event, data) {{
                                art.storage.set('qualityLevel', data.level);
                            }});
                        }} else if (video.canPlayType('application/vnd.apple.mpegurl')) {{
                            video.src = url;
                        }} else {{
                            art.notice.show = 'Unsupported playback format: m3u8';
                        }}
                    }},
                }},
            }});
            
            // Restore and persist playback rate
            art.on('ready', function() {{
                if (savedPlaybackRate && savedPlaybackRate !== 1) {{
                    art.playbackRate = savedPlaybackRate;
                }}
            }});
            
            art.on('video:ratechange', function() {{
                art.storage.set('playbackRate', art.playbackRate);
            }});
            
            {jassub_init_js}
            art.on('play', onFirstPlay);
            art.on('error', onError);
            window.art = art;
        }}

        function onFirstPlay() {{
            if (!viewTracked) {{
                viewTracked = true;
                fetch('/api/videos/{video_id}/view', {{ method: 'POST' }});
            }}
            if (!heartbeatStarted) {{
                heartbeatStarted = true;
                startHeartbeat();
            }}
        }}

        function startHeartbeat() {{
            fetch('/api/videos/{video_id}/heartbeat', {{ method: 'POST' }});
            setInterval(() => {{
                fetch('/api/videos/{video_id}/heartbeat', {{ method: 'POST' }});
            }}, 10000);
        }}

        function onError(error) {{
            console.error('Player error:', error);
        }}

        document.addEventListener('DOMContentLoaded', init);
        "#,
        subtitle_js = subtitle_js,
        fonts_js = fonts_js,
        chapters_js = chapters_js,
        video_id = id,
        plugins_js = plugins_js,
        jassub_init_js = jassub_init_js,
    );

    // Simple regex-based JS minification (safe, no panics)
    let minified_js = minify_js_simple(&js_code);

    // Build HTML with only the required script tags
    let mut scripts = vec![
        r#"<script src="https://cdn.jsdelivr.net/npm/hls.js/dist/hls.min.js"></script>"#,
        r#"<script src="https://cdn.jsdelivr.net/npm/artplayer/dist/artplayer.min.js"></script>"#,
        r#"<script src="https://cdn.jsdelivr.net/npm/artplayer-plugin-hls-control/dist/artplayer-plugin-hls-control.min.js"></script>"#,
    ];

    if has_subtitles {
        scripts.push(
            r#"<script src="https://cdn.jsdelivr.net/npm/jassub/dist/jassub.umd.js"></script>"#,
        );
    }

    scripts.push(r#"<script src="https://cdn.jsdelivr.net/npm/artplayer-plugin-auto-thumbnail/dist/artplayer-plugin-auto-thumbnail.min.js"></script>"#);

    if has_valid_chapters {
        scripts.push(r#"<script src="https://cdn.jsdelivr.net/npm/artplayer-plugin-chapter/dist/artplayer-plugin-chapter.min.js"></script>"#);
    }

    let scripts_html = scripts.join("\n    ");

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Video Player</title>
    <style>
        body, html {{ margin: 0; padding: 0; width: 100%; height: 100%; background: #000; overflow: hidden; }}
        #artplayer {{ width: 100%; height: 100%; position: relative; }}
        /* Ensure JASSUB canvas is visible above video */
        #artplayer canvas {{ position: absolute; top: 0; left: 0; pointer-events: none; z-index: 10; }}
    </style>
</head>
<body>
    <div id="artplayer"></div>
    {scripts_html}
    <script>{minified_js}</script>
</body>
</html>"#,
        scripts_html = scripts_html,
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
    // Subtitles and fonts are now served through dedicated API endpoints
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

        // Try to get the real client IP from X-Forwarded-For header, fallback to addr.ip()
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

    // Fetch content from S3 for all file types (Proxy)
    let content = state
        .s3
        .get_object()
        .bucket(&state.config.r2.bucket)
        .key(&key)
        .send()
        .await
        .map_err(|e| internal_err(anyhow::anyhow!(e)))?;

    // Stream the body directly instead of collecting into memory
    let reader = content.body.into_async_read();
    let stream = tokio_util::io::ReaderStream::new(reader);

    // Convert Byte stream to Frame stream for Axum Body
    let body_stream = stream.map(|result| {
        result // Ensure it's Bytes
            .map_err(std::io::Error::other)
    });

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
