//! GDB/MI 适配器(模式 B):经 WSL 驱动真实 gdb,轻量 MI 解析器。
//!
//! 设计依据:docs/EXPERIMENT_ENGINE.md §4。命令白名单(SECURITY §4);
//! 所有输入经临时文件/参数转义,不透传用户原始文本。

use lab_core::LabError;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

const WSL_DISTRO: &str = "Ubuntu-22.04";
const CMD_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// MI 值模型与解析
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum MiValue {
    Str(String),
    List(Vec<MiValue>),
    Tuple(Vec<(String, MiValue)>),
}

impl MiValue {
    pub fn get(&self, key: &str) -> Option<&MiValue> {
        match self {
            MiValue::Tuple(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            MiValue::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_list(&self) -> Option<&[MiValue]> {
        match self {
            MiValue::List(l) => Some(l),
            _ => None,
        }
    }
}

/// 解析 MI 结果串:`frame={addr="0x1",args=[{name="a",value="1"}]},thread-id="1"`
pub fn parse_mi_results(s: &str) -> MiValue {
    let bytes: Vec<char> = s.chars().collect();
    let mut pos = 0usize;
    let v = parse_tuple_body(&bytes, &mut pos);
    v
}

fn skip_ws(b: &[char], pos: &mut usize) {
    while *pos < b.len() && b[*pos].is_whitespace() {
        *pos += 1;
    }
}

fn parse_tuple_body(b: &[char], pos: &mut usize) -> MiValue {
    let mut pairs = Vec::new();
    loop {
        skip_ws(b, pos);
        if *pos >= b.len() || b[*pos] == '}' {
            return MiValue::Tuple(pairs);
        }
        let before = *pos;
        // name = value
        let name: String = b[*pos..].iter().take_while(|c| c.is_alphanumeric() || **c == '-' || **c == '_').collect();
        *pos += name.len();
        skip_ws(b, pos);
        if *pos < b.len() && b[*pos] == '=' {
            *pos += 1;
        }
        let value = parse_value(b, pos);
        pairs.push((name, value));
        skip_ws(b, pos);
        if *pos < b.len() && b[*pos] == ',' {
            *pos += 1;
        }
        // 防停滞:任何一轮没推进位置就强制跳过一个字符(畸形记录兜底)
        if *pos == before {
            if *pos < b.len() && b[*pos] == '}' {
                return MiValue::Tuple(pairs);
            }
            *pos += 1;
        }
        if pairs.len() > 100_000 {
            return MiValue::Tuple(pairs);
        }
    }
}

fn parse_value(b: &[char], pos: &mut usize) -> MiValue {
    skip_ws(b, pos);
    if *pos >= b.len() {
        return MiValue::Str(String::new());
    }
    match b[*pos] {
        '"' => MiValue::Str(parse_cstring(b, pos)),
        '{' => {
            *pos += 1;
            skip_ws(b, pos);
            // {..} 可能是 tuple 或 list(如 args=[{..},{..}]);以是否有 `name=` 判断
            let v = parse_tuple_body(b, pos);
            if *pos < b.len() && b[*pos] == '}' {
                *pos += 1;
            }
            if let MiValue::Tuple(pairs) = &v {
                if pairs.is_empty() {
                    // 空元组按空串处理(gdb 空结构)
                    return MiValue::Str(String::new());
                }
            }
            v
        }
        '[' => {
            *pos += 1;
            let mut items = Vec::new();
            loop {
                skip_ws(b, pos);
                if *pos >= b.len() || b[*pos] == ']' || b[*pos] == '}' {
                    // ']' 由本层闭合;'}' 异常越层 → 交还上层(不前进,防死循环)
                    if *pos < b.len() && b[*pos] == ']' {
                        *pos += 1;
                    }
                    break;
                }
                if items.len() > 100_000 {
                    break; // 畸形记录硬上限
                }
                let before = *pos;
                items.push(parse_value(b, pos));
                skip_ws(b, pos);
                if *pos < b.len() && b[*pos] == ',' {
                    *pos += 1;
                }
                // 防停滞
                if *pos == before {
                    *pos += 1;
                }
            }
            MiValue::List(items)
        }
        _ => {
            // 裸 token;若后随 '=' 则是 list 内的 name=value 形式(如 stack=[frame={..}])
            let tok: String = b[*pos..]
                .iter()
                .take_while(|c| !matches!(**c, ',' | ']' | '}' | '=' | '{'))
                .collect();
            *pos += tok.len();
            skip_ws(b, pos);
            if *pos < b.len() && b[*pos] == '=' {
                *pos += 1;
                let v = parse_value(b, pos);
                return MiValue::Tuple(vec![(tok.trim().to_string(), v)]);
            }
            MiValue::Str(tok.trim().to_string())
        }
    }
}

fn parse_cstring(b: &[char], pos: &mut usize) -> String {
    // 已知 b[*pos] == '"'
    *pos += 1;
    let mut out = String::new();
    while *pos < b.len() {
        match b[*pos] {
            '"' => {
                *pos += 1;
                break;
            }
            '\\' => {
                *pos += 1;
                if *pos < b.len() {
                    out.push(match b[*pos] {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        other => other,
                    });
                    *pos += 1;
                }
            }
            c => {
                out.push(c);
                *pos += 1;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// gdb 子进程会话
// ---------------------------------------------------------------------------

enum RawLine {
    Result { token: Option<u64>, class: String, results: MiValue },
    Stop(MiValue),
    Notify,
    /// 流输出(~"…"/@"…"/&"…"),携带原始行(console_capture 用)
    Console,
    Other,
}

fn classify(line: &str) -> RawLine {
    if let Some(rest) = line.strip_prefix('*') {
        if rest.starts_with("stopped") {
            let idx = rest.find(',').unwrap_or(rest.len());
            let results = parse_mi_results(&rest[idx..]);
            return RawLine::Stop(results);
        }
        return RawLine::Notify;
    }
    // 带 token 的结果记录:"12^done,..." —— 必须先于特殊前缀检查
    if let Some(digit_end) = line.find(|c: char| !c.is_ascii_digit()) {
        if digit_end > 0 && line[digit_end..].starts_with('^') {
            let token: Option<u64> = line[..digit_end].parse().ok();
            let rest = &line[digit_end + 1..];
            let class_end = rest.find(',').unwrap_or(rest.len());
            let class = rest[..class_end].to_string();
            let results = if class_end < rest.len() {
                parse_mi_results(&rest[class_end..])
            } else {
                MiValue::Tuple(Vec::new())
            };
            return RawLine::Result { token, class, results };
        }
    }
    for p in ['^', '=', '~', '&', '@'] {
        if let Some(rest) = line.strip_prefix(p) {
            match p {
                '^' => {
                    let class_end = rest.find(',').unwrap_or(rest.len());
                    let class = rest[..class_end].to_string();
                    let results = if class_end < rest.len() {
                        parse_mi_results(&rest[class_end..])
                    } else {
                        MiValue::Tuple(Vec::new())
                    };
                    return RawLine::Result { token: None, class, results };
                }
                '=' => return RawLine::Notify,
                '~' | '@' | '&' => return RawLine::Console,
                _ => {}
            }
        }
    }
    RawLine::Other
}

pub struct GdbSession {
    /// socat 宿主进程(杀掉即清理 gdb)
    socat: Child,
    stream_tx: std::sync::Mutex<Option<std::net::TcpStream>>,
    rx: Receiver<String>,
    token: u64,
    /// 全部寄存器名(首次查询后缓存)
    reg_names: Vec<String>,
}

#[derive(Debug)]
pub struct GdbStop {
    pub reason: String,
    pub frame_addr: Option<u64>,
    pub frame_func: Option<String>,
    pub frame_file: Option<String>,
    pub frame_line: Option<u32>,
}

// 端口随机化:不再由 pid + 自增序列推算。socat 只绑 127.0.0.1,
// 配合不可预测的端口降低同机其它进程误连/劫持 MI 会话的可能。
fn next_gdb_port() -> u16 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let n = NONCE.fetch_add(1, Ordering::SeqCst);
    let mixed = nanos ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ ((std::process::id() as u64) << 17);
    20_000 + (mixed % 35_000) as u16
}

impl GdbSession {
    /// 在 wsl_dir(WSL 路径)内调试 ./main。
    /// 传输:wsl 内 socat 把 gdb/MI 挂到 TCP(绕开 wsl.exe stdin 管道中继的 30s 延迟 bug)。
    pub fn spawn(wsl_dir: &str) -> Result<Self, LabError> {
        Self::spawn_debug(wsl_dir, "gdb", None)
    }

    /// ARM64 远程调试(P8):WSL 内 gdb-multiarch 经 socat 挂 MI,
    /// `target remote` 连接远端 gdbserver(用户提供的 ARM64 机器)。
    /// 流程:交叉编译 → scp 到远端 → ssh 启动 gdbserver → gdb-multiarch 连接。
    pub fn spawn_multiarch_remote(wsl_dir: &str, remote_target: &str) -> Result<Self, LabError> {
        Self::spawn_debug(wsl_dir, "gdb-multiarch", Some(remote_target.to_string()))
    }

    pub fn spawn_debug(
        wsl_dir: &str,
        gdb_bin: &str,
        remote_target: Option<String>,
    ) -> Result<Self, LabError> {
        let port = next_gdb_port();
        let script = format!(
            "cd '{wsl_dir}' && exec socat TCP-LISTEN:{port},bind=127.0.0.1,reuseaddr EXEC:\"{gdb_bin} -interpreter=mi2 ./main\"",
            wsl_dir = wsl_dir,
            port = port,
            gdb_bin = gdb_bin
        );
        let socat = Command::new("wsl.exe")
            .args(["-d", WSL_DISTRO, "-e", "bash", "-c", &script])
            .spawn()
            .map_err(|e| LabError::ToolUnavailable(format!("socat 启动失败: {e}")))?;

        // TCP 连接重试(WSL2 localhost 转发 + gdb 启动需要时间)
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut stream = None;
        while std::time::Instant::now() < deadline {
            match std::net::TcpStream::connect(format!("127.0.0.1:{port}")) {
                Ok(s) => {
                    s.set_nodelay(true).ok();
                    stream = Some(s);
                    break;
                }
                Err(_) => std::thread::sleep(Duration::from_millis(300)),
            }
        }
        let stream = stream.ok_or_else(|| LabError::ToolUnavailable("gdb TCP 端口连接超时".into()))?;

        let write_half = stream.try_clone().map_err(|e| LabError::Emulator(e.to_string()))?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let reader = BufReader::new(stream);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        // MI 原始输出可能含源码/表达式,不再打到 stderr
                        if tx.send(l).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let mut s = Self {
            socat,
            stream_tx: std::sync::Mutex::new(Some(write_half)),
            rx,
            token: 0,
            reg_names: Vec::new(),
        };
        s.wait_ready()?;
        // 远程调试:连接远端 gdbserver(target-select;进程此时停在入口)
        if let Some(rt) = remote_target {
            s.exec_raw(&format!("-target-select remote {rt}"))
                .map_err(|e| LabError::Emulator(format!("target remote 失败: {e}")))?;
        }
        Ok(s)
    }

    fn wait_ready(&mut self) -> Result<(), LabError> {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if std::time::Instant::now() > deadline {
                self.hard_kill();
                return Err(LabError::ToolUnavailable("gdb 启动超时(已终止调试进程)".into()));
            }
            match self.rx.recv_timeout(Duration::from_secs(5)) {
                Ok(line) => {
                    if line.starts_with("(gdb)") {
                        return Ok(());
                    }
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(_) => return Err(LabError::Emulator("gdb 连接断开".into())),
            }
        }
    }

    /// 命令超时/停滞时硬杀 socat + gdb进程树(README:超时不是沙箱,
    /// 不能让失控的调试进程残留;Drop 不触发的场景由本方法兜底)。
    fn hard_kill(&mut self) {
        let _ = self.socat.kill();
        let _ = self.socat.wait();
        *self.stream_tx.lock().unwrap() = None;
    }

    fn send(&mut self, cmd: &str) -> Result<(), LabError> {
        // MI 是按行解析的:任何控制字符(尤其 \n、\r)都能把一条命令拆成多条,
        // 用户可控的观察点表达式等必须在此统一拦截。
        if cmd.chars().any(|c| c.is_control()) {
            return Err(LabError::BadRequest(
                "MI 命令含控制字符,已拒绝(防止命令注入)".into(),
            ));
        }
        self.token += 1;
        let mut guard = self.stream_tx.lock().unwrap();
        let w = guard.as_mut().ok_or_else(|| LabError::Emulator("gdb 已退出".into()))?;
        writeln!(w, "{}{}", self.token, cmd).map_err(|e| LabError::Emulator(format!("写入失败: {e}")))?;
        w.flush().map_err(|e| LabError::Emulator(format!("flush 失败: {e}")))?;
        Ok(())
    }

    /// 发送命令并等待其结果记录(^done/^error),途中收集 *stopped。
    fn request(&mut self, cmd: &str) -> Result<Result<MiValue, String>, LabError> {
        self.send(cmd)?;
        let deadline = std::time::Instant::now() + CMD_TIMEOUT;
        loop {
            if std::time::Instant::now() > deadline {
                self.hard_kill();
                return Err(LabError::Emulator("gdb 命令超时(已终止调试进程)".into()));
            }
            match self.rx.recv_timeout(CMD_TIMEOUT) {
                Ok(line) => match classify(&line) {
                    RawLine::Result { token, class, results } => {
                        if token == Some(self.token) {
                            // "connected" 仅由 -target-select 成功时返回(P8 远程调试)
                            if class == "done" || class == "running" || class == "connected" {
                                return Ok(Ok(results));
                            }
                            let msg = results.get("msg").and_then(|m| m.as_str()).unwrap_or(&class).to_string();
                            return Ok(Err(msg));
                        }
                        // 其它 token 忽略
                    }
                    RawLine::Stop(_) => { /* exec-run 等场景由专用方法消费 */ }
                    _ => {}
                },
                Err(RecvTimeoutError::Timeout) => {
                    self.hard_kill();
                    return Err(LabError::Emulator(format!("gdb 响应超时(已终止调试进程): {cmd}")));
                }
                Err(_) => return Err(LabError::Emulator("gdb 进程退出".into())),
            }
        }
    }

    /// 执行 run/continue:等 ^running,再等 *stopped
    pub fn exec_run(&mut self, cmd: &str) -> Result<GdbStop, LabError> {
        self.send(cmd)?;
        let deadline = std::time::Instant::now() + CMD_TIMEOUT;
        loop {
            if std::time::Instant::now() > deadline {
                self.hard_kill();
                return Err(LabError::Emulator("程序执行超时(已终止调试进程)".into()));
            }
            match self.rx.recv_timeout(CMD_TIMEOUT) {
                Ok(line) => match classify(&line) {
                    RawLine::Result { token, class, results } => {
                        if token == Some(self.token) {
                            if class == "running" {
                                continue;
                            }
                            let msg = results.get("msg").and_then(|m| m.as_str()).unwrap_or(&class).to_string();
                            return Err(LabError::BadRequest(format!("gdb: {msg}")));
                        }
                    }
                    RawLine::Stop(results) => return Ok(to_stop(&results)),
                    _ => {}
                },
                Err(RecvTimeoutError::Timeout) => {
                    self.hard_kill();
                    return Err(LabError::Emulator("等待停止事件超时(已终止调试进程)".into()));
                }
                Err(_) => return Err(LabError::Emulator("gdb 进程退出".into())),
            }
        }
    }

    fn exec_raw(&mut self, cmd: &str) -> Result<MiValue, LabError> {
        self.request(cmd).and_then(|r| r.map_err(|e| LabError::BadRequest(format!("gdb: {e}"))))
    }

    // -----------------------------------------------------------------
    // 教学操作
    // -----------------------------------------------------------------

    /// 符号断点(如 "main")
    pub fn insert_symbol_breakpoint(&mut self, symbol: &str) -> Result<u64, LabError> {
        let v = self.exec_raw(&format!("-break-insert {symbol}"))?;
        let num = v
            .get("bkpt")
            .and_then(|b| b.get("number"))
            .and_then(|n| n.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Ok(num)
    }

    pub fn insert_breakpoint(&mut self, file: &str, line: u32) -> Result<u64, LabError> {
        let v = self.exec_raw(&format!("-break-insert {file}:{line}"))?;
        let num = v
            .get("bkpt")
            .and_then(|b| b.get("number"))
            .and_then(|n| n.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Ok(num)
    }

    pub fn delete_breakpoint(&mut self, num: u64) -> Result<(), LabError> {
        self.exec_raw(&format!("-break-delete {num}"))?;
        Ok(())
    }

    /// 观察点(写观察):表达式被写入时停下。
    /// 需要 frame 上下文——未启动时会报错,由调用方决定先 run。
    pub fn insert_watchpoint(&mut self, expr: &str) -> Result<u64, LabError> {
        // 严格表达式白名单:只允许标识符、寄存器前缀与简单算术/解引用,
        // 拒绝引号、换行、分号、反引号、$() 等可改变 MI 命令结构的字符。
        const ALLOWED: &str = " ()[]+-*._:$&";
        if expr.is_empty()
            || expr.len() > 200
            || !expr
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ALLOWED.contains(c))
        {
            return Err(LabError::BadRequest(
                "观察点表达式含非法字符(仅允许标识符、寄存器与简单算术)".into(),
            ));
        }
        let quoted = format!("\"{}\"", expr.replace('\\', "\\\\").replace('"', "\\\""));
        let v = self.exec_raw(&format!("-break-watch {quoted}"))?;
        let num = v
            .get("wpt")
            .and_then(|b| b.get("number"))
            .and_then(|n| n.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Ok(num)
    }

    pub fn run(&mut self) -> Result<GdbStop, LabError> {
        self.exec_run("-exec-run")
    }

    pub fn cont(&mut self) -> Result<GdbStop, LabError> {
        self.exec_run("-exec-continue")
    }

    pub fn step_instruction(&mut self, into: bool) -> Result<GdbStop, LabError> {
        let cmd = if into { "-exec-step-instruction" } else { "-exec-next-instruction" };
        self.exec_run(cmd)
    }

    pub fn finish(&mut self) -> Result<GdbStop, LabError> {
        self.exec_run("-exec-finish")
    }

    /// 当前全部寄存器(名称 → 16 进制值字符串)
    pub fn registers(&mut self) -> Result<Vec<(String, String)>, LabError> {
        if self.reg_names.is_empty() {
            let v = self.exec_raw("-data-list-register-names")?;
            if let Some(list) = v.get("register-names").and_then(|l| l.as_list()) {
                self.reg_names = list.iter().filter_map(|s| s.as_str().map(String::from)).collect();
            }
        }
        let v = self.exec_raw("-data-list-register-values x")?;
        let mut out = Vec::new();
        if let Some(list) = v.get("register-values").and_then(|l| l.as_list()) {
            for item in list {
                let num: usize = item
                    .get("number")
                    .and_then(|n| n.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(usize::MAX);
                let value = item.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if let Some(name) = self.reg_names.get(num) {
                    out.push((name.clone(), value));
                }
            }
        }
        Ok(out)
    }

    /// GPR 子集快照(与模式 A 面板对齐)
    /// ARM64 快照(P8 远程调试):X0-X30/SP/PC/NZCV
    pub fn snapshot_arm64(&mut self) -> Result<Vec<(String, String)>, LabError> {
        let all = self.registers()?;
        let mut out = Vec::new();
        // X0-X30(gdb 名小写 x0-x30)
        for i in 0..31 {
            let name = format!("x{i}");
            if let Some(v) = all.iter().find(|(n, _)| n.eq_ignore_ascii_case(&name)).map(|(_, v)| v.clone()) {
                out.push((format!("X{i}"), v));
            }
        }
        // SP/PC/NZCV(值形如 "0x246 [ ... ]" → 取首个 token 的逻辑与 x86 一致)
        for (gdb_name, display) in [("sp", "SP"), ("pc", "PC"), ("cpsr", "NZCV")] {
            if let Some(v) = all.iter().find(|(n, _)| n.eq_ignore_ascii_case(gdb_name)).map(|(_, v)| v.clone()) {
                let first = v.split_whitespace().next().unwrap_or(&v).to_string();
                out.push((display.to_string(), first));
            }
        }
        Ok(out)
    }

    pub fn snapshot(&mut self) -> Result<Vec<(String, String)>, LabError> {
        let all = self.registers()?;
        // (gdb 寄存器名, 面板显示名) — 与模式 A 的 RFLAGS 命名对齐
        const WANT: [(&str, &str); 10] = [
            ("rax", "RAX"), ("rbx", "RBX"), ("rcx", "RCX"), ("rdx", "RDX"),
            ("rsi", "RSI"), ("rdi", "RDI"), ("rbp", "RBP"), ("rsp", "RSP"),
            ("rip", "RIP"), ("eflags", "RFLAGS"),
        ];
        Ok(WANT
            .iter()
            .filter_map(|(w, display)| {
                all.iter().find(|(n, _)| n.eq_ignore_ascii_case(w)).map(|(_, v)| {
                    // 值形如 "0x246 [ PF IF ]" → 取首个 token
                    let first = v.split_whitespace().next().unwrap_or(v);
                    (display.to_string(), first.to_string())
                })
            })
            .collect())
    }

    pub fn read_memory(&mut self, addr: u64, len: u32) -> Result<Vec<u8>, LabError> {
        let v = self.exec_raw(&format!("-data-read-memory-bytes 0x{addr:x} {len}"))?;
        let mut out = Vec::new();
        if let Some(list) = v.get("memory").and_then(|l| l.as_list()) {
            for m in list {
                if let Some(hex) = m.get("contents").and_then(|c| c.as_str()) {
                    for pair in hex.as_bytes().chunks(2) {
                        let s = std::str::from_utf8(pair).unwrap_or("00");
                        out.push(u8::from_str_radix(s, 16).unwrap_or(0));
                    }
                }
            }
        }
        Ok(out)
    }

    pub fn frames(&mut self) -> Result<Vec<FrameInfo>, LabError> {
        let v = self.exec_raw("-stack-list-frames")?;
        let mut out = Vec::new();
        if let Some(list) = v.get("stack").and_then(|l| l.as_list()) {
            for item in list {
                // MI 形态:stack=[frame={..}] — 解包 frame= 前缀
                let f = item.get("frame").unwrap_or(item);
                out.push(FrameInfo {
                    level: f.get("level").and_then(|x| x.as_str()).and_then(|s| s.parse().ok()).unwrap_or(0),
                    func: f.get("func").and_then(|x| x.as_str()).unwrap_or("?").to_string(),
                    file: f.get("file").and_then(|x| x.as_str()).map(String::from),
                    line: f.get("line").and_then(|x| x.as_str()).and_then(|s| s.parse().ok()),
                    addr: f.get("addr").and_then(|x| x.as_str()).and_then(|s| s.parse().ok()),
                });
            }
        }
        Ok(out)
    }

    pub fn variables(&mut self) -> Result<Vec<(String, String)>, LabError> {
        let v = self.exec_raw("-stack-list-variables --simple-values")?;
        let mut out = Vec::new();
        if let Some(list) = v.get("variables").and_then(|l| l.as_list()) {
            for item in list {
                let var = item.get("var").or(Some(item));
                if let Some(var) = var {
                    let name = var.get("name").and_then(|x| x.as_str()).unwrap_or("?").to_string();
                    let value = var.get("value").and_then(|x| x.as_str()).unwrap_or("?").to_string();
                    out.push((name, value));
                }
            }
        }
        Ok(out)
    }

    // -------------------------------------------------------------------
    // 协程支持(P4):console 命令捕获 + 帧读取
    // -------------------------------------------------------------------

    /// 执行 console 命令并收集 ~"…" 流输出(按 token 匹配 ^done)。
    /// 用于 ptype / 打印结构体等 MI 原生不覆盖的场景。
    pub fn console_capture(&mut self, cmd: &str) -> Result<String, LabError> {
        // console 参数内不能有裸双引号
        // 白名单:console 通道只允许只读 gdb 命令(打印/类型/查看),
        // 防止 "-interpreter-exec console \"shell …\"" 执行任意命令(release 下 debug_assert 失效)
        const ALLOWED_CONSOLE_PREFIXES: [&str; 4] = ["p ", "ptype", "info ", "x "];
        if !ALLOWED_CONSOLE_PREFIXES.iter().any(|p| cmd.starts_with(p)) {
            return Err(LabError::BadRequest(format!(
                "console 命令不在白名单(仅允许只读 p/ptype/info/x): {cmd}"
            )));
        }
        if cmd.chars().any(|c| c.is_control()) {
            return Err(LabError::BadRequest("console 命令含控制字符(已拒绝)".into()));
        }
        if cmd.contains('"') || cmd.contains(';') || cmd.contains('`') || cmd.contains('$') {
            return Err(LabError::BadRequest("console 命令含非法字符(已拒绝)".into()));
        }
        self.send(&format!("-interpreter-exec console \"{cmd}\""))?;
        let deadline = std::time::Instant::now() + CMD_TIMEOUT;
        let mut out = String::new();
        loop {
            if std::time::Instant::now() > deadline {
                self.hard_kill();
                return Err(LabError::Emulator("console 命令超时(已终止调试进程)".into()));
            }
            match self.rx.recv_timeout(CMD_TIMEOUT) {
                Ok(line) => {
                    // 流输出行:~"text"、&"log"、@"target"
                    if line.starts_with('~') || line.starts_with('&') || line.starts_with('@') {
                        out.push_str(&unescape_mi_cstring(&line[1..]));
                        out.push('\n');
                        continue;
                    }
                    match classify(&line) {
                        RawLine::Result { token, class, results } => {
                            if token == Some(self.token) {
                                if class == "done" || class == "running" {
                                    return Ok(out);
                                }
                                let msg = results.get("msg").and_then(|m| m.as_str()).unwrap_or(&class).to_string();
                                return Err(LabError::BadRequest(format!("gdb: {msg}")));
                            }
                        }
                        _ => {}
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    self.hard_kill();
                    return Err(LabError::Emulator("gdb 响应超时(已终止调试进程)".into()))
                }
                Err(_) => return Err(LabError::Emulator("gdb 进程退出".into())),
            }
        }
    }

    /// 协程帧快照:(帧地址, [(字段, 值)])。解析 console `p __coro_frame` 的
    /// 单行 pretty 输出("$N = {f = v, f = v, …}")——MI varobj 对该类型 numchild=0,
    /// 不可用。当前不在协程内(<optimized out>/无符号)时报错。
    pub fn coro_frame(&mut self) -> Result<(String, Vec<(String, String)>), LabError> {
        // 1) 帧地址(取不了地址 = 不在协程内,报错)
        let addr_out = self.console_capture("p &__coro_frame")?;
        let addr = addr_out
            .lines()
            .find_map(|l| l.split_whitespace().find_map(|t| u64::from_str_radix(t.trim_start_matches("0x"), 16).ok()))
            .unwrap_or(0);
        // 2) 单行结构体文本 → 顶层逗号分割字段
        let pretty = self.console_capture("p __coro_frame")?;
        let line = pretty
            .lines()
            .find(|l| l.contains('{') && l.contains('}'))
            .ok_or_else(|| LabError::BadRequest("gdb: __coro_frame 输出无结构体文本".into()))?;
        let open = line.find('{').unwrap();
        let close = line.rfind('}').unwrap();
        if close <= open {
            return Err(LabError::BadRequest("gdb: __coro_frame 帧为空/不可读".into()));
        }
        let body = &line[open + 1..close];
        let mut out = Vec::new();
        let mut depth = 0i32;
        let mut cur = String::new();
        let flush = |chunk: &str, out: &mut Vec<(String, String)>| {
            if let Some(eq) = chunk.find(" = ") {
                let name = chunk[..eq].trim().to_string();
                let value = chunk[eq + 3..].trim().to_string();
                if !name.is_empty() {
                    out.push((name, if value.is_empty() { "?".to_string() } else { value }));
                }
            }
        };
        for c in body.chars() {
            match c {
                '{' | '<' => {
                    depth += 1;
                    cur.push(c);
                }
                '}' | '>' => {
                    depth -= 1;
                    cur.push(c);
                }
                ',' if depth == 0 => {
                    flush(&cur, &mut out);
                    cur.clear();
                }
                _ => cur.push(c),
            }
        }
        flush(&cur, &mut out);
        Ok((format!("{addr:#x}"), out))
    }

    /// 当前协程状态编号(__coro_frame.__coro_index;None = 不在协程内/不可用)
    pub fn coro_index(&mut self) -> Option<i32> {
        // 裸名 __coro_index 会被 gdb 当类型名歧义拒绝,须走帧成员
        let out = self
            .console_capture("p /d __coro_frame.__coro_index")
            .or_else(|_| self.console_capture("p /d __coro_index"))
            .ok()?;
        out.lines()
            .find_map(|l| l.rsplit('$').next())
            .and_then(|t| t.trim().rsplit(' ').next())
            .and_then(|t| t.trim().parse().ok())
    }
}

/// 还原 MI 流输出的 C 风格转义("0x41 → A,\\n → 换行,\\\" → 引号)
fn unescape_mi_cstring(s: &str) -> String {
    let body = s.strip_prefix('"').unwrap_or(s);
    let body = body.strip_suffix('"').unwrap_or(body);
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('x') => {
                let mut hex = String::new();
                for _ in 0..2 {
                    if let Some(h) = chars.next() {
                        if h.is_ascii_hexdigit() { hex.push(h); } else { break; }
                    }
                }
                if let Ok(b) = u8::from_str_radix(&hex, 16) {
                    out.push(b as char);
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn to_stop(v: &MiValue) -> GdbStop {
    let mut reason = v.get("reason").and_then(|r| r.as_str()).unwrap_or("exited-normally").to_string();
    let frame = v.get("frame");
    // 观察点触发:把 wpt 表达式与 old→new 附加进 reason,前端可直接展示
    if reason.contains("watchpoint") {
        if let Some(wpt) = v.get("wpt") {
            let exp = wpt.get("exp").and_then(|e| e.as_str()).unwrap_or("?");
            let val = v.get("value");
            let old = val.and_then(|x| x.get("old")).and_then(|s| s.as_str()).unwrap_or("?");
            let new = val.and_then(|x| x.get("new")).and_then(|s| s.as_str()).unwrap_or("?");
            reason = format!("{reason} [{exp}: {old} → {new}]");
        }
    }
    GdbStop {
        reason,
        frame_addr: frame.and_then(|f| f.get("addr")).and_then(|a| a.as_str()).and_then(|s| s.parse().ok()),
        frame_func: frame.and_then(|f| f.get("func")).and_then(|a| a.as_str()).map(String::from),
        frame_file: frame.and_then(|f| f.get("file")).and_then(|a| a.as_str()).map(String::from),
        frame_line: frame.and_then(|f| f.get("line")).and_then(|a| a.as_str()).and_then(|s| s.parse().ok()),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FrameInfo {
    pub level: u32,
    pub func: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub addr: Option<u64>,
}

impl Drop for GdbSession {
    fn drop(&mut self) {
        let _ = self.send("-gdb-exit");
        *self.stream_tx.lock().unwrap() = None;
        let _ = self.socat.kill();
        let _ = self.socat.wait();
    }
}
