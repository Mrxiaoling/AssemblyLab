//! 内容自动校验(TEST_PLAN §5):课程数据文件可解析、模式 A 实验可真实跑通、
//! 习题可加载且判题链路自洽(以参考答案自检)。
//! 模式 B/compare 课程(mode = debug/compare)依赖 WSL 工具链,不在单测内运行,
//! 由 curl/浏览器端到端验收覆盖。

use lab_judge::{load_exercises, Judge};
use lab_core::IsaDefinition;
use std::path::PathBuf;

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

/// WSL 可用性探测:ARM64 汇编(aarch64-linux-gnu-as)与模式 B 依赖 WSL2。
/// 沙箱/CI 中 wsl.exe 可能被策略拦截,此时跳过 ARM64 用例而不是把整轮测试判失败。
fn wsl_available() -> bool {
    std::process::Command::new("wsl.exe")
        .args(["-d", "Ubuntu-22.04", "-e", "true"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn all_course_files() -> Vec<PathBuf> {
    let dir = data_dir().join("courses");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("courses 目录存在")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "toml").unwrap_or(false))
        .collect();
    out.sort();
    out
}

#[test]
fn courses_parse_and_schema_ok() {
    let files = all_course_files();
    assert!(files.len() >= 22, "应至少有 22 门课,实际 {}", files.len());
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap_or_else(|e| panic!("读取 {:?}: {e}", f));
        let v: toml::Value = toml::from_str(&text).unwrap_or_else(|e| panic!("解析 {:?}: {e}", f));
        let id = v["lesson"]["id"].as_str().unwrap_or_else(|| panic!("{:?} 缺 lesson.id", f));
        let stem = f.file_stem().unwrap().to_string_lossy();
        assert_eq!(id, stem, "{:?}: lesson.id 应与文件名一致", f);
        assert!(v["lesson"]["stage"].as_integer().unwrap_or(0) >= 1, "{id}: 缺 stage");
        assert!(v["lesson"]["goal"]["problem"].as_str().is_some(), "{id}: 缺 goal.problem");
        let steps = v["steps"].as_array().unwrap_or_else(|| panic!("{id}: 缺 steps"));
        assert!(steps.len() >= 8, "{id}: steps 过少({})", steps.len());
        let exp_src = v["lesson"]["experiment"]["source"].as_str().unwrap_or("");
        assert!(!exp_src.is_empty(), "{id}: 实验源码为空");
        // mode 与源码语言一致性:C++ 源码必须声明 debug/compare
        if exp_src.contains("int main(") {
            let mode = v["lesson"]["experiment"]
                .get("mode")
                .and_then(|x| x.as_str())
                .unwrap_or("asm");
            assert!(mode == "debug" || mode == "compare", "{id}: C++ 源码应声明 mode=debug/compare");
        }
    }
}

#[test]
fn mode_a_experiments_actually_run() {
    let wsl = wsl_available();
    let mut skipped = 0usize;
    for f in all_course_files() {
        let text = std::fs::read_to_string(&f).unwrap();
        let v: toml::Value = toml::from_str(&text).unwrap();
        let src = v["lesson"]["experiment"]["source"].as_str().unwrap_or("");
        let mode = v["lesson"]["experiment"]
            .get("mode")
            .and_then(|x| x.as_str())
            .unwrap_or("asm");
        let id = v["lesson"]["id"].as_str().unwrap_or("?").to_string();
        if mode != "asm" || src.contains("int main(") {
            continue; // 模式 B/compare 不在单测跑(需 WSL)
        }
        // 按课程声明的 ISA 汇编/执行(arm64 = GNU as + Unicorn ARM64)
        let isa_id = v["lesson"]["experiment"]
            .get("isa")
            .and_then(|x| x.as_str())
            .unwrap_or("x86-64")
            .to_string();
        if isa_id != "x86-64" && !wsl {
            skipped += 1;
            continue; // ARM64 实验需 WSL GNU as
        }
        let isa = IsaDefinition::load_from_dir(&data_dir(), &isa_id)
            .unwrap_or_else(|e| panic!("{id}: ISA {isa_id} 加载失败: {e}"));
        let abi = isa.abis.first().map(|a| a.id.clone()).unwrap_or_default();
        let mut session = lab_engine::Session::create(9999, isa, abi, src)
            .unwrap_or_else(|e| panic!("{id}: 实验汇编/装载失败: {e}"));
        // 步进到退出(最多 20000 条;run 模式遇 SMC 只能靠上限,步进更可控)
        let mut steps = 0;
        loop {
            let reports = session.step(1).expect("{id}: 步进失败");
            steps += 1;
            if session.state().terminal || reports.is_empty() || steps > 20_000 {
                break;
            }
        }
        let state = session.state();
        assert!(
            state.terminal,
            "{id}: 实验步进 {} 条后仍未终止(疑似死循环)",
            steps
        );
        if let Some(stop) = &state.stop {
            assert!(
                !matches!(stop, lab_core::StopReason::Fault { .. }),
                "{id}: 实验执行出现 CPU 异常: {:?}",
                stop
            );
        }
    }
    if skipped > 0 {
        eprintln!("skip: WSL 不可用,跳过 {skipped} 门 ARM64 课程的真实执行校验");
    }
}

