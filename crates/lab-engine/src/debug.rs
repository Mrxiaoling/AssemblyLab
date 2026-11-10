//! 模式 B:真实 C++ 调试会话(编译 → gdb/MI)。
//!
//! 与模式 A(Session)同构:step/run/breakpoints/snapshot,
//! 输出同形 StepReport(数值全部来自 gdb 真实状态)。
//! 隔离等级如实标注:进程级限制(WSL timeout/ulimit 编译侧),非强沙箱。

use std::collections::HashMap;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use lab_adapters::compiler::{self, CompileOutput};
use lab_adapters::gdb::GdbSession;
use lab_adapters::process;
use lab_core::IsaDefinition;
use serde::Serialize;

use crate::{hx, Explanation, FlagDiffItem, MemChangeItem, PcJump, RegDiffItem, StepReport};

/// 一条 ARM64 远程会话独占的远端资源,会话结束(Drop)时逐一回收。
struct RemoteHandle {
    tunnel: Child,
    user_host: String,
    remote_dir: String,
}

pub struct DebugSession {
    pub id: u64,
    pub isa: IsaDefinition,
    /// 模式 B 目标架构(x86-64 本地 / arm64 远程真机)
    pub arch: String,
    /// ARM64 远程:user@host(Drop 时清理远端 gdbserver)
    pub remote_host: Option<String>,
    /// ARM64 远程会话资源句柄(ssh 隧道 + 远端目录)
    remote: Mutex<Option<RemoteHandle>>,
    pub source: Mutex<String>,
    pub compile: CompileOutput,
    gdb: Mutex<GdbSession>,
    /// 源码行 → 断点编号
    breakpoints: Mutex<HashMap<u32, u64>>,
    /// 观察点表达式 → 编号(gdb 断点表统一编号)
    watchpoints: Mutex<Vec<(String, u64)>>,
    step_counter: AtomicU64,
    last_stop: Mutex<Option<String>>, // reason 文本
    /// 程序是否已运行过(区分 run 与 continue)
    started: std::sync::atomic::AtomicBool,
}

const SOURCE_FILE: &str = "main.cpp";
const MAX_STEP_PER_REQUEST: u32 = 500;

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// ssh 命令全部走 BatchMode(依赖外部免密配置),不使用任何口令参数。
fn ssh_cmd(user_host: &str, remote_cmd: &str) -> Command {
    let mut c = Command::new("ssh");
    c.args([
        "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", user_host, remote_cmd,
    ]);
    c
}

fn run_ok(mut cmd: Command, secs: u64, label: &str) -> Result<(), String> {
    let out = process::run_with_timeout(&mut cmd, Duration::from_secs(secs))
        .map_err(|e| format!("{label} 失败: {e}"))?;
    if !out.status.success() {
        // 远端 stderr 可能含主机名/路径,只写服务端日志,不回显客户端
        tracing::warn!("{label} stderr: {}", process::stderr_text(&out));
        return Err(format!("{label} 失败(详情见服务端日志)"));
    }
    Ok(())
}

