//! lab-api — Assembly Lab 本地服务(axum)。
//! 启动:cargo run -p lab-api(监听 127.0.0.1:7600,仅本地回环,见 SECURITY_DESIGN §5)

mod api;
mod content;
mod debug;
mod export;
mod knowledge;
mod tutor;

use axum::routing::{delete, get, patch, post, put};
use axum::Router;

#[tokio::main]
async fn main() {
    tracing_subscriber_init();
    sweep_temp_dirs();
    let state = api::AppState::new();
    let base = Router::new()
        .route("/api/health", get(api::health))
        .route("/api/isas", get(api::list_isas))
        .route("/api/isas/{id}", get(api::get_isa))
        .route("/api/sessions", post(api::create_session))
        .route("/api/sessions/{id}", get(api::get_session).delete(api::delete_session))
        .route("/api/sessions/{id}/source", post(api::post_source))
        .route("/api/sessions/{id}/step", post(api::step))
        .route("/api/sessions/{id}/run", post(api::run))
        .route("/api/sessions/{id}/reset", post(api::reset))
        .route("/api/sessions/{id}/registers", patch(api::patch_registers))
        .route("/api/sessions/{id}/memory", patch(api::patch_memory).get(api::get_memory))
        .route("/api/sessions/{id}/breakpoints", put(api::put_breakpoints))
        .route("/api/sessions/{id}/trace", get(api::get_trace))
        .route("/api/sessions/{id}/disasm", get(api::get_disasm))
        .route("/api/compare", post(api::compare))
        .route("/api/courses", get(content::list_courses))
        .route("/api/courses/{id}", get(content::get_course))
        .route("/api/courses/{id}/experiment", get(content::get_course_experiment))
        .route("/api/exercises", get(content::list_exercises))
        .route("/api/exercises/{id}", get(content::get_exercise))
        .route("/api/exercises/{id}/submit", post(content::submit_exercise))
        .route("/api/progress/{id}", get(content::get_progress).put(content::put_progress))
        .route("/api/knowledge", get(knowledge::get_knowledge))
        .route("/api/mistakes", get(knowledge::get_mistakes))
        .route("/api/tutor", post(tutor::tutor))
        .route("/api/export/course/{id}", get(export::export_course))
        .route("/api/export/all", get(export::export_all));

    // 模式 B(真机调试)默认关闭——README SECURITY:做不到强隔离要默认禁用。
    // 明确知情后以 ASSEMBLYLAB_ENABLE_UNSAFE_DEBUG=1 开启(WSL gdb + 用户编译的原生二进制,
    // 隔离等级 = 进程级限制,非强沙箱;详见 docs/SECURITY_DESIGN.md)
    let unsafe_debug = std::env::var("ASSEMBLYLAB_ENABLE_UNSAFE_DEBUG").ok().as_deref() == Some("1");
    let app = if unsafe_debug {
        base
            .route("/api/debug/sessions", post(debug::create_session))
            .route("/api/debug/sessions/{id}", delete(debug::delete_session))
            .route("/api/debug/sessions/{id}/step", post(debug::step))
            .route("/api/debug/sessions/{id}/run", post(debug::run))
            .route("/api/debug/sessions/{id}/finish", post(debug::finish))
            .route("/api/debug/sessions/{id}/breakpoints", put(debug::put_breakpoints))
            .route("/api/debug/sessions/{id}/watchpoints", put(debug::put_watchpoints))
            .route("/api/debug/sessions/{id}/coro", get(debug::get_coro))
            .route("/api/debug/sessions/{id}/frames", get(debug::get_frames))
            .route("/api/debug/sessions/{id}/memory", get(debug::get_memory))
            .with_state(state)
    } else {
        base.with_state(state)
            .route(
                "/api/debug/{*rest}",
                axum::routing::any(|| async {
                    axum::Json(serde_json::json!({
                        "error": {
                            "kind": "debug_disabled",
                            "message": "模式 B(真机调试)默认关闭:隔离等级为进程级限制,非强沙箱。知情后以 ASSEMBLYLAB_ENABLE_UNSAFE_DEBUG=1 启动后端开启。"
                        }
                    }))
                }),
            )
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:7600").await.unwrap();
    tracing::info!("Assembly Lab API listening on http://127.0.0.1:7600");
    axum::serve(listener, app).await.unwrap();
}

fn sweep_temp_dirs() {
    // T2.7:清理历史会话遗留的编译临时目录(SECURITY_DESIGN §4)
    lab_adapters::compiler::sweep_orphan_dirs();
}

fn tracing_subscriber_init() {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
}
