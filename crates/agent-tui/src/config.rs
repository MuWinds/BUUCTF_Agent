//! 配置加载。
//!
//! 优先级（从高到低）：
//! 1. `--config` 命令行参数指定的 TOML 文件
//! 2. 用户目录下 `.coding-agents/config.toml`
//! 3. 内置默认值
//!
//! api_key 的读取顺序：环境变量 `CODING_AGENT_API_KEY` → 系统凭据管理器
//! （`agent_host::secret`）→ 配置文件里的明文。明文是最后手段 ——
//! 配置文件会被同步工具带走，密钥不该长期住在里面。
//!
//! ## `max_retries` 的坑
//!
//! core 里 `None` 表示无限重试，但 TOML 没有 `null`；而 serde 容器级
//! `#[serde(default)]` 会用 `LlmConfig::default()` 补齐缺失字段（得到
//! `Some(2)`）。因此**省略字段 ≠ 无限重试**，TOML 里必须用哨兵值：
//!
//! ```toml
//! max_retries = -1      # 无限重试（映射到 core 的 None）
//! max_retries = 0       # 失败即报错，不重试
//! max_retries = 3       # 最多重试 3 次
//! # 省略字段 = 默认 2 次
//! ```

use std::path::Path;

use agent_core::LlmConfig;
use anyhow::Result;
use serde::Deserialize;

/// 默认的配置文件名。
const CONFIG_FILE: &str = "config.toml";

/// TUI 配置文件的 TOML 结构 —— 与 `LlmConfig` 有意分开。
///
/// 区别只在 `max_retries`：core 用 `Option<u32>`（`None` = 无限），
/// 但 TOML 表达不了 `null`，这里用 `-1` 哨兵，反序列化后再映射。
/// 其余字段保持与 `LlmConfig` 同名，转换时逐字段复制。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ConfigToml {
    base_url: String,
    api_key: String,
    model: String,
    temperature: Option<f32>,
    context_limit: u32,
    compact_threshold: f64,
    /// `-1` 表示无限重试；`0` / 正整数按字面值；省略 = 默认。
    /// 用 `i64` 接收是因为 TOML 整数默认就是它，`-1` 才能落进来。
    max_retries: Option<i64>,
}

impl ConfigToml {
    fn into_llm_config(self) -> LlmConfig {
        let defaults = LlmConfig::default();
        LlmConfig {
            base_url: if self.base_url.is_empty() {
                defaults.base_url
            } else {
                self.base_url
            },
            api_key: self.api_key,
            model: if self.model.is_empty() {
                defaults.model
            } else {
                self.model
            },
            temperature: self.temperature,
            context_limit: if self.context_limit == 0 {
                defaults.context_limit
            } else {
                self.context_limit
            },
            compact_threshold: if self.compact_threshold.is_finite()
                && self.compact_threshold > 0.0
                && self.compact_threshold < 1.0
            {
                self.compact_threshold
            } else {
                defaults.compact_threshold
            },
            max_retries: match self.max_retries {
                // -1：无限重试（core 的 None）
                Some(-1) => None,
                Some(n) => Some(n as u32),
                // 省略：与 LlmConfig::default() 保持一致（2 次）
                None => defaults.max_retries,
            },
        }
    }
}

/// 加载配置。
///
/// 显式指定的路径必须存在（拼错路径不该静默退回默认配置 ——
/// 用户会以为自己的配置生效了）；自动发现的路径不存在则用默认值。
pub fn load(explicit: Option<&Path>) -> Result<LlmConfig> {
    let path = explicit
        .map(|p| p.to_path_buf())
        .or_else(default_config_path);

    let mut config = match path {
        Some(path) if path.exists() => {
            let text = std::fs::read_to_string(&path)?;
            let loaded: ConfigToml = toml::from_str(&text)?;
            loaded.into_llm_config()
        }
        Some(path) if explicit.is_some() => {
            return Err(anyhow::anyhow!("配置文件不存在：{}", path.display()));
        }
        _ => LlmConfig::default(),
    };

    // 密钥按「环境变量 → 凭据管理器 → 配置文件」的顺序解析。
    if let Ok(key) = std::env::var("CODING_AGENT_API_KEY") {
        config.api_key = key;
    } else if !config.api_key.is_empty() {
        // 配置文件的明文已就位，直接用
    } else {
        config.api_key = agent_host::secret::load();
    }

    if config.api_key.is_empty() {
        tracing::warn!("未配置 API Key：请设置环境变量 CODING_AGENT_API_KEY 或运行配置向导");
    }

    // 校验并给出可读错误。
    config
        .validate()
        .map_err(|e| anyhow::anyhow!("配置无效：{e}"))?;

    tracing::info!(
        model = %config.model,
        endpoint = %config.endpoint(),
        max_retries = ?config.max_retries,
        "配置就绪"
    );

    Ok(config)
}