#[test]
fn exercises_parse_and_have_structure() {
    let list = load_exercises(&data_dir());
    assert!(list.len() >= 19, "应至少有 19 道习题,实际 {}", list.len());
    for ex in &list {
        assert!(!ex.exercise.hints.level1.is_empty(), "{}: 缺 level1 提示", ex.exercise.id);
        assert!(
            !ex.exercise.answer_md.trim().is_empty(),
            "{}: 缺 answer_md 参考解析(用户主动查看解析时会是空内容)",
            ex.exercise.id
        );
        assert!(
            ex.predict.is_some() || ex.fill_blank.is_some() || ex.quiz.is_some() || ex.self_check.is_some(),
            "{}: predict/fill_blank/quiz/self_check 至少其一",
            ex.exercise.id
        );
        if let Some(p) = &ex.predict {
            assert!(!p.questions.is_empty(), "{}: predict 无问题", ex.exercise.id);
            for q in &p.questions {
                // 合法形态:flag:<名> / reg:<名> / 裸 reg|flag(key 即寄存器/标志名)
                let k = &q.expect_kind;
                assert!(
                    k.starts_with("flag:") || k.starts_with("reg:") || k == "reg" || k == "flag",
                    "{}: expect_kind 非法 {}",
                    ex.exercise.id,
                    k
                );
            }
        }
        if let Some(f) = &ex.fill_blank {
            assert!(f.asm_template.contains("??"), "{}: 模板缺 ?? 占位", ex.exercise.id);
            assert!(!f.testcases.is_empty(), "{}: 无判题用例", ex.exercise.id);
        }
    }
}

/// 课程正文数值 golden:用真实模拟引擎逐步执行,核对第 N 步寄存器快照。
/// 课程可用 `[[lesson.experiment.golden]] step/reg/value` 声明正文里引用的数值;
/// 改动实验源码却忘记改正文时,这里会失败(TEST_PLAN §5 的最小可运行实现)。
#[test]
fn course_golden_values_match_real_execution() {
    let parse_hex = |s: &str| -> u64 {
        u64::from_str_radix(s.trim().trim_start_matches("0x"), 16)
            .unwrap_or_else(|e| panic!("非法十六进制值 {s}: {e}"))
    };
    let mut checked = 0usize;
    for f in all_course_files() {
        let text = std::fs::read_to_string(&f).unwrap();
        let v: toml::Value = toml::from_str(&text).unwrap();
        let exp = match v.get("lesson").and_then(|l| l.get("experiment")) {
            Some(e) => e,
            None => continue,
        };
        let goldens = match exp.get("golden").and_then(|g| g.as_array()) {
            Some(g) if !g.is_empty() => g,
            _ => continue,
        };
        let id = v["lesson"]["id"].as_str().unwrap_or("?").to_string();
        let src = exp.get("source").and_then(|x| x.as_str()).unwrap_or("");
        let isa_id = exp.get("isa").and_then(|x| x.as_str()).unwrap_or("x86-64");
        let isa = IsaDefinition::load_from_dir(&data_dir(), isa_id)
            .unwrap_or_else(|e| panic!("{id}: ISA {isa_id} 加载失败: {e}"));
        let abi = isa.abis.first().map(|a| a.id.clone()).unwrap_or_default();
        let mut session = lab_engine::Session::create(9998, isa, abi, src)
            .unwrap_or_else(|e| panic!("{id}: 实验装载失败: {e}"));
        let mut snapshots: Vec<Vec<(String, String)>> = Vec::new();
        for _ in 0..20_000 {
            let reports = session.step(1).expect("步进失败");
            if reports.is_empty() {
                break;
            }
            snapshots.push(reports.last().unwrap().after.clone());
            if session.state().terminal {
                break;
            }
        }
        for g in goldens {
            let step = g.get("step").and_then(|x| x.as_integer()).unwrap_or(0) as usize;
            let reg = g.get("reg").and_then(|x| x.as_str()).unwrap_or("");
            let want = g.get("value").and_then(|x| x.as_str()).unwrap_or("");
            let snap = snapshots
                .get(step.saturating_sub(1))
                .unwrap_or_else(|| panic!("{id}: 实验未执行到第 {step} 步"));
            let got = snap
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(reg))
                .map(|(_, v)| v.as_str())
                .unwrap_or_else(|| panic!("{id}: 第 {step} 步快照无寄存器 {reg}"));
            assert_eq!(
                parse_hex(got),
                parse_hex(want),
                "{id}: 第 {step} 步 {reg} 真实值 {got} 与正文 golden {want} 不符"
            );
            checked += 1;
        }
    }
    assert!(checked >= 3, "golden 校验条目过少({checked}),机制形同虚设");
}

