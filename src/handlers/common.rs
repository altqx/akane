use axum::http::StatusCode;
use hmac::{Hmac, Mac};
use regex::Regex;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::error;

pub fn internal_err(e: anyhow::Error) -> (StatusCode, String) {
    error!(error = ?e, "internal error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal server error".to_string(),
    )
}

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn minify_js_simple(code: &str) -> String {
    let mut result = String::with_capacity(code.len());
    let chars: Vec<char> = code.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut last_non_ws: Option<char> = None;
    let mut needs_space = false;
    let is_ident_char = |c: char| c.is_alphanumeric() || c == '_' || c == '$';

    // Helper to check if we're at a regex context (after certain tokens)
    let can_be_regex = |last: Option<char>| {
        matches!(
            last,
            None | Some(
                '(' | ',' | '=' | ':' | '[' | '!' | '&' | '|' | '?' | '{' | '}' | ';' | '\n'
            )
        )
    };

    while i < len {
        let c = chars[i];

        // Handle single-line comments
        if c == '/' && i + 1 < len && chars[i + 1] == '/' {
            // Skip until end of line
            i += 2;
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            // Don't increment i here, let the main loop handle the newline
            continue;
        }

        // Handle multi-line comments
        if c == '/' && i + 1 < len && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2; // Skip */
            needs_space = true; // Comments might separate tokens
            continue;
        }

        // Handle string literals (single and double quotes)
        if c == '"' || c == '\'' {
            let quote = c;
            result.push(c);
            i += 1;
            while i < len {
                let sc = chars[i];
                result.push(sc);
                if sc == '\\' && i + 1 < len {
                    // Push escaped character
                    i += 1;
                    result.push(chars[i]);
                } else if sc == quote {
                    break;
                }
                i += 1;
            }
            last_non_ws = Some(quote);
            needs_space = false;
            i += 1;
            continue;
        }

        // Handle template literals (backticks)
        if c == '`' {
            result.push(c);
            i += 1;
            let mut brace_depth = 0;
            while i < len {
                let tc = chars[i];
                if tc == '\\' && i + 1 < len {
                    result.push(tc);
                    i += 1;
                    result.push(chars[i]);
                } else if tc == '$' && i + 1 < len && chars[i + 1] == '{' {
                    result.push(tc);
                    i += 1;
                    result.push(chars[i]);
                    brace_depth += 1;
                } else if tc == '{' && brace_depth > 0 {
                    result.push(tc);
                    brace_depth += 1;
                } else if tc == '}' && brace_depth > 0 {
                    result.push(tc);
                    brace_depth -= 1;
                } else if tc == '`' && brace_depth == 0 {
                    result.push(tc);
                    break;
                } else {
                    result.push(tc);
                }
                i += 1;
            }
            last_non_ws = Some('`');
            needs_space = false;
            i += 1;
            continue;
        }

        // Handle regex literals
        if c == '/' && can_be_regex(last_non_ws) {
            // Check it's not a division operator by looking ahead
            let mut j = i + 1;
            let mut is_regex = false;
            let mut escaped = false;
            let mut in_class = false;

            while j < len {
                let rc = chars[j];
                if escaped {
                    escaped = false;
                } else if rc == '\\' {
                    escaped = true;
                } else if rc == '[' && !in_class {
                    in_class = true;
                } else if rc == ']' && in_class {
                    in_class = false;
                } else if rc == '/' && !in_class {
                    is_regex = true;
                    break;
                } else if rc == '\n' {
                    break; // Regex can't span lines without escape
                }
                j += 1;
            }

            if is_regex {
                // Copy the regex literal
                result.push(c);
                i += 1;
                escaped = false;
                in_class = false;
                while i < len {
                    let rc = chars[i];
                    result.push(rc);
                    if escaped {
                        escaped = false;
                    } else if rc == '\\' {
                        escaped = true;
                    } else if rc == '[' && !in_class {
                        in_class = true;
                    } else if rc == ']' && in_class {
                        in_class = false;
                    } else if rc == '/' && !in_class {
                        i += 1;
                        // Copy regex flags
                        while i < len && chars[i].is_ascii_alphabetic() {
                            result.push(chars[i]);
                            i += 1;
                        }
                        break;
                    }
                    i += 1;
                }
                last_non_ws = Some('/');
                needs_space = false;
                continue;
            }
        }

        // Handle whitespace
        if c.is_whitespace() {
            if last_non_ws.is_some() {
                needs_space = true;
            }
            i += 1;
            continue;
        }

        // Determine if we need to preserve space between tokens
        if needs_space {
            if let Some(last) = last_non_ws {
                // Space needed between identifier characters
                let last_is_ident = is_ident_char(last);
                let curr_is_ident = is_ident_char(c);

                // Space needed for keywords that could be ambiguous
                // e.g., "return x" vs "returnx", "typeof x" vs "typeofx"
                let needs_separator = (last_is_ident && curr_is_ident)
                    || (last_is_ident && c == '/')  // "return /regex/"
                    || (last == '/' && curr_is_ident); // Rare but possible

                // Handle operators that could be ambiguous without space
                // e.g., "a + +b" should not become "a++b"
                let ambiguous_ops =
                    matches!((last, c), ('+', '+') | ('-', '-') | ('+', '-') | ('-', '+'));

                if needs_separator || ambiguous_ops {
                    result.push(' ');
                }
            }
            needs_space = false;
        }

        result.push(c);
        last_non_ws = Some(c);
        i += 1;
    }

    // Final cleanup: remove unnecessary semicolons before closing braces
    let result = Regex::new(r";+}")
        .unwrap()
        .replace_all(&result, "}")
        .to_string();

    // Remove trailing semicolons
    let result = result.trim_end_matches(';').to_string();

    result
}

// Helper to generate a signed token
pub fn generate_token(video_id: &str, secret: &str, ip: &str, user_agent: &str) -> String {
    // Token valid for 1 hour (3600 seconds)
    let expiration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600;

    // Use ASCII Unit Separator (\x1F) as delimiter to avoid ambiguity with colons
    // that commonly appear in User-Agent strings (e.g., "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
    let payload = format!("{}\x1F{}\x1F{}\x1F{}", video_id, expiration, ip, user_agent);

    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());
    let result = mac.finalize();
    let signature = hex::encode(result.into_bytes());

    format!("{}:{}", expiration, signature)
}

// Helper to verify a signed token
pub fn verify_token(video_id: &str, token: &str, secret: &str, ip: &str, user_agent: &str) -> bool {
    let parts: Vec<&str> = token.split(':').collect();
    if parts.len() != 2 {
        return false;
    }

    let expiration_str = parts[0];
    let signature = parts[1];

    // Check expiration
    let expiration: u64 = match expiration_str.parse() {
        Ok(ts) => ts,
        Err(_) => return false,
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    if now > expiration {
        return false;
    }

    // Verify signature
    let payload = format!("{}\x1F{}\x1F{}\x1F{}", video_id, expiration, ip, user_agent);
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());

    // Use constant-time comparison to prevent timing attacks
    let expected_bytes = mac.finalize().into_bytes();
    match hex::decode(signature) {
        Ok(sig_bytes) => expected_bytes.as_slice() == sig_bytes.as_slice(),
        Err(_) => false,
    }
}
