//! Unicorn 适配器集成测试(机器码内嵌,不依赖 nasm)。
//! 语义断言是引擎正确性的第一道防线(TEST_PLAN §2 的基础)。

use lab_adapters::unicorn::UnicornMachine;
use lab_core::{EmulatorAdapter, LoadSpec, RunControl, StopReason};

fn spec(bytes: &[u8]) -> LoadSpec {
    LoadSpec {
        base: 0x400000,
        entry: 0x400000,
        bytes: bytes.to_vec(),
        regions: vec![(0x400000, 0x100000), (0x1000, 0x4000), (0x7FF000000000, 0x100000)],
        panel_registers: ["RAX", "RBX", "RCX", "RDX", "RSI", "RDI", "RBP", "RSP", "RIP", "RFLAGS"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        init_regs: vec![("RSP".into(), 0x7FF0000FF000u64)],
        exit_sentinel: 0x4FFF00,
        heap_end: 0x1100000,
    }
}

fn reg(snap: &[(String, u64)], name: &str) -> u64 {
    snap.iter().find(|(n, _)| n == name).map(|(_, v)| *v).unwrap_or_else(|| panic!("缺少 {name}"))
}

#[test]
fn step_loop_with_flags_and_host_patch() {
    // mov eax,10; mov ebx,20; add rax,rbx; mov [0x2000],rax; xor ecx,ecx; sub eax,5
    let code: [u8; 28] = [
        0xB8, 0x0A, 0x00, 0x00, 0x00, // mov eax, 10
        0xBB, 0x14, 0x00, 0x00, 0x00, // mov ebx, 20
        0x48, 0x01, 0xD8, // add rax, rbx
        0x48, 0xA3, 0x00, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // mov [0x2000], rax
        0x31, 0xC9, // xor ecx, ecx
        0x83, 0xE8, 0x05, // sub eax, 5
    ];
    let mut m = UnicornMachine::new("unicorn:x86", "unicorn:x86-64").unwrap();
    m.load(&spec(&code)).unwrap();

    let events = m.step(6).unwrap();
    assert_eq!(events.len(), 6);

    // step 1: mov eax,10
    assert_eq!(reg(&events[0].after, "RAX"), 10);
    assert_eq!(reg(&events[0].after, "RIP"), 0x400005);

    // step 3: add rax,rbx → 30, PF=1
    assert_eq!(reg(&events[2].after, "RAX"), 30);
    assert_eq!(reg(&events[2].after, "RFLAGS"), 0x6); // bit1 保留 | PF

    // step 4 内存写 [0x2000]=30(注意:add 不访存)
    assert_eq!(events[3].mem_writes.len(), 1, "{:?}", events[3].mem_writes);
    assert_eq!(events[3].mem_writes[0].addr, 0x2000);
    assert_eq!(events[3].mem_writes[0].value_le[0], 30);

    // step 5: xor ecx,ecx → ZF=1
    assert_eq!(reg(&events[4].after, "RCX"), 0);
    assert_eq!(reg(&events[4].after, "RFLAGS"), 0x46); // |ZF

    // 宿主中途改寄存器(spike 场景):在 step3 后 RAX=0x999
    let mut events2 = m.step(1).unwrap(); // step 6: sub eax,5
    let _ = &mut events2;
    let snap = m.read_regs(&["RAX".into()]).unwrap();
    // 未 patch 时: 30-5=25
    assert_eq!(reg(&snap, "RAX"), 25);
}

#[test]
fn host_register_patch_midrun() {
    // mov eax,10; add eax,5 → 15; patch RAX=1000 在两条指令之间 → 最终 1005
    let code: [u8; 9] = [0xB8, 0x0A, 0x00, 0x00, 0x00, 0x83, 0xC0, 0x05, 0xC3];
    let mut m = UnicornMachine::new("unicorn:x86", "unicorn:x86-64").unwrap();
    m.load(&spec(&code)).unwrap();
    m.step(1).unwrap();
    m.write_reg("RAX", 1000).unwrap();
    m.step(1).unwrap();
    let snap = m.read_regs(&["RAX".into()]).unwrap();
    assert_eq!(reg(&snap, "RAX"), 1005);
}

#[test]
fn run_with_syscall_exit_code() {
    // mov edi,42; mov rax,60; syscall; mov ebx,99  → exit(42),rbx 不可达
    let code: [u8; 17] = [
        0xBF, 0x2A, 0x00, 0x00, 0x00, // mov edi, 42
        0xB8, 0x3C, 0x00, 0x00, 0x00, // mov rax, 60
        0x0F, 0x05, // syscall
        0xBB, 0x63, 0x00, 0x00, 0x00, // mov ebx, 99 (不可达)
    ];
    let mut m = UnicornMachine::new("unicorn:x86", "unicorn:x86-64").unwrap();
    m.load(&spec(&code)).unwrap();
    let outcome = m.run(&Default::default(), RunControl::default()).unwrap();
    assert_eq!(
        outcome.stop,
        StopReason::ProgramExit { code: Some(42) },
        "实际: {outcome:?}"
    );
}

#[test]
fn syscall_write_then_continue_rip_semantics() {
    // write(1, msg, 5); mov ebx,0x77; exit(0) → 验证 syscall 桩后 RIP 恰好推进一次
    let mut code: Vec<u8> = vec![
        0xBF, 0x01, 0x00, 0x00, 0x00, // mov edi, 1
        0xBE, 0x00, 0x10, 0x40, 0x00, // mov esi, 0x401000 (msg, 稍后 write_memory)
        0xBA, 0x05, 0x00, 0x00, 0x00, // mov edx, 5
        0xB8, 0x01, 0x00, 0x00, 0x00, // mov rax, 1
        0x0F, 0x05, // syscall
        0xBB, 0x77, 0x00, 0x00, 0x00, // mov ebx, 0x77
        0xB8, 0x3C, 0x00, 0x00, 0x00, // mov rax, 60
        0x31, 0xFF, // xor edi, edi
        0x0F, 0x05, // syscall
    ];
    let mut m = UnicornMachine::new("unicorn:x86", "unicorn:x86-64").unwrap();
    m.load(&spec(&code)).unwrap();
    m.write_memory(0x401000, b"hello").unwrap();
    let outcome = m.run(&Default::default(), RunControl::default()).unwrap();
    assert_eq!(outcome.stop, StopReason::ProgramExit { code: Some(0) }, "{outcome:?}");
    assert!(outcome.output.iter().any(|s| s.contains("hello")), "output: {outcome:?}");
    let snap = m.read_regs(&["RBX".into()]).unwrap();
    assert_eq!(reg(&snap, "RBX"), 0x77, "syscall 后 RIP 必须恰好推进 2 字节");
}

#[test]
fn breakpoint_stops_execution() {
    // mov eax,1; mov ebx,2; mov ecx,3
    let code: [u8; 15] = [
        0xB8, 0x01, 0x00, 0x00, 0x00, //
        0xBB, 0x02, 0x00, 0x00, 0x00, //
        0xB9, 0x03, 0x00, 0x00, 0x00,
    ];
    let mut m = UnicornMachine::new("unicorn:x86", "unicorn:x86-64").unwrap();
    m.load(&spec(&code)).unwrap();
    let mut bps = std::collections::HashSet::new();
    bps.insert(0x400005); // 第二条指令
    let outcome = m.run(&bps, RunControl::default()).unwrap();
    assert_eq!(outcome.stop, StopReason::Breakpoint { address: 0x400005 }, "{outcome:?}");
    let snap = m.read_regs(&["RAX".into(), "RBX".into()]).unwrap();
    assert_eq!(reg(&snap, "RAX"), 1);
    assert_eq!(reg(&snap, "RBX"), 0); // 断点指令未执行
    // 清除停止后继续
    m.clear_stop();
    let outcome = m.run(&Default::default(), RunControl::default()).unwrap();
    let snap = m.read_regs(&["RBX".into(), "RCX".into()]).unwrap();
    assert_eq!(reg(&snap, "RBX"), 2);
    assert_eq!(reg(&snap, "RCX"), 3);
}

#[test]
fn unmapped_access_is_friendly_error() {
    // mov rax, 0x9999999; mov [rax], rax → 写未映射
    let code: [u8; 13] = [
        0x48, 0xB8, 0x99, 0x99, 0x99, 0x99, 0x00, 0x00, 0x00, 0x00, // movabs rax, 0x9999999
        0x48, 0x89, 0x00, // mov [rax], rax
    ];
    let mut m = UnicornMachine::new("unicorn:x86", "unicorn:x86-64").unwrap();
    m.load(&spec(&code)).unwrap();
    let outcome = m.run(&Default::default(), RunControl::default()).unwrap();
    match outcome.stop {
        StopReason::Fault { message } => assert!(message.contains("未映射"), "{message}"),
        other => panic!("期望 Fault,实际 {other:?}"),
    }
}

#[test]
fn ret_to_sentinel_is_program_exit() {
    // 只有 ret:初始 [RSP]=sentinel → 直接退出
    let code: [u8; 1] = [0xC3];
    let mut m = UnicornMachine::new("unicorn:x86", "unicorn:x86-64").unwrap();
    m.load(&spec(&code)).unwrap();
    let outcome = m.run(&Default::default(), RunControl::default()).unwrap();
    assert_eq!(outcome.stop, StopReason::ProgramExit { code: None }, "{outcome:?}");
}
