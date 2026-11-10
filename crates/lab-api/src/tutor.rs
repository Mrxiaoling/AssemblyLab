//! AI 助教(P7 第二批)—— 本地检索式引擎。
//!
//! README 第九节:AI 必须基于实验引擎/编译器/调试器提供的真实信息,不能凭空编造。
//! 本实现 = **确定性检索**:知识源全部来自仓库内数据(ISA instruction_notes、
//! 课程步骤与解析、习题 answer_md、知识图谱共现、掌握度),零模型、零网络、零编造;
//! 回答按三层组织(30 秒版 / 深入版 / 动手版),附盲点分析与下一步推荐。
//!
//! 可选外部挂点:设置环境变量 ASSEMBLYLAB_AI_ENDPOINT 后,前端可把同一请求
//! 转发给任意 LLM 服务(本后端不做转发,保持离线纯净;是否启用由用户决定)。

use crate::api::{ApiError, ApiResult, AppState};
use axum::extract::State;
use axum::Json;
use lab_judge::load_exercises;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct TutorReq {
    pub question: String,
    /// 可选:当前课程(限定知识源范围)
    #[serde(default)]
    pub lesson_id: Option<String>,
}

#[derive(Serialize)]
pub struct TutorResp {
    pub engine: &'static str,
    pub matched_nodes: Vec<String>,
    pub layers: Layers,
    /// 关联知识点(图谱共现)
    pub related_nodes: Vec<String>,
    /// 关联课程(id+标题)
    pub related_lessons: Vec<(String, String)>,
    /// 关联习题(含解析可见性)
    pub related_exercises: Vec<(String, String)>,
    /// 盲点分析:做过未过的知识点(来自真实作答记录)
    pub blindspots: Vec<String>,
    /// 推荐下一步
    pub recommended: Vec<(String, String)>,
}

#[derive(Serialize)]
pub struct Layers {
    /// 30 秒版:一句话 + 关键指令摘要
    pub quick: String,
    /// 深入版:ISA note 的 summary/pitfalls + 课程要点
    pub deep: Vec<String>,
    /// 动手版:去哪个实验、哪道题
    pub hands_on: Vec<String>,
}

