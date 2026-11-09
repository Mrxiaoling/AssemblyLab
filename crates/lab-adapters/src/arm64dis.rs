//! ARM64(AArch64)反汇编 —— yaxpeax-arm(纯 Rust,无 C 依赖)。
//! 输出与 iced::DisasmLine 同形,引擎/前端零改动复用。
//! 注:分支目标以相对偏移显示(yaxpeax 无 pc 上下文的 plain Display)。

use lab_core::LabError;
use yaxpeax_arch::Decoder;
use yaxpeax_arm::armv8::a64::{InstDecoder, Instruction};

pub use crate::iced::DisasmLine;

/// 反汇编整段(遇无法解码字节输出 .word 占位,继续推进——ARM64 定长 4 字节)。
pub fn disassemble(code: &[u8], base: u64) -> Result<Vec<DisasmLine>, LabError> {
    let decoder = InstDecoder::default();
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + 4 <= code.len() {
        let word = u32::from_le_bytes([code[off], code[off + 1], code[off + 2], code[off + 3]]);
        let mut reader = yaxpeax_arch::U8Reader::new(&code[off..]);
        let text = match decoder.decode(&mut reader) {
            Ok(inst) => format!("{inst}"),
            Err(_) => format!(".word 0x{word:08x}"),
        };
        let mnemonic = text.split_whitespace().next().unwrap_or("").to_lowercase();
        out.push(DisasmLine {
            address: base + off as u64,
            size: 4,
            text,
            mnemonic,
            bytes_hex: hex::encode_upper(&code[off..off + 4]),
        });
        off += 4;
    }
    Ok(out)
}

/// 解码单条(取起始 4 字节;ARM64 定长)。
pub fn disassemble_one(code: &[u8], addr: u64) -> Result<Option<DisasmLine>, LabError> {
    if code.len() < 4 {
        return Ok(None);
    }
    let word = u32::from_le_bytes([code[0], code[1], code[2], code[3]]);
    let decoder = InstDecoder::default();
    let mut reader = yaxpeax_arch::U8Reader::new(code);
    let text = match decoder.decode(&mut reader) {
        Ok(inst) => format!("{inst}"),
        Err(_) => format!(".word 0x{word:08x}"),
    };
    let mnemonic = text.split_whitespace().next().unwrap_or("").to_lowercase();
    Ok(Some(DisasmLine {
        address: addr,
        size: 4,
        text,
        mnemonic,
        bytes_hex: hex::encode_upper(&code[..4]),
    }))
}

/// 仅取助记符(跳转分类用)。
#[allow(dead_code)]
pub fn mnemonic_of(word: u32) -> String {
    let bytes = word.to_le_bytes();
    let decoder = InstDecoder::default();
    let mut reader = yaxpeax_arch::U8Reader::new(&bytes[..]);
    match decoder.decode(&mut reader) {
        Ok(inst) => format!("{inst}").split_whitespace().next().unwrap_or("").to_lowercase(),
        Err(_) => "word".into(),
    }
}
