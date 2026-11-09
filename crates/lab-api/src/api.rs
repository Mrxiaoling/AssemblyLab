//! HTTP 处理器与 DTO。
//! 注:utoipa/OpenAPI 导出在 M2(前端类型生成)前接入,当前先以 serde DTO 为准。

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use lab_adapters::nasm;
use lab_core::{isa::MemoryMap, IsaDefinition, LabError};
use lab_engine::{hx, Session, U64In};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::debug::AsmLineDto;

pub type SessionRef = Arc<Mutex<Session>>;

#[derive(Clone)]
pub struct AppState {
    sessions: Arc<Mutex<HashMap<u64, SessionRef>>>,
    next_id: Arc<AtomicU64>,
    data_dir: Arc<std::path::PathBuf>,
    storage: Arc<std::sync::OnceLock<Result<lab_storage::Storage, String>>>,
    pub debug_sessions: crate::debug::DebugState,
}

impl AppState {
    pub fn new() -> Self {
        let data_dir = std::env::var("ASSEMBLYLAB_DATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
            });
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
            data_dir: Arc::new(data_dir),
            storage: Arc::new(std::sync::OnceLock::new()),
            debug_sessions: crate::debug::DebugState::new(),
        }
    }

    pub fn storage(&self) -> Option<&lab_storage::Storage> {
        let cell = self.storage.get_or_init(|| {
            let db = std::env::var("ASSEMBLYLAB_DB")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| {
                    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assemblylab.db")
                });
            lab_storage::Storage::open(&db)
        });
        match cell {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::error!("SQLite 打开失败: {e}");
                None
            }
        }
    }

    pub fn data_dir(&self) -> &std::path::Path {
        &self.data_dir
    }

    pub fn load_isa(&self, id: &str) -> Result<lab_core::IsaDefinition, LabError> {
        lab_core::isa::IsaDefinition::load_from_dir(self.data_dir(), id)
    }

    fn insert(&self, session: Session) -> u64 {
        let id = session.id;
        self.sessions.lock().unwrap().insert(id, Arc::new(Mutex::new(session)));
        id
    }

    fn get(&self, id: u64) -> Option<SessionRef> {
        self.sessions.lock().unwrap().get(&id).cloned()
    }

    fn remove(&self, id: u64) -> Option<SessionRef> {
        self.sessions.lock().unwrap().remove(&id)
    }
}

// ---------------------------------------------------------------------------
// 错误响应
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    kind: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
}

pub struct ApiError(pub LabError);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            LabError::SessionNotFound => StatusCode::NOT_FOUND,
            LabError::BadRequest(_) | LabError::Assemble { .. } | LabError::IsaNotFound(_) => {
                StatusCode::BAD_REQUEST
            }
            LabError::ToolUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let line = match &self.0 {
            LabError::Assemble { line, .. } => Some(*line),
            _ => None,
        };
        let body = ErrorBody {
            error: ErrorDetail { kind: self.0.kind().to_string(), message: self.0.to_string(), line },
        };
        (status, Json(body)).into_response()
    }
}

pub type ApiResult<T> = Result<Json<T>, ApiError>;

// ---------------------------------------------------------------------------
// 请求/响应 DTO
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateSessionReq {
    pub isa: String,
    #[serde(default = "default_abi")]
    pub abi: String,
    pub source: String,
}

fn default_abi() -> String {
    "sysv-amd64".into()
}

#[derive(Serialize)]
pub struct SessionResp {
    pub id: u64,
    pub isa: String,
    pub abi: String,
    pub image: lab_engine::AssembleInfo,
    pub snapshot: Vec<(String, String)>,
    pub state: lab_engine::SessionState,
    pub breakpoints: Vec<String>,
}

fn session_resp(s: &mut Session) -> SessionResp {
    let snapshot = s
        .snapshot()
        .unwrap_or_default()
        .into_iter()
        .map(|(n, v)| (n, hx(v)))
        .collect();
    SessionResp {
        id: s.id,
        isa: s.isa.id.clone(),
        abi: s.abi.clone(),
        image: {
            let img = s.image.lock().unwrap().clone();
            lab_engine::AssembleInfo::from(&img, &s.isa)
        },
        snapshot,
        state: s.state(),
        breakpoints: s.breakpoints().into_iter().map(hx).collect(),
    }
}

#[derive(Serialize)]
pub struct HealthResp {
    pub ok: bool,
    pub tools: ToolStatus,
}

#[derive(Serialize)]
pub struct ToolStatus {
    pub nasm: Option<String>,
    pub unicorn: bool,
    /// WSL g++(模式 B 编译,None = WSL 工具链缺失)
    pub wsl_gpp: Option<String>,
    /// WSL mingw-w64 交叉编译器(Win64 ABI 对比;None = 未安装)
    pub mingw: Option<String>,
}