fn wait_port(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// 精确清理本会话的远端 gdbserver(pid 文件)与目录,不再 `pkill -f gdbserver` 全局杀伤。
fn cleanup_remote(user_host: &str, remote_dir: &str) {
    let cmd = format!(
        "if [ -f {remote_dir}/gdbserver.pid ]; then kill \"$(cat {remote_dir}/gdbserver.pid)\" 2>/dev/null; fi; rm -rf {remote_dir}"
    );
    let mut c = ssh_cmd(user_host, &cmd);
    let _ = process::run_with_timeout(&mut c, Duration::from_secs(10));
}

fn abort_remote(
    tunnel: &mut Option<Child>,
    user_host: &str,
    remote_dir: &str,
    work_dir: &std::path::Path,
    e: String,
) -> String {
    if let Some(mut t) = tunnel.take() {
        let _ = t.kill();
        let _ = t.wait();
    }
    cleanup_remote(user_host, remote_dir);
    let _ = std::fs::remove_dir_all(work_dir);
    e
}

impl Drop for DebugSession {
    fn drop(&mut self) {
        if let Ok(mut g) = self.remote.lock() {
            if let Some(h) = g.take() {
                let mut child = h.tunnel;
                let _ = child.kill();
                let _ = child.wait();
                cleanup_remote(&h.user_host, &h.remote_dir);
            }
        }
        // 本地临时编译目录(含用户源码与可执行产物)随会话一起删除
        let _ = std::fs::remove_dir_all(&self.compile.work_dir);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CoroTimelineStop {
    pub line: u32,
    pub func: Option<String>,
    pub reason: String,
    pub coro_index: Option<i32>,
    pub rip: Option<String>,
    /// 该时刻的协程帧(帧地址 + 字段值);不在协程内(如已退出)时为 None
    pub frame_addr: Option<String>,
    pub frame_fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugState {
    pub terminated: bool,
    pub stop_reason: Option<String>,
    pub stop_line: Option<u32>,
    pub stop_func: Option<String>,
    pub step_count: u64,
}

impl DebugSession {
    pub fn create(id: u64, isa: IsaDefinition, source: &str, opt: &str) -> Result<Self, String> {
        Self::create_with_compiler(id, isa, source, opt, "g++")
    }

    pub fn create_with_compiler(id: u64, isa: IsaDefinition, source: &str, opt: &str, compiler: &str) -> Result<Self, String> {
        let arch = isa.id.clone();
        if arch == "arm64" {
            return Self::create_arm64_remote(id, isa, source, opt, compiler);
        }
        let out = compiler::compile(&compiler::CompileRequest {
            source: source.to_string(),
            opt: opt.to_string(),
            compiler: compiler.to_string(),
        })
        .map_err(|e| e.to_string())?;
        let mut gdb = GdbSession::spawn(&out.wsl_dir).map_err(|e| e.to_string())?;
        // 标准调试器行为:启动即断在 main 入口(用户随后可加行断点)
        let _ = gdb.insert_symbol_breakpoint("main");
        Ok(Self {
            id,
            arch: isa.id.clone(),
            isa,
            remote_host: None,
            remote: Mutex::new(None),
            source: Mutex::new(source.to_string()),
            compile: out,
            gdb: Mutex::new(gdb),
            breakpoints: Mutex::new(HashMap::new()),
            watchpoints: Mutex::new(Vec::new()),
            step_counter: AtomicU64::new(0),
            last_stop: Mutex::new(None),
            started: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// PC 寄存器名(arm64=PC,x86=RIP)
    fn pc_name(&self) -> &'static str {
        if self.arch == "arm64" { "PC" } else { "RIP" }
    }

    fn snapshot(&self, g: &mut GdbSession) -> Vec<(String, String)> {
        // ARM64 远程:用 aarch64 寄存器名快照(X0-X30/SP/PC/NZCV)
        if self.arch == "arm64" {
            return g.snapshot_arm64().unwrap_or_default();
        }
        g.snapshot().unwrap_or_default()
    }

    /// ARM64 远程真机调试(P8):交叉编译 → scp → 远端回环 gdbserver → ssh 隧道 → gdb-multiarch。
    ///
    /// 三层默认保护:独立开关 `ASSEMBLYLAB_ENABLE_ARM64_REMOTE=1`;主机/用户必须显式配置
    /// (代码中不内置任何内网地址);gdbserver 只绑远端 127.0.0.1,经 ssh -L 转发,不暴露到网络。
    /// ssh 全程 `-o BatchMode=yes` 依赖外部免密配置,代码不持有任何口令。
    fn create_arm64_remote(id: u64, isa: IsaDefinition, source: &str, opt: &str, _compiler: &str) -> Result<Self, String> {
        if std::env::var("ASSEMBLYLAB_ENABLE_ARM64_REMOTE").ok().as_deref() != Some("1") {
            return Err(
                "ARM64 远程真机调试默认关闭:需显式设置 ASSEMBLYLAB_ENABLE_ARM64_REMOTE=1 并配置远程主机".into(),
            );
        }
        let host = env_nonempty("ASSEMBLYLAB_ARM64_HOST")
            .ok_or_else(|| "未配置 ASSEMBLYLAB_ARM64_HOST(不提供默认主机)".to_string())?;
        let user = env_nonempty("ASSEMBLYLAB_ARM64_USER")
            .ok_or_else(|| "未配置 ASSEMBLYLAB_ARM64_USER".to_string())?;
        let valid = |s: &str| s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
        if !valid(&host) || !valid(&user) {
            return Err("远程主机/用户名含非法字符".into());
        }
        let user_host = format!("{user}@{host}");
        let remote_dir = format!("/tmp/assemblylab-arm-{id}");
        let remote_port: u16 = 2345 + (id % 1000) as u16;
        let local_port: u16 = 30000 + (id % 1000) as u16;
        let mut tunnel: Option<Child> = None;

        // 强制交叉编译器:调用方默认传 g++,会产出 x86-64 ELF 推到 ARM 真机必然失败。
        let out = compiler::compile(&compiler::CompileRequest {
            source: source.to_string(),
            opt: opt.to_string(),
            compiler: "aarch64-linux-gnu-g++".into(),
        })
        .map_err(|e| e.to_string())?;
        let win_bin = out.work_dir.join("main");
        if !win_bin.exists() {
            let _ = std::fs::remove_dir_all(&out.work_dir);
            return Err("交叉编译产物缺失(aarch64 ELF)".into());
        }
        // 清理闭包拥有自己的参数副本,避免捕获后与后续 move 冲突
        let abort = {
            let uh = user_host.clone();
            let rd = remote_dir.clone();
            let wd = out.work_dir.clone();
            move |t: &mut Option<Child>, e: String| -> String { abort_remote(t, &uh, &rd, &wd, e) }
        };

        // 1. 远端目录 → 2. 推送二进制
        run_ok(ssh_cmd(&user_host, &format!("mkdir -p {remote_dir}")), 15, "创建远端目录")
            .map_err(|e| abort(&mut tunnel, e))?;
        let win_bin_fwd = win_bin.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
        let mut scp_cmd = Command::new("scp");
        scp_cmd
            .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"])
            .arg(&win_bin_fwd)
            .arg(format!("{user_host}:{remote_dir}/main"));
        run_ok(scp_cmd, 60, "推送交叉编译产物").map_err(|e| abort(&mut tunnel, e))?;

        // 3. 远端 gdbserver 只监听回环,并写入本会话 pid 文件供精确清理(不再全局 pkill)。
        let start = format!(
            "chmod +x {remote_dir}/main; nohup gdbserver 127.0.0.1:{remote_port} {remote_dir}/main >/dev/null 2>&1 & echo $! > {remote_dir}/gdbserver.pid; exit"
        );
        run_ok(ssh_cmd(&user_host, &start), 15, "启动远端 gdbserver")
            .map_err(|e| abort(&mut tunnel, e))?;

        // 4. Windows 侧 ssh -L 隧道;gdb(WSL 内)连本机转发端口,远端调试端口不出网。
        let mut tunnel_cmd = Command::new("ssh");
        tunnel_cmd
            .args([
                "-o", "BatchMode=yes", "-o", "ExitOnForwardFailure=yes", "-N",
                "-L", &format!("127.0.0.1:{local_port}:127.0.0.1:{remote_port}"),
                &user_host,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        tunnel = Some(
            tunnel_cmd
                .spawn()
                .map_err(|e| abort(&mut tunnel, format!("建立 ssh 隧道失败: {e}")))?,
        );
        if !wait_port(local_port, Duration::from_secs(8)) {
            return Err(abort(&mut tunnel, "ssh 隧道未就绪(远端 gdbserver 或免密配置异常)".into()));
        }

        // 5. gdb-multiarch 经隧道连接
        let remote_target = format!("127.0.0.1:{local_port}");
        let mut gdb = match GdbSession::spawn_multiarch_remote(&out.wsl_dir, &remote_target) {
            Ok(g) => g,
            Err(e) => return Err(abort(&mut tunnel, format!("连接远程调试目标失败: {e}"))),
        };
        let _ = gdb.insert_symbol_breakpoint("main");

        Ok(Self {
            id,
            isa,
            arch: "arm64".into(),
            source: Mutex::new(source.to_string()),
            compile: out,
            gdb: Mutex::new(gdb),
            breakpoints: Mutex::new(HashMap::new()),
            watchpoints: Mutex::new(Vec::new()),
            step_counter: AtomicU64::new(0),
            last_stop: Mutex::new(None),
            started: std::sync::atomic::AtomicBool::new(true),
            remote_host: Some(user_host.clone()),
            remote: Mutex::new(Some(RemoteHandle {
                tunnel: tunnel.take().unwrap(),
                user_host,
                remote_dir,
            })),
        })
    }

    /// 单步(into = 步入;false = 步过指令)。count 次循环,首步带前后 diff。
    pub fn step(&self, into: bool, count: u32) -> Result<Vec<StepReport>, String> {
        if count == 0 || count > MAX_STEP_PER_REQUEST {
            return Err(format!("step count 须在 1..={MAX_STEP_PER_REQUEST}"));
        }
        let mut reports = Vec::new();
        {
            let mut g = self.gdb.lock().unwrap();
            if !self.started.load(Ordering::SeqCst) {
                // 未启动:先 run 到首个断点/入口
                g.run().map_err(|e| e.to_string())?;
                self.started.store(true, Ordering::SeqCst);
            }
            let mut before = self.snapshot(&mut g);
            for _ in 0..count {
                let stop = g.step_instruction(into).map_err(|e| e.to_string())?;
                let after = self.snapshot(&mut g);
                self.step_counter.fetch_add(1, Ordering::SeqCst);
                // 持锁期内读取机器码(build_report 不再入锁,防死锁)
                let machine = before
                    .iter()
                    .find(|(n, _)| n == self.pc_name())
                    .and_then(|(_, v)| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                    .and_then(|rip| g.read_memory(rip, 15).ok())
                    .unwrap_or_default();
                reports.push(self.build_report(&before, &after, &stop, &machine));
                before = after;
                if stop.reason.starts_with("exited") {
                    break;
                }
            }
        }
        Ok(reports)
    }

    /// 启动或继续运行,直到断点/退出。
    pub fn run(&self) -> Result<StepReport, String> {
        let mut g = self.gdb.lock().unwrap();
        let before = self.snapshot(&mut g);
        let stop = if self.started.load(Ordering::SeqCst) {
            g.cont().map_err(|e| e.to_string())?
        } else {
            self.started.store(true, Ordering::SeqCst);
            g.run().map_err(|e| e.to_string())?
        };
        let after = self.snapshot(&mut g);
        let machine = before
            .iter()
            .find(|(n, _)| n == self.pc_name())
            .and_then(|(_, v)| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
            .and_then(|rip| g.read_memory(rip, 15).ok())
            .unwrap_or_default();
        *self.last_stop.lock().unwrap() = Some(stop.reason.clone());
        drop(g);
        Ok(self.build_report(&before, &after, &stop, &machine))
    }

    /// 运行到当前函数返回(步出)。
    pub fn finish(&self) -> Result<StepReport, String> {
        let mut g = self.gdb.lock().unwrap();
        let before = self.snapshot(&mut g);
        let stop = g.finish().map_err(|e| e.to_string())?;
        let after = self.snapshot(&mut g);
        let machine = before
            .iter()
            .find(|(n, _)| n == self.pc_name())
            .and_then(|(_, v)| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
            .and_then(|rip| g.read_memory(rip, 15).ok())
            .unwrap_or_default();
        drop(g);
        Ok(self.build_report(&before, &after, &stop, &machine))
    }

    pub fn set_breakpoints(&self, lines: &[u32]) -> Result<Vec<u32>, String> {
        let mut g = self.gdb.lock().unwrap();
        let mut bps = self.breakpoints.lock().unwrap();
        // 删除全部旧断点
        for (_, num) in bps.drain() {
            let _ = g.delete_breakpoint(num);
        }
        let mut valid = Vec::new();
        for &line in lines {
            match g.insert_breakpoint(SOURCE_FILE, line) {
                Ok(num) => {
                    bps.insert(line, num);
                    valid.push(line);
                }
                Err(_) => {
                    return Err(format!("第 {line} 行无法设置断点(可能是空行或非代码行)"));
                }
            }
        }
        Ok(valid)
    }

    pub fn breakpoints(&self) -> Vec<u32> {
        self.breakpoints.lock().unwrap().keys().copied().collect()
    }

    /// 设置观察点(整表替换)。程序未启动时先自动 run 到 main(表达式求值需要 frame)。
    /// 返回实际生效的表达式列表。
    pub fn set_watchpoints(&self, exprs: &[String]) -> Result<Vec<String>, String> {
        let mut g = self.gdb.lock().unwrap();
        if !self.started.load(Ordering::SeqCst) {
            g.run().map_err(|e| e.to_string())?;
            self.started.store(true, Ordering::SeqCst);
            let reason = "watchpoint-setup-run".to_string();
            *self.last_stop.lock().unwrap() = Some(reason);
        }
        let mut wps = self.watchpoints.lock().unwrap();
        for (_, num) in wps.drain(..) {
            let _ = g.delete_breakpoint(num);
        }
        let mut valid = Vec::new();
        for e in exprs {
            let expr = e.trim();
            if expr.is_empty() {
                continue;
            }
            match g.insert_watchpoint(expr) {
                Ok(num) => {
                    wps.push((expr.to_string(), num));
                    valid.push(expr.to_string());
                }
                Err(err) => return Err(format!("无法观察「{expr}」: {err}")),
            }
        }
        Ok(valid)
    }

    pub fn watchpoints(&self) -> Vec<String> {
        self.watchpoints.lock().unwrap().iter().map(|(e, _)| e.clone()).collect()
    }

    pub fn state(&self) -> DebugState {
        let stop = self.last_stop.lock().unwrap().clone();
        DebugState {
            terminated: stop.as_ref().map(|r| r.starts_with("exited")).unwrap_or(false),
            stop_reason: stop,
            stop_line: None,
            stop_func: None,
            step_count: self.step_counter.load(Ordering::SeqCst),
        }
    }

    pub fn frames(&self) -> Vec<lab_adapters::gdb::FrameInfo> {
        self.gdb.lock().unwrap().frames().unwrap_or_default()
    }

    pub fn variables(&self) -> Vec<(String, String)> {
        self.gdb.lock().unwrap().variables().unwrap_or_default()
    }

    pub fn read_memory(&self, addr: u64, len: u32) -> Result<Vec<u8>, String> {
        self.gdb.lock().unwrap().read_memory(addr, len).map_err(|e| e.to_string())
    }

    // -----------------------------------------------------------------------
    // 协程(P4):帧布局 + 挂起/恢复时间线(全部来自真实 gdb)
    // -----------------------------------------------------------------------

    /// 协程帧:(帧地址, [(字段, 值)])。当前未暂停在协程内时报错。
    pub fn coro_frame(&self) -> Result<(String, Vec<(String, String)>), String> {
        self.gdb.lock().unwrap().coro_frame().map_err(|e| e.to_string())
    }

    /// 挂起/恢复时间线:在全部 co_await/co_return 行设断点后连续运行,
    /// 收集每次停止(行号/原因/协程状态编号),直至退出(上限 30 站)。
    /// 收集后删除临时断点,不影响后续手动调试。
    pub fn coro_timeline(&self) -> Result<Vec<CoroTimelineStop>, String> {
        let mut g = self.gdb.lock().unwrap();
        if !self.started.load(Ordering::SeqCst) {
            g.run().map_err(|e| e.to_string())?;
            self.started.store(true, Ordering::SeqCst);
        }
        let src = self.source.lock().unwrap().clone();
        let suspend_lines: Vec<u32> = src
            .lines()
            .enumerate()
            .filter(|(_, l)| l.contains("co_await") || l.contains("co_return") || l.contains("co_yield"))
            .map(|(i, _)| (i + 1) as u32)
            .collect();
        if suspend_lines.is_empty() {
            return Err("源码中没有 co_await/co_return/co_yield,不是协程".into());
        }
        let mut nums = Vec::new();
        for l in &suspend_lines {
            if let Ok(n) = g.insert_breakpoint(SOURCE_FILE, *l) {
                nums.push(n);
            }
        }
        let mut stops = Vec::new();
        for _ in 0..30 {
            let stop = match g.cont() {
                Ok(s) => s,
                Err(e) => {
                    for n in &nums { let _ = g.delete_breakpoint(*n); }
                    return Err(format!("运行中断: {e}"));
                }
            };
            let idx = g.coro_index();
            // 每站抓一次协程帧(挂起时帧最完整;退出后无帧 → None)
            let (frame_addr, frame_fields) = match g.coro_frame() {
                Ok((a, f)) => (Some(a), f),
                Err(_) => (None, Vec::new()),
            };
            stops.push(CoroTimelineStop {
                line: stop.frame_line.unwrap_or(0),
                func: stop.frame_func.clone(),
                reason: stop.reason.clone(),
                coro_index: idx,
                rip: stop.frame_addr.map(|a| format!("{a:#x}")),
                frame_addr,
                frame_fields,
            });
            if stop.reason.starts_with("exited") {
                break;
            }
        }
        for n in &nums {
            let _ = g.delete_breakpoint(*n);
        }
        Ok(stops)
    }

    /// 反汇编行(来自编译产物;带源码行标注)
    pub fn asm_lines(&self) -> Vec<compiler::AsmLine> {
        self.compile.asm_lines.clone()
    }

    // -----------------------------------------------------------------------
    // StepReport 组装(mode B:数值来自 gdb 真实状态)
    // -----------------------------------------------------------------------

    fn build_report(&self, before: &[(String, String)], after: &[(String, String)], stop: &lab_adapters::gdb::GdbStop, machine: &[u8]) -> StepReport {
        let isa = &self.isa;
        let pc_reg = self.pc_name();

        let before_map: HashMap<&str, &str> = before.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
        let after_map: HashMap<&str, &str> = after.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();

        // 寄存器 diff
        let mut reg_diff = Vec::new();
        for (name, after_v) in after {
            let before_v = before_map.get(name.as_str()).copied().unwrap_or("0x0");
            if before_v != after_v {
                let dec = |hex: &str| -> Option<u64> { u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok() };
                let (b, a) = (dec(before_v), dec(after_v));
                reg_diff.push(RegDiffItem {
                    name: name.clone(),
                    before: before_v.to_string(),
                    after: after_v.clone(),
                    dec: match (b, a) {
                        (Some(b), Some(a)) if b < 1_000_000 && a < 1_000_000 => Some((b, a)),
                        _ => None,
                    },
                });
            }
        }

        // 标志 diff(RFLAGS 位级)
        let mut flag_diff = Vec::new();
        let fb = before_map.get("RFLAGS").and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok()).unwrap_or(0);
        let fa = after_map.get("RFLAGS").and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok()).unwrap_or(0);
        if fb != fa {
            for flag in &isa.flags {
                let b = (fb >> flag.bit) & 1 == 1;
                let a = (fa >> flag.bit) & 1 == 1;
                if b != a {
                    flag_diff.push(FlagDiffItem {
                        name: flag.name.clone(),
                        before: b,
                        after: a,
                        semantics: flag.semantics.clone(),
                    });
                }
            }
        }

        // 反汇编刚执行的那条指令(机器码取自 step 前的 PC);按 ISA 分派解码器。
        let from = before_map.get(pc_reg).and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok()).unwrap_or(0);
        let decoded = lab_adapters::disasm_one_for(&self.arch, &machine, from).ok().flatten();
        let (disasm, mnemonic, machine_hex, inst_len) = match &decoded {
            Some(l) => (l.text.clone(), l.mnemonic.clone(), l.bytes_hex.clone(), l.size as u64),
            // 无法解码时按 ISA 定长回退:ARM64 固定 4 字节,x86-64 保守取 1
            None => (String::new(), String::new(), String::new(), if self.arch == "arm64" { 4 } else { 1 }),
        };

        let next_rip = after_map
            .get(pc_reg)
            .and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0);
        let kind = crate::classify_jump(if mnemonic.is_empty() { None } else { Some(&mnemonic) }, from + inst_len, next_rip);

        // 解释:元数据 + 真实 diff + 源码行
        let note = isa.note_for(&mnemonic);
        let mut detail: Vec<String> = reg_diff
            .iter()
            .map(|r| match r.dec {
                Some((b, a)) => format!("{}: {:#x}({}) → {:#x}({})", r.name, b, b, a, a),
                None => format!("{}: {} → {}", r.name, r.before, r.after),
            })
            .collect();
        for f in &flag_diff {
            detail.push(format!("{}: {} → {}({})", f.name, f.before as u8, f.after as u8, f.semantics));
        }
        if let Some(line) = stop.frame_line {
            detail.push(format!("当前源码行: {} 第 {} 行", stop.frame_file.as_deref().unwrap_or(SOURCE_FILE), line));
        }

        // 模式 B 没有内存写钩子:如实留空并在解释里标注,不伪造内存变化。
        let mem_changes: Vec<MemChangeItem> = Vec::new();

        StepReport {
            step_no: self.step_counter.load(Ordering::SeqCst),
            address: hx(from),
            size: inst_len as u32,
            machine_bytes_hex: machine_hex,
            disasm,
            mnemonic: mnemonic.clone(),
            after: after.iter().map(|(n, v)| (n.clone(), v.clone())).collect(),
            reg_diff,
            flag_diff,
            mem_changes,
            pc_jump: PcJump { from: hx(from), to: hx(next_rip), kind },
            explanation: Explanation {
                category: note.map(|n| n.category.clone()).unwrap_or_default(),
                summary: note.map(|n| n.summary.clone()).unwrap_or_else(|| {
                    format!("源码行 {}:{}", stop.frame_file.as_deref().unwrap_or(SOURCE_FILE), stop.frame_line.unwrap_or(0))
                }),
                pitfalls: note.map(|n| n.pitfalls.clone()).unwrap_or_default(),
                detail,
                related_cpp: note.and_then(|n| n.related_cpp.clone()),
                warnings: vec!["模式 B 未追踪内存写事件,请在内存面板按地址查看内存变化".into()],
            },
            stop: None,
            source_file: stop.frame_file.clone(),
            source_line: stop.frame_line,
        }
    }
}
