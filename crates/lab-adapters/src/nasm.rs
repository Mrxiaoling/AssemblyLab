//! NASM 外部进程封装(-f bin 扁平二进制 + -l 列表文件解析)。
//! 安全:源码经临时文件传入,绝不拼接命令行(SECURITY_DESIGN §4)。
//! 超时/输出截断统一处理;错误行号映射回用户源码。

use crate::process;
use lab_core::{LabError, LineMapping, ProgramImage};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// NASM 可执行文件查找顺序:环境变量 ASSEMBLYLAB_NASM → 仓库 tools/nasm/nasm.exe → PATH。
pub fn find_nasm() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("ASSEMBLYLAB_NASM") {
        let path = Path::new(&p);
        if path.is_file() {
            return Some(path.to_path_buf());
        }
    }
    if let Ok(repo) = std::env::var("ASSEMBLYLAB_ROOT") {
        let path = Path::new(&repo).join("tools/nasm/nasm.exe");
        if path.is_file() {
            return Some(path);
        }
    }
    // 相对仓库根(cargo 运行时 CWD 常为仓库根或 crate 目录)
    for probe in ["tools/nasm/nasm.exe", "../../tools/nasm/nasm.exe"] {
        let path = Path::new(probe);
        if path.is_file() {
            return std::path::absolute(path).ok();
        }
    }
    which_nasm_from_path()
}

fn which_nasm_from_path() -> Option<std::path::PathBuf> {
    let path_var = std::env::var("PATH").ok()?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join("nasm.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
        let candidate = dir.join("nasm");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 汇编源码。返回 ProgramImage。
/// `default_base`: 源码没有 org 时的默认基地址(自动 prepend,行号偏移已处理)。
pub fn assemble(source: &str, default_base: u64) -> Result<ProgramImage, LabError> {
    let nasm = find_nasm().ok_or_else(|| {
        LabError::ToolUnavailable(
            "未找到 nasm。请将其放入 tools/nasm/ 或设置 ASSEMBLYLAB_NASM 环境变量。".into(),
        )
    })?;

    for line in source.lines() {
        let t = line.trim_start().to_ascii_lowercase();
        if (t.starts_with("section .data") || t.starts_with("section .bss")
            || t.starts_with("segment .data"))
            && !t.contains(';')
        {
            return Err(LabError::BadRequest(
                "Phase 1 教学沙箱使用单段扁平程序:请把数据声明放在代码之后(如 `result: dq 0`),不要使用 section .data/.bss".into(),
            ));
        }
    }

    let has_org = source
        .lines()
        .any(|l| l.trim_start().to_ascii_lowercase().starts_with("org "));
    let (full_source, line_offset) = if has_org {
        (source.to_string(), 0usize)
    } else {
        (format!("[bits 64]\norg {default_base:#x}\n{source}"), 2usize)
    };

    let tmp = std::env::temp_dir().join(format!("assemblylab-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    // uid = 进程内单调序号 + 纳秒时间:Windows 时钟精度有限,纯时间戳在并发
    // 调用下可能重复(曾致并行测试互踩临时源文件),序号保证目录唯一。
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let uid = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0))
        .wrapping_mul(7919)
        ^ seq.wrapping_mul(2).wrapping_add(1);
    let dir = tmp.join(format!("asm-{uid}"));
    std::fs::create_dir_all(&dir).map_err(|e| LabError::Io(e.to_string()))?;
    let src_path = dir.join("prog.asm");
    let bin_path = dir.join("prog.bin");
    let lst_path = dir.join("prog.lst");
    std::fs::write(&src_path, &full_source).map_err(|e| LabError::Io(e.to_string()))?;

    let mut cmd = Command::new(&nasm);
    cmd.args([
        "-f",
        "bin",
        "-l",
        lst_path.to_str().unwrap(),
        "-o",
        bin_path.to_str().unwrap(),
        src_path.to_str().unwrap(),
    ]);
    let result = match process::run_with_timeout(&mut cmd, process::DEFAULT_TIMEOUT) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            tracing::warn!("nasm 调用失败: {e}");
            return Err(LabError::ToolUnavailable("nasm 调用失败或超时".into()));
        }
    };

    if !result.status.success() {
        let stderr = process::stderr_text(&result);
        let err = parse_nasm_error(&stderr, line_offset).unwrap_or_else(|| LabError::Assemble {
            line: 0,
            message: stderr.lines().next().unwrap_or("汇编失败").to_string(),
        });
        let _ = std::fs::remove_dir_all(&dir);
        return Err(err);
    }

    let bytes = std::fs::read(&bin_path).map_err(|e| LabError::Io(e.to_string()))?;
    let lst = std::fs::read_to_string(&lst_path).unwrap_or_default();
    let base = detect_org(&full_source).unwrap_or(default_base);
    let mut image = parse_lst(&lst, line_offset);
    // 单段扁平程序:lst 地址列 = 段内偏移,虚拟地址 = org + 偏移
    for m in &mut image.line_map {
        m.addr += base;
    }
    for sym in &mut image.symbols {
        sym.1 += base;
    }
    image.bytes = bytes;
    image.base = base;
    // 入口:优先 _start/main 标签
    image.entry = image
        .symbols
        .iter()
        .find(|(n, _)| n == "_start" || n == "main" || n == "start")
        .map(|(_, a)| *a)
        .unwrap_or(base);
    image.source_lines = full_source.lines().count();
    let _ = std::fs::remove_dir_all(&dir);
    Ok(image)
}

