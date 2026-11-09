//! 知识图谱 / 掌握度 / 错题本(P7 第一批)。
//!
//! 图谱 = data/knowledge/nodes.toml(静态数据);掌握度 = 习题 attempts × 节点引用
//! 实时聚合;错题本 = 每题最近一次作答为 wrong/revealed 的题目。

use crate::api::{ApiError, ApiResult, AppState};
use axum::extract::State;
use axum::Json;
use lab_judge::load_exercises;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
pub struct KnowledgeNode {
    pub id: String,
    pub title: String,
    pub stage: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<String>,
}

#[derive(Serialize)]
pub struct KnowledgeGraph {
    pub nodes: Vec<KnowledgeNode>,
    pub edges: Vec<[String; 2]>,
    /// 节点 → 掌握度(通过题数/总题数/平均尝试)
    pub mastery: HashMap<String, NodeMastery>,
}

#[derive(Serialize)]
pub struct NodeMastery {
    pub total: u32,
    pub passed: u32,
    pub attempts: u32,
}

/// 读取知识图谱数据(节点 + 共现边;tutor 复用)
pub fn load_graph(data_dir: &std::path::Path) -> Result<(Vec<KnowledgeNode>, Vec<[String; 2]>), lab_core::LabError> {
    let path = data_dir.join("knowledge").join("nodes.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|_| lab_core::LabError::BadRequest("knowledge 数据不存在".into()))?;
    let v: toml::Value = toml::from_str(&text)
        .map_err(|e| lab_core::LabError::BadRequest(format!("解析失败: {e}")))?;
    build_graph(&v)
}

pub async fn get_knowledge(State(st): State<AppState>) -> ApiResult<KnowledgeGraph> {
    let (nodes, edges) = load_graph(st.data_dir()).map_err(ApiError)?;

    // 掌握度聚合
    let exercises = load_exercises(st.data_dir());
    let mut ex_node: HashMap<String, Vec<String>> = HashMap::new();
    for e in &exercises {
        ex_node.insert(e.exercise.id.clone(), e.exercise.knowledge.clone());
    }
    let mut agg: HashMap<String, (u32, u32, u32)> = HashMap::new(); // total, passed, attempts
    if let Some(storage) = st.storage() {
        for a in storage.latest_attempts() {
            if let Some(nodes) = ex_node.get(&a.exercise_id) {
                for node in nodes {
                    let e = agg.entry(node.clone()).or_insert((0, 0, 0));
                    e.0 += 1;
                    e.2 += a.attempt_no.max(0) as u32;
                    if a.status == "passed" {
                        e.1 += 1;
                    }
                }
            }
        }
    }
    let mastery = agg
        .into_iter()
        .map(|(k, (total, passed, attempts))| {
            (k, NodeMastery { total, passed, attempts })
        })
        .collect();

    Ok(Json(KnowledgeGraph { nodes, edges, mastery }))
}

#[derive(Serialize)]
pub struct MistakeItem {
    pub exercise_id: String,
    pub title: String,
    pub kind: String,
    pub stage: u32,
    pub status: String,
    pub attempt_no: i64,
    pub knowledge: Vec<String>,
}

pub async fn get_mistakes(State(st): State<AppState>) -> ApiResult<Vec<MistakeItem>> {
    let exercises = load_exercises(st.data_dir());
    let mut out = Vec::new();
    if let Some(storage) = st.storage() {
        for a in storage.latest_attempts() {
            if a.status == "passed" || a.status == "not_attempted" {
                continue; // 错题本:最近一次未通过
            }
            if let Some(e) = exercises.iter().find(|e| e.exercise.id == a.exercise_id) {
                out.push(MistakeItem {
                    exercise_id: e.exercise.id.clone(),
                    title: e.exercise.title.clone(),
                    kind: e.exercise.kind.clone(),
                    stage: e.exercise.stage,
                    status: a.status.clone(),
                    attempt_no: a.attempt_no,
                    knowledge: e.exercise.knowledge.clone(),
                });
            }
        }
    }
    Ok(Json(out))
}

fn build_graph(v: &toml::Value) -> Result<(Vec<KnowledgeNode>, Vec<[String; 2]>), lab_core::LabError> {
    let mut nodes = Vec::new();
    if let Some(arr) = v["node"].as_array() {
        for n in arr {
            // toml Index 对缺键 panic:一律 .get()
            nodes.push(KnowledgeNode {
                id: n.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                title: n.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                stage: n.get("stage").and_then(|x| x.as_integer()).unwrap_or(0) as u32,
                desc: n.get("desc").and_then(|x| x.as_str()).map(String::from),
                related: n
                    .get("related")
                    .and_then(|x| x.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
            });
        }
    }
    let mut edges = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for n in &nodes {
        for r in &n.related {
            let key = if n.id < *r { (n.id.clone(), r.clone()) } else { (r.clone(), n.id.clone()) };
            if seen.insert(key.clone()) {
                edges.push([key.0, key.1]);
            }
        }
    }
    Ok((nodes, edges))
}