/// 判题链路自检:用参考答案作答,应全部通过(新 P3 习题覆盖)
#[test]
fn p3_exercises_judge_self_check() {
    let isa = IsaDefinition::load_from_dir(&data_dir(), "x86-64").expect("ISA 加载");
    let judge = Judge::new(isa);

    let cases: Vec<(&str, Vec<(&str, &str)>)> = vec![
        // 数组求和:sum=35, 循环变量=5
        ("stage03-q01-predict-array-sum", vec![("RAX", "0x23"), ("RCX", "5")]),
        // 端序:小端首字节 0x44,次字节 0x33(movzx 零扩展)
        ("stage03-q03-predict-endian", vec![("RAX", "0x44"), ("RCX", "0x33")]),
        // 递归:5! = 120,退出时 RDI=0
        ("stage04-q03-predict-factorial", vec![("RBX", "0x78"), ("RDI", "0")]),
    ];
    for (id, answers) in cases {
        let ex = load_exercises(&data_dir())
            .into_iter()
            .find(|e| e.exercise.id == id)
            .unwrap_or_else(|| panic!("{id}: 习题未加载"));
        let map: std::collections::HashMap<String, String> = answers
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let r = judge.judge(&ex, &map);
        assert!(r.passed, "{id}: 参考答案未通过判题: {:?}", r.items);
        // 错误答案必须不通过(抽一问改错)
        let mut wrong = map.clone();
        if let Some(k) = wrong.keys().next().cloned() {
            wrong.insert(k, "0xdeadbeef".into());
            let r2 = judge.judge(&ex, &wrong);
            assert!(!r2.passed, "{id}: 错误答案竟然通过了", );
        }
    }
}

/// ARM64 习题判题自检(按 exercise.isa 装载)
#[test]
fn p5_arm64_exercises_judge_self_check() {
    if !wsl_available() {
        eprintln!("skip: WSL 不可用(沙箱/CI)——ARM64 习题判题自检跳过");
        return;
    }
    let cases: Vec<(&str, Vec<(&str, &str)>)> = vec![
        // 21 + (2<<1) + 100 = 125 = 0x7D;NZCV 全 0
        ("stage10-q01-predict-adds", vec![("X2", "0x7d"), ("NZCV", "0")]),
        // 4! = 24 = 0x18;最后的 ldp 是最外层(LIFO 最后读)→ X1 = 4
        ("stage10-q02-predict-fact", vec![("X19", "0x18"), ("X1", "0x4")]),
    ];
    for (id, answers) in cases {
        let ex = load_exercises(&data_dir())
            .into_iter()
            .find(|e| e.exercise.id == id)
            .unwrap_or_else(|| panic!("{id}: 习题未加载"));
        let isa_id = if ex.exercise.isa.is_empty() { "x86-64" } else { &ex.exercise.isa };
        let isa = IsaDefinition::load_from_dir(&data_dir(), isa_id).expect("ISA 加载");
        let judge = Judge::new(isa);
        let map: std::collections::HashMap<String, String> = answers
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let r = judge.judge(&ex, &map);
        assert!(r.passed, "{id}: ARM64 参考答案未通过判题: {:?}", r.items);
    }
}

