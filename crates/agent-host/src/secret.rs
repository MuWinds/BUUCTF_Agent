//! API Key 的安全存储。
//!
//! 密钥不能跟其他配置一起明文落盘 —— `settings.json` 会被同步工具、备份、
//! 甚至误提交带走。这里存进系统凭据管理器（Windows 是凭据管理器，
//! macOS 是钥匙串，Linux 是 Secret Service）。
//!
//! 凭据后端不可用时降级为仅内存保存：功能照常，但重启后要重填。
//! 这比"存不了就报错不让用"要好。

const SERVICE: &str = "coding-agent";
const ACCOUNT: &str = "llm-api-key";

fn entry() -> Option<keyring::Entry> {
    match keyring::Entry::new(SERVICE, ACCOUNT) {
        Ok(entry) => Some(entry),
        Err(e) => {
            tracing::warn!("系统凭据存储不可用：{e}");
            None
        }
    }
}

/// 读出保存的密钥。没有保存过或后端不可用时返回空串。
pub fn load() -> String {
    let Some(entry) = entry() else {
        return String::new();
    };

    match entry.get_password() {
        Ok(secret) => secret,
        // NoEntry 是正常情况（还没存过），不该记为警告
        Err(keyring::Error::NoEntry) => String::new(),
        Err(e) => {
            tracing::warn!("读取 API Key 失败：{e}");
            String::new()
        }
    }
}

/// 保存密钥。
///
/// **空串表示"没有提供"，不是"要删除"** —— 此时保持已存的凭据不变。
/// 启动流程里任何一次读取失败都会传进来一个空值，若按删除处理，
/// 用户的密钥就会被静默清掉（这个 bug 真实发生过）。
/// 要清除请用 [`clear`]。
///
/// 返回是否真的落盘了 —— 调用方据此提示用户"重启后需要重填"。
pub fn save(secret: &str) -> bool {
    if secret.is_empty() {
        return true;
    }

    let Some(entry) = entry() else {
        return false;
    };

    match entry.set_password(secret) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("保存 API Key 失败：{e}");
            false
        }
    }
}

/// 显式清除保存的密钥。
pub fn clear() {
    let Some(entry) = entry() else {
        return;
    };
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => tracing::warn!("清除 API Key 失败：{e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证凭据后端在本机真的可用。
    ///
    /// 用独立的 service 名，不碰应用真正使用的那条凭据。
    #[test]
    fn credential_store_round_trips() {
        const TEST_SERVICE: &str = "coding-agent-selftest";
        const TEST_ACCOUNT: &str = "probe";

        let entry = match keyring::Entry::new(TEST_SERVICE, TEST_ACCOUNT) {
            Ok(entry) => entry,
            Err(e) => panic!("凭据后端不可用：{e}"),
        };

        entry
            .set_password("hello-secret")
            .expect("写入凭据应当成功");

        let got = entry.get_password().expect("读取凭据应当成功");
        assert_eq!(got, "hello-secret");

        entry.delete_credential().expect("删除凭据应当成功");

        assert!(
            matches!(entry.get_password(), Err(keyring::Error::NoEntry)),
            "删除后应当读不到"
        );
    }

    /// 空值不该删除已存的凭据。
    ///
    /// 这条守着一个真实发生过的数据丢失：启动流程里读取失败会传进空值，
    /// 若按删除处理，用户的密钥就没了。
    #[test]
    fn empty_secret_does_not_erase_stored_one() {
        assert!(save(""), "空值应当被视为无操作而非失败");

        // 真实凭据不受影响：save("") 根本不该碰存储
        let before = load();
        assert!(save(""));
        assert_eq!(load(), before, "空值调用后已存的密钥必须原样保留");
    }
}
