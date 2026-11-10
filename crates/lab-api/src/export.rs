//! 离线课程包导出(P7 第二批):单文件自包含 HTML。
//!
//! 数据全部内嵌(课程 JSON + 习题 + 知识节点),零网络依赖,file:// 直接可开;
//! 实验源码以内嵌 <pre> 呈现(离线包定位 = 阅读与复习;动手实验回平台)。
//! README 原则:内容是结构化数据的只读投影,不产生第二份真相。

use crate::api::{ApiError, AppState};
use axum::extract::{Path, Query, State};
use lab_judge::load_exercises;
use serde::Deserialize;

/// 离线包默认**不含**参考解析(否则离线文件本身就是答案表)。
/// 需要复习答案时显式带 ?include_answers=true 导出。
#[derive(Debug, Default, Deserialize)]
pub struct ExportQuery {
    #[serde(default)]
    pub include_answers: bool,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn render_course_html(
    v: &toml::Value,
    exercises: &[lab_judge::ExercisePackage],
    include_answers: bool,
) -> String {
    // toml Index 对缺键 panic:统一 .get()
    let empty = toml::Value::Table(Default::default());
    let get = |v: &toml::Value, path: &[&str]| -> String {
        let mut cur = v;
        for k in path {
            cur = match cur.get(k) {
                Some(next) => next,
                None => &empty,
            };
        }
        cur.as_str().unwrap_or("").to_string()
    };
    let title = get(v, &["lesson", "title"]);
    let stage = v.get("lesson").and_then(|l| l.get("stage")).and_then(|x| x.as_integer()).unwrap_or(0);
    let problem = get(v, &["lesson", "goal", "problem"]);
    let exp_src = get(v, &["lesson", "experiment", "source"]);
    let mode = get(v, &["lesson", "experiment", "mode"]);

    let mut steps_html = String::new();
    if let Some(steps) = v["steps"].as_array() {
        for (i, s) in steps.iter().enumerate() {
            let kind = s.get("kind").and_then(|x| x.as_str()).unwrap_or("");
            let stitle = s.get("title").and_then(|x| x.as_str()).unwrap_or("");
            let content = s.get("content").and_then(|x| x.as_str()).unwrap_or("");
            steps_html.push_str(&format!(
                "<section class='step'><div class='k'>{}</div><h3>{}. {}</h3><pre class='c'>{}</pre></section>",
                esc(kind),
                i + 1,
                esc(stitle),
                esc(content)
            ));
        }
    }

    let mut ex_html = String::new();
    for e in exercises {
        let lesson_id = v.get("lesson").and_then(|l| l.get("id")).and_then(|x| x.as_str()).unwrap_or("");
        if e.exercise.lesson != lesson_id {
            continue;
        }
        let head = format!(
            "<section class='ex'><h4>习题:{}({})</h4>",
            esc(&e.exercise.title),
            esc(&e.exercise.kind)
        );
        if include_answers && !e.exercise.answer_md.is_empty() {
            ex_html.push_str(&format!(
                "{}<details><summary>参考解析</summary><pre class='c'>{}</pre></details></section>",
                head,
                esc(&e.exercise.answer_md)
            ));
        } else {
            ex_html.push_str(&format!(
                "{}<p class='meta'>参考解析请在平台内主动查看(离线包默认不含答案)。</p></section>",
                head
            ));
        }
    }

    format!(
        r#"<!DOCTYPE html><html lang="zh"><head><meta charset="utf-8">
<title>{title} — Assembly Lab 离线课程包</title>
<style>
body{{background:#141414;color:#cfcfcf;font-family:'Cascadia Code',Consolas,monospace;max-width:880px;margin:0 auto;padding:24px;line-height:1.7}}
h1{{color:#63e2b7}} .k{{display:inline-block;border:1px solid #63e2b766;color:#63e2b7;font-size:12px;border-radius:3px;padding:1px 6px;margin-bottom:4px}}
.step{{border:1px solid #2a2a2a;border-radius:6px;padding:10px 16px;margin:12px 0;background:#181818}}
.ex{{border:1px dashed #2a4a3a;border-radius:6px;padding:10px 16px;margin:12px 0}}
pre{{white-space:pre-wrap}}
.c{{background:#111;border:1px solid #222;border-radius:4px;padding:10px 14px;color:#9ad1a8;font-size:13px}}
.meta{{color:#888;font-size:13px}}
details summary{{cursor:pointer;color:#7ab0e0}}
footer{{color:#555;font-size:12px;margin-top:30px}}
</style></head><body>
<h1>{title}</h1>
<div class="meta">阶段 {stage} · {problem}</div>
<h2>实验源码(模式:{mode})</h2>
<pre class="c">{exp}</pre>
<h2>课程步骤</h2>
{steps}
{ex}
<footer>Assembly Lab 离线课程包 · 由数据文件导出 · 动手实验请回平台(真实引擎)</footer>
</body></html>"#,
        title = esc(&title),
        stage = stage,
        problem = esc(&problem),
        mode = esc(&mode),
        exp = esc(&exp_src),
        steps = steps_html,
        ex = ex_html,
    )
}

/// GET /api/export/course/{id} — 单门课离线 HTML(附件下载)
pub async fn export_course(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ExportQuery>,
) -> Result<axum::response::Response, ApiError> {
    let path = st.data_dir().join("courses").join(format!("{id}.toml"));
    let text = std::fs::read_to_string(&path)
        .map_err(|_| ApiError(lab_core::LabError::BadRequest(format!("课程 {id} 不存在"))))?;
    let v: toml::Value = toml::from_str(&text)
        .map_err(|e| ApiError(lab_core::LabError::BadRequest(format!("解析失败: {e}"))))?;
    let exercises = load_exercises(st.data_dir());
    let html = render_course_html(&v, &exercises, q.include_answers);
    use axum::http::header;
    let resp = axum::response::Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{id}.html\""))
        .body(axum::body::Body::from(html))
        .map_err(|e| ApiError(lab_core::LabError::Io(e.to_string())))?;
    Ok(resp)
}

/// GET /api/export/all — 全部课程合集(单 HTML 目录 + 内嵌各课正文)
pub async fn export_all(
    State(st): State<AppState>,
    Query(q): Query<ExportQuery>,
) -> Result<axum::response::Response, ApiError> {
    let dir = st.data_dir().join("courses");
    let exercises = load_exercises(st.data_dir());
    let mut toc = String::new();
    let mut bodies = String::new();
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| ApiError(lab_core::LabError::Io(e.to_string())))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "toml").unwrap_or(false))
        .collect();
    paths.sort();
    for p in &paths {
        if let Ok(text) = std::fs::read_to_string(p) {
            if let Ok(v) = toml::from_str::<toml::Value>(&text) {
                let empty = toml::Value::Table(Default::default());
                let get = |v: &toml::Value, path: &[&str]| -> String {
                    let mut cur = v;
                    for k in path {
                        cur = match cur.get(k) {
                            Some(next) => next,
                            None => &empty,
                        };
                    }
                    cur.as_str().unwrap_or("").to_string()
                };
                let id = get(&v, &["lesson", "id"]);
                let title = get(&v, &["lesson", "title"]);
                let stage = v.get("lesson").and_then(|l| l.get("stage")).and_then(|x| x.as_integer()).unwrap_or(0);
                toc.push_str(&format!("<li><a href=\"#c{}\">[阶段 {}] {}</a></li>", esc(&id), stage, esc(&title)));
                bodies.push_str(&format!("<div id=\"c{}\">{}</div><hr/>", esc(&id), render_course_html(&v, &exercises, q.include_answers)));
            }
        }
    }
    let html = format!(
        r#"<!DOCTYPE html><html lang="zh"><head><meta charset="utf-8"><title>Assembly Lab 离线课程包(全)</title>
<style>body{{background:#141414;color:#cfcfcf;font-family:Consolas,monospace;max-width:960px;margin:0 auto;padding:24px;line-height:1.7}} a{{color:#7ab0e0}} hr{{border-color:#2a2a2a}}</style>
</head><body><h1>Assembly Lab 离线课程包</h1><ol>{toc}</ol><hr/>{bodies}
<footer>由数据文件导出 · 动手实验请回平台(真实引擎)</footer></body></html>"#
    );
    use axum::http::header;
    let resp = axum::response::Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CONTENT_DISPOSITION, "attachment; filename=\"assemblylab-offline.html\"")
        .body(axum::body::Body::from(html))
        .map_err(|e| ApiError(lab_core::LabError::Io(e.to_string())))?;
    Ok(resp)
}
