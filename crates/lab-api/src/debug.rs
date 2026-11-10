//! 模式 B 调试路由。

use crate::api::{ApiError, ApiResult, AppState};
use axum::extract::{Path, State};
use axum::Json;
use lab_core::{isa::IsaDefinition, LabError};
use lab_engine::debug::DebugSession;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub type DebugRef = Arc<DebugSession>;

/// 模式 B 每个会话都持有 gdb/socat(ARM64 还含 ssh 隧道与远端 gdbserver),必须限量。
pub const MAX_DEBUG_SESSIONS: usize = 8;

#[derive(Clone)]
pub struct DebugState {
    sessions: Arc<Mutex<HashMap<u64, DebugRef>>>,
    last_used: Arc<Mutex<HashMap<u64, std::time::Instant>>>,
    next_id: Arc<AtomicU64>,
}

impl DebugState {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            last_used: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn can_create(&self) -> bool {
        self.sweep(std::time::Duration::from_secs(30 * 60));
        self.sessions.lock().unwrap().len() < MAX_DEBUG_SESSIONS
    }

    fn insert(&self, s: DebugSession) -> u64 {
        let id = s.id;
        self.sessions.lock().unwrap().insert(id, Arc::new(s));
        self.last_used.lock().unwrap().insert(id, std::time::Instant::now());
        id
    }
    fn get(&self, id: u64) -> Option<DebugRef> {
        let s = self.sessions.lock().unwrap().get(&id).cloned();
        if s.is_some() {
            self.last_used.lock().unwrap().insert(id, std::time::Instant::now());
        }
        s
    }
    fn remove(&self, id: u64) -> Option<DebugRef> {
        self.last_used.lock().unwrap().remove(&id);
        self.sessions.lock().unwrap().remove(&id)
    }

    /// 回收空闲会话;移除 Arc 后 DebugSession::drop 会清理远端 gdbserver 与临时目录。
    pub fn sweep(&self, ttl: std::time::Duration) {
        let now = std::time::Instant::now();
        let mut expired = Vec::new();
        if let Ok(mut used) = self.last_used.lock() {
            used.retain(|id, t| {
                let keep = now.duration_since(*t) < ttl;
                if !keep {
                    expired.push(*id);
                }
                keep
            });
        }
        if !expired.is_empty() {
            let n = expired.len();
            let mut sessions = self.sessions.lock().unwrap();
            for id in expired {
                sessions.remove(&id);
            }
            tracing::info!("已回收 {n} 个空闲调试会话");
        }
    }
}

// ---------------------------------------------------------------------------
// DTO
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateDebugReq {
    pub source: String,
    #[serde(default = "default_opt")]
    pub opt: String,
    /// g++(默认)| clang++(协程帧 DWARF 最完整)
    #[serde(default)]
    pub compiler: String,
    /// 模式 B 目标架构(默认 x86-64 本地;arm64 = 远程真机 gdbserver)
    #[serde(default)]
    pub isa: String,
}

fn default_opt() -> String {
    "O0".into()
}

#[derive(Serialize)]
pub struct AsmLineDto {
    pub addr: Option<String>,
    pub text: String,
    pub function: Option<String>,
    pub source_line: Option<u32>,
}

#[derive(Serialize)]
pub struct FrameDto {
    pub level: u32,
    pub func: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub addr: Option<String>,
}

#[derive(Serialize)]
pub struct DebugSessionResp {
    pub id: u64,
    pub asm_lines: Vec<AsmLineDto>,
    pub compiler: String,
    pub opt: String,
    pub state: lab_engine::debug::DebugState,
    pub breakpoints: Vec<u32>,
    pub watchpoints: Vec<String>,
}

