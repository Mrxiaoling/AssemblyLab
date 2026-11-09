//! lab-judge — 习题判题(P1 两题型:预测结果 / 补全指令)。
//! 判题原则(EXERCISE_DESIGN.md):确定性判题 = 在隔离会话中真实执行,
//! 逐项比对;AI/解释类题型 P7 再接入。

use lab_core::{isa::IsaDefinition, StopReason};
use lab_engine::Session;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// 习题数据 schema(data/exercises/*.toml)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExercisePackage {
    pub exercise: ExerciseMeta,
    #[serde(default)]
    pub predict: Option<PredictSpec>,
    #[serde(default)]
    pub fill_blank: Option<FillBlankSpec>,
    /// 静态问答(cpp2asm 分析 / debug-challenge 定位)
    #[serde(default)]
    pub quiz: Option<QuizSpec>,
    /// 自评要点清单(asm2cpp / explain)
    #[serde(default)]
    pub self_check: Option<SelfCheckSpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExerciseMeta {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String, // predict | fill_blank
    pub title: String,
    pub stage: u32,
    pub lesson: String,
    #[serde(default)]
    pub knowledge: Vec<String>,
    pub difficulty: u32,
    #[serde(default)]
    pub hints: Hints,
    #[serde(default)]
    pub answer_md: String,
    #[serde(default)]
    pub common_mistakes: Vec<String>,
    #[serde(default)]
    pub related_experiment: Option<String>,
    /// 目标架构(默认 x86-64;ARM64 习题由对应 ISA 汇编/执行判题)
    #[serde(default)]
    pub isa: String,
}

#[derive(Debug, Clone, Deserialize, Default, Serialize)]
pub struct Hints {
    #[serde(default)]
    pub level1: String,
    #[serde(default)]
    pub level2: String,
}

/// 预测题:给定程序,预测终态若干项。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PredictSpec {
    pub asm: String,
    /// 执行方式:run(跑到退出)或 step:N(单步 N 条)
    pub exec: String,
    pub questions: Vec<PredictQuestion>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PredictQuestion {
    pub key: String,
    pub prompt: String,
    /// reg:<名称> 或 flag:<名称>
    pub expect_kind: String,
}