pub async fn tutor(State(st): State<AppState>, Json(req): Json<TutorReq>) -> ApiResult<TutorResp> {
    let q = req.question.trim().to_string();
    if q.is_empty() {
        return Err(ApiError(lab_core::LabError::BadRequest("问题为空".into())));
    }
    // 分词:按非字母数字汉字切 + 2-gram 汉字兜底
    let tokens = tokenize(&q);

    // ---- 知识源 1:ISA notes(两架构) ----
    let mut isa_notes: Vec<(String, String, String, String)> = Vec::new(); // isa, mnemonic, summary, pitfalls-join
    for isa_id in ["x86-64", "arm64"] {
        if let Ok(isa) = st.load_isa(isa_id) {
            for n in &isa.instruction_notes {
                isa_notes.push((
                    isa_id.to_string(),
                    n.mnemonic.clone(),
                    n.summary.clone(),
                    n.pitfalls.join(" "),
                ));
            }
        }
    }
    // ---- 知识源 2:课程(标题/问题/steps 文本) ----
    let courses_dir = st.data_dir().join("courses");
    let mut course_texts: Vec<(String, String, String)> = Vec::new(); // id, title, haystack
    if let Ok(rd) = std::fs::read_dir(&courses_dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().map(|x| x != "toml").unwrap_or(true) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&p) {
                if let Ok(v) = toml::from_str::<toml::Value>(&text) {
                    // toml Index 对缺键 panic(第 4 次):全部 .get()
                    fn get(v: &toml::Value, k: &str) -> String { v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string() }
                    let id = get(&v["lesson"], "id");
                    let title = get(&v["lesson"], "title");
                    let mut hay = title.clone();
                    hay.push(' ');
                    hay.push_str(v["lesson"].get("goal").and_then(|g| g.get("problem")).and_then(|x| x.as_str()).unwrap_or(""));
                    for s in v.get("steps").and_then(|x| x.as_array()).map(|a| a.as_slice()).unwrap_or(&[]) {
                        hay.push(' ');
                        hay.push_str(&get(s, "title"));
                        hay.push(' ');
                        hay.push_str(&get(s, "content"));
                    }
                    course_texts.push((id, title, hay));
                }
            }
        }
    }
    // ---- 知识源 3:习题解析 ----
    let exercises = tokio::task::spawn_blocking({
        let dir = st.data_dir().to_path_buf();
        move || load_exercises(&dir)
    })
    .await
    .map_err(|e| ApiError(lab_core::LabError::Emulator(e.to_string())))?;

    // ---- 打分:token 命中计数 ----
    let score_text = |hay: &str| -> usize {
        let hay_l = hay.to_lowercase();
        tokens.iter().filter(|t| hay_l.contains(t.as_str())).count()
    };

    let mut note_hits: Vec<(usize, &_)> = isa_notes
        .iter()
        .map(|n| (score_text(&n.2) + score_text(&n.1) * 2 + score_text(&n.3), n))
        .filter(|(s, _)| *s > 0)
        .collect();
    note_hits.sort_by(|a, b| b.0.cmp(&a.0));

    let mut course_hits: Vec<(usize, &(String, String, String))> = course_texts
        .iter()
        .map(|c| (score_text(&c.2), c))
        .filter(|(s, _)| *s > 0)
        .collect();
    course_hits.sort_by(|a, b| b.0.cmp(&a.0));

    let mut ex_hits: Vec<(usize, &lab_judge::ExercisePackage)> = exercises
        .iter()
        .map(|e| {
            let hay = format!("{} {} {}", e.exercise.title, e.exercise.answer_md, e.exercise.knowledge.join(" "));
            (score_text(&hay), e)
        })
        .filter(|(s, _)| *s > 0)
        .collect();
    ex_hits.sort_by(|a, b| b.0.cmp(&a.0));

    // ---- 匹配的知识节点(标题命中) ----
    let (kg_nodes, _) = super::knowledge::load_graph(st.data_dir()).map_err(ApiError)?;
    let mut node_hits: Vec<(usize, &super::knowledge::KnowledgeNode)> = kg_nodes
        .iter()
        .map(|n| {
            let s = score_text(&n.title) * 2 + score_text(&n.id);
            (s, n)
        })
        .filter(|(s, _)| *s > 0)
        .collect();
    node_hits.sort_by(|a, b| b.0.cmp(&a.0));
    let matched_nodes: Vec<String> =
        node_hits.iter().take(6).map(|(_, n)| format!("{}({})", n.title, n.id)).collect();

    // ---- 三层回答 ----
    let quick = if let Some((_, n)) = node_hits.first() {
        format!("关于「{}」:{}。{}", n.title, n.desc.as_deref().unwrap_or("见下方深入要点"), quick_hint(&tokens, &note_hits))
    } else if let Some((_, (_, _, hay))) = course_hits.first() {
        // 课程命中:摘第一步内容
        let snip: String = hay.chars().take(160).collect();
        format!("最相关的课程内容:{snip}…")
    } else {
        "未在本地知识库(ISA 注释/26 门课程/19 道习题解析)中命中关键词。试试用指令名(mov/lock/svc…)或概念(虚表/协程帧/端序…)提问。".into()
    };

    let mut deep: Vec<String> = Vec::new();
    for (_, (isa, mn, summary, pitfalls)) in note_hits.iter().take(4) {
        deep.push(format!("[{isa}] {mn}:{summary}"));
        if !pitfalls.is_empty() {
            deep.push(format!("  ⚠ 陷阱:{pitfalls}"));
        }
    }
    for (_, (id, title, _)) in course_hits.iter().take(2) {
        deep.push(format!("相关课程:{title}(id: {id})——学习中心可打开逐步学习"));
    }
    if deep.is_empty() {
        deep.push("(无更多命中)".into());
    }

    let mut hands_on: Vec<String> = Vec::new();
    for e in ex_hits.iter().take(3).map(|(_, e)| *e) {
        hands_on.push(format!("习题「{}」({})——做一遍即验证", e.exercise.title, e.exercise.id));
    }
    for (_, (_, title, _)) in course_hits.iter().take(2) {
        hands_on.push(format!("课程实验:{title}(学习中心 → 打开本课实验)"));
    }
    if hands_on.is_empty() {
        hands_on.push("在实验工作台粘贴相关汇编逐条单步——一切以真实执行为准。".into());
    }

    // ---- 盲点:做过未过的知识节点 ----
    let mut blindspots: Vec<String> = Vec::new();
    let ex_node: HashMap<String, Vec<String>> =
        exercises.iter().map(|e| (e.exercise.id.clone(), e.exercise.knowledge.clone())).collect();
    if let Some(storage) = st.storage() {
        let mut bad: HashMap<String, u32> = HashMap::new();
        for a in storage.latest_attempts() {
            if a.status != "passed" {
                if let Some(nodes) = ex_node.get(&a.exercise_id) {
                    for n in nodes {
                        *bad.entry(n.clone()).or_insert(0) += 1;
                    }
                }
            }
        }
        let mut v: Vec<(String, u32)> = bad.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        for (n, c) in v.iter().take(3) {
            let title = kg_nodes.iter().find(|x| x.id == *n).map(|x| x.title.clone()).unwrap_or_else(|| n.clone());
            blindspots.push(format!("{title}(最近作答未通过 {c} 次)——建议回到对应课程/实验"));
        }
    }

    // ---- 推荐下一步:未做题 top3(按难度升序) ----
    let done: std::collections::HashSet<String> = if let Some(storage) = st.storage() {
        storage.latest_attempts().into_iter().map(|a| a.exercise_id).collect()
    } else {
        Default::default()
    };
    let mut recommended: Vec<(String, String)> = exercises
        .iter()
        .filter(|e| !done.contains(&e.exercise.id))
        .take(3)
        .map(|e| (e.exercise.id.clone(), e.exercise.title.clone()))
        .collect();

    // 限定 lesson 时把该课习题排前
    if let Some(lid) = &req.lesson_id {
        recommended.sort_by_key(|(id, _)| {
            let in_lesson = exercises
                .iter()
                .find(|e| &e.exercise.id == id)
                .map(|e| &e.exercise.lesson == lid)
                .unwrap_or(false);
            if in_lesson { 0 } else { 1 }
        });
    }

    Ok(Json(TutorResp {
        engine: "local-retrieval(确定性检索:数据源 = ISA 注释/课程/习题解析/知识图谱,零编造)",
        matched_nodes,
        layers: Layers { quick, deep, hands_on },
        related_nodes: kg_nodes
            .iter()
            .filter(|n| node_hits.iter().take(3).any(|(_, h)| h.related.contains(&n.id)))
            .take(6)
            .map(|n| format!("{}({})", n.title, n.id))
            .collect(),
        related_lessons: course_hits.iter().take(3).map(|(_, (id, t, _))| (id.clone(), t.clone())).collect(),
        related_exercises: ex_hits.iter().take(3).map(|(_, e)| (e.exercise.id.clone(), e.exercise.title.clone())).collect(),
        blindspots,
        recommended,
    }))
}

fn quick_hint(tokens: &[String], note_hits: &[(usize, &(String, String, String, String))]) -> String {
    if let Some((_, (_, mn, summary, _))) = note_hits.first() {
        let _ = tokens;
        format!("核心指令 {mn}:{summary}")
    } else {
        String::new()
    }
}

/// 分词:ASCII 词元 + 汉字 2-gram(检索用)
fn tokenize(q: &str) -> Vec<String> {
    let lower = q.to_lowercase();
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in lower.chars() {
        if c.is_ascii_alphanumeric() {
            cur.push(c);
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            if !c.is_ascii_whitespace() {
                // 汉字等:单独 + 与前一字组成 2-gram
                out.push(c.to_string());
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    // 汉字 2-gram
    let chars: Vec<char> = lower.chars().collect();
    for w in chars.windows(2) {
        if w[0] as u32 > 0x2e7f && w[1] as u32 > 0x2e7f {
            out.push(w.iter().collect());
        }
    }
    out.sort();
    out.dedup();
    out
}