fn session_resp(s: &DebugSession) -> DebugSessionResp {
    DebugSessionResp {
        id: s.id,
        asm_lines: s
            .asm_lines()
            .into_iter()
            .map(|l| AsmLineDto {
                addr: l.addr.map(|a| lab_engine::hx(a)),
                text: l.text,
                function: l.function,
                source_line: l.source_line,
            })
            .collect(),
        compiler: s.compile.compiler_version.clone(),
        opt: s.compile.opt.clone(),
        state: s.state(),
        breakpoints: s.breakpoints(),
        watchpoints: s.watchpoints(),
    }
}

#[derive(Serialize)]
pub struct DebugStepResp {
    pub reports: Vec<lab_engine::StepReport>,
    pub state: lab_engine::debug::DebugState,
}

#[derive(Deserialize)]
pub struct StepReq {
    #[serde(default = "default_count")]
    pub count: u32,
    #[serde(default = "default_into")]
    pub into: bool,
}

fn default_count() -> u32 {
    1
}
fn default_into() -> bool {
    true
}

#[derive(Deserialize)]
pub struct PutBpsReq {
    pub lines: Vec<u32>,
}

#[derive(Deserialize)]
pub struct PutWatchReq {
    pub expressions: Vec<String>,
}

async fn with_debug<T, F>(st: &DebugState, id: u64, f: F) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce(&DebugSession) -> Result<T, String> + Send + 'static,
{
    let Some(sess) = st.get(id) else {
        return Err(ApiError(LabError::SessionNotFound));
    };
    let result = tokio::task::spawn_blocking(move || f(&sess).map_err(|e| ApiError(LabError::BadRequest(e))))
        .await
        .map_err(|e| ApiError(LabError::Emulator(format!("任务失败: {e}"))))?;
    result.map(Json)
}

// ---------------------------------------------------------------------------
// 处理器
// ---------------------------------------------------------------------------

pub async fn create_session(
    State(app): State<AppState>,
    Json(req): Json<CreateDebugReq>,
) -> ApiResult<DebugSessionResp> {
    if !app.debug_sessions.can_create() {
        return Err(ApiError(LabError::BadRequest(format!(
            "模式 B 会话数已达上限 {MAX_DEBUG_SESSIONS},请先删除旧会话"
        ))));
    }
    let isa: IsaDefinition = app.load_isa(if req.isa.is_empty() { "x86-64" } else { &req.isa }).map_err(ApiError)?;
    let id = app.debug_sessions.next_id.fetch_add(1, Ordering::SeqCst);
    let source = req.source.clone();
    let opt = req.opt.clone();
    let compiler = if req.compiler.is_empty() { "g++".to_string() } else { req.compiler.clone() };
    let session = tokio::task::spawn_blocking(move || {
        lab_engine::debug::DebugSession::create_with_compiler(id, isa, &source, &opt, &compiler)
    })
    .await
    .map_err(|e| ApiError(LabError::Emulator(format!("任务失败: {e}"))))?
    .map_err(|e| ApiError(LabError::BadRequest(e)))?;
    app.debug_sessions.insert(session);
    let r = app.debug_sessions.get(id).unwrap();
    Ok(Json(session_resp(&r)))
}

pub async fn delete_session(State(app): State<AppState>, Path(id): Path<u64>) -> ApiResult<serde_json::Value> {
    app.debug_sessions.remove(id).ok_or_else(|| ApiError(LabError::SessionNotFound))?;
    Ok(Json(serde_json::json!({ "deleted": id })))
}

pub async fn step(
    State(app): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<StepReq>,
) -> ApiResult<DebugStepResp> {
    with_debug(&app.debug_sessions, id, move |s| {
        let reports = s.step(req.into, req.count)?;
        let state = s.state();
        Ok(DebugStepResp { reports, state })
    })
    .await
}

pub async fn run(State(app): State<AppState>, Path(id): Path<u64>) -> ApiResult<DebugStepResp> {
    with_debug(&app.debug_sessions, id, move |s| {
        let report = s.run()?;
        let state = s.state();
        Ok(DebugStepResp { reports: vec![report], state })
    })
    .await
}

