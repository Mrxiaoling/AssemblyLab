//! Unicorn 模式 A 适配器(预编译 DLL,运行时加载)。
//!
//! 设计依据:docs/EXPERIMENT_ENGINE.md §2,实证记录 spikes/unicorn_step_diff/RESULT.md。
//! 线程模型:引擎句柄非 Send-safe,由持有方(Session)用 Mutex 串行化访问;
//! 本类型 `unsafe impl Send` 仅为了能放进 Mutex,跨线程使用是调用方的契约责任。

use std::collections::HashSet;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::OnceLock;

use lab_core::{
    EmulatorAdapter, LabError, LoadSpec, MemWrite, RegSnapshot, RegValue, RunControl, RunOutcome,
    StepEvents, StopReason,
};

use crate::consts as uc;

// ---------------------------------------------------------------------------
// DLL 运行时加载
// ---------------------------------------------------------------------------

extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
}

type FnOpen = unsafe extern "system" fn(u32, u32, *mut *mut c_void) -> u32;
type FnClose = unsafe extern "system" fn(*mut c_void) -> u32;
type FnMemMap = unsafe extern "system" fn(*mut c_void, u64, u64, u32) -> u32;
type FnMemWrite = unsafe extern "system" fn(*mut c_void, u64, *const u8, u64) -> u32;
type FnMemRead = unsafe extern "system" fn(*mut c_void, u64, *mut u8, u64) -> u32;
type FnRegRead = unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> u32;
type FnRegWrite = unsafe extern "system" fn(*mut c_void, u32, *const c_void) -> u32;
type FnEmuStart = unsafe extern "system" fn(*mut c_void, u64, u64, u64, usize) -> u32;
type FnEmuStop = unsafe extern "system" fn(*mut c_void) -> u32;
type FnHookAdd7 = unsafe extern "system" fn(*mut c_void, *mut u64, u32, *mut c_void, *mut c_void, u64, u64) -> u32;
type FnHookAdd8 = unsafe extern "system" fn(*mut c_void, *mut u64, u32, *mut c_void, *mut c_void, u64, u64, u32) -> u32;
type FnErrno = unsafe extern "system" fn(*mut c_void) -> u32;
type FnVersion = unsafe extern "system" fn(*mut u32, *mut u32) -> u32;
type FnQuery = unsafe extern "system" fn(*mut c_void, u32, *mut u64) -> u32;

#[derive(Clone, Copy)]
pub struct UcApiTable {
    pub uc_open: FnOpen,
    pub uc_close: FnClose,
    pub uc_mem_map: FnMemMap,
    pub uc_mem_write: FnMemWrite,
    pub uc_mem_read: FnMemRead,
    pub uc_reg_read: FnRegRead,
    pub uc_reg_write: FnRegWrite,
    pub uc_emu_start: FnEmuStart,
    pub uc_emu_stop: FnEmuStop,
    pub uc_hook_add: FnHookAdd7,
    pub uc_hook_add_insn: FnHookAdd8,
    pub uc_errno: FnErrno,
    pub uc_version: FnVersion,
    pub uc_query: FnQuery,
}

unsafe fn get_sym(h: *mut c_void, name: &str) -> Result<*mut c_void, LabError> {
    let mut cname = format!("{name}\0");
    let p = GetProcAddress(h, cname.as_mut_ptr());
    if p.is_null() {
        return Err(LabError::Emulator(format!("unicorn.dll 缺少导出符号 {name}")));
    }
    Ok(p)
}

fn dll_candidate_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(p) = std::env::var("ASSEMBLYLAB_UNICORN_DLL") {
        v.push(PathBuf::from(p));
    }
    // 编译期 crate 目录 → 仓库根 vendor/
    v.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/unicorn.dll"));
    // exe 同级 vendor/(打包分发形态)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            v.push(dir.join("vendor/unicorn.dll"));
        }
    }
    v
}