/// 补全题:模板中有 `??` 占位一条指令;填入后跑 testcases。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FillBlankSpec {
    pub asm_template: String,
    /// 每组用例:预置寄存器 → 期望(退出码 / 内存)
    pub testcases: Vec<FillCase>,
    #[serde(default)]
    pub hole_hint: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InitReg {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FillCase {
    #[serde(default)]
    pub init_regs: Vec<InitReg>,
    #[serde(default)]
    pub expect_exit_code: Option<i64>,
    #[serde(default)]
    pub expect_mem: Option<ExpectMem>,
    /// 桩输出中应包含的文本(write 系编程题用)
    #[serde(default)]
    pub expect_output: Option<String>,
}

/// 静态问答:答案预存在题目里(取自真实编译产物/调试结论),
/// 支持多个可接受答案用 | 分隔;比对忽略大小写与首尾空白。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuizSpec {
    /// 展示文本(预生成汇编/程序等)
    #[serde(default)]
    pub display: String,
    pub questions: Vec<QuizQuestion>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuizQuestion {
    pub key: String,
    pub prompt: String,
    /// 参考答案;多答案用 | 分隔
    pub expect: String,
}

/// 自评要点清单:参考解析分条,用户逐条确认理解(L-C;诚实模式)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SelfCheckSpec {
    /// 题面(汇编或概念说明)
    #[serde(default)]
    pub display: String,
    /// 要点逐条:全部勾选 = 通过
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExpectMem {
    pub addr: String,
    pub qword_le: String,
}

// ---------------------------------------------------------------------------
// 作答与判题结果
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubmitReq {
    /// predict: { "RAX": "0x19", "OF": "true" };fill_blank: { "hole": "add rax, rbx" }
    pub answers: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct JudgeItem {
    pub key: String,
    pub prompt: String,
    pub expected: String,
    pub actual: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct JudgeResult {
    pub passed: bool,
    pub items: Vec<JudgeItem>,
    /// 本次作答序号(提示梯度:1 → hint1,2+ → hint2)
    pub attempt_no: i64,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// 判题器
// ---------------------------------------------------------------------------

pub struct Judge {
    pub isa: IsaDefinition,
}

impl Judge {
    pub fn new(isa: IsaDefinition) -> Self {
        Self { isa }
    }

    pub fn judge(
        &self,
        ex: &ExercisePackage,
        answers: &std::collections::HashMap<String, String>,
    ) -> JudgeResult {
        match ex.exercise.kind.as_str() {
            "predict" => self.judge_predict(ex.predict.as_ref().expect("predict 题缺少 predict 段"), answers),
            "fill_blank" | "fix_bug" | "programming" => {
                // 同一判题模型:模板 + ?? 占位 + testcases(fix_bug 修的是错行,programming 规模大)
                self.judge_fill(ex.fill_blank.as_ref().expect("fill 题缺少 fill_blank 段"), answers)
            }
            "cpp2asm" | "debug_challenge" => {
                self.judge_quiz(ex.quiz.as_ref().expect("quiz 题缺少 quiz 段"), answers)
            }
            "asm2cpp" | "explain" => {
                self.judge_self_check(ex.self_check.as_ref().expect("self_check 题缺少 self_check 段"), answers)
            }
            other => JudgeResult {
                passed: false,
                items: Vec::new(),
                attempt_no: 0,
                error: Some(format!("暂不支持的题型 {other}(P7 接入)")),
            },
        }
    }

    fn make_session(&self, source: &str) -> Result<Session, String> {
        let abi = self.isa.abis.first().map(|a| a.id.clone()).unwrap_or_default();
        Session::create(0, self.isa.clone(), abi, source)
            .map_err(|e| format!("装载失败: {e}"))
    }

    fn judge_predict(
        &self,
        spec: &PredictSpec,
        answers: &std::collections::HashMap<String, String>,
    ) -> JudgeResult {
        let mut items = Vec::new();
        let mut error = None;
        let snapshot = match self.execute_and_capture(&spec.asm, &spec.exec) {
            Ok(s) => s,
            Err(e) => {
                error = Some(e);
                Vec::new()
            }
        };
        for q in &spec.questions {
            let expected = snapshot
                .iter()
                .find(|(n, _)| *n == q.key)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| "(未捕获)".into());
            let answer = answers.get(&q.key).map(|s| s.trim().to_string()).unwrap_or_default();
            let ok = !snapshot.is_empty() && values_equal(&answer, &expected, &q.expect_kind);
            items.push(JudgeItem {
                key: q.key.clone(),
                prompt: q.prompt.clone(),
                expected,
                actual: if answer.is_empty() { "(未作答)".into() } else { answer },
                ok,
            });
        }
        let passed = !items.is_empty() && items.iter().all(|i| i.ok) && error.is_none();
        JudgeResult { passed, items, attempt_no: 0, error }
    }

    /// 执行程序并返回(寄存器名, hex)终态快照 + 标志展开
    fn execute_and_capture(&self, asm: &str, exec: &str) -> Result<Vec<(String, String)>, String> {
        let mut s = self.make_session(asm)?;
        match exec {
            "run" => {
                let outcome = s.run().map_err(|e| e.to_string())?;
                if let StopReason::Fault { message } = &outcome.stop {
                    return Err(format!("执行错误: {message}"));
                }
            }
            e if e.starts_with("step:") => {
                let n: u32 = e[5..].parse().map_err(|_| "step:N 格式错误")?;
                s.step(n).map_err(|e| e.to_string())?;
            }
            other => return Err(format!("未知 exec 方式 {other}")),
        }
        let snap = s.snapshot().map_err(|e| e.to_string())?;
        Ok(snap.into_iter().map(|(n, v)| (n, format!("{v:#x}"))).collect())
    }

    fn judge_fill(
        &self,
        spec: &FillBlankSpec,
        answers: &std::collections::HashMap<String, String>,
    ) -> JudgeResult {
        let hole = answers.get("hole").cloned().unwrap_or_default();
        let mut items = Vec::new();
        let mut error = None;
        let mut all_ok = !hole.trim().is_empty();
        for (i, case) in spec.testcases.iter().enumerate() {
            let src = spec.asm_template.replace("??", &hole);
            let mut s = match self.make_session(&src) {
                Ok(s) => s,
                Err(e) => {
                    error = Some(format!("用例 {}: {e}", i + 1));
                    all_ok = false;
                    break;
                }
            };
            for r in &case.init_regs {
                if let Ok(v) = lab_core::isa::parse_int(&r.value) {
                    let _ = s.set_register(&r.name, v);
                }
            }
            let outcome = match s.run() {
                Ok(o) => o,
                Err(e) => {
                    error = Some(format!("用例 {}: {e}", i + 1));
                    all_ok = false;
                    break;
                }
            };
            let case_ok = match (&outcome.stop, &case.expect_exit_code) {
                (StopReason::ProgramExit { code: Some(c) }, Some(expect)) => *c as i64 == *expect,
                (StopReason::ProgramExit { .. }, None) => true,
                (StopReason::Fault { message }, _) => {
                    error = Some(format!("用例 {}: {message}", i + 1));
                    false
                }
                _ => false,
            };
            let mem_ok = match &case.expect_mem {
                Some(m) => {
                    let addr = lab_core::isa::parse_int(&m.addr).unwrap_or(0);
                    match s.read_memory(addr, 8) {
                        Ok(view) => {
                            let mut b8 = [0u8; 8];
                            b8.copy_from_slice(&view.bytes[..8]);
                            let got = u64::from_le_bytes(b8);
                            let want = lab_core::isa::parse_int(&m.qword_le).unwrap_or(u64::MAX);
                            got == want
                        }
                        Err(_) => false,
                    }
                }
                None => true,
            };
            // expect_output:桩输出须包含指定文本(编程题的可见结果断言)
            let output_ok = match &case.expect_output {
                Some(want) => outcome.output.join("").contains(want.trim()),
                None => true,
            };
            if let Some(want) = &case.expect_output {
                if !output_ok {
                    items.push(JudgeItem {
                        key: format!("case{}:output", i + 1),
                        prompt: "输出包含".into(),
                        expected: want.trim().to_string(),
                        actual: outcome.output.join(""),
                        ok: false,
                    });
                }
            }
            let ok = case_ok && mem_ok && output_ok;
            all_ok &= ok;
            items.push(JudgeItem {
                key: format!("case{}", i + 1),
                prompt: format!("用例 {}: {:?}", i + 1, case.init_regs),
                expected: describe_case(case),
                actual: if ok { "通过".into() } else { format!("实际 {}", outcome.stop.kind_str()) },
                ok,
            });
        }
        JudgeResult { passed: all_ok && error.is_none(), items, attempt_no: 0, error }
    }

    /// 静态问答(cpp2asm / debug-challenge):文本比对,| 分隔多答案,忽略大小写与空白
    fn judge_quiz(
        &self,
        spec: &QuizSpec,
        answers: &std::collections::HashMap<String, String>,
    ) -> JudgeResult {
        let mut items = Vec::new();
        for q in &spec.questions {
            let ans = answers.get(&q.key).map(|s| s.trim().to_string()).unwrap_or_default();
            let accepted: Vec<String> = q.expect.split('|').map(|x| normalize_text(x)).collect();
            let expected = q.expect.replace('|', " 或 ");
            let ok = !ans.is_empty() && accepted.contains(&normalize_text(&ans));
            items.push(JudgeItem {
                key: q.key.clone(),
                prompt: q.prompt.clone(),
                expected,
                actual: if ans.is_empty() { "(未作答)".into() } else { ans },
                ok,
            });
        }
        let passed = !items.is_empty() && items.iter().all(|i| i.ok);
        JudgeResult { passed, items, attempt_no: 0, error: None }
    }

    /// 自评要点(asm2cpp / explain):逐条勾选,全部确认 = 通过(L-C 诚实模式)
    fn judge_self_check(
        &self,
        spec: &SelfCheckSpec,
        answers: &std::collections::HashMap<String, String>,
    ) -> JudgeResult {
        let mut items = Vec::new();
        for (i, point) in spec.items.iter().enumerate() {
            let key = format!("item{i}");
            let ok = answers.get(&key).map(|v| v == "true").unwrap_or(false);
            items.push(JudgeItem {
                key,
                prompt: point.clone(),
                expected: "已理解".into(),
                actual: if ok { "已理解".into() } else { "(未确认)".into() },
                ok,
            });
        }
        let passed = !items.is_empty() && items.iter().all(|i| i.ok);
        JudgeResult { passed, items, attempt_no: 0, error: None }
    }
}

fn normalize_text(s: &str) -> String {
    s.trim().to_ascii_lowercase().replace(' ', "")
}

fn describe_case(c: &FillCase) -> String {
    let mut parts = Vec::new();
    if let Some(code) = c.expect_exit_code {
        parts.push(format!("退出码 {code}"));
    }
    if let Some(m) = &c.expect_mem {
        parts.push(format!("[{}] = {}", m.addr, m.qword_le));
    }
    if let Some(o) = &c.expect_output {
        parts.push(format!("输出含 {:?}", o.trim()));
    }
    parts.join(", ")
}

trait StopKindStr {
    fn kind_str(&self) -> &'static str;
}
impl StopKindStr for StopReason {
    fn kind_str(&self) -> &'static str {
        match self {
            StopReason::ProgramExit { .. } => "program_exit",
            StopReason::Breakpoint { .. } => "breakpoint",
            StopReason::InstructionLimit { .. } => "instruction_limit",
            StopReason::Timeout => "timeout",
            StopReason::Fault { .. } => "fault",
            StopReason::StepsDone => "steps_done",
        }
    }
}

/// 值等价:寄存器支持 hex(0x..)/十进制;标志支持 true/false/1/0
fn values_equal(answer: &str, expected: &str, kind: &str) -> bool {
    let a = answer.trim();
    if kind.starts_with("flag:") {
        let ab = matches!(a.to_ascii_lowercase().as_str(), "true" | "1" | "1.0");
        let eb = matches!(expected, "0x1" | "true" | "1");
        return ab == eb;
    }
    match (lab_core::isa::parse_int(a), lab_core::isa::parse_int(expected)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a.eq_ignore_ascii_case(expected),
    }
}

/// 从 data/exercises 目录加载全部习题
pub fn load_exercises(data_dir: &std::path::Path) -> Vec<ExercisePackage> {
    let dir = data_dir.join("exercises");
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut paths: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|x| x == "toml").unwrap_or(false)).collect();
        paths.sort();
        for p in paths {
            if let Ok(text) = std::fs::read_to_string(&p) {
                if let Ok(ex) = toml::from_str::<ExercisePackage>(&text) {
                    out.push(ex);
                }
            }
        }
    }
    out
}

pub fn load_exercise(data_dir: &std::path::Path, id: &str) -> Option<ExercisePackage> {
    load_exercises(data_dir).into_iter().find(|e| e.exercise.id == id)
}
