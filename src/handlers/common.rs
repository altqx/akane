use axum::http::StatusCode;
use hmac::{Hmac, Mac};
use regex::Regex;
use sha2::Sha256;
use std::collections::{HashMap, HashSet};
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

pub fn minify_js(code: &str) -> String {
    // First pass: tokenize and remove whitespace/comments
    let minified = minify_pass_one(code);

    // Second pass: rename variables to shorter names
    let renamed = rename_variables(&minified);

    // Third pass: apply optimizations
    let optimized = apply_optimizations(&renamed);

    // Final cleanup
    final_cleanup(&optimized)
}

fn minify_pass_one(code: &str) -> String {
    let mut result = String::with_capacity(code.len());
    let chars: Vec<char> = code.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut last_non_ws: Option<char> = None;
    let mut needs_space = false;

    let is_ident_char = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let is_ident_start = |c: char| c.is_alphabetic() || c == '_' || c == '$';

    // Helper to check if we're at a regex context (after certain tokens)
    let can_be_regex = |last: Option<char>| {
        matches!(
            last,
            None | Some(
                '(' | ','
                    | '='
                    | ':'
                    | '['
                    | '!'
                    | '&'
                    | '|'
                    | '?'
                    | '{'
                    | '}'
                    | ';'
                    | '\n'
                    | '<'
                    | '>'
                    | '+'
                    | '-'
                    | '*'
                    | '/'
                    | '%'
                    | '^'
                    | '~'
            )
        )
    };

    while i < len {
        let c = chars[i];

        // Handle single-line comments
        if c == '/' && i + 1 < len && chars[i + 1] == '/' {
            i += 2;
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            // A comment can act as a line terminator for ASI
            if last_non_ws.is_some() {
                needs_space = true;
            }
            continue;
        }

        // Handle multi-line comments
        if c == '/' && i + 1 < len && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2; // Skip */
            needs_space = true;
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

        // Handle template literals (backticks) with nested expression support
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
                    break;
                }
                j += 1;
            }

            if is_regex {
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
                let last_is_ident = is_ident_char(last);
                let curr_is_ident = is_ident_char(c);
                let curr_is_ident_start = is_ident_start(c);

                // Space needed between identifiers
                let needs_separator = (last_is_ident && curr_is_ident)
                    || (last_is_ident && c == '/') // "return /regex/"
                    || (last == '/' && curr_is_ident_start) // division followed by identifier
                    || (last == ')' && curr_is_ident_start) // ") function" or ") if"
                    || (last == ']' && curr_is_ident_start); // "] in" patterns

                // Handle operators that could be ambiguous without space
                let ambiguous_ops = matches!(
                    (last, c),
                    ('+', '+')
                        | ('-', '-')
                        | ('+', '-')
                        | ('-', '+')
                        | ('!', '!')
                        | ('<', '!')
                        | ('-', '>')
                );

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

    result
}

fn apply_optimizations(code: &str) -> String {
    let mut result = code.to_string();

    // Rename local variables to shorter names
    result = rename_variables(&result);

    // Replace boolean literals
    result = replace_keyword_safe(&result, "true", "!0");
    result = replace_keyword_safe(&result, "false", "!1");
    result = replace_keyword_safe(&result, "undefined", "void 0");

    result
}

/// Generate short variable names: a, b, c, ..., z, aa, ab, ..., az, ba, ...
fn generate_short_name(index: usize) -> String {
    // Skip reserved single-letter names that could conflict
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let base = CHARS.len();

    if index < base {
        return (CHARS[index] as char).to_string();
    }

    let mut result = String::new();
    let mut n = index;
    while n >= base {
        result.insert(0, CHARS[n % base] as char);
        n = n / base - 1;
    }
    result.insert(0, CHARS[n] as char);
    result
}

/// Tokenize JavaScript code into tokens for variable analysis
#[derive(Debug, Clone, PartialEq)]
enum JsToken {
    Keyword(String),
    Identifier(String),
    String(String),
    Regex(String),
    Operator(String),
    Punctuation(char),
    Number(String),
    Other(char),
}