fn load_api() -> Result<&'static UcApiTable, LabError> {
    static API: OnceLock<Result<UcApiTable, LabError>> = OnceLock::new();
    let result = API.get_or_init(|| unsafe {
        let mut last_err = String::new();
        for path in dll_candidate_paths() {
            let Some(s) = path.to_str() else { continue };
            let mut wide = format!("{s}\0");
            let h = LoadLibraryA(wide.as_mut_ptr());
            if h.is_null() {
                last_err = format!("LoadLibraryA 失败: {}", path.display());
                continue;
            }
            let t = UcApiTable {
                uc_open: std::mem::transmute(get_sym(h, "uc_open")?),
                uc_close: std::mem::transmute(get_sym(h, "uc_close")?),
                uc_mem_map: std::mem::transmute(get_sym(h, "uc_mem_map")?),
                uc_mem_write: std::mem::transmute(get_sym(h, "uc_mem_write")?),
                uc_mem_read: std::mem::transmute(get_sym(h, "uc_mem_read")?),
                uc_reg_read: std::mem::transmute(get_sym(h, "uc_reg_read")?),
                uc_reg_write: std::mem::transmute(get_sym(h, "uc_reg_write")?),
                uc_emu_start: std::mem::transmute(get_sym(h, "uc_emu_start")?),
                uc_emu_stop: std::mem::transmute(get_sym(h, "uc_emu_stop")?),
                uc_hook_add: std::mem::transmute(get_sym(h, "uc_hook_add")?),
                uc_hook_add_insn: std::mem::transmute(get_sym(h, "uc_hook_add")?),
                uc_errno: std::mem::transmute(get_sym(h, "uc_errno")?),
                uc_version: std::mem::transmute(get_sym(h, "uc_version")?),
                uc_query: std::mem::transmute(get_sym(h, "uc_query")?),
            };
            let mut major: u32 = 0;
            let mut minor: u32 = 0;
            (t.uc_version)(&mut major, &mut minor);
            if major != 2 {
                return Err(LabError::Emulator(format!(
                    "unicorn 版本不符合预期: {}.{}(需要 2.x)@ {}",
                    major, minor,
                    path.display()
                )));
            }
            return Ok(t);
        }
        Err(LabError::Emulator(last_err))
    });
    result.as_ref().map_err(|e| e.clone())
}

// ---------------------------------------------------------------------------
// Hook 状态(回调与宿主共享,同线程访问)
// ---------------------------------------------------------------------------

struct HookState {
    api: UcApiTable,
    breakpoints: HashSet<u64>,
    /// 断点仅在 run 模式生效;单步(step)无视断点(调试器 step-over 语义)
    bp_enabled: bool,
    exit_sentinel: u64,
    budget_remaining: u64,
    stop_requested: bool,
    exit: Option<StopReason>,
    mem_writes: Vec<MemWrite>,
    output: Vec<String>,
    stdin_buf: Vec<u8>,
    last_addr: u64,
    last_size: u32,
    fault: Option<(String, u64)>,
    /// syscall 桩写入的 RIP(用于判断 unicorn 是否自动推进)
    syscall_addr: Option<u64>,
    heap_end: u64,
    /// SMC 停滞保护:连续命中同一地址的次数。Unicorn 2.1.4 对"写入当前翻译块
    /// (<512B 直线流)"的指令会回滚到该指令且不执行(count 也不前进),run 模式
    /// 将无限重复;连续 N 次同址即强制停止。
    stall_addr: u64,
    stall_count: u32,
}

impl HookState {
    fn new(api: UcApiTable, exit_sentinel: u64, heap_end: u64) -> Self {
        Self {
            api,
            breakpoints: HashSet::new(),
            bp_enabled: false,
            exit_sentinel,
            budget_remaining: 0,
            stop_requested: false,
            exit: None,
            mem_writes: Vec::new(),
            output: Vec::new(),
            stdin_buf: Vec::new(),
            last_addr: 0,
            last_size: 0,
            fault: None,
            syscall_addr: None,
            heap_end,
            stall_addr: u64::MAX,
            stall_count: 0,
        }
    }
}

