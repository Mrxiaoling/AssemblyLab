//! lab-engine — 会话编排与 StepReport 组装(解释层)。
//!
//! 分层原则(EXPERIMENT_ENGINE.md §3):StepEvents 只含引擎真实执行结果,
//! 本 crate 在其上计算 diff 并附加解释;所有数值必须引用执行结果,
//! 元数据(instruction_notes)只提供文字说明。

pub mod debug;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use lab_adapters::nasm;
use lab_adapters::unicorn::UnicornMachine;
use lab_core::{
    EmulatorAdapter, IsaDefinition, LabError, LineMapping, LoadSpec, ProgramImage, RegSnapshot,
    RegValue, RunControl, RunOutcome, StepEvents, StopReason,
};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// 对外 DTO(JSON 形态;u64 一律十六进制字符串,避免 JS 精度丢失)
// ---------------------------------------------------------------------------

/// u64 → "0x..." 十六进制字符串(API 序列化约定)
pub fn hx(v: u64) -> String {
    format!("{v:#x}")
}

/// 接受数字或 "0x.." 字符串
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum U64In {
    Num(u64),
    Str(String),
}

impl U64In {
    pub fn parse(&self) -> Result<u64, LabError> {
        match self {
            U64In::Num(n) => Ok(*n),
            U64In::Str(s) => lab_core::isa::parse_int(s),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RegDiffItem {
    pub name: String,
    pub before: String,
    pub after: String,
    /// 十进制注释(小于 2^53 才给,便于初学者)
    pub dec: Option<(u64, u64)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlagDiffItem {
    pub name: String,
    pub before: bool,
    pub after: bool,
    pub semantics: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemChangeItem {
    pub addr: String,
    pub size: u32,
    pub value_hex: String,
    pub region: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PcJump {
    pub from: String,
    pub to: String,
    pub kind: String, // seq | branch | call | ret | syscall
}

#[derive(Debug, Clone, Serialize)]
pub struct Explanation {
    /// 指令类别(instruction_notes)
    pub category: String,
    /// 指令说明
    pub summary: String,
    /// 常见误区
    pub pitfalls: Vec<String>,
    /// 本次真实变化的人读描述(全部来自引擎结果)
    pub detail: Vec<String>,
    /// 与 C++ 的联系
    pub related_cpp: Option<String>,
    /// 元数据声称受影响的标志与实际 diff 不一致时给出告警
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepReport {
    pub step_no: u64,
    pub address: String,
    pub size: u32,
    pub machine_bytes_hex: String,
    pub disasm: String,
    pub mnemonic: String,
    /// 执行后完整快照(面板顺序)
    pub after: Vec<(String, String)>,
    pub reg_diff: Vec<RegDiffItem>,
    pub flag_diff: Vec<FlagDiffItem>,
    pub mem_changes: Vec<MemChangeItem>,
    pub pc_jump: PcJump,
    pub explanation: Explanation,
    pub stop: Option<StopReason>,
    /// 模式 B:对应 C++ 源码位置(有调试信息时)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub executed: u64,
    pub output: Vec<String>,
    pub stop: StopReason,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TraceEntry {
    Step { step_no: u64, report: Box<StepReport> },
    Run { summary: RunSummary },
}

pub struct TraceLog {
    entries: VecDeque<TraceEntry>,
    pub dropped: u64,
    cap: usize,
}

impl TraceLog {
    const CAP: usize = 20_000;
    fn new() -> Self {
        Self { entries: VecDeque::new(), dropped: 0, cap: Self::CAP }
    }
    fn push(&mut self, e: TraceEntry) {
        if self.entries.len() >= self.cap {
            self.entries.pop_front();
            self.dropped += 1;
        }
        self.entries.push_back(e);
    }
    fn clear(&mut self) {
        self.entries.clear();
        self.dropped = 0;
    }
}

use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// 会话
// ---------------------------------------------------------------------------

/// 终态判定:断点不是终态(可继续),退出/错误/上限/超时是终态(需重置)。
fn is_terminal(stop: &StopReason) -> bool {
    !matches!(stop, StopReason::Breakpoint { .. })
}

pub struct Session {
    pub id: u64,
    pub isa: IsaDefinition,
    pub abi: String,
    pub source: Mutex<String>,
    pub image: Mutex<ProgramImage>,
    machine: Mutex<UnicornMachine>,
    breakpoints: Mutex<HashSet<u64>>,
    trace: Mutex<TraceLog>,
    step_counter: AtomicU64,
    /// 最近一次停止原因(terminal 状态判断)
    last_stop: Mutex<Option<StopReason>>,
    pub limits: RunControl,
}

const MAX_STEP_PER_REQUEST: u32 = 1000;

impl Session {
    pub fn create(id: u64, isa: IsaDefinition, abi: String, source: &str) -> Result<Self, LabError> {
        let image = assemble_for(&isa, source)?;
        let machine = UnicornMachine::new(&isa.emulator_arch, &isa.emulator_mode)?;
        let mut s = Self {
            id,
            isa,
            abi,
            source: Mutex::new(source.to_string()),
            image: Mutex::new(image),
            machine: Mutex::new(machine),
            breakpoints: Mutex::new(HashSet::new()),
            trace: Mutex::new(TraceLog::new()),
            step_counter: AtomicU64::new(0),
            last_stop: Mutex::new(None),
            limits: RunControl::default(),
        };
        s.reload_machine()?;
        Ok(s)
    }

    /// 由 ISA + image 构建 LoadSpec 并装载(创建/重置/重编译共用)。
    fn build_load_spec(&self, image: &ProgramImage) -> LoadSpec {
        let isa = &self.isa;
        let mut init_regs: Vec<(String, RegValue)> = isa
            .panel_registers
            .iter()
            .filter(|n| n.as_str() != isa.pc_register && n.as_str() != isa.stack_register && n.as_str() != isa.flags_register)
            .map(|n| (n.clone(), 0))
            .collect();
        init_regs.push((isa.flags_register.clone(), isa.flags_init));
        init_regs.push((isa.stack_register.clone(), isa.stack_init_value()));
        if let Some(lr) = &isa.link_register {
            // ARM64:X30(LR)预置终止桩 → 主程序 ret 即退出(等价 x86 [RSP]=哨兵)
            init_regs.push((lr.clone(), isa.exit_sentinel_addr()));
        }
        let heap_end = match &isa.memory_map.heap {
            Some(h) => lab_core::isa::parse_int(&h.base).unwrap_or(0) + h.size,
            None => isa.code_base() + isa.memory_map.code.size,
        };
        LoadSpec {
            base: image.base,
            entry: image.entry,
            bytes: image.bytes.clone(),
            regions: isa
                .memory_map
                .regions()
                .into_iter()
                .map(|(_, r)| {
                    (
                        lab_core::isa::parse_int(&r.base).unwrap_or(0),
                        r.size,
                    )
                })
                .collect(),
            panel_registers: isa.panel_registers.clone(),
            init_regs,
            exit_sentinel: isa.exit_sentinel_addr(),
            heap_end,
        }
    }

    fn reload_machine(&mut self) -> Result<(), LabError> {
        let image = self.image.lock().unwrap().clone();
        let spec = self.build_load_spec(&image);
        self.machine.lock().unwrap().load(&spec)?;
        *self.last_stop.lock().unwrap() = None;
        self.step_counter.store(0, Ordering::SeqCst);
        self.trace.lock().unwrap().clear();
        Ok(())
    }

    /// 重新汇编并重载;保留仍有效的断点。
    pub fn reassemble(&mut self, source: &str) -> Result<AssembleInfo, LabError> {
        let image = assemble_for(&self.isa.clone(), source)?;
        let code_end = image.base + image.bytes.len() as u64;
        {
            let mut bps = self.breakpoints.lock().unwrap();
            bps.retain(|a| *a >= image.base && *a < code_end);
        }
        *self.source.lock().unwrap() = source.to_string();
        *self.image.lock().unwrap() = image.clone();
        self.reload_machine()?;
        Ok(AssembleInfo::from(&image, &self.isa))
    }

    pub fn reset(&mut self) -> Result<(), LabError> {
        self.reload_machine()
    }

    pub fn step(&mut self, count: u32) -> Result<Vec<StepReport>, LabError> {
        if count == 0 || count > MAX_STEP_PER_REQUEST {
            return Err(LabError::BadRequest(format!("step count 须在 1..={MAX_STEP_PER_REQUEST}")));
        }
        {
            let last = self.last_stop.lock().unwrap();
            if let Some(stop) = last.as_ref().filter(|s| is_terminal(s)) {
                return Err(LabError::BadRequest(format!(
                    "程序已停止({stop_text});请先重置。",
                    stop_text = stop_text(stop)
                )));
            }
        }
        self.machine.lock().unwrap().clear_stop();
        let events = self.machine.lock().unwrap().step(count)?;
        let mut reports = Vec::with_capacity(events.len());
        let image = self.image.lock().unwrap().clone();
        for ev in events {
            let mut report = self.build_report(&ev, &image);
            self.step_counter.fetch_add(1, Ordering::SeqCst);
            let step_no = self.step_counter.load(Ordering::SeqCst);
            report.step_no = step_no;
            if let Some(stop) = &ev.stop {
                *self.last_stop.lock().unwrap() = Some(stop.clone());
            }
            self.trace.lock().unwrap().push(TraceEntry::Step {
                step_no,
                report: Box::new(report.clone()),
            });
            reports.push(report);
        }
        Ok(reports)
    }

    pub fn run(&mut self) -> Result<RunSummary, LabError> {
        {
            let last = self.last_stop.lock().unwrap();
            if let Some(stop) = last.as_ref().filter(|s| is_terminal(s)) {
                return Err(LabError::BadRequest(format!(
                    "程序已停止({stop_text});请先重置。",
                    stop_text = stop_text(stop)
                )));
            }
        }
        self.machine.lock().unwrap().clear_stop();
        let bps = self.breakpoints.lock().unwrap().clone();
        let outcome: RunOutcome = self.machine.lock().unwrap().run(&bps, self.limits)?;
        if let StopReason::Fault { .. } = &outcome.stop {
            // Fault 是终态
        }
        *self.last_stop.lock().unwrap() = Some(outcome.stop.clone());
        let summary = RunSummary {
            executed: outcome.executed,
            output: outcome.output.clone(),
            stop: outcome.stop.clone(),
        };
        self.trace.lock().unwrap().push(TraceEntry::Run { summary: summary.clone() });
        Ok(summary)
    }

    pub fn set_breakpoints(&mut self, addrs: &[u64]) -> Result<Vec<u64>, LabError> {
        let image = self.image.lock().unwrap();
        let code_end = image.base + image.bytes.len() as u64;
        let mut valid = Vec::new();
        for a in addrs {
            if *a >= image.base && *a < code_end {
                valid.push(*a);
            } else {
                return Err(LabError::BadRequest(format!(
                    "断点 {a:#x} 不在代码范围内 [{:#x}, {code_end:#x})",
                    image.base
                )));
            }
        }
        let mut bps = self.breakpoints.lock().unwrap();
        *bps = valid.iter().copied().collect();
        drop(image);
        self.machine.lock().unwrap().clear_stop();
        Ok(valid)
    }

    pub fn breakpoints(&self) -> Vec<u64> {
        self.breakpoints.lock().unwrap().iter().copied().collect()
    }

    pub fn set_register(&mut self, name: &str, value: RegValue) -> Result<(), LabError> {
        self.machine.lock().unwrap().write_reg(name, value)
    }

    pub fn write_memory(&mut self, addr: u64, bytes: &[u8]) -> Result<(), LabError> {
        self.machine.lock().unwrap().write_memory(addr, bytes)
    }

    pub fn read_memory(&mut self, addr: u64, len: u32) -> Result<MemoryView, LabError> {
        let bytes = self.machine.lock().unwrap().read_memory(addr, len)?;
        let region = self.isa.memory_map.region_of(addr).unwrap_or("unmapped").to_string();
        Ok(MemoryView { addr, region, bytes })
    }

    pub fn snapshot(&mut self) -> Result<RegSnapshot, LabError> {
        let names: Vec<String> = self.isa.panel_registers.clone();
        self.machine.lock().unwrap().read_regs(&names)
    }

    pub fn state(&mut self) -> SessionState {
        let guard = self.last_stop.lock().unwrap();
        let terminal = guard.as_ref().map(is_terminal).unwrap_or(false);
        let stop = guard.clone();
        SessionState { terminal, stop, step_count: self.step_counter.load(Ordering::SeqCst) }
    }

    pub fn trace(&mut self) -> (Vec<TraceEntry>, u64) {
        let t = self.trace.lock().unwrap();
        (t.entries.iter().cloned().collect(), t.dropped)
    }

    // -----------------------------------------------------------------------
    // StepReport 组装:diff + 解释(全部数值引用引擎结果)
    // -----------------------------------------------------------------------

    fn build_report(&self, ev: &StepEvents, image: &ProgramImage) -> StepReport {
        let isa = &self.isa;

        // 机器码字节
        let off = (ev.address - image.base).min(image.bytes.len() as u64) as usize;
        let len = (ev.size as usize).min(image.bytes.len().saturating_sub(off));
        let machine_bytes = &image.bytes[off..off + len];

        // 反汇编(ISA 分派:x86-64 → iced;arm64 → yaxpeax)
        let disasm = lab_adapters::disasm_one_for(&isa.id, machine_bytes, ev.address)
            .ok()
            .flatten();

        // 寄存器 diff
        let mut reg_diff = Vec::new();
        let before_map: HashMap<&str, u64> =
            ev.before.iter().map(|(n, v)| (n.as_str(), *v)).collect();
        let after_map: HashMap<&str, u64> =
            ev.after.iter().map(|(n, v)| (n.as_str(), *v)).collect();
        for (name, after_v) in ev.after.iter() {
            let before_v = before_map.get(name.as_str()).copied().unwrap_or(0);
            if before_v != *after_v {
                let dec = if *after_v <= u32::MAX as u64 && before_v <= u32::MAX as u64 {
                    Some((before_v, *after_v))
                } else {
                    None
                };
                reg_diff.push(RegDiffItem {
                    name: name.clone(),
                    before: hx(before_v),
                    after: hx(*after_v),
                    dec,
                });
            }
        }

        // 标志 diff(位级,来自真实 RFLAGS)
        let mut flag_diff = Vec::new();
        let fb = before_map.get(isa.flags_register.as_str()).copied().unwrap_or(0);
        let fa = after_map.get(isa.flags_register.as_str()).copied().unwrap_or(0);
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

        // 内存变化
        let mem_changes: Vec<MemChangeItem> = ev
            .mem_writes
            .iter()
            .map(|w| MemChangeItem {
                addr: hx(w.addr),
                size: w.size,
                value_hex: {
                    let n = w.size.min(8) as usize;
                    hex::encode_upper(&w.value_le[..n])
                },
                region: isa.memory_map.region_of(w.addr).unwrap_or("unmapped").to_string(),
            })
            .collect();

        // PC 跳转分类
        let next_rip = after_map.get(isa.pc_register.as_str()).copied().unwrap_or(0);
        let from = ev.address;
        let seq_to = from + ev.size as u64;
        let kind = classify_jump(disasm.as_ref().map(|d| d.mnemonic.as_str()), seq_to, next_rip);
        let pc_jump = PcJump { from: hx(from), to: hx(next_rip), kind: kind.clone() };

        // 解释(元数据文字 + 真实 diff 数值)
        let mnemonic_str = disasm.as_ref().map(|d| d.mnemonic.clone()).unwrap_or_default();
        let note = isa.note_for(&mnemonic_str);
        let mut detail = Vec::new();
        for r in &reg_diff {
            match r.dec {
                Some((b, a)) if a < 1_000_000 && b < 1_000_000 => {
                    detail.push(format!("{}: {:#x}({}) → {:#x}({})", r.name, b, b, a, a))
                }
                _ => detail.push(format!("{}: {} → {}", r.name, r.before, r.after)),
            }
        }
        for f in &flag_diff {
            detail.push(format!(
                "{}: {} → {}({})",
                f.name,
                if f.before { 1 } else { 0 },
                if f.after { 1 } else { 0 },
                f.semantics
            ));
        }
        for m in &mem_changes {
            detail.push(format!("写内存 [{}] = 0x{} ({}字节, {})", m.addr, m.value_hex, m.size, m.region));
        }
        let sp = isa.stack_register.as_str();
        let pc = isa.pc_register.as_str();
        match kind.as_str() {
            "call" => detail.push(format!("CALL: 返回地址已保存(x86 压栈 / ARM64 写 LR),{sp} 或寄存器状态更新,{pc} 跳转到目标")),
            "ret" => detail.push(format!("RET: 取回返回地址(x86 弹栈 / ARM64 读 LR),{pc} 跳回调用方")),
            "branch" => detail.push(format!("控制流转移: RIP {} → {}", hx(from), hx(next_rip))),
            _ => {}
        }

        // 一致性告警:元数据声称的 flags 与实际 diff 对不上时警告(不静默修正)
        let mut warnings = Vec::new();
        let no_flags = if isa.id == "arm64" { NO_FLAGS_INSTRUCTIONS_A64 } else { NO_FLAGS_INSTRUCTIONS };
        if note.is_some() && no_flags.contains(&mnemonic_str.to_ascii_uppercase().as_str())
            && !flag_diff.is_empty()
        {
            warnings.push(format!(
                "元数据声明 {mnemonic_str} 不影响标志位,但实际观察到 {} 项标志变化(以执行结果为准,请报告此不一致)",
                flag_diff.len()
            ));
        }

        let explanation = Explanation {
            category: note.map(|n| n.category.clone()).unwrap_or_default(),
            summary: note.map(|n| n.summary.clone()).unwrap_or_else(|| {
                format!("(暂无 {mnemonic_str} 的教学注释)")
            }),
            pitfalls: note.map(|n| n.pitfalls.clone()).unwrap_or_default(),
            detail,
            related_cpp: note.and_then(|n| n.related_cpp.clone()),
            warnings,
        };

        StepReport {
            step_no: 0, // 由调用方填
            address: hx(ev.address),
            size: ev.size,
            machine_bytes_hex: hex::encode_upper(machine_bytes),
            disasm: disasm.map(|d| d.text).unwrap_or_else(|| "(无法反汇编)".into()),
            mnemonic: mnemonic_str,
            after: ev.after.iter().map(|(n, v)| (n.clone(), hx(*v))).collect(),
            reg_diff,
            flag_diff,
            mem_changes,
            pc_jump,
            explanation,
            stop: ev.stop.clone(),
            source_file: None,
            source_line: None,
        }
    }
}

/// 已知不影响标志位的指令(一致性告警用;P1 最小子集)
const NO_FLAGS_INSTRUCTIONS: &[&str] = &["MOV", "LEA", "NOP", "PUSH", "POP"];
const NO_FLAGS_INSTRUCTIONS_A64: &[&str] = &["MOV", "MOVZ", "MOVK", "LDR", "STR", "STP", "LDP", "NOP", "ADRP", "ADR"];

pub fn classify_jump(mnemonic: Option<&str>, seq_to: u64, next_rip: u64) -> String {
    let m = mnemonic.unwrap_or("");
    let ml = m.to_ascii_lowercase();
    if next_rip == seq_to {
        return "seq".into();
    }
    match ml.as_str() {
        "call" | "bl" | "blr" => "call".into(),
        "ret" => "ret".into(),
        "syscall" | "svc" => "syscall".into(),
        _ => "branch".into(),
    }
}

fn stop_text(stop: &StopReason) -> String {
    match stop {
        StopReason::ProgramExit { code } => match code {
            Some(c) => format!("程序退出,返回码 {c}"),
            None => "程序退出".into(),
        },
        StopReason::Breakpoint { address } => format!("断点 @{address:#x}"),
        StopReason::InstructionLimit { .. } => "达到指令数上限".into(),
        StopReason::Timeout => "超时".into(),
        StopReason::Fault { message } => format!("执行错误:{message}"),
        StopReason::StepsDone => "步进结束".into(),
    }
}

// ---------------------------------------------------------------------------
// 视图 DTO
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct MemoryView {
    pub addr: u64,
    pub region: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionState {
    /// 终态(需重置才能继续);断点暂停不算终态
    pub terminal: bool,
    pub stop: Option<StopReason>,
    pub step_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssembleInfo {
    pub base: String,
    pub entry: String,
    pub size: u32,
    pub symbols: Vec<SymbolInfo>,
    pub line_map: Vec<LineMapping>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolInfo {
    pub name: String,
    pub addr: String,
}

impl AssembleInfo {
    pub fn from(image: &ProgramImage, _isa: &IsaDefinition) -> Self {
        Self {
            base: hx(image.base),
            entry: hx(image.entry),
            size: image.bytes.len() as u32,
            symbols: image
                .symbols
                .iter()
                .map(|(n, a)| SymbolInfo { name: n.clone(), addr: hx(*a) })
                .collect(),
            line_map: image.line_map.clone(),
        }
    }
}

/// 按 ISA 的汇编器声明分派汇编(nasm=x86 / gnu-as-aarch64=ARM64)。
fn assemble_for(isa: &IsaDefinition, source: &str) -> Result<ProgramImage, LabError> {
    match isa.assembler.as_str() {
        "nasm" => nasm::assemble(source, isa.code_base()),
        "gnu-as-aarch64" => lab_adapters::gnu_as::assemble(source, isa.code_base()),
        other => Err(LabError::BadRequest(format!(
            "ISA {} 声明了未支持的汇编器 {other}(支持 nasm / gnu-as-aarch64)",
            isa.id
        ))),
    }
}

/// 反汇编整个程序段(工作台汇编/反汇编视图;按 ISA 分派)。
pub fn disassemble_image(isa_id: &str, image: &ProgramImage) -> Result<Vec<lab_adapters::iced::DisasmLine>, LabError> {
    lab_adapters::disasm_for(isa_id, &image.bytes, image.base)
}

/// 初始化空汇编镜像注释(避免 unused 警告的占位使用)
#[allow(dead_code)]
fn _unused(_x: &HashMap<String, u64>) {}
