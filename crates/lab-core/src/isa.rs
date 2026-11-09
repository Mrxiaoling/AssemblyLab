//! ISA 数据包模型(TOML 数据文件 → 类型)。
//! schema 详见 docs/ISA_DESIGN.md §2。数据文件位于 data/isas/<arch>/。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::error::LabError;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IsaPackage {
    pub isa: IsaMeta,
    #[serde(default)]
    pub registers: Vec<RegisterDef>,
    #[serde(default)]
    pub flags: Vec<FlagDef>,
    #[serde(default)]
    pub abis: Vec<AbiDefinition>,
    #[serde(default)]
    pub instruction_notes: Vec<InstructionNote>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IsaMeta {
    pub id: String,
    pub name: String,
    pub endian: String,
    /// 模拟器映射,如 "unicorn:x86"
    pub emulator_arch: String,
    pub emulator_mode: String,
    pub assembler: String,
    pub assembler_syntax: String,
    pub memory_map: MemoryMap,
    /// 单步面板展示的寄存器顺序
    pub panel_registers: Vec<String>,
    /// 初始栈顶(教学沙箱)
    pub stack_init: String,
    /// 终止桩地址(ret 到此即程序结束)
    pub exit_sentinel: String,
    /// PC 寄存器名(x86=RIP,arm64=PC)
    #[serde(default = "default_pc_reg")]
    pub pc_register: String,
    /// 栈指针寄存器名(x86=RSP,arm64=SP)
    #[serde(default = "default_sp_reg")]
    pub stack_register: String,
    /// 标志寄存器名(x86=RFLAGS,arm64=NZCV)
    #[serde(default = "default_flags_reg")]
    pub flags_register: String,
    /// 标志寄存器初始值(x86=0x2)
    #[serde(default = "default_flags_init")]
    pub flags_init: u64,
    /// 链接寄存器(arm64=X30):初始化为 exit_sentinel,ret 即退出
    #[serde(default)]
    pub link_register: Option<String>,
    #[serde(default)]
    pub syscall_stub: Option<SyscallStubCfg>,
}

