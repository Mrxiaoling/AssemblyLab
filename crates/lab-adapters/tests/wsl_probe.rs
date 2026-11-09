//! 探针:验证 wsl.exe 管道是否支持交互式流式读写(gdb MI 的前提)。
//!
//! Phase 2 一次性诊断(结论:管道非流式 → 已改用 socat/TCP,见 gdb.rs)。
//! 依赖手工准备的 /mnt/c/.../Temp/p2gdb 调试目录,且会真实复现 ~30s/命令
//! 的管道延迟 —— 默认 ignore,需要时 `cargo test -- --ignored` 单独运行。

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
#[ignore = "Phase 2 一次性诊断探针:依赖手工准备的 p2gdb 目录,且故意复现 30s 管道延迟"]
fn wsl_pipe_streams_interactively() {
    let mut child = Command::new("wsl.exe")
        .args(["-d", "Ubuntu-22.04", "-e", "bash", "-c", "cd /mnt/c/Users/ling/AppData/Local/Temp/p2gdb && gdb --interpreter=mi2 ./main"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().flatten() {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    // 等待就绪提示(最多 15s)
    let t0 = Instant::now();
    let mut ready = false;
    let mut first_line_at = None;
    while t0.elapsed() < Duration::from_secs(15) {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => {
                if first_line_at.is_none() {
                    first_line_at = Some(t0.elapsed());
                    eprintln!("[probe] first line at {:?}: {}", t0.elapsed(), &line[..line.len().min(60)]);
                }
                if line.contains("(gdb)") {
                    ready = true;
                    break;
                }
            }
            Err(_) => continue,
        }
    }
    eprintln!("[probe] ready={ready} elapsed={:?}", t0.elapsed());
    assert!(ready, "15s 内未收到 (gdb) 提示 —— wsl.exe 管道不流式?");

    // 发一条 MI 命令,验证能得到响应
    // 与适配器一致的序列:先发一条前置命令,再发 break-insert
    writeln!(stdin, "-gdb-set pagination off").unwrap();
    stdin.flush().unwrap();
    let tp = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(l) => {
                eprintln!("[probe-pre +{:?}] {}", tp.elapsed(), &l[..l.len().min(60)]);
                if l.starts_with("1^") { break; }
            }
            Err(_) => continue,
        }
    }
    writeln!(stdin, "-break-insert main.cpp:1").unwrap();
    stdin.flush().unwrap();
    let t1 = Instant::now();
    let mut got_done = false;
    while t1.elapsed() < Duration::from_secs(10) {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => {
                eprintln!("[probe] +{:?} {}", t1.elapsed(), &line[..line.len().min(80)]);
                if line.contains("^done") {
                    got_done = true;
                    break;
                }
            }
            Err(_) => continue,
        }
    }
    assert!(got_done, "-break-insert 未得到 ^done");
    let _ = first_line_at;
}
