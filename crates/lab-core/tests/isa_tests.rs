//! ISA 数据包加载测试(加载仓库内 x86-64 定义)。

use lab_core::isa::IsaDefinition;

fn load() -> IsaDefinition {
    // cargo test 的工作目录 = crate 目录(crates/lab-core)
    IsaDefinition::load_from_dir(std::path::Path::new("../../data"), "x86-64").unwrap()
}

#[test]
fn loads_x86_64_package() {
    let isa = load();
    assert_eq!(isa.id, "x86-64");
    assert_eq!(isa.code_base(), 0x400000);
    assert_eq!(isa.stack_init_value(), 0x7FF0000FF000);
    assert_eq!(isa.exit_sentinel_addr(), 0x4FFF00);
    assert!(isa.panel_registers.contains(&"RFLAGS".to_string()));
}

#[test]
fn flags_and_region_lookup() {
    let isa = load();
    assert_eq!(isa.flag_name(6), Some("ZF"));
    assert_eq!(isa.flag_name(11), Some("OF"));
    assert_eq!(isa.memory_map.region_of(0x400100), Some("code"));
    assert_eq!(isa.memory_map.region_of(0x7FF000010000), Some("stack"));
    assert_eq!(isa.memory_map.region_of(0x1234), None);
}

#[test]
fn abis_and_notes() {
    let isa = load();
    eprintln!("DEBUG abis={:?}", isa.abis.iter().map(|a| (&a.id, a.integer_arg_regs.len())).collect::<Vec<_>>());
    let sysv = isa.abi("sysv-amd64").unwrap();
    assert_eq!(sysv.integer_arg_regs[0].reg, "RDI");
    assert_eq!(sysv.stack_alignment, 16);
    let win = isa.abi("win-x64").unwrap();
    assert_eq!(win.integer_arg_regs[0].reg, "RCX");
    assert_eq!(win.shadow_space, 32);
    assert!(isa.note_for("MOV").is_some());
    assert!(isa.note_for("不存在").is_none());
}
