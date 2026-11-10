//! 本地 API 防护层:可选 token 鉴权 + 固定窗口限流 + 跨域来源校验。
//!
//! 服务只绑定 127.0.0.1,但同源内其它进程/恶意网页仍可发请求(尤其会消耗
//! LLM 额度、触发不可信代码执行)。因此默认做来源与频率限制:
//! - `ASSEMBLYLAB_API_TOKEN`:设置后所有请求需带 `X-AssemblyLab-Token`。
//! - `ASSEMBLYLAB_RATE_LIMIT`:全局每分钟请求上限(默认 600)。
//! - `ASSEMBLYLAB_LLM_RATE_LIMIT`:LLM 每分钟请求上限(默认 10)。
//! - `Origin` 若存在,只允许本机来源(localhost/127.0.0.1/[::1]);`null`(file://)放行。

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const TOKEN_HEADER: &str = "x-assemblylab-token";

struct Config {
    token: Option<String>,
    max_per_min: u32,
    llm_max_per_min: u32,
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

fn config() -> &'static Config {
    static CONFIG: OnceLock<Config> = OnceLock::new();
    CONFIG.get_or_init(|| Config {
        token: std::env::var("ASSEMBLYLAB_API_TOKEN")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        max_per_min: env_u32("ASSEMBLYLAB_RATE_LIMIT", 600),
        llm_max_per_min: env_u32("ASSEMBLYLAB_LLM_RATE_LIMIT", 10),
    })
}

struct Window {
    start: Instant,
    count: u32,
}

fn windows() -> &'static Mutex<HashMap<String, Window>> {
    static WINDOWS: OnceLock<Mutex<HashMap<String, Window>>> = OnceLock::new();
    WINDOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn allow(key: &str, limit: u32) -> bool {
    let mut map = match windows().lock() {
        Ok(m) => m,
        Err(poisoned) => poisoned.into_inner(),
    };
    let now = Instant::now();
    let w = map.entry(key.to_string()).or_insert(Window { start: now, count: 0 });
    if now.duration_since(w.start) >= Duration::from_secs(60) {
        w.start = now;
        w.count = 0;
    }
    if w.count >= limit {
        return false;
    }
    w.count += 1;
    true
}

/// 所有路径段只允许 `[A-Za-z0-9_-]`:拒绝 `..`、路径分隔符与百分号编码,
/// 从入口统一挡掉 courses/export/progress/isas 等接口的 id 路径穿越。
fn path_is_safe(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if lower.contains("..")
        || lower.contains("%2e")
        || lower.contains("%2f")
        || lower.contains("%5c")
        || lower.contains('%')
        || lower.contains('\\')
    {
        return false;
    }
    path.split('/')
        .filter(|s| !s.is_empty())
        .all(|s| s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')))
}

fn origin_is_local(raw: &str) -> bool {
    let v = raw.trim();
    if v == "null" {
        return true; // file:// 页面
    }
    let rest = v
        .strip_prefix("http://")
        .or_else(|| v.strip_prefix("https://"))
        .unwrap_or(v);
    let host = rest.split('/').next().unwrap_or(rest);
    let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    let host = host.trim_matches(|c| c == '[' || c == ']');
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn reject(status: StatusCode, kind: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "error": { "kind": kind, "message": message } })),
    )
        .into_response()
}

pub async fn guard(req: Request<Body>, next: Next) -> Response {
    let cfg = config();

    if !path_is_safe(req.uri().path()) {
        return reject(StatusCode::BAD_REQUEST, "invalid_path", "非法请求路径");
    }

    if let Some(origin) = req.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        if !origin_is_local(origin) {
            return reject(
                StatusCode::FORBIDDEN,
                "forbidden_origin",
                "仅允许本机来源访问该服务",
            );
        }
    }

    if let Some(token) = &cfg.token {
        let got = req
            .headers()
            .get(TOKEN_HEADER)
            .and_then(|v| v.to_str().ok());
        if got != Some(token.as_str()) {
            return reject(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "需要 X-AssemblyLab-Token 请求头",
            );
        }
    }

    let is_llm = req.uri().path() == "/api/tutor/llm";
    let (key, limit) = if is_llm {
        ("llm", cfg.llm_max_per_min)
    } else {
        ("global", cfg.max_per_min)
    };
    if !allow(key, limit) {
        return reject(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "请求过于频繁,请稍后再试",
        );
    }

    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_origins_allowed() {
        for o in [
            "http://127.0.0.1:5173",
            "http://localhost:7600",
            "http://[::1]:8080",
            "null",
        ] {
            assert!(origin_is_local(o), "{o}");
        }
    }

    #[test]
    fn remote_origins_rejected() {
        for o in ["http://evil.example.com", "http://10.20.15.47", "https://attacker.cn"] {
            assert!(!origin_is_local(o), "{o}");
        }
    }

    #[test]
    fn path_traversal_blocked() {
        for p in [
            "/api/courses/../secrets",
            "/api/courses/%2e%2e%2fetc",
            "/api/export/course/..\\secret",
            "/api/exercises/stage01-q01/../../data",
        ] {
            assert!(!path_is_safe(p), "{p}");
        }
        assert!(path_is_safe("/api/courses/stage01-lesson01"));
        assert!(path_is_safe("/api/exercises/stage06-q04-cpp2asm/answer"));
        assert!(path_is_safe("/api/isas/x86-64"));
    }
}
