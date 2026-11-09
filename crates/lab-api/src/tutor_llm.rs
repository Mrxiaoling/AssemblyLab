//! AI 助教外部 LLM 挂点(P8 可选)。
//!
//! README 第九节:AI 必须基于实验引擎/编译器/调试器提供的真实信息,不能凭空编造。
//! 本挂点把**本地检索到的真实上下文**(ISA 注释/课程要点/习题解析)注入 prompt,
//! 转发给用户配置的 OpenAI 兼容端点(ASSEMBLYLAB_AI_ENDPOINT/ASSEMBLYLAB_AI_KEY/ASSEMBLYLAB_AI_MODEL)。
//! 未配置时返回 503 与配置说明;本后端不做端点内置,离线纯净。

use crate::api::{ApiError, ApiResult, AppState};
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct LlmReq {
    pub question: String,
}

#[derive(Serialize)]
pub struct LlmResp {
    pub configured: bool,
    pub model: Option<String>,
    pub answer: Option<String>,
    pub config_hint: Option<String>,
}

/// 从本地知识库收集真实上下文(检索逻辑与 tutor 一致,简化版)
fn build_context(st: &AppState, question: &str) -> String {
    let tokens: Vec<String> = question
        .to_lowercase()
        .split(|c: char| c.is_ascii_whitespace() || c.is_ascii_punctuation())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();

    let mut ctx = String::from("以下是与问题相关的真实汇编教学数据:\n");

    // ISA notes(两架构)
    for isa_id in ["x86-64", "arm64"] {
        if let Ok(isa) = st.load_isa(isa_id) {
            for n in &isa.instruction_notes {
                let hay = format!("{} {} {}", n.mnemonic, n.summary, n.pitfalls.join(" ")).to_lowercase();
                if tokens.iter().any(|t| hay.contains(t.as_str())) {
                    ctx.push_str(&format!(
                        "[{} ISA] {} ({}):{}\n",
                        isa_id.to_uppercase(),
                        n.mnemonic,
                        n.category,
                        n.summary
                    ));
                }
            }
        }
    }
    ctx
}

pub async fn tutor_llm(
    State(st): State<AppState>,
    Json(req): Json<LlmReq>,
) -> ApiResult<LlmResp> {
    let endpoint = std::env::var("ASSEMBLYLAB_AI_ENDPOINT").ok();
    let key = std::env::var("ASSEMBLYLAB_AI_KEY").ok();
    let model = std::env::var("ASSEMBLYLAB_AI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());

    if endpoint.is_none() {
        return Ok(Json(LlmResp {
            configured: false,
            model: None,
            answer: None,
            config_hint: Some(
                "外部 LLM 未配置。设置环境变量 ASSEMBLYLAB_AI_ENDPOINT(OpenAI 兼容端点 URL)、\
                 ASSEMBLYLAB_AI_KEY(Bearer 密钥,可选)、ASSEMBLYLAB_AI_MODEL(模型名)后启用。"
                    .into(),
            ),
        }));
    }
    let endpoint = endpoint.unwrap();

    // 真实上下文注入(README:AI 必须基于真实信息)
    let context = build_context(&st, &req.question);
    let prompt = format!(
        "你是汇编语言教学助教。仅基于以下真实教学数据回答问题,不要编造寄存器状态或指令行为。\
         用中文回答,区分 x86-64 与 ARM64。\n\n{context}\n\n问题:{question}",
        context = context,
        question = req.question
    );

    // OpenAI 兼容 chat completions
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.3,
    });

    let client = reqwest::Client::new();
    let mut rb = client
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(60));
    if let Some(k) = &key {
        rb = rb.bearer_auth(k);
    }
    let resp = rb
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| ApiError(lab_core::LabError::ToolUnavailable(format!("LLM 请求失败: {e}"))))?;

    if !resp.status().is_success() {
        return Err(ApiError(lab_core::LabError::ToolUnavailable(format!(
            "LLM 端点返回 {}",
            resp.status()
        ))));
    }

    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ApiError(lab_core::LabError::ToolUnavailable(format!("LLM 响应解析失败: {e}"))))?;
    let answer = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("(LLM 未返回内容)")
        .to_string();

    Ok(Json(LlmResp {
        configured: true,
        model: Some(model),
        answer: Some(answer),
        config_hint: None,
    }))
}