extern "system" fn on_code(uc: *mut c_void, addr: u64, size: u32, ud: *mut c_void) {
    let hs = unsafe { &mut *(ud as *mut HookState) };
    hs.last_addr = addr;
    hs.last_size = size;
    // 注意:命中停止条件时必须调用 uc_emu_stop,否则当前指令仍会执行完,
    // 且 stop 标志会“迟到”(曾导致断点后多执行一条指令的测试失败)。
    let stop = |hs: &mut HookState, reason: StopReason| {
        hs.exit = Some(reason);
        hs.stop_requested = true;
        unsafe { (hs.api.uc_emu_stop)(uc) };
    };
    if addr == hs.exit_sentinel {
        stop(hs, StopReason::ProgramExit { code: None });
        return;
    }
    // SMC 停滞保护:同一条指令被连续命中(写当前翻译块的回滚循环)→ 强制停止
    if addr == hs.stall_addr {
        hs.stall_count += 1;
        if hs.stall_count >= 128 {
            stop(
                hs,
                StopReason::Fault {
                    message: "同一指令被反复回滚执行(写入落在指令所在翻译块内,Unicorn 无法推进)。\
                              请把可写数据与代码保持距离(数据标签前加 align 512)"
                        .into(),
                },
            );
            return;
        }
    } else {
        hs.stall_addr = addr;
        hs.stall_count = 1;
    }
    if hs.bp_enabled && hs.breakpoints.contains(&addr) {
        stop(hs, StopReason::Breakpoint { address: addr });
        return;
    }
    if hs.budget_remaining == 0 {
        stop(hs, StopReason::InstructionLimit { executed: u64::MAX });
        return;
    }
    hs.budget_remaining -= 1;
}

extern "system" fn on_mem_write(_uc: *mut c_void, mem_type: u32, addr: u64, size: u32, value: i64, ud: *mut c_void) -> bool {
    let hs = unsafe { &mut *(ud as *mut HookState) };
    if mem_type == uc::UC_MEM_WRITE && hs.mem_writes.len() < 64 * 1024 {
        let mut le = [0u8; 8];
        le[..(size as usize).min(8)]
            .copy_from_slice(&value.to_le_bytes()[..(size as usize).min(8)]);
        hs.mem_writes.push(MemWrite { addr, size, value_le: le });
    }
    true
}

extern "system" fn on_fault(_uc: *mut c_void, mem_type: u32, addr: u64, _size: i32, _value: i64, ud: *mut c_void) -> bool {
    let hs = unsafe { &mut *(ud as *mut HookState) };
    let kind = match mem_type {
        uc::UC_MEM_READ | uc::UC_MEM_READ_UNMAPPED | uc::UC_MEM_READ_PROT => "读",
        uc::UC_MEM_WRITE | uc::UC_MEM_WRITE_UNMAPPED | uc::UC_MEM_WRITE_PROT => "写",
        _ => "取指",
    };
    if hs.fault.is_none() {
        hs.fault = Some((kind.to_string(), addr));
    }
    false // 不放行 → 引擎停止并返回相应错误
}

