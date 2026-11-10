//! 编译器适配集成测试(真实 WSL g++)。

use lab_adapters::compiler::{compile, CompileRequest, parse_objdump};

const SRC: &str = r#"
int add(int a, int b) {
    return a + b;
}

int main() {
    int r = add(10, 20);
    return r;
}
"#;


/// WSL 可用性探测(沙箱/CI 中 wsl.exe 可能被策略拦截:跳过而非失败)
fn wsl_available() -> bool {
    std::process::Command::new("wsl.exe")
        .args(["-d", "Ubuntu-22.04", "-e", "true"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn detect_wsl_toolchain() {
    if !wsl_available() {
        eprintln!("skip: WSL 不可用(沙箱/CI)——WSL 依赖测试跳过");
        return;
    }
    let v = lab_adapters::compiler::detect_toolchain();
    assert!(v.is_some(), "WSL 内未找到 g++(T2.0 安装未完成?)");
    println!("toolchain: {}", v.unwrap());
}

#[test]
fn compile_with_source_mapping() {
    if !wsl_available() {
        eprintln!("skip: WSL 不可用(沙箱/CI)——WSL 依赖测试跳过");
        return;
    }
    let out = compile(&CompileRequest {
        source: SRC.into(),
        opt: "O0".into(),
        compiler: "g++".into(),
    })
    .expect("编译失败");
    assert!(!out.asm_lines.is_empty());
    assert!(out.work_dir.join("main").exists(), "应产出可执行文件");

    // add 函数存在且有源码行标注
    let add_lines: Vec<_> = out
        .asm_lines
        .iter()
        .filter(|l| l.function.as_deref().map(|f| f.starts_with("add")).unwrap_or(false) && l.addr.is_some())
        .collect();
    assert!(!add_lines.is_empty(), "add 函数应有指令");
    assert!(
        add_lines.iter().any(|l| l.source_line.is_some()),
        "指令应带源码行号(DWARF)"
    );
    // return a + b 是第 2 行
    assert!(
        add_lines.iter().any(|l| l.source_line == Some(2)),
        "应有源码第 2 行的映射,实际 {:?}",
        add_lines.iter().map(|l| l.source_line).collect::<Vec<_>>()
    );

    // 清理
    lab_adapters::compiler::cleanup(&out.work_dir);
}

#[test]
fn compile_error_line_mapping() {
    if !wsl_available() {
        eprintln!("skip: WSL 不可用(沙箱/CI)——WSL 依赖测试跳过");
        return;
    }
    let result = compile(&CompileRequest {
        source: "int main() { return oops; }\n".into(),
        opt: "O0".into(),
        compiler: "g++".into(),
    });
    match result {
        Err(lab_core::LabError::Assemble { line, message }) => {
            assert_eq!(line, 1, "错误应定位到第 1 行");
            assert!(message.contains("error"), "{message}");
        }
        other => panic!("期望编译错误,实际 {other:?}"),
    }
}

#[test]
fn objdump_parser_shapes() {
    if !wsl_available() {
        eprintln!("skip: WSL 不可用(沙箱/CI)——WSL 依赖测试跳过");
        return;
    }
    let sample = "\n0000000000001129 <main>:\n/main.cpp:8\n  1129: push rbp\n  112a: mov rbp, rsp\n";
    let lines = parse_objdump(sample);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].function.as_deref(), Some("main"));
    assert_eq!(lines[1].source_line, Some(8));
    assert_eq!(lines[2].addr, Some(0x112a));
    assert_eq!(lines[2].text, "mov rbp, rsp");
}
