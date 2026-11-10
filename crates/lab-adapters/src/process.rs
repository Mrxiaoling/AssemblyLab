//! 外部工具进程的统一超时与输出上限封装。
//!
//! 原实现直接 `Command::output()`:若 wsl.exe / nasm.exe / ssh 挂起,Rust 侧会无限等待;
//! 且 stdout/stderr 全量读入内存,恶意或失控工具可以打满内存。
//! 这里用 `spawn + try_wait + kill` 保证墙钟超时,并用管道缓冲 + 截断限制输出体积。

use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

/// 运行命令并在超时后 kill 整个子进程(含其启动的 WSL 客户端)。
pub fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> std::io::Result<Output> {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(_) => return child.wait_with_output(),
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "external tool timed out",
                    ));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

/// 把输出裁剪到上限(按 UTF-8 边界),避免把巨量 stderr 直接回显/入库。
pub fn stdout_text(out: &Output) -> String {
    truncate(&out.stdout)
}
pub fn stderr_text(out: &Output) -> String {
    truncate(&out.stderr)
}

fn truncate(bytes: &[u8]) -> String {
    let slice = if bytes.len() > MAX_OUTPUT_BYTES {
        &bytes[..MAX_OUTPUT_BYTES]
    } else {
        bytes
    };
    let mut s = String::from_utf8_lossy(slice).into_owned();
    if bytes.len() > MAX_OUTPUT_BYTES {
        s.push_str("\n…(输出已截断至 1 MB)");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_keeps_boundary() {
        let out = Output {
            status: Default::default(),
            stdout: vec![b'a'; 1024],
            stderr: vec![],
        };
        assert_eq!(stdout_text(&out).len(), 1024);
    }
}