/// P7:八种题型全覆盖自检(新增 6 题的判题链路)
#[test]
fn p7_all_kinds_judge_self_check() {
    let cases: Vec<(&str, Vec<(&str, &str)>)> = vec![
        // cpp2asm 静态问答:this=RDI、y 偏移 4、不在对象内
        ("stage06-q04-cpp2asm", vec![
            ("this_reg", "RDI"), ("y_offset", "4"), ("in_object", "否"),
        ]),
        // debug_challenge:偏移 8 / 修复后 0x41 / 不会崩
        ("stage03-q04-debug-challenge", vec![
            ("offset", "8"), ("buf0", "0x41"), ("crash", "不会"),
        ]),
        // asm2cpp 自评:全勾通过
        ("stage09-q04-asm2cpp", vec![
            ("item0", "true"), ("item1", "true"), ("item2", "true"), ("item3", "true"),
        ]),
        // explain 自评:全勾通过
        ("stage05-q04-explain-asif", vec![
            ("item0", "true"), ("item1", "true"), ("item2", "true"),
            ("item3", "true"), ("item4", "true"),
        ]),
        // fix_bug:修复为 add
        ("stage01-q03-fix-bug", vec![("hole", "add eax, ebx")]),
        // programming strlen:三组用例(核心填空 je .done)
        ("stage03-q05-programming-strlen", vec![("hole", "je .done")]),
        // 综合项目 strcpy:test cl,cl 后 jz .done
        ("project-q01-programming-strcpy", vec![("hole", "jz .done")]),
    ];
    for (id, answers) in cases {
        let ex = load_exercises(&data_dir())
            .into_iter()
            .find(|e| e.exercise.id == id)
            .unwrap_or_else(|| panic!("{id}: 习题未加载"));
        let isa_id = if ex.exercise.isa.is_empty() { "x86-64" } else { &ex.exercise.isa };
        let isa = IsaDefinition::load_from_dir(&data_dir(), isa_id).expect("ISA 加载");
        let judge = Judge::new(isa);
        let map: std::collections::HashMap<String, String> = answers
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let r = judge.judge(&ex, &map);
        assert!(r.passed, "{id}: 参考答案未通过判题: {:?}", r.items);
    }
    // 8 种题型齐全
    let kinds: std::collections::HashSet<String> =
        load_exercises(&data_dir()).into_iter().map(|e| e.exercise.kind).collect();
    for k in ["predict", "fill_blank", "asm2cpp", "cpp2asm", "fix_bug", "debug_challenge", "programming", "explain"] {
        assert!(kinds.contains(k), "缺少题型 {k}");
    }
}

/// P6 习题判题自检(syscall 返回值 / CAS 失败路径 / ARM64 LL-SC)
#[test]
fn p6_exercises_judge_self_check() {
    let cases: Vec<(&str, Vec<(&str, &str)>)> = vec![
        // write(fd=2, 5) → 5;exit(5)
        ("stage08-q01-predict-write", vec![("RBX", "5"), ("RDI", "5")]),
        // CAS 失败:RAX=1(带回现值);内存仍 1
        ("stage09-q01-predict-cas-fail", vec![("RBX", "0x1"), ("RCX", "0x1")]),
        // ARM64 LL/SC:内存=1;stlxr 状态 0
        ("stage10-q03-predict-llsc", vec![("X9", "0x1"), ("X3", "0x0")]),
    ];
    let wsl = wsl_available();
    for (id, answers) in cases {
        let ex = load_exercises(&data_dir())
            .into_iter()
            .find(|e| e.exercise.id == id)
            .unwrap_or_else(|| panic!("{id}: 习题未加载"));
        let isa_id = if ex.exercise.isa.is_empty() { "x86-64" } else { &ex.exercise.isa };
        if isa_id != "x86-64" && !wsl {
            eprintln!("skip: WSL 不可用,跳过 ARM64 用例 {id}");
            continue;
        }
        let isa = IsaDefinition::load_from_dir(&data_dir(), isa_id).expect("ISA 加载");
        let judge = Judge::new(isa);
        let map: std::collections::HashMap<String, String> = answers
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let r = judge.judge(&ex, &map);
        assert!(r.passed, "{id}: 参考答案未通过判题: {:?}", r.items);
        let mut wrong = map.clone();
        if let Some(k) = wrong.keys().next().cloned() {
            wrong.insert(k, "0xdeadbeef".into());
            assert!(!judge.judge(&ex, &wrong).passed, "{id}: 错误答案竟然通过");
        }
    }
}

/// 补全题:参考答案自检
#[test]
fn p3_fill_blank_reference_answer() {
    let isa = IsaDefinition::load_from_dir(&data_dir(), "x86-64").expect("ISA 加载");
    let judge = Judge::new(isa);
    let ex = load_exercises(&data_dir())
        .into_iter()
        .find(|e| e.exercise.id == "stage03-q02-fill-indexed")
        .expect("习题未加载");
    let mut map = std::collections::HashMap::new();
    map.insert("hole".to_string(), "add eax, [rbx+rcx*4]".to_string());
    let r = judge.judge(&ex, &map);
    assert!(r.passed, "补全题参考答案未通过: {:?}", r.error);
}
