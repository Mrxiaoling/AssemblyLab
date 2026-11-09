//! NASM 适配器集成测试(真实 nasm 进程)。

use lab_adapters::nasm::{assemble, find_nasm, nasm_version};
use lab_core::LabError;

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
    xor edi, edi
    syscall

result: dq 0
"#;

#[test]
fn nasm_available() {
    assert!(find_nasm().is_some(), "tools/nasm/nasm.exe 未找到");
    let v = nasm_version().unwrap();
    assert!(v.contains("NASM version"), "{v}");
}

#[test]
fn assemble_sample_program() {
    let img = assemble(SAMPLE, 0x400000).unwrap();
    eprintln!("DEBUG symbols={:?}
DEBUG line_map={:?}", img.symbols, img.line_map);
    // 程序 35 字节代码 + 8 数据 = 43(单段扁平,无填充)
    assert_eq!(img.bytes.len(), 43, "实际 {}", img.bytes.len());
    assert_eq!(img.base, 0x400000);
    assert_eq!(img.entry, 0x400000, "入口应解析 _start 标签");
    // 行号映射:第 6 行(相对用户源码;自动 prepend 不适用,因为有 org)
    let l6 = img.line_map.iter().find(|m| m.line == 6).expect("行 6 映射");
    assert_eq!((l6.addr, l6.size), (0x400000, 5));
    // 符号:result @ 0x400024
    let result = img.symbols.iter().find(|(n, _)| n == "result").expect("result 符号");
    assert_eq!(result.1, 0x400023, "result 实际地址 {:#x}", result.1);
    let start = img.symbols.iter().find(|(n, _)| n == "_start").expect("_start 符号");
    assert_eq!(start.1, 0x400000);
}

#[test]
fn error_line_mapping() {
    let src = "mov eax, 10\nmov ebx, undefined_symbol\n";
    match assemble(src, 0x400000) {
        Err(LabError::Assemble { line, message }) => {
            assert_eq!(line, 2, "错误应定位到用户源码第 2 行(自动 prepend 偏移已扣除)");
            assert!(message.contains("undefined_symbol") || message.to_lowercase().contains("symbol"), "{message}");
        }
        other => panic!("期望汇编错误,实际 {other:?}"),
    }
}

#[test]
fn section_data_rejected() {
    let src = "org 0x400000\nmov eax,1\nsection .data\nx: dq 0\n";
    match assemble(src, 0x400000) {
        Err(LabError::BadRequest(msg)) => assert!(msg.contains("单段扁平"), "{msg}"),
        other => panic!("期望 BadRequest,实际 {other:?}"),
    }
}

#[test]
fn auto_prepend_org_when_missing() {
    // 无 org → 自动 prepend,错误行号需扣除 2 行偏移
    let src = "mov eax, 1\nmov ebx, nosuchsym\n";
    match assemble(src, 0x400000) {
        Err(LabError::Assemble { line, .. }) => assert_eq!(line, 2, "行号应回映射到用户源码"),
        other => panic!("期望汇编错误,实际 {other:?}"),
    }
}
