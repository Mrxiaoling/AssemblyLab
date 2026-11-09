//! lab-core — Assembly Lab 领域类型与错误定义。
//!
//! 本 crate 不依赖任何具体 ISA / 模拟器,是引擎与适配器共同的语言。
//! 设计文档:docs/ARCHITECTURE.md §3,docs/ISA_DESIGN.md。

pub mod error;
pub mod isa;

pub use error::LabError;
pub use isa::{AbiDefinition, FlagDef, IsaDefinition, MemoryRegion, RegisterDef, SubRegister};

/// 寄存器值的通用表示:名称 → 64 位无符号值。
/// 向量寄存器(P5)引入后扩展为 bytes 表示;P1 只有 GPR/FLAGS。
pub type RegValue = u64;

/// 寄存器快照:有序(ISA 定义的面板顺序)。
pub type RegSnapshot = Vec<(String, RegValue)>;

/// 一次内存写入事件(引擎真实记录)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MemWrite {
    pub addr: u64,
    pub size: u32,
    /// 小端原始字节(最多 8 字节;向量写 P5 扩展)
    pub value_le: [u8; 8],
}

/// 执行停止原因。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopReason {
    /// 程序正常退出(syscall exit 或返回到终止桩)
    ProgramExit { code: Option<u64> },
    /// 命中断点
    Breakpoint { address: u64 },
    /// 到达指令数上限
    InstructionLimit { executed: u64 },
    /// 墙钟超时
    Timeout,
    /// 执行错误(如未映射访问)
    Fault { message: String },
    /// 单步次数已用尽(正常)
    StepsDone,
}

/// 一步的原始事件(引擎层事实,不做任何解释)。
/// 解释层(EXPERIMENT_ENGINE.md §3)据此计算 StepReport。
#[derive(Debug, Clone)]
pub struct StepEvents {
    /// 本条指令地址与长度
    pub address: u64,
    pub size: u32,
    pub before: RegSnapshot,
    pub after: RegSnapshot,
    pub mem_writes: Vec<MemWrite>,
    pub stop: Option<StopReason>,
}

/// 运行(非逐步)模式的结果:只记录事实汇总,不逐步快照。
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub executed: u64,
    pub output: Vec<String>,
    pub final_snapshot: RegSnapshot,
    pub stop: StopReason,
}

/// 运行控制上限(SECURITY_DESIGN §2.4)。
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct RunControl {
    /// 单次 run 最大指令数
    pub max_instructions: u64,
    /// 单次 run 墙钟超时(微秒)
    pub timeout_us: u64,
}

impl Default for RunControl {
    fn default() -> Self {
        Self { max_instructions: 1_000_000, timeout_us: 5_000_000 }
    }
}

/// 装载规格:Session 由 ISA 定义 + ProgramImage 组装,适配器不接触 ISA 结构。
#[derive(Debug, Clone)]
pub struct LoadSpec {
    pub base: u64,
    pub entry: u64,
    pub bytes: Vec<u8>,
    /// 全部内存区域 (base, size),适配器按此映射
    pub regions: Vec<(u64, u64)>,
    /// 单步快照寄存器名(ISA panel 顺序)
    pub panel_registers: Vec<String>,
    /// 初始寄存器(RSP/RFLAGS 等)
    pub init_regs: Vec<(String, RegValue)>,
    /// 终止桩地址(位于已映射区域,ret 到此 = 程序结束)
    pub exit_sentinel: u64,
    /// 堆尾(brk 桩返回值)
    pub heap_end: u64,
}

/// 模式 A 引擎抽象(ISA 中立)。实现:lab-adapters::unicorn。
/// 线程约定:实现不是 Send 的(持有模拟器原生句柄),由 Session 用
/// Mutex 保证同一时刻只有一个线程访问(ARCHITECTURE D-AR-1)。
pub trait EmulatorAdapter {
    /// 装载程序(机器码)并初始化寄存器/栈。
    fn load(&mut self, spec: &LoadSpec) -> Result<(), LabError>;

    /// 执行 n 条指令,返回每步事件(n <= 单请求上限,由 Session 校验)。
    fn step(&mut self, count: u32) -> Result<Vec<StepEvents>, LabError>;

    /// 连续运行直至断点/退出/上限。
    fn run(&mut self, breakpoints: &std::collections::HashSet<u64>, ctrl: RunControl)
        -> Result<RunOutcome, LabError>;

    /// 读指定寄存器
    fn read_regs(&mut self, names: &[String]) -> Result<RegSnapshot, LabError>;
    /// 写寄存器(教学:修改状态)
    fn write_reg(&mut self, name: &str, value: RegValue) -> Result<(), LabError>;
    /// 读内存
    fn read_memory(&mut self, addr: u64, len: u32) -> Result<Vec<u8>, LabError>;
    /// 写内存
    fn write_memory(&mut self, addr: u64, bytes: &[u8]) -> Result<(), LabError>;
    /// 清除停止状态(断点/退出后显式继续前调用)
    fn clear_stop(&mut self);
}

/// 汇编产物:机器码 + 地址/行号/符号映射。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ProgramImage {
    /// 扁平二进制(-f bin)
    pub bytes: Vec<u8>,
    /// 装载基地址(源码 org 或 ISA 默认)
    pub base: u64,
    /// 入口地址(_start/main 标签或 base)
    pub entry: u64,
    /// 源码行 → (地址, 长度)。同一行可有多条(逗号分隔指令)。
    pub line_map: Vec<LineMapping>,
    /// 标签 → 地址
    pub symbols: Vec<(String, u64)>,
    /// 使用的源码行数(自动 prepend org 时 > 原始行数)
    pub source_lines: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LineMapping {
    pub line: usize,
    pub addr: u64,
    pub size: u32,
}