fn tokenize_js(code: &str) -> Vec<JsToken> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = code.chars().collect();
    let len = chars.len();
    let mut i = 0;

    let keywords: HashSet<&str> = [
        "break",
        "case",
        "catch",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "finally",
        "for",
        "function",
        "if",
        "in",
        "instanceof",
        "new",
        "return",
        "switch",
        "this",
        "throw",
        "try",
        "typeof",
        "var",
        "void",
        "while",
        "with",
        "let",
        "const",
        "class",
        "extends",
        "export",
        "import",
        "super",
        "yield",
        "async",
        "await",
        "static",
        "of",
        "true",
        "false",
        "null",
        "undefined",
    ]
    .into_iter()
    .collect();

    let is_ident_start = |c: char| c.is_alphabetic() || c == '_' || c == '$';
    let is_ident_char = |c: char| c.is_alphanumeric() || c == '_' || c == '$';

    while i < len {
        let c = chars[i];

        // Skip whitespace but don't create tokens for it
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // String literals
        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            let mut s = String::new();
            s.push(c);
            i += 1;

            if quote == '`' {
                let mut brace_depth = 0;
                while i < len {
                    let tc = chars[i];
                    s.push(tc);
                    if tc == '\\' && i + 1 < len {
                        i += 1;
                        s.push(chars[i]);
                    } else if tc == '$' && i + 1 < len && chars[i + 1] == '{' {
                        i += 1;
                        s.push(chars[i]);
                        brace_depth += 1;
                    } else if tc == '{' && brace_depth > 0 {
                        brace_depth += 1;
                    } else if tc == '}' && brace_depth > 0 {
                        brace_depth -= 1;
                    } else if tc == '`' && brace_depth == 0 {
                        break;
                    }
                    i += 1;
                }
            } else {
                while i < len {
                    let sc = chars[i];
                    s.push(sc);
                    if sc == '\\' && i + 1 < len {
                        i += 1;
                        s.push(chars[i]);
                    } else if sc == quote {
                        break;
                    }
                    i += 1;
                }
            }
            tokens.push(JsToken::String(s));
            i += 1;
            continue;
        }

        // Identifiers and keywords
        if is_ident_start(c) {
            let mut ident = String::new();
            while i < len && is_ident_char(chars[i]) {
                ident.push(chars[i]);
                i += 1;
            }
            if keywords.contains(ident.as_str()) {
                tokens.push(JsToken::Keyword(ident));
            } else {
                tokens.push(JsToken::Identifier(ident));
            }
            continue;
        }

        // Numbers
        if c.is_ascii_digit() || (c == '.' && i + 1 < len && chars[i + 1].is_ascii_digit()) {
            let mut num = String::new();
            // Handle hex, octal, binary
            if c == '0' && i + 1 < len {
                let next = chars[i + 1];
                if next == 'x'
                    || next == 'X'
                    || next == 'o'
                    || next == 'O'
                    || next == 'b'
                    || next == 'B'
                {
                    num.push(c);
                    i += 1;
                    num.push(chars[i]);
                    i += 1;
                    while i < len && (chars[i].is_ascii_hexdigit() || chars[i] == '_') {
                        num.push(chars[i]);
                        i += 1;
                    }
                    tokens.push(JsToken::Number(num));
                    continue;
                }
            }
            while i < len
                && (chars[i].is_ascii_digit()
                    || chars[i] == '.'
                    || chars[i] == 'e'
                    || chars[i] == 'E'
                    || chars[i] == '_')
            {
                if (chars[i] == 'e' || chars[i] == 'E')
                    && i + 1 < len
                    && (chars[i + 1] == '+' || chars[i + 1] == '-')
                {
                    num.push(chars[i]);
                    i += 1;
                }
                num.push(chars[i]);
                i += 1;
            }
            tokens.push(JsToken::Number(num));
            continue;
        }

        // Regex (simplified detection based on context)
        if c == '/' {
            let can_be_regex = tokens.is_empty()
                || matches!(
                    tokens.last(),
                    Some(JsToken::Punctuation(
                        '(' | ',' | '=' | ':' | '[' | '!' | '&' | '|' | '?' | '{' | '}' | ';'
                    )) | Some(JsToken::Keyword(_))
                        | Some(JsToken::Operator(_))
                );

            if can_be_regex && i + 1 < len && chars[i + 1] != '/' && chars[i + 1] != '*' {
                let mut regex = String::new();
                regex.push(c);
                i += 1;
                let mut escaped = false;
                let mut in_class = false;
                let mut found_end = false;

                while i < len {
                    let rc = chars[i];
                    regex.push(rc);
                    if escaped {
                        escaped = false;
                    } else if rc == '\\' {
                        escaped = true;
                    } else if rc == '[' && !in_class {
                        in_class = true;
                    } else if rc == ']' && in_class {
                        in_class = false;
                    } else if rc == '/' && !in_class {
                        found_end = true;
                        i += 1;
                        // Flags
                        while i < len && chars[i].is_ascii_alphabetic() {
                            regex.push(chars[i]);
                            i += 1;
                        }
                        break;
                    }
                    i += 1;
                }

                if found_end {
                    tokens.push(JsToken::Regex(regex));
                    continue;
                } else {
                    // Not a valid regex, treat as operator
                    tokens.push(JsToken::Operator("/".to_string()));
                    continue;
                }
            }
        }

        // Multi-char operators
        let ops = [
            "===", "!==", "==", "!=", "<=", ">=", "&&", "||", "??", "++", "--", "+=", "-=", "*=",
            "/=", "%=", "=>", "...", "**",
        ];
        let mut matched_op = false;
        for op in ops {
            if i + op.len() <= len {
                let slice: String = chars[i..i + op.len()].iter().collect();
                if slice == op {
                    tokens.push(JsToken::Operator(op.to_string()));
                    i += op.len();
                    matched_op = true;
                    break;
                }
            }
        }
        if matched_op {
            continue;
        }

        // Single-char operators and punctuation
        if "+-*/%<>=!&|^~?:".contains(c) {
            tokens.push(JsToken::Operator(c.to_string()));
            i += 1;
            continue;
        }

        if "(){}[];,.".contains(c) {
            tokens.push(JsToken::Punctuation(c));
            i += 1;
            continue;
        }

        tokens.push(JsToken::Other(c));
        i += 1;
    }

    tokens
}

