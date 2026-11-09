//! lab-adapters — 引擎与工具适配器。
//! - unicorn: 模式 A 指令模拟器(预编译 DLL 运行时加载;x86-64 / ARM64)
//! - nasm:    x86 汇编器外部进程封装
//! - gnu_as:  ARM64 汇编器(WSL aarch64-linux-gnu-as + objcopy)
//! - iced:    x86 反汇编(iced-x86)
//! - arm64dis: ARM64 反汇编(yaxpeax-arm,纯 Rust)
//! 设计文档:docs/EXPERIMENT_ENGINE.md。

pub mod arm64dis;
pub mod compiler;
pub mod consts;
pub mod gdb;
pub mod gnu_as;
pub mod iced;
pub mod nasm;
pub mod unicorn;

use lab_core::LabError;

/// 按目标反汇编(ISA 分派:x86-64 → iced;arm64 → yaxpeax)。
pub fn disasm_for(isa_id: &str, code: &[u8], addr: u64) -> Result<Vec<iced::DisasmLine>, LabError> {
    match isa_id {
        "arm64" => arm64dis::disassemble(code, addr),
        _ => iced::disassemble(code, addr),
    }
}

/// 单条反汇编(同上分派)。
pub fn disasm_one_for(isa_id: &str, code: &[u8], addr: u64) -> Result<Option<iced::DisasmLine>, LabError> {
    match isa_id {
        "arm64" => arm64dis::disassemble_one(code, addr),
        _ => iced::disassemble_one(code, addr),
    }
}