/// syscall 教学桩(EXPERIMENT_ENGINE.md §2.4)。
/// 注意:注册为 UC_HOOK_INSN,unicorn 不会执行该指令;RIP 推进由本桩负责。
extern "system" fn on_syscall(uc: *mut c_void, ud: *mut c_void) {
    let hs = unsafe { &mut *(ud as *mut HookState) };
    let api = match load_api() {
        Ok(a) => a,
        Err(_) => return,
    };
    let rip = hs.syscall_addr.unwrap_or_else(|| unsafe { reg_read_u64(api, uc, uc::UC_X86_REG_RIP) });
    let rax = unsafe { reg_read_u64(api, uc, uc::UC_X86_REG_RAX) };
    let rdi = unsafe { reg_read_u64(api, uc, uc::UC_X86_REG_RDI) };
    let rsi = unsafe { reg_read_u64(api, uc, uc::UC_X86_REG_RSI) };
    let rdx = unsafe { reg_read_u64(api, uc, uc::UC_X86_REG_RDX) };

    // 实测约定:INSN hook 触发时 RIP 已指向下一条指令,unicorn 返回后自动推进,
    // 桩内不得再手动 +2(否则跳过一条指令——见集成测试 syscall_write_then_continue)。
    let _ = rip;

    let emu_stop = |hs: &mut HookState, reason: StopReason| {
        hs.exit = Some(reason);
        hs.stop_requested = true;
        unsafe { (api.uc_emu_stop)(uc) };
    };

    match rax {
        1 => {
            // write(fd, buf, count)
            let len = rdx.min(4096) as u32;
            let mut buf = vec![0u8; len as usize];
            let r = unsafe { (api.uc_mem_read)(uc, rsi, buf.as_mut_ptr(), len as u64) };
            if r == uc::UC_ERR_OK {
                let text = String::from_utf8_lossy(&buf).to_string();
                let fd_note = if rdi == 1 { "" } else if rdi == 2 { "[stderr] " } else { "" };
                hs.output.push(format!("{fd_note}{text}"));
                unsafe { reg_write_u64(api, uc, uc::UC_X86_REG_RAX, len as u64) };
            } else {
                emu_stop(hs, StopReason::Fault { message: format!("write: 读缓冲失败 @ {rsi:#x}") });
            }
        }
        60 => {
            emu_stop(hs, StopReason::ProgramExit { code: Some(rdi) });
        }
        0 => {
            // read(fd, buf, count) — 会话标准输入缓冲
            let len = rdx.min(4096) as usize;
            let take = hs.stdin_buf.len().min(len);
            let chunk: Vec<u8> = hs.stdin_buf.drain(..take).collect();
            if !chunk.is_empty() {
                unsafe { (api.uc_mem_write)(uc, rsi, chunk.as_ptr(), chunk.len() as u64) };
            }
            unsafe { reg_write_u64(api, uc, uc::UC_X86_REG_RAX, chunk.len() as u64) };
        }
        12 => {
            // brk(教学桩):返回当前堆尾
            unsafe { reg_write_u64(api, uc, uc::UC_X86_REG_RAX, hs.heap_end) };
        }
        other => {
            emu_stop(hs, StopReason::Fault { message: format!("模拟器未实现的 syscall 号 {other}(教学桩仅支持 read/write/brk/exit)") });
        }
    }
}

/// ARM64 SVC 系统调用教学桩(UC_HOOK_INTR):write(64)/exit(93)。
/// INTR 触发时 PC 已越过 SVC 指令;放行即继续,emu_stop 即终止。
extern "system" fn on_intr(uc: *mut c_void, _intno: u32, ud: *mut c_void) {
    let hs = unsafe { &mut *(ud as *mut HookState) };
    let api = match load_api() {
        Ok(a) => a,
        Err(_) => return,
    };
    let x8 = unsafe { reg_read_u64(api, uc, uc::UC_ARM64_REG_X8) };
    let x0 = unsafe { reg_read_u64(api, uc, uc::UC_ARM64_REG_X0) };
    let x1 = unsafe { reg_read_u64(api, uc, uc::UC_ARM64_REG_X1) };
    let x2 = unsafe { reg_read_u64(api, uc, uc::UC_ARM64_REG_X2) };

    let emu_stop = |hs: &mut HookState, reason: StopReason| {
        hs.exit = Some(reason);
        hs.stop_requested = true;
        unsafe { (api.uc_emu_stop)(uc) };
    };

    match x8 {
        93 => {
            // exit(code)
            emu_stop(hs, StopReason::ProgramExit { code: Some(x0) });
        }
        64 => {
            // write(fd, buf, count) —— 与 x86 桩同语义,输出进 hs.output
            let len = x2.min(4096) as u32;
            let mut buf = vec![0u8; len as usize];
            let r = unsafe { (api.uc_mem_read)(uc, x1, buf.as_mut_ptr(), len as u64) };
            if r == uc::UC_ERR_OK {
                let fd_note = if x0 == 1 { "" } else if x0 == 2 { "[stderr] " } else { "" };
                hs.output.push(format!("{fd_note}{}", String::from_utf8_lossy(&buf)));
                unsafe { reg_write_u64(api, uc, uc::UC_ARM64_REG_X0, len as u64) };
            } else {
                emu_stop(hs, StopReason::Fault { message: format!("write: 读缓冲失败 @ {x1:#x}") });
            }
        }
        other => {
            emu_stop(
                hs,
                StopReason::Fault { message: format!("模拟器未实现的 ARM64 syscall 号 {other}(教学桩仅支持 write/exit)") },
            );
        }
    }
}