#[derive(Deserialize)]
pub struct StepReq {
    #[serde(default = "default_count")]
    pub count: u32,
}

fn default_count() -> u32 {
    1
}

#[derive(Serialize)]
pub struct StepResp {
    pub reports: Vec<lab_engine::StepReport>,
    pub state: lab_engine::SessionState,
}

#[derive(Serialize)]
pub struct RunResp {
    pub outcome: lab_engine::RunSummary,
    pub state: lab_engine::SessionState,
}

#[derive(Deserialize)]
pub struct PatchRegistersReq {
    /// 寄存器名 → 值(数字或 "0x.." 字符串)
    pub values: HashMap<String, U64In>,
}

#[derive(Deserialize)]
pub struct PatchMemoryReq {
    pub addr: U64In,
    /// 十六进制字节串(如 "0A00")
    pub hex: String,
}

#[derive(Deserialize)]
pub struct PutBreakpointsReq {
    pub addresses: Vec<U64In>,
}

#[derive(Deserialize)]
pub struct MemoryQuery {
    pub addr: U64In,
    #[serde(default = "default_mem_len")]
    pub len: u32,
}

fn default_mem_len() -> u32 {
    256
}

#[derive(Serialize)]
pub struct MemoryResp {
    pub addr: String,
    pub region: String,
    pub hex: String,
    pub ascii: String,
}

#[derive(Serialize)]
pub struct TraceResp {
    pub entries: Vec<lab_engine::TraceEntry>,
    pub dropped: u64,
}

#[derive(Serialize)]
pub struct DisasmResp {
    pub base: String,
    pub lines: Vec<DisasmLineDto>,
}

#[derive(Serialize)]
pub struct DisasmLineDto {
    pub address: String,
    pub size: u32,
    pub text: String,
    pub mnemonic: String,
    pub bytes_hex: String,
}

