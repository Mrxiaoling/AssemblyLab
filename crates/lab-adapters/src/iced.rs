//! iced-x86 反汇编适配:指令文本(NASM 语法)、长度、助记符。
//! P1 只需要展示与解释所需的最小信息;flags 读写元数据后续按需增加。

use iced_x86::{Decoder, Formatter, NasmFormatter};
use lab_core::LabError;

pub struct DisasmLine {
    pub address: u64,
    pub size: u32,
    pub text: String,
    pub mnemonic: String,
    /// 该指令的机器码字节(hex)
    pub bytes_hex: String,
}

/// 反汇编一段机器码(64 位)。
pub fn disassemble(code: &[u8], base: u64) -> Result<Vec<DisasmLine>, LabError> {
    let mut decoder = Decoder::with_ip(64, code, base, 0);
    let mut out = Vec::new();
    let mut formatter = NasmFormatter::new();
    let mut output = String::new();
    while decoder.can_decode() {
        let instr = decoder.decode();
        output.clear();
        formatter.format(&instr, &mut output);
        let start = (instr.ip() - base) as usize;
        let end = (start + instr.len()).min(code.len());
        out.push(DisasmLine {
            address: instr.ip(),
            size: instr.len() as u32,
            text: output.clone(),
            mnemonic: format!("{:?}", instr.mnemonic()).to_ascii_lowercase(),
            bytes_hex: hex::encode_upper(&code[start.min(code.len())..end]),
        });
    }
    Ok(out)
}

/// 反汇编单条指令(用于 StepReport)。
pub fn disassemble_one(code: &[u8], addr: u64) -> Result<Option<DisasmLine>, LabError> {
    if code.is_empty() {
        return Ok(None);
    }
    let mut decoder = Decoder::with_ip(64, code, addr, 0);
    let instr = decoder.decode();
    let mut formatter = NasmFormatter::new();
    let mut output = String::new();
    formatter.format(&instr, &mut output);
    Ok(Some(DisasmLine {
        address: instr.ip(),
        size: instr.len() as u32,
        text: output.clone(),
        mnemonic: format!("{:?}", instr.mnemonic()).to_ascii_lowercase(),
        bytes_hex: hex::encode_upper(&code[..instr.len().min(code.len())]),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disasm_basic() {
        let code = [0xB8, 0x0A, 0x00, 0x00, 0x00, 0x48, 0x01, 0xD8];
        let lines = disassemble(&code, 0x400000).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].mnemonic, "mov");
        assert!(lines[0].text.contains("eax,0Ah"), "{}", lines[0].text);
        assert_eq!(lines[1].size, 3);
    }
}