// ---------------------------------------------------------------------------
// UnicornMachine
// ---------------------------------------------------------------------------

/// 目标架构(决定 uc_open 常量、寄存器表、PC 寄存器、syscall 桩形式)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UcArch {
    X86,
    Arm64,
}

pub struct UnicornMachine {
    api: &'static UcApiTable,
    uc: *mut c_void,
    hs: *mut HookState,
    /// (名称, unicorn 寄存器 id) — 顺序 = ISA panel_registers
    panel: Vec<(String, u32)>,
    exit_sentinel: u64,
    heap_end: u64,
    arch: UcArch,
}

unsafe impl Send for UnicornMachine {}
// Sync 不实现:所有访问必须经持有方的 Mutex 串行化。

impl UnicornMachine {
    pub fn new(emulator_arch: &str, emulator_mode: &str) -> Result<Self, LabError> {
        let arch = match (emulator_arch, emulator_mode) {
            ("unicorn:x86", "unicorn:x86-64") => UcArch::X86,
            ("unicorn:arm64", "unicorn:arm") => UcArch::Arm64,
            _ => {
                return Err(LabError::BadRequest(format!(
                    "不支持的模拟器映射: {emulator_arch} / {emulator_mode}(支持 unicorn:x86|x86-64 与 unicorn:arm64|arm)"
                )))
            }
        };
        let api = load_api()?;
        let uc = unsafe { Self::open_instance(api, arch)? };
        let hs = Box::into_raw(Box::new(HookState::new(*api, 0, 0)));
        Ok(Self { api, uc, hs, panel: Vec::new(), exit_sentinel: 0, heap_end: 0, arch })
    }

    unsafe fn open_instance(api: &UcApiTable, arch: UcArch) -> Result<*mut c_void, LabError> {
        let mut uc: *mut c_void = std::ptr::null_mut();
        let (a, m) = match arch {
            UcArch::X86 => (uc::UC_ARCH_X86, uc::UC_MODE_64),
            UcArch::Arm64 => (uc::UC_ARCH_ARM64, uc::UC_MODE_ARM),
        };
        let err = (api.uc_open)(a, m, &mut uc);
        if err != uc::UC_ERR_OK {
            return Err(LabError::Emulator(format!("uc_open 失败: 错误码 {err}")));
        }
        Ok(uc)
    }

    /// PC 寄存器 id(架构相关)
    fn pc_id(&self) -> u32 {
        match self.arch {
            UcArch::X86 => uc::UC_X86_REG_RIP,
            UcArch::Arm64 => uc::UC_ARM64_REG_PC,
        }
    }

    unsafe fn reg_id(&self, name: &str) -> Result<u32, LabError> {
        let hit = match self.arch {
            UcArch::X86 => uc::x86_reg_by_name(name),
            UcArch::Arm64 => uc::arm64_reg_by_name(name),
        };
        hit.ok_or_else(|| LabError::BadRequest(format!(
            "未知寄存器 {name}(当前架构 {} 支持其 ISA 数据包内声明的寄存器)",
            if self.arch == UcArch::X86 { "x86-64" } else { "arm64" }
        )))
    }

    unsafe fn reg_read_u64(&self, id: u32) -> u64 {
        reg_read_u64(self.api, self.uc, id)
    }

    unsafe fn reg_write_u64(&self, id: u32, v: u64) {
        reg_write_u64(self.api, self.uc, id, v)
    }

