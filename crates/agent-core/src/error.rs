//! core 层错误类型。
//!
//! 实现 `Serialize` 是为了能直接跨进程/跨语言边界传给 UI，
//! 消费方拿到的是 `{ code, message, retryable }`。

use serde::{Serialize, Serializer};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("配置错误：{0}")]
    Config(String),

    #[error("网络请求失败：{0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON 解析失败：{0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Internal(String),
}

impl Error {
    /// 稳定的错误码，供 UI 做差异化提示（不要用 message 文本做判断）。
    pub fn code(&self) -> &'static str {
        match self {
            Self::Config(_) => "config",
            Self::Http(_) => "http",
            Self::Json(_) => "json",
            Self::Internal(_) => "internal",
        }
    }

    /// 是否值得让用户重试。
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Http(_))
    }
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("Error", 3)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.to_string())?;
        s.serialize_field("retryable", &self.retryable())?;
        s.end()
    }
}
