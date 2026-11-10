//! 本地设置：中继服务器列表。
//!
//! 中继是连接的前置条件，未配置时不创建也不加入房间。

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub relays: Vec<String>,
    /// 短码远端解析服务地址（默认内置，可改成自己的服务）
    #[serde(default)]
    pub resolver_base_url: Option<String>,
    /// 设置结构版本：旧文件里没有这个字段（0），迁移一次后就固定为当前版本。
    #[serde(default)]
    pub version: u32,
}

/// 当前设置结构版本。历史残留清理只做一次，之后用户手动填的地址不再被删。
pub const SETTINGS_VERSION: u32 = 1;

const SCHEMES: [&str; 6] = ["tcp://", "udp://", "ws://", "wss://", "quic://", "wg://"];

/// 内置的短码解析服务地址——**全项目唯一出处**（前端从 `app_info` 读，不要另写常量）。
///
/// 服务端在宝塔里把整站转发到解析进程（`location /` → `proxy_pass`），路径原样透传，
/// 因此这里**不能**带 `/api/lanlink` 之类的前缀；接口路径固定为 `POST {base}/r`、
/// `GET {base}/r/{code}`。
/// 支持 `http://`（本地调试）与 `https://`（默认校验证书，见 `http.rs`）。
/// 用户可以在设置页改成自己的地址，也可以一键恢复成这个默认值。
pub const DEFAULT_RESOLVER_BASE_URL: &str = "https://lanlink.woyuxiaji.cn";

/// 内置的解析服务 Token：服务端开启校验，注册与查询都必须携带。
///
/// 只给朋友用的内置方案，Token 编译进客户端，不在设置页暴露。
pub const RESOLVER_TOKEN: &str = "35f895777b3008cae056a22822d712d7457e3447";

/// 旧版本写入过的占位解析地址，加载时迁移成当前内置默认值。
const LEGACY_RESOLVER_BASE_URLS: &[&str] = &["https://lanlink.example.com/api/lanlink"];

/// 旧版本曾经自动写入的中继地址。
///
/// 仅用于识别并清除历史残留，**不会作为默认值使用**；新装用户的中继列表为空。
const LEGACY_DEFAULT_RELAYS: &[&str] = &["tcp://150.158.128.228:11010"];

impl Settings {
    /// 新建并保存时统一带上当前版本，避免迁移逻辑重复执行。
    pub fn new(relays: Vec<String>, resolver_base_url: Option<String>) -> Settings {
        Settings {
            relays,
            resolver_base_url,
            version: SETTINGS_VERSION,
        }
    }

    pub fn load() -> Settings {
        let mut settings: Settings = match path() {
            Some(path) => fs::read_to_string(path)
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or_default(),
            None => Settings::default(),
        };
        let mut dirty = migrate(&mut settings);
        // 解析服务地址：旧占位地址统一迁移；没存过（或存成空）时用内置默认值，全新安装无需手填
        let stored = settings.resolver_base_url.as_deref().map(str::trim);
        let stale = stored.is_some_and(|value| {
            LEGACY_RESOLVER_BASE_URLS
                .iter()
                .any(|legacy| legacy.eq_ignore_ascii_case(value.trim_end_matches('/')))
        });
        let resolver = if stale {
            DEFAULT_RESOLVER_BASE_URL.to_string()
        } else {
            resolver_or_default(stored)
        };
        if settings.resolver_base_url.as_deref() != Some(resolver.as_str()) {
            settings.resolver_base_url = Some(resolver);
            dirty = true;
        }
        if dirty {
            let _ = settings.save();
        }
        settings
    }

    pub fn save(&self) -> Result<(), String> {
        let Some(path) = path() else {
            return Err("无法定位设置文件".to_string());
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|err| format!("无法创建设置目录：{err}"))?;
        }
        let mut current = self.clone();
        // 落盘时始终标记为已迁移，避免下次启动又把用户填的中继当成历史残留清掉
        current.version = current.version.max(SETTINGS_VERSION);
        let json = serde_json::to_string_pretty(&current)
            .map_err(|err| format!("序列化设置失败：{err}"))?;
        fs::write(&path, json).map_err(|err| format!("写入设置失败：{err}"))?;
        crate::config::hand_back(&path);
        Ok(())
    }
}

/// 一次性升级迁移：清掉旧版本自动写入的默认中继，并把版本号抬到当前值。
///
/// 返回是否需要回写文件。已经是当前版本的设置不做任何改动——
/// 否则用户手动填写的同一地址会在每次启动时被删掉。
fn migrate(settings: &mut Settings) -> bool {
    if settings.version >= SETTINGS_VERSION {
        return false;
    }
    settings.relays.retain(|relay| {
        !LEGACY_DEFAULT_RELAYS
            .iter()
            .any(|legacy| legacy.eq_ignore_ascii_case(relay.trim()))
    });
    settings.version = SETTINGS_VERSION;
    true
}