#[derive(Serialize)]
pub struct IsaListItem {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct IsaDetail {
    pub id: String,
    pub name: String,
    pub registers: Vec<lab_core::RegisterDef>,
    pub flags: Vec<lab_core::FlagDef>,
    pub abis: Vec<lab_core::AbiDefinition>,
    pub memory_map: MemoryMap,
}

// 序列化帮助:RegisterDef 等未实现 Serialize — 手工转换
fn isa_detail(isa: &lab_core::IsaDefinition) -> IsaDetail {
    IsaDetail {
        id: isa.id.clone(),
        name: isa.name.clone(),
        registers: isa.registers.clone(),
        flags: isa.flags.clone(),
        abis: isa.abis.clone(),
        memory_map: isa.memory_map.clone(),
    }
}

// ---------------------------------------------------------------------------
// 处理器
// ---------------------------------------------------------------------------

pub async fn health(State(_st): State<AppState>) -> ApiResult<HealthResp> {
    // 工具探测放 blocking 线程(进程启动)
    let nasm = tokio::task::spawn_blocking(|| nasm::nasm_version().ok()).await.unwrap_or(None);
    let unicorn = tokio::task::spawn_blocking(|| {
        lab_adapters::unicorn::UnicornMachine::new("unicorn:x86", "unicorn:x86-64").is_ok()
    })
    .await
    .unwrap_or(false);
    let wsl_gpp =
        tokio::task::spawn_blocking(|| lab_adapters::compiler::detect_toolchain()).await.unwrap_or(None);
    let mingw =
        tokio::task::spawn_blocking(|| lab_adapters::compiler::detect_mingw()).await.unwrap_or(None);
    Ok(Json(HealthResp { ok: true, tools: ToolStatus { nasm, unicorn, wsl_gpp, mingw } }))
}

pub async fn list_isas(State(st): State<AppState>) -> ApiResult<Vec<IsaListItem>> {
    let mut out = Vec::new();
    let dir = st.data_dir().join("isas");
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            if entry.path().join("isa.toml").is_file() {
                if let Ok(isa) = st.load_isa(&entry.file_name().to_string_lossy()) {
                    out.push(IsaListItem { id: isa.id, name: isa.name });
                }
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(out))
}

pub async fn get_isa(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<IsaDetail> {
    let isa = st.load_isa(&id).map_err(ApiError)?;
    Ok(Json(isa_detail(&isa)))
}

pub async fn create_session(
    State(st): State<AppState>,
    Json(req): Json<CreateSessionReq>,
) -> ApiResult<SessionResp> {
    let isa = st.load_isa(&req.isa).map_err(ApiError)?;
    let id = st.next_id.fetch_add(1, Ordering::SeqCst);
    let source = req.source.clone();
    let abi = req.abi.clone();
    let isa2 = isa.clone();
    let session = tokio::task::spawn_blocking(move || Session::create(id, isa2, abi, &source))
        .await
        .map_err(|e| ApiError(LabError::Emulator(format!("任务失败: {e}"))))?
        .map_err(ApiError)?;
    st.insert(session);
    let r = st.get(id).unwrap();
    let mut s = r.lock().unwrap();
    Ok(Json(session_resp(&mut s)))
}

async fn with_session<T, F>(
    st: &AppState,
    id: u64,
    f: F,
) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Session) -> Result<T, LabError> + Send + 'static,
{
    let Some(sess) = st.get(id) else {
        return Err(ApiError(LabError::SessionNotFound));
    };
    let result = tokio::task::spawn_blocking(move || {
        let mut s = sess.lock().unwrap();
        f(&mut s).map_err(ApiError)
    })
    .await
    .map_err(|e| ApiError(LabError::Emulator(format!("任务失败: {e}"))))?;
    result.map(Json)
}

pub async fn get_session(State(st): State<AppState>, Path(id): Path<u64>) -> ApiResult<SessionResp> {
    let sess = st.get(id).ok_or_else(|| ApiError(LabError::SessionNotFound))?;
    let resp = tokio::task::spawn_blocking(move || {
        let mut s = sess.lock().unwrap();
        session_resp(&mut s)
    })
    .await
    .map_err(|e| ApiError(LabError::Emulator(format!("任务失败: {e}"))))?;
    Ok(Json(resp))
}

pub async fn delete_session(State(st): State<AppState>, Path(id): Path<u64>) -> ApiResult<serde_json::Value> {
    st.remove(id).ok_or(ApiError(LabError::SessionNotFound))?;
    Ok(Json(serde_json::json!({ "deleted": id })))
}

pub async fn post_source(
    State(st): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<CreateSessionReq>,
) -> ApiResult<SessionResp> {
    with_session(&st, id, move |s| {
        s.reassemble(&req.source)?;
        Ok(session_resp(s))
    })
    .await
}

pub async fn step(State(st): State<AppState>, Path(id): Path<u64>, Json(req): Json<StepReq>) -> ApiResult<StepResp> {
    with_session(&st, id, move |s| {
        let reports = s.step(req.count)?;
        let state = s.state();
        Ok(StepResp { reports, state })
    })
    .await
}

pub async fn run(State(st): State<AppState>, Path(id): Path<u64>) -> ApiResult<RunResp> {
    with_session(&st, id, move |s| {
        let outcome = s.run()?;
        let state = s.state();
        Ok(RunResp { outcome, state })
    })
    .await
}

pub async fn reset(State(st): State<AppState>, Path(id): Path<u64>) -> ApiResult<SessionResp> {
    with_session(&st, id, move |s| {
        s.reset()?;
        Ok(session_resp(s))
    })
    .await
}

pub async fn patch_registers(
    State(st): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<PatchRegistersReq>,
) -> ApiResult<SessionResp> {
    with_session(&st, id, move |s| {
        for (name, v) in &req.values {
            s.set_register(name, v.parse()?)?;
        }
        Ok(session_resp(s))
    })
    .await
}

pub async fn patch_memory(
    State(st): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<PatchMemoryReq>,
) -> ApiResult<serde_json::Value> {
    let addr = req.addr.parse().map_err(ApiError)?;
    let bytes = hex::decode(&req.hex)
        .map_err(|e| ApiError(LabError::BadRequest(format!("hex 解析失败: {e}"))))?;
    with_session(&st, id, move |s| {
        s.write_memory(addr, &bytes)?;
        Ok(serde_json::json!({ "written": bytes.len() }))
    })
    .await
}

pub async fn get_memory(
    State(st): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<MemoryQuery>,
) -> ApiResult<MemoryResp> {
    let addr = q.addr.parse().map_err(ApiError)?;
    let len = q.len.min(4096);
    with_session(&st, id, move |s| {
        let view = s.read_memory(addr, len)?;
        let ascii: String = view
            .bytes
            .iter()
            .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
            .collect();
        Ok(MemoryResp { addr: hx(view.addr), region: view.region, hex: hex::encode_upper(&view.bytes), ascii })
    })
    .await
}

pub async fn put_breakpoints(
    State(st): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<PutBreakpointsReq>,
) -> ApiResult<Vec<String>> {
    let addrs: Vec<u64> = req
        .addresses
        .iter()
        .map(|a| a.parse().map_err(ApiError))
        .collect::<Result<_, _>>()?;
    with_session(&st, id, move |s| {
        let valid = s.set_breakpoints(&addrs)?;
        Ok(valid.into_iter().map(hx).collect())
    })
    .await
}

pub async fn get_trace(State(st): State<AppState>, Path(id): Path<u64>) -> ApiResult<TraceResp> {
    with_session(&st, id, move |s| {
        let (entries, dropped) = s.trace();
        Ok(TraceResp { entries, dropped })
    })
    .await
}

pub async fn get_disasm(State(st): State<AppState>, Path(id): Path<u64>) -> ApiResult<DisasmResp> {
    with_session(&st, id, move |s| {
        let img = s.image.lock().unwrap().clone();
        let isa_id = s.isa.id.clone();
        let lines = lab_engine::disassemble_image(&isa_id, &img)?
            .into_iter()
            .map(|l| DisasmLineDto {
                address: hx(l.address),
                size: l.size,
                text: l.text,
                mnemonic: l.mnemonic,
                bytes_hex: l.bytes_hex,
            })
            .collect();
        Ok(DisasmResp { base: hx(img.base), lines })
    })
    .await
}

// ---------------------------------------------------------------------------
// 优化级别对比(P3):同一源码 × 多优化级别(× 目标编译器)汇编产物
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CompareReq {
    pub source: String,
    /// 默认 O0..O3;子集可选
    #[serde(default = "default_compare_opts")]
    pub opts: Vec<String>,
    /// "g++"(SysV,默认)| "x86_64-w64-mingw32-g++"(Win64 ABI)
    #[serde(default)]
    pub compiler: String,
}

fn default_compare_opts() -> Vec<String> {
    vec!["O0".into(), "O1".into(), "O2".into(), "O3".into()]
}

#[derive(Serialize)]
pub struct CompareEntry {
    pub opt: String,
    pub compiler: String,
    pub compiler_version: String,
    pub asm_lines: Vec<AsmLineDto>,
    pub instruction_count: usize,
}

pub async fn compare(
    State(_st): State<AppState>,
    Json(req): Json<CompareReq>,
) -> ApiResult<Vec<CompareEntry>> {
    const ALLOWED: [&str; 4] = ["O0", "O1", "O2", "O3"];
    let mut opts: Vec<String> = req
        .opts
        .iter()
        .filter(|o| ALLOWED.contains(&o.as_str()))
        .cloned()
        .collect();
    if opts.is_empty() {
        return Err(ApiError(LabError::BadRequest("opts 至少需要一个 O0..O3".into())));
    }
    // 去重并按 O0→O3 排序(便于对照阅读)
    opts.sort();
    opts.dedup();
    if opts.len() > 4 {
        return Err(ApiError(LabError::BadRequest("opts 最多 4 项".into())));
    }
    // 编译器名白名单(防注入;clang++ 供协程/多编译器对比)
    let compiler = match req.compiler.as_str() {
        "" | "g++" => "g++".to_string(),
        "clang++" => "clang++".to_string(),
        "x86_64-w64-mingw32-g++" => "x86_64-w64-mingw32-g++".to_string(),
        "aarch64-linux-gnu-g++" => "aarch64-linux-gnu-g++".to_string(),
        other => {
            return Err(ApiError(LabError::BadRequest(format!(
                "不支持的编译器: {other}(可选 g++ / clang++ / x86_64-w64-mingw32-g++ / aarch64-linux-gnu-g++)"
            ))))
        }
    };

    let source = req.source;
    let entries = tokio::task::spawn_blocking(move || -> Result<Vec<CompareEntry>, LabError> {
        let mut out = Vec::new();
        for opt in &opts {
            let out_c = lab_adapters::compiler::compile(&lab_adapters::compiler::CompileRequest {
                source: source.clone(),
                opt: opt.clone(),
                compiler: compiler.clone(),
            })?;
            let instruction_count = out_c.asm_lines.iter().filter(|l| l.addr.is_some()).count();
            out.push(CompareEntry {
                opt: opt.clone(),
                compiler_version: out_c.compiler_version.clone(),
                compiler: compiler.clone(),
                instruction_count,
                asm_lines: out_c
                    .asm_lines
                    .iter()
                    .map(|l| AsmLineDto {
                        addr: l.addr.map(|a| hx(a)),
                        text: l.text.clone(),
                        function: l.function.clone(),
                        source_line: l.source_line,
                    })
                    .collect(),
            });
            // 对比产物只取反汇编,目录即刻清理
            lab_adapters::compiler::cleanup(&out_c.work_dir);
        }
        Ok(out)
    })
    .await
    .map_err(|e| ApiError(LabError::Emulator(format!("任务失败: {e}"))))?;
    Ok(Json(entries.map_err(ApiError)?))
}