/// 用户目录下的 `.coding-agents` 配置目录。
///
/// 放在 `~` 而不是平台约定目录（Windows AppData / Unix ~/.config）：
/// 用户随手 `ls ~/.coding-agents` 就能看到自己的配置，
/// 也方便整个目录一起备份或迁移到别的机器。
fn default_config_path() -> Option<std::path::PathBuf> {
    let dir = dirs::home_dir()?.join(".coding-agents");
    Some(dir.join(CONFIG_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 解析一段 TOML 文本，跳过密钥与环境变量环节。
    fn parse(text: &str) -> LlmConfig {
        let loaded: ConfigToml = toml::from_str(text).expect("TOML 应可解析");
        loaded.into_llm_config()
    }

    /// 省略 `max_retries` 时应得到默认值（2 次），而不是无限。
    ///
    /// 这是 serde 容器级 default 的坑：缺失字段由 `LlmConfig::default()`
    /// 补齐（`Some(2)`），与「省略 = None = 无限」的直觉相反。
    #[test]
    fn omitted_max_retries_means_default_not_infinite() {
        let config = parse("base_url = \"http://x/v1\"\nmodel = \"m\"\n");
        assert_eq!(config.max_retries, Some(2), "省略字段应回落默认重试次数");
    }

    /// `max_retries = -1` 是显式的「无限重试」哨兵。
    #[test]
    fn negative_one_means_infinite_retries() {
        let config = parse("base_url = \"http://x/v1\"\nmodel = \"m\"\nmax_retries = -1\n");
        assert_eq!(config.max_retries, None, "-1 应映射为 None（无限重试）");
    }

    /// 0 与正整数按字面值保留。
    #[test]
    fn explicit_counts_are_preserved() {
        assert_eq!(
            parse("base_url = \"http://x/v1\"\nmodel = \"m\"\nmax_retries = 0\n").max_retries,
            Some(0),
            "0 = 失败即报错"
        );
        assert_eq!(
            parse("base_url = \"http://x/v1\"\nmodel = \"m\"\nmax_retries = 5\n").max_retries,
            Some(5)
        );
    }

    /// 配置文件里没写的字段用默认值补齐。
    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let config = parse("base_url = \"http://127.0.0.1:8787/v1\"\nmodel = \"basic-chat\"\n");
        assert_eq!(config.base_url, "http://127.0.0.1:8787/v1");
        assert_eq!(config.model, "basic-chat");
        assert_eq!(config.context_limit, LlmConfig::default().context_limit);
        assert_eq!(config.compact_threshold, 0.7);
    }

    #[test]
    fn loads_from_explicit_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "base_url = \"http://127.0.0.1:8787/v1\"\nmodel = \"basic-chat\"\nmax_retries = -1\n",
        )
        .unwrap();

        let config = load(Some(&path)).unwrap();
        assert_eq!(config.base_url, "http://127.0.0.1:8787/v1");
        assert_eq!(config.model, "basic-chat");
        assert_eq!(config.max_retries, None, "-1 应映射为无限重试");
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        // 自动发现的路径不存在时用默认配置，且默认配置能通过校验
        let config = load(None).unwrap();
        assert!(!config.base_url.is_empty());
        assert!(!config.model.is_empty());
        assert!(config.context_limit > 0);
    }

    /// 默认配置路径应指向用户目录下的 `.coding-agents/config.toml`。
    ///
    /// 守着一次真实回归：之前用 `dirs::config_dir()`（Windows 落在
    /// AppData，Unix 落在 ~/.config），用户放在 `~/.coding-agents`
    /// 的配置完全读不到，app 一直拿默认值跑。
    #[test]
    fn default_config_path_is_home_coding_agents() {
        let path = default_config_path().expect("home_dir 应可用");
        let home = dirs::home_dir().expect("home_dir 应可用");
        assert_eq!(path, home.join(".coding-agents").join(CONFIG_FILE));
    }

    #[test]
    fn explicit_missing_path_is_an_error() {
        // 显式指定的路径不存在必须报错，不能静默退回默认
        let err = load(Some(Path::new("/nonexistent/config.toml"))).unwrap_err();
        assert!(err.to_string().contains("不存在"));
    }
}