    fn check(&self, err: u32, what: &str) -> Result<(), LabError> {
        if err == uc::UC_ERR_OK {
            return Ok(());
        }
        Err(match err {
            uc::UC_ERR_READ_UNMAPPED => LabError::UnmappedAccess { kind: "读".into(), addr: 0 },
            uc::UC_ERR_WRITE_UNMAPPED => LabError::UnmappedAccess { kind: "写".into(), addr: 0 },
            uc::UC_ERR_FETCH_UNMAPPED => LabError::UnmappedAccess { kind: "取指".into(), addr: 0 },
            other => LabError::Emulator(format!("{what}: 错误码 {other}")),
        })
    }
}

impl Drop for UnicornMachine {
    fn drop(&mut self) {
        if !self.uc.is_null() {
            unsafe { (self.api.uc_close)(self.uc) };
            self.uc = std::ptr::null_mut();
        }
        if !self.hs.is_null() {
            drop(unsafe { Box::from_raw(self.hs) });
            self.hs = std::ptr::null_mut();
        }
    }
}

unsafe fn reg_read_u64(api: &UcApiTable, uc: *mut c_void, id: u32) -> u64 {
    let mut v: u64 = 0;
    (api.uc_reg_read)(uc, id, &mut v as *mut u64 as *mut c_void);
    v
}

unsafe fn reg_write_u64(api: &UcApiTable, uc: *mut c_void, id: u32, v: u64) {
    (api.uc_reg_write)(uc, id, &v as *const u64 as *const c_void);
}

impl EmulatorAdapter for UnicornMachine {
    fn load(&mut self, spec: &LoadSpec) -> Result<(), LabError> {
        unsafe {
            // 重建实例(简单可靠:卸载一切旧状态)
            if !self.uc.is_null() {
                (self.api.uc_close)(self.uc);
            }
            self.uc = Self::open_instance(self.api, self.arch)?;
            let hs = &mut *self.hs;
            *hs = HookState::new(*self.api, spec.exit_sentinel, spec.heap_end);
            self.exit_sentinel = spec.exit_sentinel;
            self.heap_end = spec.heap_end;

            // 映射教学沙箱区域
            for (base, size) in &spec.regions {
                self.check(
                    (self.api.uc_mem_map)(self.uc, *base, *size, uc::UC_PROT_ALL),
                    "uc_mem_map",
                )?;
            }

            // 注册钩子(全域: begin > end)
            let ud = self.hs as *mut c_void;
            let mut hh: u64 = 0;
            self.check(
                (self.api.uc_hook_add)(self.uc, &mut hh, uc::UC_HOOK_CODE, on_code as *mut c_void, ud, 1, 0),
                "hook code",
            )?;
            let mut hh2: u64 = 0;
            self.check(
                (self.api.uc_hook_add)(self.uc, &mut hh2, uc::UC_HOOK_MEM_WRITE, on_mem_write as *mut c_void, ud, 1, 0),
                "hook mem",
            )?;
            let faults = uc::UC_HOOK_MEM_READ_UNMAPPED | uc::UC_HOOK_MEM_WRITE_UNMAPPED | uc::UC_HOOK_MEM_FETCH_UNMAPPED;
            let mut hh3: u64 = 0;
            self.check(
                (self.api.uc_hook_add)(self.uc, &mut hh3, faults, on_fault as *mut c_void, ud, 1, 0),
                "hook fault",
            )?;
            match self.arch {
                UcArch::X86 => {
                    let mut hh4: u64 = 0;
                    self.check(
                        (self.api.uc_hook_add_insn)(
                            self.uc, &mut hh4, uc::UC_HOOK_INSN, on_syscall as *mut c_void, ud, 1, 0,
                            uc::UC_X86_INS_SYSCALL,
                        ),
                        "hook syscall",
                    )?;
                }
                UcArch::Arm64 => {
                    let mut hh4: u64 = 0;
                    self.check(
                        (self.api.uc_hook_add)(
                            self.uc, &mut hh4, uc::UC_HOOK_INTR, on_intr as *mut c_void, ud, 1, 0,
                        ),
                        "hook intr(arm64 svc)",
                    )?;
                }
            }

            // 写入程序
            self.check(
                (self.api.uc_mem_write)(self.uc, spec.base, spec.bytes.as_ptr(), spec.bytes.len() as u64),
                "uc_mem_write(代码)",
            )?;

            // 初始寄存器
            for (name, value) in &spec.init_regs {
                let id = self.reg_id(name)?;
                self.reg_write_u64(id, *value);
            }
            let pc_id = self.pc_id();
            self.reg_write_u64(pc_id, spec.entry);
            if self.arch == UcArch::X86 {
                // x86:[RSP] = 终止桩(主程序 ret 即退出)
                let rsp = spec
                    .init_regs
                    .iter()
                    .find(|(n, _)| n == "RSP")
                    .map(|(_, v)| *v)
                    .unwrap_or(0);
                let sentinel_bytes = spec.exit_sentinel.to_le_bytes();
                self.check(
                    (self.api.uc_mem_write)(self.uc, rsp, sentinel_bytes.as_ptr(), 8),
                    "写入终止桩",
                )?;
            }
            // arm64:X30(LR)由引擎按 ISA link_register 预置为终止桩,ret 即退出
            // 面板寄存器名 → id
            let ids = spec
                .panel_registers
                .iter()
                .map(|n| self.reg_id(n).map(|id| (n.clone(), id)))
                .collect::<Result<Vec<_>, LabError>>()?;
            self.panel = ids;
        }
        Ok(())
    }