fn detect_org(source: &str) -> Option<u64> {
    for line in source.lines() {
        let t = line.trim_start().to_ascii_lowercase();
        if let Some(rest) = t.strip_prefix("org ") {
            let v = rest.trim().trim_end_matches(',');
            if let Ok(addr) = lab_core::isa::parse_int(v) {
                return Some(addr);
            }
        }
    }
    None
}

/// 解析 nasm 错误输出 "<...路径>prog.asm:N: error: message"。
/// 注意 Windows 绝对路径含盘符冒号(C:\...),必须先定位 ".asm:" 再解析。
fn parse_nasm_error(stderr: &str, line_offset: usize) -> Option<LabError> {
    for line in stderr.lines() {
        let Some(idx) = line.find(".asm:") else { continue };
        let rest = &line[idx + ".asm:".len()..];
        let mut parts = rest.splitn(3, ':');
        let line_no: usize = parts.next()?.trim().parse().ok()?;
        let severity = parts.next()?.trim();
        let msg = parts.next()?.trim().to_string();
        if severity.contains("error") || severity.contains("fatal") {
            return Some(LabError::Assemble {
                line: line_no.saturating_sub(line_offset),
                message: msg,
            });
        }
    }
    None
}

/// 解析 nasm 列表文件。
/// 格式:`<行号> <段内偏移8hex> <字节(可含[..])> <源码>`;无指令行没有偏移列。
/// 返回的 addr 为**段内偏移**,调用方负责加 org(base)。
/// 独立标签行(`_start:`)没有偏移列,通过“前看下一条有地址的行”补齐。
fn parse_lst(lst: &str, line_offset: usize) -> ProgramImage {
    let mut line_map: Vec<LineMapping> = Vec::new();
    let mut symbols: Vec<(String, u64)> = Vec::new();
    let mut pending_labels: Vec<(usize, String)> = Vec::new();

    let flush_pending = |pending: &mut Vec<(usize, String)>, symbols: &mut Vec<(String, u64)>, addr: u64| {
        for (_, name) in pending.drain(..) {
            symbols.push((name, addr));
        }
    };

    for raw in lst.lines() {
        let t = raw.trim_start();
        if t.is_empty() {
            continue;
        }
        let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(line_no) = digits.parse::<usize>() else { continue };
        if line_no == 0 {
            continue;
        }
        let user_line = line_no.saturating_sub(line_offset);
        let rest = t[digits.len()..].trim_start();
        let mut iter = rest.split_whitespace();
        let first = iter.next().unwrap_or("");
        let is_addr = !first.is_empty()
            && first.len() <= 8
            && first.chars().all(|c| c.is_ascii_hexdigit());

        if is_addr {
            let addr = u64::from_str_radix(first, 16).unwrap_or(0);
            let after_addr = rest[first.len()..].trim_start();
            let bytes_tok = after_addr.split_whitespace().next().unwrap_or("");
            let size = bytes_tok.chars().filter(|c| c.is_ascii_hexdigit()).count() as u32 / 2;
            let source = after_addr[bytes_tok.len()..].trim_start();

            if size > 0 {
                flush_pending(&mut pending_labels, &mut symbols, addr);
                line_map.push(LineMapping { line: user_line, addr, size });
            }
            // 标签(可能带指令,如 `result: dq 0`)
            let trimmed = source.trim();
            if let Some(colon) = trimmed.find(':') {
                let label = trimmed[..colon].trim();
                if is_label_name(label) {
                    if size > 0 {
                        symbols.push((label.to_string(), addr));
                    } else {
                        pending_labels.push((user_line, label.to_string()));
                    }
                }
            }
        } else {
            // 无地址列:标签行(first 非纯 hex,整段 rest 是源码)或续行字节
            let hex_only = !first.is_empty() && first.chars().all(|c| c.is_ascii_hexdigit());
            if hex_only {
                let size = first.chars().filter(|c| c.is_ascii_hexdigit()).count() as u32 / 2;
                if size > 0 {
                    if let Some(last) = line_map.last_mut() {
                        if last.line == user_line {
                            last.size += size;
                        }
                    }
                }
            }
            // 标签检查(标签行:整段 rest;续行:first 之后的源码)
            let source = if hex_only { rest[first.len()..].trim_start() } else { rest };
            if let Some(colon) = source.find(':') {
                let label = source[..colon].trim();
                if is_label_name(label) {
                    pending_labels.push((user_line, label.to_string()));
                }
            }
        }
    }
    ProgramImage { line_map, symbols, ..Default::default() }
}

fn is_label_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars().next().map(|c| c.is_alphabetic() || c == '_' || c == '.').unwrap_or(false)
        && s.chars().all(|c| c.is_alphanumeric() || "_.$#@~?".contains(c))
        && !["section", "segment", "org", "bits", "default", "global"].contains(&s.to_ascii_lowercase().as_str())
}

/// 工具健康检查(供 /api/health)。
pub fn nasm_version() -> Result<String, LabError> {
    let nasm = find_nasm().ok_or_else(|| LabError::ToolUnavailable("nasm 未找到".into()))?;
    let mut cmd = Command::new(&nasm);
    cmd.arg("-v");
    let out = process::run_with_timeout(&mut cmd, process::DEFAULT_TIMEOUT)
        .map_err(|e| LabError::ToolUnavailable(format!("nasm 启动失败或超时: {e}")))?;
    Ok(process::stdout_text(&out).trim().to_string())
}

/// 供测试使用:临时目录清理辅助
pub fn cleanup_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

