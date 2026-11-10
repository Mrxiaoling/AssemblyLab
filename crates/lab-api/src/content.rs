//! 课程 / 习题 / 进度路由(P1)。

use crate::api::{ApiError, ApiResult, AppState};
use axum::extract::{Path, State};
use axum::Json;
use lab_judge::{load_exercise, load_exercises, Judge, SubmitReq};
use serde::Serialize;

// ---------------------------------------------------------------------------
// 课程
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct LessonListItem {
    pub id: String,
    pub title: String,
    pub stage: u32,
    pub requires: Vec<String>,
    pub minutes: u32,
    pub progress: String,
}

#[derive(Serialize)]
pub struct LessonDetail {
    pub id: String,
    pub title: String,
    pub stage: u32,
    pub problem: String,
    pub steps: Vec<toml::Value>,
    pub experiment: Option<toml::Value>,
    pub progress: String,
    pub exercises: Vec<ExerciseBrief>,
}

#[derive(Serialize)]
pub struct ExerciseBrief {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub difficulty: u32,
}

fn lesson_path(st: &AppState, id: &str) -> std::path::PathBuf {
    st.data_dir().join("courses").join(format!("{id}.toml"))
}

pub async fn list_courses(State(st): State<AppState>) -> ApiResult<Vec<LessonListItem>> {
    let dir = st.data_dir().join("courses");
    let mut items = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut paths: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "toml").unwrap_or(false))
            .collect();
        paths.sort();
        for p in paths {
            if let Ok(Some(text)) = tokio::task::spawn_blocking(move || std::fs::read_to_string(p).ok())
                .await
            {
                if let Ok(v) = toml::from_str::<toml::Value>(&text) {
                    let id = v["lesson"]["id"].as_str().unwrap_or("").to_string();
                    let progress = st
                        .storage()
                        .and_then(|s| s.get_progress(&id))
                        .map(|p| p.status)
                        .unwrap_or_else(|| "not_started".into());
                    items.push(LessonListItem {
                        id,
                        title: v["lesson"]["title"].as_str().unwrap_or("").to_string(),
                        stage: v["lesson"]["stage"].as_integer().unwrap_or(0) as u32,
                        requires: v["lesson"]["requires"]
                            .as_array()
                            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                            .unwrap_or_default(),
                        minutes: v["lesson"]["estimated_minutes"].as_integer().unwrap_or(0) as u32,
                        progress,
                    });
                }
            }
        }
    }
    Ok(Json(items))
}