    fn step(&mut self, count: u32) -> Result<Vec<StepEvents>, LabError> {
        let mut out = Vec::with_capacity(count as usize);
        unsafe {
            let hs = &mut *self.hs;
            for _ in 0..count {
                if hs.stop_requested {
                    break;
                }
                let rip = self.reg_read_u64(self.pc_id());
                if rip == self.exit_sentinel {
                    // 已到终止哨兵:产出带 ProgramExit 的事件(否则本步为空报告,
                    // 调用方无法得知程序已结束——单步到退出的存量坑,两架构同)
                    hs.stop_requested = true;
                    let snap = self.snapshot_panel();
                    out.push(StepEvents {
                        address: rip,
                        size: 0,
                        before: snap.clone(),
                        after: snap,
                        mem_writes: Vec::new(),
                        stop: Some(StopReason::ProgramExit { code: None }),
                    });
                    break;
                }
                let before = self.snapshot_panel();
                hs.mem_writes.clear();
                hs.exit = None;
                hs.fault = None;
                hs.budget_remaining = u64::MAX;
                hs.bp_enabled = false; // 单步无视断点
                let err = (self.api.uc_emu_start)(self.uc, rip, self.exit_sentinel, 0, 1);
                // SMC 停滞检测:RIP 未前进且无停止请求。正常指令(含 jmp $ 自环)
                // 也可能不前进,故累计 stall_count 超阈值才报错,给死循环留余量。
                if err == uc::UC_ERR_OK && !hs.stop_requested {
                    let rip_after = self.reg_read_u64(self.pc_id());
                    if rip_after == rip && hs.stall_count >= 32 {
                        hs.stop_requested = true;
                        hs.exit = Some(StopReason::Fault {
                            message: format!(
                                "RIP 在 {rip:#x} 反复停滞:该指令的写入落在自身所在翻译块内 \
                                 (Unicorn 2.1.4 边界),或程序在此死循环。\
                                 可写数据请与代码保持距离(数据标签前加 align 512)。"
                            ),
                        });
                    }
                }
                let after = self.snapshot_panel();
                let writes = std::mem::take(&mut hs.mem_writes);
                let stop = if let Some(e) = hs.exit.take() {
                    Some(e)
                } else if let Some((kind, addr)) = hs.fault.clone() {
                    hs.stop_requested = true;
                    Some(StopReason::Fault { message: format!("访问了未映射的内存({kind} @ {addr:#x})") })
                } else if err != uc::UC_ERR_OK {
                    hs.stop_requested = true;
                    Some(StopReason::Fault { message: format!("执行错误(错误码 {err})") })
                } else {
                    None
                };
                out.push(StepEvents {
                    address: hs.last_addr,
                    size: hs.last_size,
                    before,
                    after,
                    mem_writes: writes,
                    stop,
                });
                if hs.stop_requested {
                    break;
                }
            }
        }
        Ok(out)
    }

