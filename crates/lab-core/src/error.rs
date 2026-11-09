//! 统一错误类型。API 层负责映射为 HTTP 状态码与 JSON。

use thiserror::Error;

#[derive(Debug, Error, Clone)]
pub enum LabError {
    #[error("汇编失败: 第 {line} 行: {message}")]
    Assemble { line: usize, message: String },

    #[error("访问了未映射的内存 ({kind} @ {addr:#x})")]
    UnmappedAccess { kind: String, addr: u64 },

    #[error("指令执行错误: {0}")]
    InstructionFault(String),

    #[error("会话不存在或已结束")]
    SessionNotFound,

    #[error("无效请求: {0}")]
    BadRequest(String),

    #[error("模拟器错误: {0}")]
    Emulator(String),

    #[error("工具不可用: {0}")]
    ToolUnavailable(String),

    #[error("ISA 未注册: {0}")]
    IsaNotFound(String),

    #[error("IO 错误: {0}")]
    Io(String),
}

impl LabError {
    /// 稳定的错误种类标识(API 错误响应使用)。
    pub fn kind(&self) -> &'static str {
        match self {
            LabError::Assemble { .. } => "assemble_error",
            LabError::UnmappedAccess { .. } => "unmapped_access",
            LabError::InstructionFault(_) => "instruction_fault",
            LabError::SessionNotFound => "session_not_found",
            LabError::BadRequest(_) => "bad_request",
            LabError::Emulator(_) => "emulator_error",
            LabError::ToolUnavailable(_) => "tool_unavailable",
            LabError::IsaNotFound(_) => "isa_not_found",
            LabError::Io(_) => "io_error",
        }
    }
}