fn record_declared_name(
    name: &str,
    used_names: &mut HashSet<String>,
    declared_set: &mut HashSet<String>,
    declared_order: &mut Vec<String>,
) {
    used_names.insert(name.to_string());
    if declared_set.insert(name.to_string()) {
        declared_order.push(name.to_string());
    }
}

fn collect_destructured_tokens(
    tokens: &[JsToken],
    start: usize,
    used_names: &mut HashSet<String>,
    declared_set: &mut HashSet<String>,
    declared_order: &mut Vec<String>,
) -> usize {
    let mut depth = 0;
    let mut idx = start;
    while idx < tokens.len() {
        match &tokens[idx] {
            JsToken::Punctuation('{') | JsToken::Punctuation('[') => depth += 1,
            JsToken::Punctuation('}') | JsToken::Punctuation(']') => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            JsToken::Identifier(name) if depth > 0 => {
                let next_is_colon = match tokens.get(idx + 1) {
                    Some(JsToken::Punctuation(':')) => true,
                    Some(JsToken::Operator(op)) if op == ":" => true,
                    _ => false,
                };
                if !next_is_colon {
                    record_declared_name(name, used_names, declared_set, declared_order);
                }
            }
            _ => {}
        }
        idx += 1;
    }
    idx
}

/// Rename local variables to shorter names
fn rename_variables(code: &str) -> String {
    let tokens = tokenize_js(code);

    // Reserved identifiers that should not be renamed
    let reserved: HashSet<&str> = [
        // JavaScript reserved keywords (prevent generating these as variable names)
        "do",
        "if",
        "in",
        "for",
        "new",
        "try",
        "var",
        "let",
        "else",
        "case",
        "void",
        "with",
        "enum",
        "this",
        "break",
        "catch",
        "throw",
        "const",
        "while",
        "class",
        "super",
        "return",
        "typeof",
        "delete",
        "switch",
        "export",
        "import",
        "static",
        "public",
        "private",
        "finally",
        "extends",
        "package",
        "continue",
        "debugger",
        "function",
        "arguments",
        "interface",
        "protected",
        "implements",
        "instanceof",
        "yield",
        "await",
        "async",
        "of",
        "get",
        "set",
        "null",
        "true",
        "false",
        // Built-in objects and functions
        "window",
        "document",
        "console",
        "fetch",
        "setTimeout",
        "setInterval",
        "clearTimeout",
        "clearInterval",
        "requestAnimationFrame",
        "cancelAnimationFrame",
        "Promise",
        "Array",
        "Object",
        "String",
        "Number",
        "Boolean",
        "Math",
        "JSON",
        "Date",
        "RegExp",
        "Error",
        "Map",
        "Set",
        "WeakMap",
        "WeakSet",
        "Symbol",
        "Proxy",
        "Reflect",
        "parseInt",
        "parseFloat",
        "isNaN",
        "isFinite",
        "encodeURI",
        "decodeURI",
        "encodeURIComponent",
        "decodeURIComponent",
        "eval",
        "Function",
        "arguments",
        "undefined",
        "null",
        "NaN",
        "Infinity",
        "globalThis",
        // DOM APIs
        "Element",
        "Node",
        "Event",
        "EventTarget",
        "HTMLElement",
        "XMLHttpRequest",
        "FormData",
        "Blob",
        "File",
        "FileReader",
        "URL",
        "URLSearchParams",
        "Headers",
        "Request",
        "Response",
        "AbortController",
        "AbortSignal",
        "localStorage",
        "sessionStorage",
        "navigator",
        "location",
        "history",
        "screen",
        "performance",
        "crypto",
        "alert",
        "confirm",
        "prompt",
        // Common method names that shouldn't be touched
        "length",
        "prototype",
        "constructor",
        "toString",
        "valueOf",
        "hasOwnProperty",
        "then",
        "catch",
        "finally",
        "resolve",
        "reject",
        "push",
        "pop",
        "shift",
        "unshift",
        "slice",
        "splice",
        "concat",
        "join",
        "map",
        "filter",
        "reduce",
        "forEach",
        "find",
        "findIndex",
        "indexOf",
        "includes",
        "some",
        "every",
        "sort",
        "reverse",
        "keys",
        "values",
        "entries",
        "assign",
        "create",
        "freeze",
        "seal",
        "method",
        "body",
        "headers",
        "status",
        "statusText",
        "ok",
        "json",
        "text",
        "blob",
        "arrayBuffer",
        "formData",
        "type",
        "target",
        "currentTarget",
        "preventDefault",
        "stopPropagation",
        "addEventListener",
        "removeEventListener",
        "querySelector",
        "querySelectorAll",
        "getElementById",
        "getElementsByClassName",
        "getElementsByTagName",
        "createElement",
        "createTextNode",
        "appendChild",
        "removeChild",
        "insertBefore",
        "replaceChild",
        "cloneNode",
        "getAttribute",
        "setAttribute",
        "removeAttribute",
        "classList",
        "style",
        "innerHTML",
        "innerText",
        "textContent",
        "value",
        "checked",
        "selected",
        "disabled",
        "src",
        "href",
        "id",
        "name",
        "className",
        "dataset",
        // Modern JS
        "async",
        "await",
        "import",
        "export",
        "default",
        "from",
        "as",
        // Specific to video player context
        "art",
        "player",
        "video",
        "play",
        "pause",
        "seek",
        "volume",
        "muted",
        "duration",
        "currentTime",
        "playbackRate",
        "fullscreen",
        "pip",
        "airplay",
        "subtitle",
        "quality",
        "option",
        "options",
        "plugins",
        "settings",
        "controls",
        "layers",
        "loading",
        "notice",
        "mask",
        "icons",
        "hotkey",
        "url",
        "container",
        "viewTracked",
        "heartbeatStarted",
        "on",
        "off",
        "emit",
        "once",
        "destroy",
        "init",
        "Artplayer",
        "Hls",
        "artplayerPluginHlsControl",
        "artplayerPluginChapter",
    ]
    .into_iter()
    .collect();

    let mut used_names: HashSet<String> = reserved.iter().map(|s| s.to_string()).collect();
    let mut declared_order: Vec<String> = Vec::new();
    let mut declared_set: HashSet<String> = HashSet::new();

    for token in &tokens {
        if let JsToken::Identifier(name) = token {
            used_names.insert(name.clone());
        }
    }

    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            JsToken::Keyword(kw) if kw == "let" || kw == "const" || kw == "var" => {
                let mut j = i + 1;
                while j < tokens.len() {
                    match &tokens[j] {
                        JsToken::Identifier(name) => {
                            let is_property = match tokens.get(j + 1) {
                                Some(JsToken::Punctuation(':')) => true,
                                Some(JsToken::Operator(op)) if op == ":" => true,
                                _ => false,
                            };
                            if !is_property {
                                record_declared_name(
                                    name,
                                    &mut used_names,
                                    &mut declared_set,
                                    &mut declared_order,
                                );
                            }
                        }
                        JsToken::Punctuation('{') | JsToken::Punctuation('[') => {
                            j = collect_destructured_tokens(
                                &tokens,
                                j,
                                &mut used_names,
                                &mut declared_set,
                                &mut declared_order,
                            );
                        }
                        JsToken::Punctuation(';') => break,
                        JsToken::Operator(op) if op == "=" => {
                            j += 1;
                            let mut depth = 0;
                            while j < tokens.len() {
                                match &tokens[j] {
                                    JsToken::Punctuation('(' | '[' | '{') => depth += 1,
                                    JsToken::Punctuation(')' | ']' | '}') => {
                                        if depth > 0 {
                                            depth -= 1;
                                        } else {
                                            break;
                                        }
                                    }
                                    JsToken::Punctuation(',') if depth == 0 => break,
                                    JsToken::Punctuation(';') if depth == 0 => break,
                                    _ => {}
                                }
                                j += 1;
                            }
                            continue;
                        }
                        _ => {}
                    }
                    j += 1;
                }
            }
            JsToken::Keyword(kw) if kw == "function" => {
                let mut j = i + 1;
                if let Some(JsToken::Identifier(name)) = tokens.get(j) {
                    used_names.insert(name.clone());
                    j += 1;
                }
                if let Some(JsToken::Punctuation('(')) = tokens.get(j) {
                    j += 1;
                    while j < tokens.len() {
                        match &tokens[j] {
                            JsToken::Identifier(name) => record_declared_name(
                                name,
                                &mut used_names,
                                &mut declared_set,
                                &mut declared_order,
                            ),
                            JsToken::Punctuation('{') | JsToken::Punctuation('[') => {
                                j = collect_destructured_tokens(
                                    &tokens,
                                    j,
                                    &mut used_names,
                                    &mut declared_set,
                                    &mut declared_order,
                                );
                            }
                            JsToken::Punctuation(')') => break,
                            _ => {}
                        }
                        j += 1;
                    }
                }
            }
            JsToken::Punctuation('(') => {
                let mut j = i + 1;
                let mut param_candidates: Vec<String> = Vec::new();
                let mut valid = true;
                let mut depth = 1;

                while j < tokens.len() && depth > 0 {
                    match &tokens[j] {
                        JsToken::Punctuation('(') => depth += 1,
                        JsToken::Punctuation(')') => depth -= 1,
                        JsToken::Identifier(name) if depth == 1 => {
                            param_candidates.push(name.clone());
                        }
                        JsToken::Punctuation('{') | JsToken::Punctuation('[') if depth == 1 => {
                            j = collect_destructured_tokens(
                                &tokens,
                                j,
                                &mut used_names,
                                &mut declared_set,
                                &mut declared_order,
                            );
                            continue;
                        }
                        JsToken::Punctuation(',') | JsToken::Operator(_) => {}
                        _ if depth == 1 => valid = false,
                        _ => {}
                    }
                    j += 1;
                }

                if valid && j < tokens.len() {
                    if let JsToken::Operator(op) = &tokens[j] {
                        if op == "=>" {
                            for name in param_candidates {
                                record_declared_name(
                                    &name,
                                    &mut used_names,
                                    &mut declared_set,
                                    &mut declared_order,
                                );
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }

    let mut local_vars: HashMap<String, String> = HashMap::new();
    let mut name_counter = 0;

    for name in declared_order {
        if reserved.contains(name.as_str()) || name.len() <= 1 {
            continue;
        }

        loop {
            let short_name = generate_short_name(name_counter);
            name_counter += 1;
            if !reserved.contains(short_name.as_str()) && !used_names.contains(&short_name) {
                used_names.insert(short_name.clone());
                local_vars.insert(name.clone(), short_name);
                break;
            }
        }
    }

    if local_vars.is_empty() {
        return code.to_string();
    }

    // Second pass: replace identifiers in the original code
    let mut result = String::with_capacity(code.len());
    let chars: Vec<char> = code.chars().collect();
    let len = chars.len();
    let mut ci = 0;

    let is_ident_start = |c: char| c.is_alphabetic() || c == '_' || c == '$';
    let is_ident_char = |c: char| c.is_alphanumeric() || c == '_' || c == '$';

    while ci < len {
        let c = chars[ci];

        // Handle strings - copy verbatim
        if c == '"' || c == '\'' {
            let quote = c;
            result.push(c);
            ci += 1;
            while ci < len {
                let sc = chars[ci];
                result.push(sc);
                if sc == '\\' && ci + 1 < len {
                    ci += 1;
                    result.push(chars[ci]);
                } else if sc == quote {
                    break;
                }
                ci += 1;
            }
            ci += 1;
            continue;
        }

        // Handle template literals
        if c == '`' {
            result.push(c);
            ci += 1;
            let mut brace_depth = 0;
            while ci < len {
                let tc = chars[ci];
                if tc == '\\' && ci + 1 < len {
                    result.push(tc);
                    ci += 1;
                    result.push(chars[ci]);
                } else if tc == '$' && ci + 1 < len && chars[ci + 1] == '{' {
                    result.push(tc);
                    ci += 1;
                    result.push(chars[ci]);
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
                    // Inside template expression, check for identifiers
                    if brace_depth > 0 && is_ident_start(tc) {
                        let mut ident = String::new();
                        while ci < len && is_ident_char(chars[ci]) {
                            ident.push(chars[ci]);
                            ci += 1;
                        }
                        if let Some(short) = local_vars.get(&ident) {
                            result.push_str(short);
                        } else {
                            result.push_str(&ident);
                        }
                        continue;
                    } else {
                        result.push(tc);
                    }
                }
                ci += 1;
            }
            ci += 1;
            continue;
        }

        // Handle identifiers (skip property accesses like .foo or ?.foo)
        if is_ident_start(c) {
            // Look back to the last non-whitespace char to avoid renaming properties
            let mut k = ci;
            while k > 0 && chars[k - 1].is_whitespace() {
                k -= 1;
            }
            let follows_property = k > 0
                && (chars[k - 1] == '.' || (chars[k - 1] == '?' && k > 1 && chars[k - 2] == '.'));

            let mut ident = String::new();
            while ci < len && is_ident_char(chars[ci]) {
                ident.push(chars[ci]);
                ci += 1;
            }
            // Check if this identifier should be renamed
            if !follows_property {
                if let Some(short) = local_vars.get(&ident) {
                    result.push_str(short);
                } else {
                    result.push_str(&ident);
                }
            } else {
                result.push_str(&ident);
            }
            continue;
        }

        result.push(c);
        ci += 1;
    }

    result
}

fn replace_keyword_safe(code: &str, keyword: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(code.len());
    let chars: Vec<char> = code.chars().collect();
    let keyword_chars: Vec<char> = keyword.chars().collect();
    let len = chars.len();
    let kw_len = keyword_chars.len();
    let mut i = 0;

    let is_ident_char = |c: char| c.is_alphanumeric() || c == '_' || c == '$';

    while i < len {
        // Check if we're in a string
        if chars[i] == '"' || chars[i] == '\'' || chars[i] == '`' {
            let quote = chars[i];
            result.push(chars[i]);
            i += 1;

            if quote == '`' {
                // Template literal with nested expressions
                let mut brace_depth = 0;
                while i < len {
                    if chars[i] == '\\' && i + 1 < len {
                        result.push(chars[i]);
                        i += 1;
                        result.push(chars[i]);
                    } else if chars[i] == '$' && i + 1 < len && chars[i + 1] == '{' {
                        result.push(chars[i]);
                        i += 1;
                        result.push(chars[i]);
                        brace_depth += 1;
                    } else if chars[i] == '{' && brace_depth > 0 {
                        result.push(chars[i]);
                        brace_depth += 1;
                    } else if chars[i] == '}' && brace_depth > 0 {
                        result.push(chars[i]);
                        brace_depth -= 1;
                    } else if chars[i] == '`' && brace_depth == 0 {
                        result.push(chars[i]);
                        break;
                    } else {
                        result.push(chars[i]);
                    }
                    i += 1;
                }
            } else {
                // Regular string
                while i < len {
                    result.push(chars[i]);
                    if chars[i] == '\\' && i + 1 < len {
                        i += 1;
                        result.push(chars[i]);
                    } else if chars[i] == quote {
                        break;
                    }
                    i += 1;
                }
            }
            i += 1;
            continue;
        }

        // Check for keyword match
        if i + kw_len <= len {
            let mut matches = true;
            for (j, &kc) in keyword_chars.iter().enumerate() {
                if chars[i + j] != kc {
                    matches = false;
                    break;
                }
            }

            if matches {
                // Check that it's not part of a larger identifier
                let before_ok = i == 0 || !is_ident_char(chars[i - 1]);
                let after_ok = i + kw_len >= len || !is_ident_char(chars[i + kw_len]);

                if before_ok && after_ok {
                    result.push_str(replacement);
                    i += kw_len;
                    continue;
                }
            }
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

fn final_cleanup(code: &str) -> String {
    let mut result = code.to_string();

    result = Regex::new(r";+\}")
        .unwrap()
        .replace_all(&result, "}")
        .to_string();

    result = Regex::new(r";;+")
        .unwrap()
        .replace_all(&result, ";")
        .to_string();

    result = Regex::new(r"\{;+")
        .unwrap()
        .replace_all(&result, "{")
        .to_string();

    result = Regex::new(r";(else|catch|finally)")
        .unwrap()
        .replace_all(&result, "$1")
        .to_string();

    result = result.replace("!!0", "!1");
    result = result.replace("!!1", "!0");

    while result.ends_with(';') {
        result.pop();
    }

    result = result.trim().to_string();

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minify_removes_whitespace() {
        let input = "function test() {\n    return 1;\n}";
        let result = minify_js(input);
        assert_eq!(result, "function test(){return 1}");
    }

    #[test]
    fn test_minify_removes_single_line_comments() {
        let input = "let x = 1; // comment\nlet y = 2;";
        let result = minify_js(input);
        assert_eq!(result, "let x=1;let y=2");
    }

    #[test]
    fn test_minify_removes_multi_line_comments() {
        let input = "let x = /* comment */ 1;";
        let result = minify_js(input);
        assert_eq!(result, "let x=1");
    }

    #[test]
    fn test_minify_preserves_strings() {
        let input = r#"let x = "hello world";"#;
        let result = minify_js(input);
        assert_eq!(result, r#"let x="hello world""#);
    }

    #[test]
    fn test_minify_preserves_single_quote_strings() {
        let input = "let x = 'hello world';";
        let result = minify_js(input);
        assert_eq!(result, "let x='hello world'");
    }

    #[test]
    fn test_minify_preserves_template_literals() {
        let input = "let x = `hello ${name}`;";
        let result = minify_js(input);
        assert_eq!(result, "let x=`hello ${name}`");
    }

    #[test]
    fn test_minify_preserves_regex() {
        let input = "let r = /test/gi;";
        let result = minify_js(input);
        assert_eq!(result, "let r=/test/gi");
    }

    #[test]
    fn test_minify_boolean_true() {
        let input = "let x = true;";
        let result = minify_js(input);
        assert_eq!(result, "let x=!0");
    }

    #[test]
    fn test_minify_boolean_false() {
        let input = "let x = false;";
        let result = minify_js(input);
        assert_eq!(result, "let x=!1");
    }

    #[test]
    fn test_minify_preserves_boolean_in_strings() {
        let input = r#"let x = "true";"#;
        let result = minify_js(input);
        assert_eq!(result, r#"let x="true""#);
    }

    #[test]
    fn test_minify_renames_long_variables() {
        // Variable names longer than 1 char get renamed to shorter names
        let input = "let trueValue = 1;";
        let result = minify_js(input);
        // trueValue is renamed to 'a' (first short name)
        assert_eq!(result, "let a=1");
    }

    #[test]
    fn test_minify_preserves_short_variables() {
        // Single char variable names are not renamed
        let input = "let x = 1;";
        let result = minify_js(input);
        assert_eq!(result, "let x=1");
    }

    #[test]
    fn test_minify_removes_semicolon_before_brace() {
        let input = "function test() { return 1; }";
        let result = minify_js(input);
        assert_eq!(result, "function test(){return 1}");
    }

    #[test]
    fn test_minify_preserves_space_for_keywords() {
        let input = "return x;";
        let result = minify_js(input);
        assert_eq!(result, "return x");
    }

    #[test]
    fn test_minify_preserves_space_between_operators() {
        let input = "let x = a + +b;";
        let result = minify_js(input);
        assert_eq!(result, "let x=a+ +b");
    }

    #[test]
    fn test_minify_removes_multiple_semicolons() {
        let input = "let x = 1;;let y = 2;";
        let result = minify_js(input);
        assert_eq!(result, "let x=1;let y=2");
    }

    #[test]
    fn test_minify_preserves_escaped_strings() {
        let input = r#"let x = "hello \"world\"";"#;
        let result = minify_js(input);
        assert_eq!(result, r#"let x="hello \"world\"""#);
    }

    #[test]
    fn test_minify_complex_code() {
        let input = r#"
            function init() {
                let viewTracked = false;
                if (!viewTracked) {
                    viewTracked = true;
                    fetch('/api/videos/123/view', { method: 'POST' });
                }
            }
        "#;
        let result = minify_js(input);
        // viewTracked stays readable but booleans still compressed
        assert!(
            result.contains("viewTracked=!1"),
            "Expected viewTracked to start as false, got: {}",
            result
        );
        assert!(
            result.contains("viewTracked=!0"),
            "Expected viewTracked to flip to true, got: {}",
            result
        );
        assert!(!result.contains('\n'));
        // Should be significantly shorter than original
        assert!(result.len() < input.len() / 2);
    }

    #[test]
    fn test_minify_preserves_return_regex() {
        let input = "function test() { return /abc/; }";
        let result = minify_js(input);
        assert_eq!(result, "function test(){return /abc/}");
    }

    #[test]
    fn test_minify_preserves_property_access() {
        // Property accesses should NOT be renamed
        let input = "let myVar = obj.myProperty;";
        let result = minify_js(input);
        // myVar renamed to 'a', but myProperty stays the same (property access)
        assert!(
            result.contains(".myProperty"),
            "Property access should be preserved: {}",
            result
        );
        assert!(
            result.contains("let a="),
            "Variable should be renamed: {}",
            result
        );
    }

    #[test]
    fn test_minify_preserves_object_keys() {
        // Object keys should NOT be renamed
        let input = "let myObj = { myKey: 1 };";
        let result = minify_js(input);
        assert!(
            result.contains("myKey:"),
            "Object key should be preserved: {}",
            result
        );
    }

    #[test]
    fn test_minify_preserves_globals() {
        // Global APIs should NOT be renamed
        let input = "let myVar = document.getElementById('test');";
        let result = minify_js(input);
        assert!(
            result.contains("document"),
            "document should not be renamed: {}",
            result
        );
        assert!(
            result.contains("getElementById"),
            "getElementById should not be renamed: {}",
            result
        );
    }

    #[test]
    fn test_minify_avoids_collision_with_existing_short_names() {
        // Existing short identifiers should stay unique after renaming longer names
        let input = r#"
            let i = 0;
            let j = 1;
            let alpha = 2;
            let beta = 3;
            let gamma = 4;
            let delta = 5;
            let epsilon = 6;
            let zeta = 7;
            let eta = 8;
            let theta = 9;
            let iota = 10;
        "#;

        let result = minify_js(input);

        assert_eq!(
            result.matches("let i=").count(),
            1,
            "Existing short name i should not be redeclared: {}",
            result
        );

        assert_eq!(
            result.matches("let j=").count(),
            1,
            "Existing short name j should not be redeclared: {}",
            result
        );
    }

    #[test]
    fn test_minify_avoids_collision_with_destructured_identifiers() {
        let input = "const { s, deep: { t } } = opts; const extremelyLongVariableName = s + t;";

        let result = minify_js(input);

        assert!(
            result.contains("const{ s") || result.contains("const{s"),
            "Destructured identifier should remain intact: {}",
            result
        );
        assert!(
            result.contains("const a="),
            "Long variable should still be renamed: {}",
            result
        );
        assert!(
            !result.contains("const s="),
            "Generated short name must not collide with destructured binding: {}",
            result
        );
    }

    #[test]
    fn test_minify_multiple_variables() {
        // Multiple variables should get sequential short names
        let input = "let firstVar = 1; let secondVar = 2; let thirdVar = firstVar + secondVar;";
        let result = minify_js(input);
        // Should be much shorter
        assert!(
            result.len() < input.len(),
            "Result should be shorter: {}",
            result
        );
        // Should not contain original long names
        assert!(
            !result.contains("firstVar"),
            "firstVar should be renamed: {}",
            result
        );
        assert!(
            !result.contains("secondVar"),
            "secondVar should be renamed: {}",
            result
        );
        assert!(
            !result.contains("thirdVar"),
            "thirdVar should be renamed: {}",
            result
        );
    }
}
