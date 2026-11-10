//! GNU as 汇编适配(ARM64 / aarch64-linux-gnu,经 WSL2 调用真实交叉工具链)。
//!
//! 流程:`aarch64-linux-gnu-as -g -o prog.o prog.s` → `objcopy -O binary` 得扁平二进制,
//! 符号来自 `nm`,行号映射来自 `objdump -d -l`。错误行号映射回用户源码。
//! 安全与 nasm 适配一致:临时目录唯一 + timeout 包裹 + 输出截断。

use crate::process;
use lab_core::{LabError, ProgramImage};
use std::process::Command;

const WSL_DISTRO: &str = "Ubuntu-22.04";
const AS_TIMEOUT_SECS: u64 = 20;

/// 汇编 ARM64 GNU 语法源码。`base` 为装载基地址(org 等价物由引擎指定)。
pub fn assemble(source: &str, base: u64) -> Result<ProgramImage, LabError> {
    // 1) 教学约束:单 .text 段(与 nasm 适配同规则)
    for raw in source.lines() {
        let t = raw.split("//").next().unwrap_or("").trim().to_ascii_lowercase();
        if (t.starts_with(".data") || t.starts_with(".bss")) && !t.starts_with(".data.")
        {
            return Err(LabError::BadRequest(
                "教学沙箱使用单 .text 段:请把数据声明放在代码之后(如 `value: .quad 0`),不要使用 .data/.bss 段".into(),
            ));
        }
    }

    // 2) 唯一临时目录(进程内序号 ⊕ 纳秒,Windows 时钟精度不足纯时间戳会撞车)
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let uid = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0))
        .wrapping_mul(7919)
        ^ seq.wrapping_mul(2).wrapping_add(1);
    let dir = std::env::temp_dir().join(format!("assemblylab-{}\\as-{uid}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| LabError::Io(e.to_string()))?;
    let src_path = dir.join("prog.s");

    // 行号对齐:插入 .file 0 "prog.s" 使 objdump -l 标注 prog.s 行号
    let full = format!(".file 0 \"prog.s\"\n{source}");
    std::fs::write(&src_path, &full).map_err(|e| LabError::Io(e.to_string()))?;

    let wsl_dir = crate::compiler::to_wsl_path(&dir)?;
    let run = |script: &str| -> Result<(bool, String, String), LabError> {
        let mut cmd = Command::new("wsl.exe");
        cmd.args(["-d", WSL_DISTRO, "-e", "bash", "-c", script]);
        let out = process::run_with_timeout(&mut cmd, process::DEFAULT_TIMEOUT).map_err(|e| {
            tracing::warn!("WSL 汇编工具调用失败: {e}");
            LabError::ToolUnavailable("WSL 汇编工具不可用或超时".into())
        })?;
        Ok((
            out.status.code().unwrap_or(-1) == 0,
            process::stdout_text(&out),
            process::stderr_text(&out),
        ))
    };

    // 3) 汇编(ulimit + timeout 包裹,SECURITY §3)
    let asm_cmd = format!(
        "cd '{wsl_dir}' && ulimit -t 5 -v 1000000 -f 20000 && timeout {t} aarch64-linux-gnu-as -g -o prog.o prog.s 2> as_err.txt; echo $? > rc; cat as_err.txt",
        wsl_dir = wsl_dir,
        t = AS_TIMEOUT_SECS
    );
    let (ok, _out, _err) = run(&asm_cmd)?;
    let rc = std::fs::read_to_string(dir.join("rc")).unwrap_or_default().trim().to_string();
    if rc != "0" || !ok {
        let err_text = std::fs::read_to_string(dir.join("as_err.txt")).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        return Err(as_error(&err_text));
    }

    // 4) 扁平二进制 + 符号 + 行号映射(单次执行:nm 与 objdump 输出用分隔符切分)
    let (ok2, obj_out, err2) = run(&format!(
        "cd '{wsl_dir}' && timeout 10 aarch64-linux-gnu-objcopy -O binary prog.o prog.bin && echo '===SYM===' && timeout 10 aarch64-linux-gnu-nm prog.o && echo '===LST===' && timeout 10 aarch64-linux-gnu-objdump -d -l --no-show-raw-insn prog.o",
        wsl_dir = wsl_dir
    ))?;
    if !ok2 {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(LabError::ToolUnavailable(format!("objcopy/objdump 失败: {err2}")));
    }

    let bin = std::fs::read(dir.join("prog.bin")).map_err(|e| LabError::Io(e.to_string()))?;
    if bin.is_empty() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(LabError::BadRequest("汇编产物为空(源码没有可执行指令?)".into()));
    }

    // 5) 解析符号(nm:0000000000000000 T _start)与行号映射(objdump -l)
    let mut symbols: Vec<(String, u64)> = Vec::new();
    let mut section = 0; // 0=objcopy回显 1=符号 2=lst
    let mut lst_text = String::new();
    for line in obj_out.lines() {
        match line.trim() {
            "===SYM===" => {
                section = 1;
                continue;
            }
            "===LST===" => {
                section = 2;
                continue;
            }
            _ => {}
        }
        if section == 1 {
            let mut parts = line.split_whitespace();
            if let (Some(addr), Some(typ), Some(name)) = (parts.next(), parts.next(), parts.next()) {
                if typ.len() == 1 && typ.chars().all(|c| c.is_ascii_uppercase()) {
                    if let Ok(a) = u64::from_str_radix(addr, 16) {
                        symbols.push((name.to_string(), base + a));
                    }
                }
            }
        } else if section == 2 {
            lst_text.push_str(line);
            lst_text.push('\n');
        }
    }
    let entry = symbols
        .iter()
        .find(|(n, _)| n == "_start")
        .map(|(_, a)| *a)
        .unwrap_or(base);
    let line_map = crate::compiler::parse_objdump_line_map(&lst_text, base);

    let _ = std::fs::remove_dir_all(&dir);
    let source_lines = source.lines().count();
    Ok(ProgramImage { bytes: bin, base, entry, line_map, symbols, source_lines })
}

/// 解析 as 错误:"prog.s:12: 错误: ..." / "prog.s:12: Error: ..."
/// 行号扣除 .file 前导 1 行。
fn as_error(stderr: &str) -> LabError {
    for line in stderr.lines() {
        if let Some(idx) = line.find("prog.s:") {
            let rest = &line[idx + "prog.s:".len()..];
            let mut parts = rest.splitn(3, ':');
            let n: usize = parts.next().unwrap_or("0").trim().parse().unwrap_or(0);
            let msg = parts.next().unwrap_or("").trim().to_string();
            if n > 0 {
                return LabError::Assemble { line: n.saturating_sub(1), message: msg };
            }
        }
    }
    LabError::Assemble { line: 0, message: format!("汇编失败: {}", stderr.lines().next().unwrap_or("")) }
}