    fn run(&mut self, breakpoints: &HashSet<u64>, ctrl: RunControl) -> Result<RunOutcome, LabError> {
        unsafe {
            let hs = &mut *self.hs;
            if hs.stop_requested {
                return Ok(RunOutcome {
                    executed: 0,
                    output: Vec::new(),
                    final_snapshot: self.snapshot_panel(),
                    stop: StopReason::StepsDone,
                });
            }
            hs.breakpoints.clone_from(breakpoints);
            hs.bp_enabled = true;
            hs.budget_remaining = ctrl.max_instructions;
            hs.exit = None;
            hs.fault = None;
            hs.stop_requested = false;
            let executed_start = ctrl.max_instructions - hs.budget_remaining;
            let rip = self.reg_read_u64(self.pc_id());
            let err = (self.api.uc_emu_start)(self.uc, rip, self.exit_sentinel, ctrl.timeout_us, 0);
            let executed = ctrl.max_instructions - hs.budget_remaining;
            let mut timed_out: u64 = 0;
            (self.api.uc_query)(self.uc, uc::UC_QUERY_TIMEOUT, &mut timed_out);

            let stop = if let Some(e) = hs.exit.take() {
                e
            } else if let Some((kind, addr)) = hs.fault.clone() {
                StopReason::Fault { message: format!("访问了未映射的内存({kind} @ {addr:#x})") }
            } else if timed_out == 1 {
                StopReason::Timeout
            } else if err != uc::UC_ERR_OK {
                StopReason::Fault { message: format!("执行错误(错误码 {err})") }
            } else if executed >= ctrl.max_instructions {
                StopReason::InstructionLimit { executed }
            } else {
                StopReason::ProgramExit { code: None }
            };
            let _ = executed_start;
            Ok(RunOutcome {
                executed,
                output: std::mem::take(&mut hs.output),
                final_snapshot: self.snapshot_panel(),
                stop,
            })
        }
    }

    fn read_regs(&mut self, names: &[String]) -> Result<RegSnapshot, LabError> {
        let mut out = Vec::with_capacity(names.len());
        unsafe {
            for name in names {
                let id = self.reg_id(name)?;
                out.push((name.clone(), self.reg_read_u64(id)));
            }
        }
        Ok(out)
    }

    fn write_reg(&mut self, name: &str, value: RegValue) -> Result<(), LabError> {
        unsafe {
            let id = self.reg_id(name)?;
            self.reg_write_u64(id, value);
        }
        Ok(())
    }

    fn read_memory(&mut self, addr: u64, len: u32) -> Result<Vec<u8>, LabError> {
        let mut buf = vec![0u8; len as usize];
        unsafe {
            self.check(
                (self.api.uc_mem_read)(self.uc, addr, buf.as_mut_ptr(), len as u64),
                "uc_mem_read",
            )?;
        }
        Ok(buf)
    }

    fn write_memory(&mut self, addr: u64, bytes: &[u8]) -> Result<(), LabError> {
        unsafe {
            self.check(
                (self.api.uc_mem_write)(self.uc, addr, bytes.as_ptr(), bytes.len() as u64),
                "uc_mem_write",
            )?;
        }
        Ok(())
    }

    fn clear_stop(&mut self) {
        unsafe {
            let hs = &mut *self.hs;
            hs.stop_requested = false;
            hs.exit = None;
            hs.fault = None;
        }
    }
}

impl UnicornMachine {
    unsafe fn snapshot_panel(&self) -> RegSnapshot {
        self.panel
            .iter()
            .map(|(name, id)| (name.clone(), reg_read_u64(self.api, self.uc, *id)))
            .collect()
    }
}