pub async fn get_course(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<LessonDetail> {
    let path = lesson_path(&st, &id);
    let text = std::fs::read_to_string(&path).map_err(|_| {
        ApiError(lab_core::LabError::BadRequest(format!("课程 {id} 不存在")))
    })?;
    let v: toml::Value = toml::from_str(&text).map_err(|e| {
        ApiError(lab_core::LabError::BadRequest(format!("课程解析失败: {e}")))
    })?;
    let progress = st
        .storage()
        .and_then(|s| s.get_progress(&id))
        .map(|p| p.status)
        .unwrap_or_else(|| "not_started".into());
    // 关联习题
    let exercises = load_exercises(st.data_dir())
        .into_iter()
        .filter(|e| e.exercise.lesson == id)
        .map(|e| ExerciseBrief {
            id: e.exercise.id,
            title: e.exercise.title,
            kind: e.exercise.kind,
            difficulty: e.exercise.difficulty,
        })
        .collect();
    let lesson_tbl = &v["lesson"];
    Ok(Json(LessonDetail {
        id: lesson_tbl["id"].as_str().unwrap_or(&id).to_string(),
        title: lesson_tbl["title"].as_str().unwrap_or("").to_string(),
        stage: lesson_tbl["stage"].as_integer().unwrap_or(0) as u32,
        problem: lesson_tbl["goal"]["problem"].as_str().unwrap_or("").to_string(),
        steps: v["steps"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
        experiment: v["lesson"]["experiment"].as_table().map(|_| v["lesson"]["experiment"].clone()),
        progress,
        exercises,
    }))
}

#[derive(Serialize)]
pub struct CourseSource {
    pub isa: String,
    pub source: String,
    pub watch: Vec<String>,
    /// 实验入口:asm=模式A工作台 | debug=真机调试 | compare=优化级别对比
    pub mode: String,
    /// 建议编译器(协程课用 clang++,其余默认 g++)
    pub compiler: String,
}

/// 实验源码(供前端「打开实验」用)
pub async fn get_course_experiment(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<CourseSource> {
    let path = lesson_path(&st, &id);
    let text = std::fs::read_to_string(&path)
        .map_err(|_| ApiError(lab_core::LabError::BadRequest("课程不存在".into())))?;
    let v: toml::Value = toml::from_str(&text)
        .map_err(|e| ApiError(lab_core::LabError::BadRequest(format!("解析失败: {e}"))))?;
    // toml::Value 的 [] 索引对缺键会 panic(P3 教训):统一用 .get()
    let exp = v["lesson"].get("experiment");
    let empty = toml::Value::Table(Default::default());
    let exp = exp.unwrap_or(&empty);
    let get_str = |k: &str, dflt: &str| exp.get(k).and_then(|x| x.as_str()).unwrap_or(dflt).to_string();
    let mode = get_str("mode", "asm");
    // C++ 源码课程漏写 mode 时按 debug 兜底(源码含 int main 特征)
    let source = get_str("source", "");
    let mode = if mode == "asm" && source.contains("int main(") {
        "debug".to_string()
    } else {
        mode
    };
    Ok(Json(CourseSource {
        isa: get_str("isa", "x86-64"),
        source,
        compiler: get_str("compiler", "g++"),
        watch: exp
            .get("watch")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        mode,
    }))
}

// ---------------------------------------------------------------------------
// 习题
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ExerciseListItem {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub stage: u32,
    pub lesson: String,
    pub difficulty: u32,
}

pub async fn list_exercises(
    State(st): State<AppState>,
) -> ApiResult<Vec<ExerciseListItem>> {
    let list = tokio::task::spawn_blocking({
        let dir = st.data_dir().to_path_buf();
        move || load_exercises(&dir)
    })
    .await
    .map_err(|e| ApiError(lab_core::LabError::Emulator(e.to_string())))?;
    Ok(Json(list
        .into_iter()
        .map(|e| ExerciseListItem {
            id: e.exercise.id,
            title: e.exercise.title,
            kind: e.exercise.kind,
            stage: e.exercise.stage,
            lesson: e.exercise.lesson,
            difficulty: e.exercise.difficulty,
        })
        .collect()))
}

#[derive(Serialize)]
pub struct ExerciseDetail {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub difficulty: u32,
    pub knowledge: Vec<String>,
    pub lesson: String,
    /// predict: 程序 + 问题列表;fill: 模板 + 提示
    pub predict: Option<serde_json::Value>,
    pub fill_blank: Option<serde_json::Value>,
    pub quiz: Option<serde_json::Value>,
    pub self_check: Option<serde_json::Value>,
    pub attempt_count: i64,
    pub status: String, // not_attempted | wrong | passed | revealed
}

pub async fn get_exercise(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ExerciseDetail> {
    let ex = tokio::task::spawn_blocking({
        let dir = st.data_dir().to_path_buf();
        let id = id.clone();
        move || load_exercise(&dir, &id)
    })
    .await
    .map_err(|e| ApiError(lab_core::LabError::Emulator(e.to_string())))?
    .ok_or_else(|| ApiError(lab_core::LabError::BadRequest("习题不存在".into())))?;
    let attempts = st
        .storage()
        .map(|s| s.attempts(&ex.exercise.id))
        .unwrap_or_default();
    let status = attempts
        .last()
        .map(|a| a.status.clone())
        .unwrap_or_else(|| "not_attempted".into());
    Ok(Json(ExerciseDetail {
        id: ex.exercise.id,
        kind: ex.exercise.kind.clone(),
        title: ex.exercise.title,
        difficulty: ex.exercise.difficulty,
        knowledge: ex.exercise.knowledge.clone(),
        lesson: ex.exercise.lesson.clone(),
        predict: ex.predict.map(|p| serde_json::to_value(p).unwrap_or_default()),
        // fill_blank/quiz 的判题依据(testcases 的期望值/quiz 的参考答案)不得泄漏给前端:
        // 只下发题面结构,期望值由判题器在提交时以真实执行/比对得出
        fill_blank: ex.fill_blank.map(|p| {
            let mut v = serde_json::to_value(p).unwrap_or_default();
            if let Some(t) = v.get_mut("testcases").and_then(|x| x.as_array_mut()) {
                for tc in t {
                    tc.as_object_mut().map(|o| {
                        o.remove("expect_mem");
                        o.remove("expect_exit_code");
                        o.remove("expect_output");
                    });
                }
            }
            v
        }),
        quiz: ex.quiz.map(|p| {
            let mut v = serde_json::to_value(p).unwrap_or_default();
            if let Some(qs) = v.get_mut("questions").and_then(|x| x.as_array_mut()) {
                for q in qs {
                    q.as_object_mut().map(|o| {
                        o.remove("expect");
                    });
                }
            }
            v
        }),
        self_check: ex.self_check.map(|p| serde_json::to_value(p).unwrap_or_default()),
        attempt_count: attempts.len() as i64,
        status,
    }))
}

/// 提交反馈的公开 DTO:刻意不含 `expected`(标准答案/期望执行结果)。
/// 判题依据只留在服务端,避免「随便提交一次就能从响应里读到答案」。
#[derive(Serialize)]
pub struct PublicJudgeItem {
    pub key: String,
    pub prompt: String,
    pub actual: String,
    pub ok: bool,
}

#[derive(Serialize)]
pub struct SubmitResp {
    pub passed: bool,
    pub items: Vec<PublicJudgeItem>,
    pub attempt_no: i64,
    pub error: Option<String>,
    /// 提示梯度:第 1 次错误给 level1,第 2 次+给 level2;不通过时附上
    pub hint: Option<String>,
}

/// 用户主动点「查看参考解析」时才返回;提交响应永不包含答案。
#[derive(Serialize)]
pub struct AnswerResp {
    pub answer_md: Option<String>,
}

pub async fn get_exercise_answer(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<AnswerResp> {
    let ex = tokio::task::spawn_blocking({
        let dir = st.data_dir().to_path_buf();
        let id = id.clone();
        move || load_exercise(&dir, &id)
    })
    .await
    .map_err(|e| ApiError(lab_core::LabError::Emulator(e.to_string())))?
    .ok_or_else(|| ApiError(lab_core::LabError::BadRequest("习题不存在".into())))?;
    Ok(Json(AnswerResp {
        answer_md: Some(ex.exercise.answer_md.clone()).filter(|s| !s.is_empty()),
    }))
}

pub async fn submit_exercise(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SubmitReq>,
) -> ApiResult<SubmitResp> {
    let ex = tokio::task::spawn_blocking({
        let dir = st.data_dir().to_path_buf();
        let id = id.clone();
        move || load_exercise(&dir, &id)
    })
    .await
    .map_err(|e| ApiError(lab_core::LabError::Emulator(e.to_string())))?
    .ok_or_else(|| ApiError(lab_core::LabError::BadRequest("习题不存在".into())))?;

    let isa = st
        .load_isa(if ex.exercise.isa.is_empty() { "x86-64" } else { &ex.exercise.isa })
        .map_err(ApiError)?;
    let ex = std::sync::Arc::new(ex);
    let answers = req.answers.clone();
    let judged = tokio::task::spawn_blocking({
        let ex = ex.clone();
        move || {
            let judge = Judge::new(isa);
            judge.judge(&ex, &answers)
        }
    })
    .await
    .map_err(|e| ApiError(lab_core::LabError::Emulator(e.to_string())))?;

    // 自评题(诚实模式)没有机器可验证的判题依据,不计入掌握度、不触发进度更新。
    let verified = ex.self_check.is_none();
    let status = if !judged.passed {
        "wrong"
    } else if verified {
        "passed"
    } else {
        "self_check"
    };
    let ex_id = ex.exercise.id.clone();
    let attempt_no = st
        .storage()
        .and_then(|s| {
            s.record_attempt(
                &ex_id,
                status,
                &serde_json::to_string(&req.answers).unwrap_or_default(),
                &serde_json::to_string(&judged).unwrap_or_default(),
            )
            .ok()
        })
        .unwrap_or(1);

    let hint = if judged.passed {
        None
    } else if attempt_no <= 1 {
        Some(ex.exercise.hints.level1.clone()).filter(|s| !s.is_empty())
    } else {
        Some(ex.exercise.hints.level2.clone()).filter(|s| !s.is_empty())
    };
    // 参考解析不再随提交返回:由用户主动调用 /api/exercises/{id}/answer 获取。
    // (README 反馈机制:第一次错误给方向性提示,用户主动看答案才给全解)

    if judged.passed && verified {
        let lesson = ex.exercise.lesson.clone();
        if let Some(s) = st.storage() {
            let _ = s.set_progress(&lesson, "in_progress");
        }
    }

    Ok(Json(SubmitResp {
        passed: judged.passed,
        items: judged
            .items
            .into_iter()
            .map(|i| PublicJudgeItem {
                key: i.key,
                prompt: i.prompt,
                actual: i.actual,
                ok: i.ok,
            })
            .collect(),
        attempt_no: judged.attempt_no.max(attempt_no),
        error: judged.error,
        hint,
    }))
}

// ---------------------------------------------------------------------------
// 进度
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ProgressResp {
    pub lesson_id: String,
    pub status: String,
}

pub async fn get_progress(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ProgressResp> {
    let p = st
        .storage()
        .and_then(|s| s.get_progress(&id))
        .unwrap_or(lab_storage::LessonProgress {
            lesson_id: id.clone(),
            status: "not_started".into(),
            updated_at: 0,
        });
    Ok(Json(ProgressResp { lesson_id: p.lesson_id, status: p.status }))
}

#[derive(serde::Deserialize)]
pub struct PutProgressReq {
    pub status: String,
}

pub async fn put_progress(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<PutProgressReq>,
) -> ApiResult<ProgressResp> {
    // 状态白名单:防任意值篡改进度
    const ALLOWED_STATUS: [&str; 3] = ["not_started", "in_progress", "done"];
    if !ALLOWED_STATUS.contains(&req.status.as_str()) {
        return Err(ApiError(lab_core::LabError::BadRequest(format!(
            "非法进度状态 {}(可选 not_started/in_progress/done)",
            req.status
        ))));
    }
    if let Some(s) = st.storage() {
        s.set_progress(&id, &req.status)
            .map_err(|e| ApiError(lab_core::LabError::Io(e)))?;
    }
    Ok(Json(ProgressResp { lesson_id: id, status: req.status }))
}