fn default_pc_reg() -> String {
    "RIP".into()
}
fn default_sp_reg() -> String {
    "RSP".into()
}
fn default_flags_reg() -> String {
    "RFLAGS".into()
}
fn default_flags_init() -> u64 {
    2
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryMap {
    pub code: MemoryRegion,
    #[serde(default)]
    pub data: Option<MemoryRegion>,
    #[serde(default)]
    pub heap: Option<MemoryRegion>,
    pub stack: MemoryRegion,
}

impl MemoryMap {
    pub fn regions(&self) -> Vec<(&'static str, &MemoryRegion)> {
        let mut v: Vec<(&'static str, &MemoryRegion)> = vec![("code", &self.code), ("stack", &self.stack)];
        if let Some(d) = &self.data {
            v.push(("data", d));
        }
        if let Some(h) = &self.heap {
            v.push(("heap", h));
        }
        v
    }

    /// 地址落在哪个区域(用于内存面板标注)
    pub fn region_of(&self, addr: u64) -> Option<&'static str> {
        for (name, r) in self.regions() {
            let base = parse_int(&r.base).ok()?;
            if addr >= base && addr < base + r.size {
                return Some(name);
            }
        }
        None
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRegion {
    pub base: String,
    #[serde(deserialize_with = "de_int_or_str")]
    pub size: u64,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterDef {
    pub canonical: String,
    pub size: u32,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub special: Option<String>,
    #[serde(default)]
    pub subregisters: Vec<SubRegister>,
    #[serde(default)]
    pub role_notes: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubRegister {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    /// zero_extend_upper | preserve_upper
    pub write_semantics: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlagDef {
    pub name: String,
    pub bit: u32,
    #[serde(default)]
    pub semantics: String,
    #[serde(default = "default_true")]
    pub watch: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AbiDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub integer_arg_regs: Vec<ArgReg>,
    #[serde(default)]
    pub return_reg: Option<String>,
    #[serde(default)]
    pub caller_saved: Vec<String>,
    #[serde(default)]
    pub callee_saved: Vec<String>,
    #[serde(default)]
    pub stack_alignment: u32,
    #[serde(default)]
    pub red_zone: u32,
    #[serde(default)]
    pub shadow_space: u32,
    #[serde(default)]
    pub syscall: Option<SyscallCfg>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArgReg {
    pub order: u8,
    pub reg: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SyscallCfg {
    pub conv: String,
    #[serde(default)]
    pub nr_reg: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SyscallStubCfg {
    /// 桩实现的 syscall 号 → 名称(教学标注)
    pub implemented: HashMap<String, String>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstructionNote {
    pub mnemonic: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub pitfalls: Vec<String>,
    #[serde(default)]
    pub related_cpp: Option<String>,
}

// 解析 "0x400000" / "0x100000" / 纯数字 字符串
pub fn parse_int(s: &str) -> Result<u64, LabError> {
    let s = s.trim().replace('_', "");
    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.as_str()),
    };
    let v = if let Some(hexpart) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
        u64::from_str_radix(hexpart, 16)
    } else {
        body.parse::<u64>()
    }
    .map_err(|e| LabError::BadRequest(format!("无法解析数值 {s:?}: {e}")))?;
    Ok(if neg { v.wrapping_neg() } else { v })
}

fn de_int_or_str<'de, D>(d: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct V;
    impl serde::de::Visitor<'_> for V {
        type Value = u64;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("integer or numeric string")
        }
        fn visit_u64<E>(self, v: u64) -> Result<u64, E> {
            Ok(v)
        }
        fn visit_i64<E>(self, v: i64) -> Result<u64, E> {
            Ok(v as u64)
        }
        fn visit_str<E>(self, s: &str) -> Result<u64, E>
        where
            E: serde::de::Error,
        {
            parse_int(s).map_err(serde::de::Error::custom)
        }
    }
    d.deserialize_any(V)
}

impl IsaPackage {
    pub fn isa(&self) -> IsaDefinition {
        IsaDefinition {
            id: self.isa.id.clone(),
            name: self.isa.name.clone(),
            endian: self.isa.endian.clone(),
            emulator_arch: self.isa.emulator_arch.clone(),
            emulator_mode: self.isa.emulator_mode.clone(),
            assembler: self.isa.assembler.clone(),
            memory_map: self.isa.memory_map.clone(),
            panel_registers: self.isa.panel_registers.clone(),
            stack_init: self.isa.stack_init.clone(),
            exit_sentinel: self.isa.exit_sentinel.clone(),
            pc_register: self.isa.pc_register.clone(),
            stack_register: self.isa.stack_register.clone(),
            flags_register: self.isa.flags_register.clone(),
            flags_init: self.isa.flags_init,
            link_register: self.isa.link_register.clone(),
            registers: self.registers.clone(),
            flags: self.flags.clone(),
            abis: self.abis.clone(),
            instruction_notes: self.instruction_notes.clone(),
        }
    }
}

/// 运行期 ISA 定义(加载后的只读快照)。
#[derive(Debug, Clone)]
pub struct IsaDefinition {
    pub id: String,
    pub name: String,
    pub endian: String,
    pub emulator_arch: String,
    pub emulator_mode: String,
    pub assembler: String,
    pub memory_map: MemoryMap,
    pub panel_registers: Vec<String>,
    pub stack_init: String,
    pub exit_sentinel: String,
    pub pc_register: String,
    pub stack_register: String,
    pub flags_register: String,
    pub flags_init: u64,
    pub link_register: Option<String>,
    pub registers: Vec<RegisterDef>,
    pub flags: Vec<FlagDef>,
    pub abis: Vec<AbiDefinition>,
    pub instruction_notes: Vec<InstructionNote>,
}

impl IsaDefinition {
    pub fn code_base(&self) -> u64 {
        parse_int(&self.memory_map.code.base).unwrap_or(0x4000_0000)
    }
    pub fn stack_base(&self) -> u64 {
        parse_int(&self.memory_map.stack.base).unwrap_or(0)
    }
    pub fn stack_top(&self) -> u64 {
        self.stack_base() + self.memory_map.stack.size
    }
    pub fn stack_init_value(&self) -> u64 {
        parse_int(&self.stack_init).unwrap_or_else(|_| self.stack_top())
    }
    pub fn exit_sentinel_addr(&self) -> u64 {
        parse_int(&self.exit_sentinel).unwrap_or(0)
    }
    pub fn flag_name(&self, bit: u32) -> Option<&str> {
        self.flags.iter().find(|f| f.bit == bit).map(|f| f.name.as_str())
    }
    pub fn note_for(&self, mnemonic: &str) -> Option<&InstructionNote> {
        self.instruction_notes.iter().find(|n| n.mnemonic.eq_ignore_ascii_case(mnemonic))
    }
    pub fn abi(&self, id: &str) -> Option<&AbiDefinition> {
        self.abis.iter().find(|a| a.id == id)
    }

    /// 从 data/isas/<id>/isa.toml(及其它文件)加载。
    /// 单文件制:registers/flags/abi/notes 都并入 isa.toml(Phase 1 简化,
    /// 拆分文件留待内容规模增大后)。
    pub fn load_from_dir(data_dir: &Path, isa_id: &str) -> Result<IsaDefinition, LabError> {
        // id 白名单:防路径遍历(..、绝对路径、特殊字符)
        if isa_id.is_empty()
            || !isa_id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(LabError::IsaNotFound(isa_id.to_string()));
        }
        let path = data_dir.join("isas").join(isa_id).join("isa.toml");
        let text = std::fs::read_to_string(&path)
            .map_err(|_| LabError::IsaNotFound(isa_id.to_string()))?;
        let pkg: IsaPackage = toml::from_str(&text)
            .map_err(|e| LabError::BadRequest(format!("ISA {isa_id} 数据解析失败: {e}")))?;
        let def = pkg.isa();
        if def.id != isa_id {
            return Err(LabError::BadRequest(format!(
                "ISA id 不匹配: 文件内 {} vs 目录 {isa_id}",
                def.id
            )));
        }
        Ok(def)
    }
}
