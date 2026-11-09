//! 引擎级集成测试:汇编 → 装载 → 单步/运行 → diff/解释(真实 nasm + unicorn)。
//! 这是 golden 测试体系(TEST_PLAN §2)的执行载体。

use lab_engine::Session;

const SAMPLE: &str = r#"[bits 64]
org 0x400000
global _start

_start:
    mov eax, 10
    mov ebx, 20
    add rax, rbx
    mov [result], rax
    xor ecx, ecx
    sub eax, 5
    mov rax, 60
    mov edi, 7
    syscall

result: dq 0
"#;

fn new_session() -> Session {
    let isa = lab_core::isa::IsaDefinition::load_from_dir(
        std::path::Path::new("../../data"),
        "x86-64",
    )
    .unwrap();
    Session::create(1, isa, "sysv-amd64".into(), SAMPLE).unwrap()
}

fn get_reg<'a>(snap: &'a [(String, String)], name: &str) -> &'a str {
    snap.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str()).unwrap_or("?")
}

#[test]
fn step_reports_have_diffs_and_explanations() {
    let mut s = new_session();
    let reports = s.step(4).unwrap();
    assert_eq!(reports.len(), 4);
    // step_no 从 1 递增
    assert_eq!(reports[0].step_no, 1);
    assert_eq!(reports[3].step_no, 4);

    // 第 1 条:mov eax,10
    let r0 = &reports[0];
    assert_eq!(r0.disasm, "mov eax,0Ah");
    assert_eq!(r0.mnemonic, "mov");
    assert!(r0.flag_diff.is_empty(), "MOV 不影响标志");
    assert!(r0.explanation.summary.contains("复制"), "解释来自元数据: {}", r0.explanation.summary);

    // 第 3 条:add → PF=1
    let r2 = &reports[2];
    assert_eq!(r2.mnemonic, "add");
    assert!(r2.flag_diff.iter().any(|f| f.name == "PF" && !f.before && f.after));

    // 第 4 条:内存写 [result]
    let r3 = &reports[3];
    assert_eq!(r3.mem_changes.len(), 1);
    assert_eq!(r3.mem_changes[0].value_hex, "1E00000000000000");
    assert!(r3.explanation.detail.iter().any(|d| d.contains("写内存")));
}

#[test]
fn run_to_syscall_exit() {
    let mut s = new_session();
    let summary = s.run().unwrap();
    assert_eq!(summary.executed, 9);
    match &summary.stop {
        lab_core::StopReason::ProgramExit { code } => assert_eq!(*code, Some(7)),
        other => panic!("期望 ProgramExit,实际 {other:?}"),
    }
    // 终态后 step 被拒绝
    assert!(s.step(1).is_err());
    // 重置后可再次执行
    s.reset().unwrap();
    let reports = s.step(2).unwrap();
    assert_eq!(reports.len(), 2);
}

#[test]
fn breakpoint_then_continue_with_patch() {
    let mut s = new_session();
    // add 指令 @0x40000A
    s.set_breakpoints(&[0x40000A]).unwrap();
    let summary = s.run().unwrap();
    assert!(matches!(summary.stop, lab_core::StopReason::Breakpoint { address: 0x40000A }));

    // 断点处改寄存器,单步继续(单步无视断点)
    s.set_register("RAX", 0x99).unwrap();
    let reports = s.step(1).unwrap();
    assert_eq!(reports[0].mnemonic, "add");
    let rax_after = reports[0].after.iter().find(|(n, _)| n == "RAX").unwrap();
    assert_eq!(rax_after.1, "0xad", "0x99 + 0x14 = 0xAD,宿主改值必须生效");
}

#[test]
fn reassemble_error_line_mapping() {
    let mut s = new_session();
    match s.reassemble("mov eax, 1\nmov ebx, no_such_symbol\n") {
        Err(lab_core::LabError::Assemble { line, .. }) => assert_eq!(line, 2),
        other => panic!("期望汇编错误,实际 {other:?}"),
    }
    // 原会话不受影响
    assert!(s.step(1).is_ok());
}

#[test]
fn trace_records_steps_and_runs() {
    let mut s = new_session();
    s.step(3).unwrap();
    s.set_breakpoints(&[0x40000A]).unwrap();
    s.run().unwrap();
    let (entries, dropped) = s.trace();
    assert_eq!(dropped, 0);
    assert_eq!(entries.len(), 4); // 3 step + 1 run
    assert_eq!(entries[0].kind_str(), "step");
    assert_eq!(entries[3].kind_str(), "run");
}

trait KindStrExt {
    fn kind_str(&self) -> &'static str;
}

impl KindStrExt for lab_engine::TraceEntry {
    fn kind_str(&self) -> &'static str {
        match self {
            lab_engine::TraceEntry::Step { .. } => "step",
            lab_engine::TraceEntry::Run { .. } => "run",
        }
    }
}
