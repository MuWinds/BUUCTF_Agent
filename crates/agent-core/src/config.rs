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
    /// 只用于 UI 上显示占用比例和自动压缩的阈值判断。服务端不会告诉我们
    /// 这个值，各家模型差异又大，所以由用户填；填错只影响那个进度环，
    /// 不影响能否正常对话。
    pub context_limit: u32,
    /// 自动压缩的触发阈值（0~1）：上下文占用超过窗口的该比例时压缩。
    ///
    /// 占用优先取服务端返回的真实 prompt_tokens（最后一条 assistant 记录的
    /// context_used），没有记录时才退回到字符估算。0.7 表示留 30% 余量
    /// 给当前轮次的输出和工具结果回灌 —— 若等到 100% 才动手，
    /// 摘要请求本身就会放不进窗口。
    #[serde(default = "default_compact_threshold")]
    pub compact_threshold: f64,
    /// LLM 请求失败时的自动重试次数。
    ///
    /// - `Some(0)`：失败即报错，不重试
    /// - `Some(n)`：最多自动重试 n 次（总尝试 n+1 次）
    /// - `None`：无限重试，直到成功或用户取消 —— 应对 LLM 供应商的不稳定
    pub max_retries: Option<u32>,
}

/// 压缩阈值的字段级默认值。
///
/// 单独写函数而非依赖 `#[serde(default)]` 的 `f64::default()`（=0.0）：
/// 旧版本配置里没有这个字段，反序列化得到 0.0 会让 validate 拒绝整个配置，
/// 等于升级即坏。0.7 是历史行为，新老配置都成立。
fn default_compact_threshold() -> f64 {
    0.7
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            model: "gpt-4o-mini".to_string(),
            temperature: None,
            context_limit: 128_000,
            compact_threshold: 0.7,
            max_retries: Some(2),
        }
    }
}

impl LlmConfig {
    /// 压缩阈值落在 (0, 1) 之外时归一化到默认值 —— 填错不该影响对话。
    ///
    /// 独立于 `validate`：后者在发请求前强制校验，这里只是防御
    /// 用户在设置页手滑填了 `1.5` 这类明显没意义的值。
    pub fn effective_compact_threshold(&self) -> f64 {
        if self.compact_threshold.is_finite()
            && self.compact_threshold > 0.0
            && self.compact_threshold < 1.0
        {
            self.compact_threshold
        } else {
            0.7
        }
    }
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
        // 阈值必须严格落在 (0, 1)：0 表示「一到任何占用就压缩」、
        // 1 表示「满窗口才压缩」，都不是合理的触发点
        if self.compact_threshold.is_finite()
            && !(self.compact_threshold > 0.0 && self.compact_threshold < 1.0)
        {
            return Err("压缩阈值必须是 0 到 1 之间的小数".into());
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

        // 阈值必须落在 (0, 1) 开区间
        let zero_threshold = LlmConfig {
            compact_threshold: 0.0,
            ..Default::default()
        };
        assert!(zero_threshold.validate().is_err());

        let one_threshold = LlmConfig {
            compact_threshold: 1.0,
            ..Default::default()
        };
        assert!(one_threshold.validate().is_err());
    }

    /// 旧版本持久化的配置没有 compact_threshold 字段，反序列化后必须是 0.7 ——
    /// 若得到 f64 默认的 0.0，validate 会把整个配置拒掉，等于升级即坏。
    #[test]
    fn legacy_config_defaults_threshold_to_0_7() {
        let json = r#"{"base_url":"https://x","api_key":"k","model":"m","temperature":null,"context_limit":128000,"max_retries":null}"#;
        let config: LlmConfig = serde_json::from_str(json).expect("旧配置应当可反序列化");
        assert_eq!(config.compact_threshold, 0.7, "缺字段必须回退到历史行为");
        config.validate().expect("回退后的配置应当合法");
    }

    /// 手滑填了 0 或 1 时，决策路径上回退到 0.7 —— 不该让压缩行为被非法值带偏。
    #[test]
    fn invalid_threshold_effective_falls_back() {
        let bad = LlmConfig {
            compact_threshold: 1.5,
            ..Default::default()
        };
        assert_eq!(bad.effective_compact_threshold(), 0.7);

        let nan = LlmConfig {
            compact_threshold: f64::NAN,
            ..Default::default()
        };
        assert_eq!(nan.effective_compact_threshold(), 0.7);
    }

    #[test]
    fn accepts_valid_config() {
        LlmConfig::default()
            .validate()
            .expect("默认配置应当是有效的");
    }
}