/// 解析服务地址：空值（含未存过、只有空白）回落到内置默认地址。
fn resolver_or_default(value: Option<&str>) -> String {
    let trimmed = value.map(str::trim).unwrap_or("");
    if trimmed.is_empty() {
        DEFAULT_RESOLVER_BASE_URL.to_string()
    } else {
        trimmed.to_string()
    }
}

/// 规范化中继输入：按行拆分、去空、去重，并校验协议前缀、主机名与端口。
///
/// 空输入（含只有空行 / 空白字符）视为「未配置」，返回空列表而不是报错。
pub fn normalize_relays(input: &str) -> Result<Vec<String>, String> {
    let mut relays = Vec::new();
    for line in input.split(['\n', '\r', ',', ';']) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        validate_relay(line)?;
        if !relays.contains(&line.to_string()) {
            relays.push(line.to_string());
        }
    }
    Ok(relays)
}

fn validate_relay(line: &str) -> Result<(), String> {
    if line.chars().any(char::is_whitespace) {
        return Err(format!("中继地址「{line}」中间不能有空格"));
    }
    let lower = line.to_ascii_lowercase();
    if !SCHEMES.iter().any(|scheme| lower.starts_with(scheme)) {
        return Err(format!(
            "中继地址「{line}」缺少协议前缀，请填写 tcp:// 或 udp:// 开头的地址，例如 tcp://你的中继地址:11010"
        ));
    }
    // 去掉 scheme 与可能的路径，只校验 host:port
    let rest = line.split_once("://").map(|(_, rest)| rest).unwrap_or("");
    let host_port = rest.split('/').next().unwrap_or("");
    if host_port.is_empty() {
        return Err(format!("中继地址「{line}」缺少主机名"));
    }

    let (host, port) = if let Some(after) = host_port.strip_prefix('[') {
        // IPv6：[::1]:11010
        match after.split_once(']') {
            Some((host, tail)) => (host, tail.strip_prefix(':')),
            None => return Err(format!("中继地址「{line}」的 IPv6 地址缺少右括号")),
        }
    } else {
        match host_port.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_port, None),
        }
    };

    if host.is_empty() {
        return Err(format!("中继地址「{line}」缺少主机名"));
    }
    if let Some(port) = port {
        match port.parse::<u16>() {
            Ok(0) | Err(_) => {
                return Err(format!("中继地址「{line}」的端口「{port}」不合法，应为 1-65535"));
            }
            Ok(_) => {}
        }
    }
    Ok(())
}

fn path() -> Option<PathBuf> {
    Some(crate::config::data_dir().join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::{
        migrate, normalize_relays, resolver_or_default, Settings, DEFAULT_RESOLVER_BASE_URL,
        SETTINGS_VERSION,
    };

    #[test]
    fn resolver_falls_back_to_builtin_default() {
        assert!(
            DEFAULT_RESOLVER_BASE_URL.starts_with("https://"),
            "默认解析地址应当是 https"
        );
        assert_eq!(resolver_or_default(None), DEFAULT_RESOLVER_BASE_URL);
        assert_eq!(resolver_or_default(Some("   ")), DEFAULT_RESOLVER_BASE_URL);
        assert_eq!(
            resolver_or_default(Some(" https://my.host/api/lanlink ")),
            "https://my.host/api/lanlink"
        );
    }

    #[test]
    fn legacy_relay_is_only_cleared_once() {
        let mut legacy = Settings {
            relays: vec!["tcp://150.158.128.228:11010".to_string()],
            resolver_base_url: None,
            version: 0,
        };
        assert!(migrate(&mut legacy));
        assert!(legacy.relays.is_empty(), "旧版本自动写入的默认中继应被清掉");
        assert_eq!(legacy.version, SETTINGS_VERSION);

        // 同一个地址由用户手动填写，版本已是最新，必须保留
        let mut saved = Settings::new(vec!["tcp://150.158.128.228:11010".to_string()], None);
        assert!(!migrate(&mut saved));
        assert_eq!(saved.relays, vec!["tcp://150.158.128.228:11010"]);
    }

    #[test]
    fn blank_input_is_unconfigured() {
        for input in ["", "\n", "  \n\t\n ", ",,;;"] {
            assert_eq!(normalize_relays(input).unwrap(), Vec::<String>::new());
        }
    }

    #[test]
    fn trims_and_dedupes() {
        let relays = normalize_relays(" tcp://1.2.3.4:11010 \ntcp://1.2.3.4:11010\nudp://[::1]:11010\n").unwrap();
        assert_eq!(relays, vec!["tcp://1.2.3.4:11010", "udp://[::1]:11010"]);
    }

    #[test]
    fn rejects_missing_scheme_and_bad_port() {
        assert!(normalize_relays("1.2.3.4:11010").unwrap_err().contains("缺少协议前缀"));
        assert!(normalize_relays("tcp://1.2.3.4:0").unwrap_err().contains("不合法"));
        assert!(normalize_relays("tcp://1.2.3.4:70000").unwrap_err().contains("不合法"));
        assert!(normalize_relays("tcp://1.2.3.4:abc").unwrap_err().contains("不合法"));
        assert!(normalize_relays("tcp://:11010").unwrap_err().contains("缺少主机名"));
    }
}
