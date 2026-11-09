//! 模式 B 集成测试:编译 → gdb/MI 断点 → 单步 → 帧/变量/内存。

use lab_adapters::compiler::{compile, CompileRequest};
use lab_adapters::gdb::GdbSession;

const SRC: &str = r#"
int add(int a, int b) {
    return a + b;
}

int main() {
    int r = add(10, 20);
    return r;
}
"#;

#[test]
fn gdb_breakpoint_step_frames() {
    // 1. 编译(保留产物目录供 gdb)
    let out = compile(&CompileRequest { source: SRC.into(), opt: "O0".into(), compiler: "g++".into() })
        .expect("编译失败");

    // 2. gdb 启动
    let mut g = GdbSession::spawn(&out.wsl_dir).expect("gdb 启动失败");

    // 3. 断点:main 第 8 行(int r = add(10, 20);)
    let bp = g.insert_breakpoint("main.cpp", 8).expect("断点失败");
    let stop = g.run().expect("run 失败");
    assert_eq!(stop.reason, "breakpoint-hit", "{stop:?}");
    assert_eq!(stop.frame_line, Some(8));

    // 4. 指令级单步(步入 add)
    let stop2 = g.step_instruction(true).expect("step 失败");
    assert!(matches!(stop2.reason.as_str(), "end-stepping-range" | "signal-received" | "breakpoint-hit"), "{stop2:?}");

    // 5. 寄存器快照
    let snap = g.snapshot().expect("寄存器读取失败");
    assert!(snap.iter().any(|(n, _)| n == "RAX"), "应含 RAX");
    assert!(snap.iter().any(|(n, _)| n == "RIP"), "应含 RIP");
    assert!(snap.iter().any(|(n, _)| n == "RFLAGS"), "应含 RFLAGS");

    // 6. 内存读取(main 可执行头部)
    let mem = g.read_memory(0x400000, 8).or_else(|_| {
        let rip = snap.iter().find(|(n, _)| n == "RIP").unwrap().1.clone();
        let addr = u64::from_str_radix(rip.trim_start_matches("0x"), 16).unwrap_or(0);
        g.read_memory(addr, 8)
    });
    assert!(mem.is_ok(), "内存读取失败");

    // 7. 清理断点继续执行到退出
    g.delete_breakpoint(bp).unwrap();
    let stop3 = g.cont().expect("continue 失败");
    assert!(matches!(stop3.reason.as_str(), "exited-normally" | "exited"), "{stop3:?}");

    drop(g);
    lab_adapters::compiler::cleanup(&out.work_dir);
}

#[test]
fn gdb_frames_and_variables() {
    let out = compile(&CompileRequest { source: SRC.into(), opt: "O0".into(), compiler: "g++".into() }).unwrap();
    let mut g = GdbSession::spawn(&out.wsl_dir).unwrap();

    // 断点在 add 函数内第 2 行(return a + b;)
    g.insert_breakpoint("main.cpp", 2).unwrap();
    let stop = g.run().unwrap();
    assert_eq!(stop.reason, "breakpoint-hit");
    assert!(stop.frame_func.as_deref().map(|f| f.starts_with("add")).unwrap_or(false), "{stop:?}");

    // 调用栈:0 = add, 1 = main
    let frames = g.frames().unwrap();
    assert!(frames.len() >= 2, "应有两个以上栈帧: {frames:?}");
    assert!(frames[0].func.starts_with("add"));
    assert!(frames[1].func.starts_with("main"));

    // 变量:参数 a=10, b=20
    let vars = g.variables().unwrap();
    let a = vars.iter().find(|(n, _)| n == "a").map(|(_, v)| v.clone());
    let b = vars.iter().find(|(n, _)| n == "b").map(|(_, v)| v.clone());
    assert_eq!(a.as_deref(), Some("10"), "vars: {vars:?}");
    assert_eq!(b.as_deref(), Some("20"), "vars: {vars:?}");

    drop(g);
    lab_adapters::compiler::cleanup(&out.work_dir);
}
