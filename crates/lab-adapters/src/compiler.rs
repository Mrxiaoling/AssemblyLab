//! C++ 编译器适配(模式 B,经 WSL2 调用真实 g++/clang++)。
//!
//! 安全(SECURITY_DESIGN §3/§4):编译在 WSL 内以 `timeout` + `ulimit` 包裹;
//! 源码写入临时目录(会话结束清理);编译器输出大小截断。
//! 源码↔汇编映射:objdump -d -l -M intel(与 Compiler Explorer 同思路)。

use crate::process;
use lab_core::LabError;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct CompileRequest {
    pub source: String,
    /// "O0" | "O1" | "O2" | "O3"
    pub opt: String,
    /// "g++"(P2 默认;clang++ 预留)
    pub compiler: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CompileError {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AsmLine {
    /// 指令地址(数据/标签行可能为 None)
    pub addr: Option<u64>,
    /// 反汇编文本(无原始字节)
    pub text: String,
    /// 所属函数
    pub function: Option<String>,
    /// 对应 C++ 源码行(有调试信息时)
    pub source_line: Option<u32>,
    pub source_file: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CompileOutput {
    pub asm_lines: Vec<AsmLine>,
    pub errors: Vec<CompileError>,
    /// 编译产物目录(含 main.o / main 可执行文件),调试会话沿用;用毕清理
    pub work_dir: PathBuf,
    /// WSL 侧路径(传给 gdb)
    pub wsl_dir: String,
    pub compiler_version: String,
    pub opt: String,
}

const WSL_DISTRO: &str = "Ubuntu-22.04";
const COMPILE_TIMEOUT_SECS: u64 = 30;

/// Windows 临时目录 → WSL 路径
pub fn to_wsl_path(p: &Path) -> Result<String, LabError> {
    let win = p.to_str().ok_or_else(|| LabError::BadRequest("路径非 UTF-8".into()))?;
    let fwd = win.replace('\\', "/");
    if fwd.contains('\'') || fwd.contains('\n') || fwd.contains('$') || fwd.contains('`') {
        return Err(LabError::BadRequest("临时路径含非法字符".into()));
    }
    let out = wsl_bash(&format!("wslpath -a '{fwd}'"))?;
    let s = out.trim().to_string();
    if s.is_empty() {
        return Err(LabError::ToolUnavailable("wslpath 转换失败(详情见服务端日志)".into()));
    }
    Ok(s)
}

fn wsl_command(script: &str) -> Result<std::process::Output, LabError> {
    let mut cmd = Command::new("wsl.exe");
    cmd.args(["-d", WSL_DISTRO, "-e", "bash", "-c", script]);
    process::run_with_timeout(&mut cmd, process::DEFAULT_TIMEOUT).map_err(|e| {
        tracing::warn!("WSL 工具调用失败: {e}");
        LabError::ToolUnavailable("外部工具不可用或超时(详情见服务端日志)".into())
    })
}

fn wsl_bash(script: &str) -> Result<String, LabError> {
    Ok(process::stdout_text(&wsl_command(script)?))
}

fn wsl_bash_checked(script: &str) -> Result<(bool, String, String), LabError> {
    let out = wsl_command(script)?;
    let code = out.status.code().unwrap_or(-1);
    Ok((code == 0, process::stdout_text(&out), process::stderr_text(&out)))
}

/// 探测 WSL 工具链(health 用)
pub fn detect_toolchain() -> Option<String> {
    let (ok, out, _) = wsl_bash_checked("command -v g++ >/dev/null && g++ --version | head -1").ok()?;
    if ok { Some(out.trim().to_string()) } else { None }
}

/// 探测 mingw-w64 交叉编译器(Win64 ABI 对比实验;health 用)
pub fn detect_mingw() -> Option<String> {
    let (ok, out, _) = wsl_bash_checked(
        "command -v x86_64-w64-mingw32-g++ >/dev/null && x86_64-w64-mingw32-g++ --version | head -1",
    )
    .ok()?;
    if ok { Some(out.trim().to_string()) } else { None }
}

/// 编译 C++ 并产出带源码行标注的反汇编。
pub fn compile(req: &CompileRequest) -> Result<CompileOutput, LabError> {
    // 精确白名单:opt 会被拼进 shell 命令,防止 "O;/..." 类注入
    const ALLOWED_OPTS: [&str; 4] = ["O0", "O1", "O2", "O3"];
    if !ALLOWED_OPTS.contains(&req.opt.as_str()) {
        return Err(LabError::BadRequest(format!("opt 须为 O0..O3,收到 {}", req.opt)));
    }
    let compiler = if req.compiler.is_empty() { "g++".to_string() } else { req.compiler.clone() };
    // 白名单(编译器名会被拼进 shell 命令,必须防注入)
    const ALLOWED_COMPILERS: [&str; 4] =
        ["g++", "clang++", "x86_64-w64-mingw32-g++", "aarch64-linux-gnu-g++"];
    if !ALLOWED_COMPILERS.contains(&compiler.as_str()) {
        return Err(LabError::BadRequest(format!(
            "不支持的编译器 {compiler}(可选: {ALLOWED_COMPILERS:?})"
        )));
    }

    // 1. 唯一临时目录(Windows 侧)
    let uid = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("assemblylab\\p2-{uid}"));
    std::fs::create_dir_all(&dir).map_err(|e| LabError::Io(e.to_string()))?;
    let src_path = dir.join("main.cpp");
    std::fs::write(&src_path, &req.source).map_err(|e| LabError::Io(e.to_string()))?;

    let wsl_dir = to_wsl_path(&dir)?;

    // 2. 资源限制 + 超时内编译(SECURITY §3):先出 main.o(反汇编视图),
    //    再链接出 main(gdb 调试);成败以 rc 文件为准(避免 echo 吞退出码)。
    let limits = "ulimit -t 10 -v 2000000 -f 20000";
    let compile_cmd = format!(
        "cd '{wsl_dir}' && {limits} && timeout {t} {compiler} -std=c++20 -g -{opt} -c main.cpp -o main.o 2> compile_err.txt; echo $? > rc1; test -f main.o && timeout {t} {compiler} -std=c++20 -g -{opt} main.o -o main 2>> compile_err.txt; echo $? > rc2",
        wsl_dir = wsl_dir, limits = limits, t = COMPILE_TIMEOUT_SECS, compiler = compiler, opt = req.opt
    );
    let _ = wsl_bash_checked(&compile_cmd)?;

    let rc1 = std::fs::read_to_string(dir.join("rc1")).unwrap_or_default().trim().to_string();
    let err_text = std::fs::read_to_string(dir.join("compile_err.txt")).unwrap_or_default();

    if rc1 != "0" {
        let errors = parse_compile_errors(&err_text);
        cleanup(&dir);
        return Err(LabError::Assemble {
            line: errors.first().map(|e| e.line).unwrap_or(0),
            message: errors
                .first()
                .map(|e| e.message.clone())
                .unwrap_or_else(|| "编译失败".into()),
        });
    }

    // 3. 反汇编 main.o(仅用户代码,带行号;x86 用 intel 语法,aarch64 用默认语法)
    let (objdump, notation) = if compiler.starts_with("aarch64") {
        ("aarch64-linux-gnu-objdump", "")
    } else {
        ("objdump", " -M intel")
    };
    let disasm_cmd = format!(
        "cd '{wsl_dir}' && timeout 10 {objdump} -d -l --no-show-raw-insn{notation} main.o",
        wsl_dir = wsl_dir
    );
    let (dis_ok, dis_out, _) = wsl_bash_checked(&disasm_cmd)?;
    if !dis_ok {
        cleanup(&dir);
        return Err(LabError::ToolUnavailable("objdump 反汇编失败".into()));
    }
    let mut asm_lines = parse_objdump(&dis_out);

    // 符号去混淆(一次 c++filt 批量)
    let mangled: Vec<String> = asm_lines
        .iter()
        .filter_map(|l| l.function.clone())
        .filter(|f| {
            f.starts_with("_Z")
                && f.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && f.len() <= 512
        })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    if !mangled.is_empty() {
        // 每个符号单独加引号并过滤字符,避免符号名里的引号/换行改变 shell 命令结构
        let args = mangled.iter().map(|m| format!("'{m}'")).collect::<Vec<_>>().join(" ");
        let cmd = format!("printf '%s\\n' {args} | c++filt");
        let demangled = wsl_bash(&cmd).unwrap_or_default();
        let map: std::collections::HashMap<String, String> = mangled
            .iter()
            .zip(demangled.lines().map(|l| l.trim().to_string()))
            .map(|(m, d)| (m.clone(), d))
            .collect();
        for l in &mut asm_lines {
            if let Some(f) = &l.function {
                if let Some(d) = map.get(f) {
                    l.function = Some(d.clone());
                }
            }
        }
    }

    let version = wsl_bash(&format!("{compiler} --version | head -1")).unwrap_or_default().trim().to_string();

    Ok(CompileOutput {
        asm_lines,
        errors: parse_compile_errors(&err_text),
        work_dir: dir,
        wsl_dir,
        compiler_version: version,
        opt: req.opt.clone(),
    })
}

/// 解析 g++ 错误输出 "main.cpp:12:5: error: ..."
pub fn parse_compile_errors(stderr: &str) -> Vec<CompileError> {
    let mut out = Vec::new();
    for line in stderr.lines() {
        // main.cpp:12:5: error: xxx
        if let Some(idx) = line.find("main.cpp:") {
            let rest = &line[idx + "main.cpp:".len()..];
            let mut parts = rest.splitn(4, ':');
            let line_no: usize = parts.next().unwrap_or("0").trim().parse().unwrap_or(0);
            let _col = parts.next().unwrap_or("");
            let severity = parts.next().unwrap_or("");
            let msg = parts.next().unwrap_or("").to_string();
            if severity.contains("error") || severity.contains("warning") {
                if severity.contains("error") || out.is_empty() {
                    out.push(CompileError { line: line_no, message: format!("{severity}: {msg}") });
                }
            }
        }
    }
    out
}

/// 解析 `objdump -d -l --no-show-raw-insn -M intel`:
///   0000000000001129 <main>:
///   /mnt/.../main.cpp:8          ← 源码行标注(-l)
///     1129: 55                    push rbp
pub fn parse_objdump(text: &str) -> Vec<AsmLine> {
    let mut out = Vec::new();
    let mut current_fn: Option<String> = None;
    let mut pending_file: Option<String> = None;
    let mut pending_line: Option<u32> = None;

    for raw in text.lines() {
        let line = raw.trim_end();
        let t = line.trim();

        // 函数头:0000000000001129 <main>:
        if t.ends_with(">:") && !t.starts_with('/') {
            if let Some(lt) = t.find(" <") {
                let addr = u64::from_str_radix(t.split_whitespace().next().unwrap_or("0"), 16).ok();
                let _ = addr;
                current_fn = Some(t[lt + 2..t.len() - 2].to_string());
                out.push(AsmLine { addr: None, text: t.to_string(), function: current_fn.clone(), source_line: None, source_file: None });
                continue;
            }
        }
        // 源码行标注:路径:数字(-l 开关;可能前有文件名)
        if let Some(colon) = t.rfind(':') {
            let (path_part, num_part) = (&t[..colon], &t[colon + 1..]);
            if let Ok(n) = num_part.trim().parse::<u32>() {
                if !path_part.is_empty() && !path_part.contains(' ') == false
                    || path_part.ends_with(".cpp")
                    || path_part.ends_with(".h")
                    || path_part.ends_with(".c")
                    || path_part.ends_with(".s")
                    || path_part.ends_with(".S")
                    || path_part.contains(".cpp:")
                {
                    pending_file = Some(path_part.trim_start_matches('/').to_string());
                    pending_line = Some(n);
                    continue;
                }
            }
        }
        // 指令:1129: push rbp
        if let Some(colon) = t.find(':') {
            let addr_part = &t[..colon];
            if let Ok(addr) = u64::from_str_radix(addr_part, 16) {
                if !addr_part.is_empty() && addr_part.chars().all(|c| c.is_ascii_hexdigit()) {
                    let text = t[colon + 1..].trim().to_string();
                    out.push(AsmLine {
                        addr: Some(addr),
                        text,
                        function: current_fn.clone(),
                        source_line: pending_line.take(),
                        source_file: pending_file.take(),
                    });
                    continue;
                }
            }
        }
        // 空行重置 pending?保守:不清(同一函数内标注持续有效直到下一条标注)
    }
    out
}

pub fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// 把 parse_objdump 的结果转成行号映射(基地址加偏移;GNU as 场景 .o 内地址从 0 起)。
pub fn parse_objdump_line_map(text: &str, base: u64) -> Vec<lab_core::LineMapping> {
    parse_objdump(text)
        .into_iter()
        .filter_map(|l| {
            let addr = l.addr?;
            let line = l.source_line?;
            Some(lab_core::LineMapping { line: line as usize, addr: base + addr, size: 0 })
        })
        .collect()
}

#[allow(dead_code)]
fn _t(_: Duration) {}

/// 清理全部历史临时目录(后端启动时调用)
pub fn sweep_orphan_dirs() {
    let base = std::env::temp_dir().join("assemblylab");
    if let Ok(rd) = std::fs::read_dir(&base) {
        for e in rd.flatten() {
            cleanup(&e.path());
        }
    }
}