pub async fn finish(State(app): State<AppState>, Path(id): Path<u64>) -> ApiResult<DebugStepResp> {
    with_debug(&app.debug_sessions, id, move |s| {
        let report = s.finish()?;
        let state = s.state();
        Ok(DebugStepResp { reports: vec![report], state })
    })
    .await
}

pub async fn put_breakpoints(
    State(app): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<PutBpsReq>,
) -> ApiResult<Vec<u32>> {
    with_debug(&app.debug_sessions, id, move |s| s.set_breakpoints(&req.lines)).await
}

/// 观察点(整表替换;程序未启动时自动 run 到 main)
pub async fn put_watchpoints(
    State(app): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<PutWatchReq>,
) -> ApiResult<Vec<String>> {
    with_debug(&app.debug_sessions, id, move |s| s.set_watchpoints(&req.expressions)).await
}

// ---------------------------------------------------------------------------
// 协程(P4):帧布局 + 挂起/恢复时间线
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct CoroStopDto {
    pub line: u32,
    pub func: Option<String>,
    pub reason: String,
    pub coro_index: Option<i32>,
    pub rip: Option<String>,
    pub frame_addr: Option<String>,
    pub frame_fields: Vec<(String, String)>,
}

#[derive(Serialize)]
pub struct CoroResp {
    pub compiler: String,
    pub timeline: Vec<CoroStopDto>,
    pub timeline_error: Option<String>,
}

/// 一次性收集挂起/恢复时间线(每站附协程帧快照;未启动会自动 run 到 main)。
pub async fn get_coro(State(app): State<AppState>, Path(id): Path<u64>) -> ApiResult<CoroResp> {
    with_debug(&app.debug_sessions, id, move |s| {
        let mut resp = CoroResp {
            compiler: s.compile.compiler_version.clone(),
            timeline: Vec::new(),
            timeline_error: None,
        };
        match s.coro_timeline() {
            Ok(stops) => {
                resp.timeline = stops
                    .into_iter()
                    .map(|t| CoroStopDto {
                        line: t.line,
                        func: t.func,
                        reason: t.reason,
                        coro_index: t.coro_index,
                        rip: t.rip,
                        frame_addr: t.frame_addr,
                        frame_fields: t.frame_fields,
                    })
                    .collect();
            }
            Err(e) => resp.timeline_error = Some(e),
        }
        Ok(resp)
    })
    .await
}

#[derive(Serialize)]
pub struct FramesResp {
    pub frames: Vec<FrameDto>,
    pub variables: Vec<(String, String)>,
}

pub async fn get_frames(State(app): State<AppState>, Path(id): Path<u64>) -> ApiResult<FramesResp> {
    with_debug(&app.debug_sessions, id, move |s| {
        Ok(FramesResp {
            frames: s
                .frames()
                .into_iter()
                .map(|f| FrameDto {
                    level: f.level,
                    func: f.func,
                    file: f.file,
                    line: f.line,
                    addr: f.addr.map(|a| lab_engine::hx(a)),
                })
                .collect(),
            variables: s.variables(),
        })
    })
    .await
}

#[derive(Deserialize)]
pub struct MemoryQuery {
    pub addr: String,
    #[serde(default = "default_len")]
    pub len: u32,
}

fn default_len() -> u32 {
    64
}

#[derive(Serialize)]
pub struct DebugMemoryResp {
    pub addr: String,
    pub hex: String,
    pub ascii: String,
}

pub async fn get_memory(
    State(app): State<AppState>,
    Path(id): Path<u64>,
    axum::extract::Query(q): axum::extract::Query<MemoryQuery>,
) -> ApiResult<DebugMemoryResp> {
    let addr = lab_core::isa::parse_int(&q.addr).map_err(ApiError)?;
    let len = q.len.min(4096);
    with_debug(&app.debug_sessions, id, move |s| {
        let bytes = s.read_memory(addr, len)?;
        let ascii: String = bytes
            .iter()
            .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
            .collect();
        Ok(DebugMemoryResp { addr: lab_engine::hx(addr), hex: hex::encode_upper(&bytes), ascii })
    })
    .await
}
