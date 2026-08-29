//! LLM 配置。
//!
//! 持久化由前端的 `tauri-plugin-store` 负责（配置页保存后调用 `set_llm_config`
//! 同步到 Rust 侧内存）。api_key 目前随 store 落盘，M7 会迁到系统凭据管理器。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// 形如 `https://api.openai.com/v1`。
    /// 也接受直接填完整的 `.../chat/completions`，见 `endpoint()`。
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: Option<f32>,
    /// 模型的上下文窗口大小（token）。
    ///
    /// 只用于 UI 上显示占用比例。服务端不会告诉我们这个值，各家模型差异又大，
    /// 所以由用户填；填错只影响那个进度环，不影响能否正常对话。
    pub context_limit: u32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            model: "gpt-4o-mini".to_string(),
            temperature: None,
            context_limit: 128_000,
        }
    }
}

impl LlmConfig {
    /// 拼出 chat completions 的完整 URL。
    ///
    /// 用户填 base_url 的方式五花八门，这里做温和归一化：
    /// - 已经指向 `/chat/completions` → 原样使用
    /// - 否则去掉尾部斜杠后追加 `/chat/completions`
    ///
    /// 刻意**不**自动补 `/v1`：很多中转站的路径前缀并非 `/v1`，
    /// 擅自补会把能用的地址改坏。
    pub fn endpoint(&self) -> String {
        let base = self.base_url.trim().trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/chat/completions")
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.base_url.trim().is_empty() {
            return Err("请先填写 API 地址".into());
        }
        if !self.base_url.starts_with("http://") && !self.base_url.starts_with("https://") {
            return Err("API 地址必须以 http:// 或 https:// 开头".into());
        }
        if self.model.trim().is_empty() {
            return Err("请先填写模型名".into());
        }
        if self.context_limit == 0 {
            return Err("上下文窗口必须大于 0".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_chat_completions_path() {
        let config = LlmConfig {
            base_url: "https://api.example.com/v1".into(),
            ..Default::default()
        };
        assert_eq!(
            config.endpoint(),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn trims_trailing_slash() {
        let config = LlmConfig {
            base_url: "https://api.example.com/v1/".into(),
            ..Default::default()
        };
        assert_eq!(
            config.endpoint(),
            "https://api.example.com/v1/chat/completions"
        );
    }

    /// 用户直接填了完整端点时不该被重复追加。
    #[test]
    fn keeps_explicit_endpoint() {
        let config = LlmConfig {
            base_url: "https://gw.example.com/openai/chat/completions".into(),
            ..Default::default()
        };
        assert_eq!(
            config.endpoint(),
            "https://gw.example.com/openai/chat/completions"
        );
    }

    /// 不该擅自补 /v1：很多中转站的路径前缀并非 /v1，补了反而把能用的地址改坏。
    #[test]
    fn does_not_inject_v1() {
        let config = LlmConfig {
            base_url: "https://gw.example.com/proxy".into(),
            ..Default::default()
        };
        assert_eq!(
            config.endpoint(),
            "https://gw.example.com/proxy/chat/completions"
        );
    }

    #[test]
    fn rejects_incomplete_config() {
        let missing_url = LlmConfig {
            base_url: String::new(),
            ..Default::default()
        };
        assert!(missing_url.validate().is_err());

        let bad_scheme = LlmConfig {
            base_url: "api.example.com".into(),
            ..Default::default()
        };
        assert!(bad_scheme.validate().is_err());

        let missing_model = LlmConfig {
            model: "  ".into(),
            ..Default::default()
        };
        assert!(missing_model.validate().is_err());

        let zero_context = LlmConfig {
            context_limit: 0,
            ..Default::default()
        };
        assert!(zero_context.validate().is_err());
    }

    #[test]
    fn accepts_valid_config() {
        LlmConfig::default()
            .validate()
            .expect("默认配置应当是有效的");
    }
}
