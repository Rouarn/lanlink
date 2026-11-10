//! 房间码：短码（本地表）+ 完整码（自包含），解析器可插拔。
//!
//! 解析顺序：先按格式判断 → 长码走 `SelfContainedResolver`；
//! 短码走 `LocalTableResolver`（本机缓存），查不到再走 `RemoteResolver`
//! （地址可配置，未配置时给出明确提示）。
//! 不内置任何房间或服务器地址。

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    pub code: String,
    /// 房间名（创建者填写，可中文）
    pub name: String,
    pub network: String,
    pub secret: String,
    /// 创建者 `host`，加入者 `member`
    pub role: String,
    pub relays: Vec<String>,
    /// `short` 本地短码 / `long` 自包含完整码
    pub kind: String,
    /// 短码是否已注册到解析服务（可被其他机器解析）
    #[serde(default)]
    pub shared: bool,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub last_used: u64,
}

const LONG_PREFIX: &str = "LL1.";
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
const RECENT_LIMIT: usize = 6;
const CACHE_LIMIT: usize = 20;
/// 解析服务请求超时
const REMOTE_TIMEOUT: Duration = Duration::from_secs(5);
/// 短码查不到时的兜底建议：长码自包含，不依赖解析服务。
const LONG_FALLBACK_HINT: &str =
    "可以让对方发完整房间码（LL1. 开头）给你：长码自带网络名、密钥和中继，不依赖解析服务。";

/// 房间码解析器。
pub trait RoomCodeResolver {
    fn resolve(&self, code: &str) -> Result<Room, String>;
}

/// 本地表：本机创建 / 加入过的房间缓存。
pub struct LocalTableResolver;

impl RoomCodeResolver for LocalTableResolver {
    fn resolve(&self, code: &str) -> Result<Room, String> {
        cached()
            .into_iter()
            .find(|room| room.code == code)
            .ok_or_else(|| "本机没有这个短码对应的房间".to_string())
    }
}

/// 远端解析（共享服务）。接口约定见 docs/room-code-service.md。
pub struct RemoteResolver {
    pub base_url: Option<String>,
    pub token: Option<String>,
}

impl RoomCodeResolver for RemoteResolver {
    fn resolve(&self, code: &str) -> Result<Room, String> {
        let Some(base) = self.base_url.as_deref().filter(|base| !base.is_empty()) else {
            return Err(format!(
                "短码 {code} 是本地码，只有创建它的电脑能解析。到设置里填写解析服务地址后短码才能跨网络解析。{LONG_FALLBACK_HINT}"
            ));
        };
        let value = crate::http::get_json(
            base,
            &format!("/r/{code}"),
            self.token.as_deref(),
            REMOTE_TIMEOUT,
        )
        .map_err(|err| {
            let reason = match err {
                crate::http::HttpError::Status(404) => format!(
                    "解析服务里没有房间码 {code}：可能是码打错了，或房间已过期被删除"
                ),
                crate::http::HttpError::Status(410) => {
                    format!("房间码 {code} 已过期，请让房主重新创建房间")
                }
                crate::http::HttpError::Status(401) | crate::http::HttpError::Status(403) => {
                    "解析服务拒绝了请求：客户端内置 Token 与服务端不一致".to_string()
                }
                crate::http::HttpError::Status(code) => {
                    format!("解析服务暂时不可用（HTTP {code}）")
                }
                crate::http::HttpError::Dns(message) => {
                    format!("解析服务连不上（域名解析失败：{message}），请检查网络。{LONG_FALLBACK_HINT}")
                }
                crate::http::HttpError::Timeout(message) => {
                    format!("解析服务连不上（连接超时：{message}），请检查网络。{LONG_FALLBACK_HINT}")
                }
                crate::http::HttpError::Tls(message) => {
                    format!("解析服务加密连接失败：{message}。{LONG_FALLBACK_HINT}")
                }
                other => format!("解析服务连不上（{other}），请检查网络。{LONG_FALLBACK_HINT}"),
            };
            reason
        })?;

        let text = |key: &str| {
            value
                .get(key)
                .and_then(|item| item.as_str())
                .unwrap_or_default()
                .to_string()
        };
        let relays = value
            .get("relays")
            .and_then(|item| item.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let network = text("network_name");
        let secret = text("network_secret");
        if network.is_empty() || secret.is_empty() {
            return Err("解析服务返回的房间配置缺少网络名或密钥".to_string());
        }
        if relays.is_empty() {
            return Err("这个房间码里没有中继信息，无法连接".to_string());
        }
        let name = text("name");
        Ok(Room {
            code: code.to_string(),
            name: if name.is_empty() { network.clone() } else { name },
            network,
            secret,
            role: "member".to_string(),
            relays,
            kind: "short".to_string(),
            shared: true,
            created_at: 0,
            last_used: now(),
        })
    }
}

/// 自包含完整码。
pub struct SelfContainedResolver;

impl RoomCodeResolver for SelfContainedResolver {
    fn resolve(&self, code: &str) -> Result<Room, String> {
        let payload = decode_long(code)?;
        let relays = payload.relays.clone();
        if relays.is_empty() {
            return Err("这个房间码里没有中继信息，无法连接".to_string());
        }
        Ok(Room {
            code: code.to_string(),
            name: if payload.name.is_empty() {
                payload.network.clone()
            } else {
                payload.name
            },
            network: payload.network,
            secret: payload.secret,
            role: "member".to_string(),
            relays,
            kind: "long".to_string(),
            shared: false,
            created_at: 0,
            last_used: now(),
        })
    }
}

/// 按约定顺序解析房间码。
pub fn resolve_code(code: &str, remote_base: Option<String>, token: Option<String>) -> Result<Room, String> {
    let code = code.trim();
    if code.is_empty() {
        return Err("请填写房间码".to_string());
    }

    if code.starts_with(LONG_PREFIX) {
        return SelfContainedResolver.resolve(code);
    }
    if is_short_code(code) {
        let code = code.to_ascii_uppercase();
        if let Ok(room) = LocalTableResolver.resolve(&code) {
            return Ok(room);
        }
        return RemoteResolver {
            base_url: remote_base,
            token,
        }
        .resolve(&code);
    }
    Err("房间码格式无法识别：短码形如 ROOM-8K3M，完整码以 LL1. 开头".to_string())
}

/// 创建房间：本地生成短码，写入本地表；配置了解析服务时同步注册，供其他机器解析。
pub fn create(
    name: &str,
    relays: Vec<String>,
    remote_base: Option<String>,
    token: Option<String>,
) -> Result<Room, String> {
    if relays.is_empty() {
        return Err("还没有配置中继服务器，请先在「设置」里填写".to_string());
    }
    let name = name.trim();
    if name.is_empty() {
        return Err("房间名不能为空".to_string());
    }

    let slug = ascii_slug(name);
    let mut code = format!("{slug}-{}", random_tail());
    while cached().iter().any(|room| room.code == code) {
        code = format!("{slug}-{}", random_tail());
    }

    let mut room = Room {
        network: network_from(&code),
        secret: derive_secret(&code),
        name: name.to_string(),
        code,
        role: "host".to_string(),
        relays,
        kind: "short".to_string(),
        shared: false,
        created_at: now(),
        last_used: now(),
    };

    if let Some(base) = remote_base.as_deref().filter(|base| !base.is_empty()) {
        // 注册失败不影响本地建房，只退化成「仅本机可解析」
        for _ in 0..3 {
            match register(&room, base, token.as_deref()) {
                Ok(()) => {
                    room.shared = true;
                    break;
                }
                Err(crate::http::HttpError::Status(409)) => {
                    let tail = random_tail();
                    room.code = format!("{slug}-{tail}");
                    room.network = network_from(&room.code);
                    room.secret = derive_secret(&room.code);
                }
                Err(_) => break,
            }
        }
    }

    remember(&room);
    Ok(room)
}

/// 加入房间：解析短码或完整码，并写入最近房间。
///
/// 中继只来自房间记录本身，**不读本地设置**：全新安装、没配过中继也能直接加入。
pub fn join(code: &str, remote_base: Option<String>, token: Option<String>) -> Result<Room, String> {
    let mut room = resolve_code(code, remote_base, token)?;
    if room.relays.is_empty() {
        return Err("这个房间码里没有中继信息，无法连接".to_string());
    }
    room.last_used = now();
    if room.role != "host" {
        room.role = "member".to_string();
    }
    remember(&room);
    Ok(room)
}

/// 从缓存里恢复房间（保留房主身份），缓存没有就走解析。
///
/// 只有房主自己的房间会用本地设置里的中继覆盖（创建时改过配置的场景）；
/// 加入方的房间沿用记录里的中继，避免本地中继设置影响别人的房间。
pub fn lookup(
    code: &str,
    relays: Vec<String>,
    remote_base: Option<String>,
    token: Option<String>,
) -> Result<Room, String> {
    let code = code.trim();
    if let Some(mut room) = cached().into_iter().find(|room| room.code == code) {
        if room.role == "host" && !relays.is_empty() {
            room.relays = relays;
        }
        room.last_used = now();
        return Ok(room);
    }
    join(code, remote_base, token)
}

/// 导出完整房间码：版本 + 网络名 + 密钥 + 中继列表 + IP 模式，压缩后 base64url。
pub fn export_long(room: &Room) -> Result<String, String> {
    let payload = LongPayload {
        version: 1,
        network: room.network.clone(),
        secret: room.secret.clone(),
        relays: room.relays.clone(),
        ip_mode: "dhcp".to_string(),
        name: room.name.clone(),
    };
    let json = serde_json::to_vec(&payload).map_err(|err| format!("序列化房间失败：{err}"))?;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder
        .write_all(&json)
        .map_err(|err| format!("压缩失败：{err}"))?;
    let compressed = encoder.finish().map_err(|err| format!("压缩失败：{err}"))?;
    Ok(format!("{LONG_PREFIX}{}", URL_SAFE_NO_PAD.encode(compressed)))
}

pub fn touch(room: &Room) {
    remember(room);
}

pub fn recent() -> Vec<Room> {
    let mut rooms = cached();
    rooms.sort_by(|a, b| b.last_used.cmp(&a.last_used));
    rooms.truncate(RECENT_LIMIT);
    rooms
}

/// 删除一条历史房间：同时从本地短码缓存移除，立即落盘。
pub fn forget(code: &str) -> Result<(), String> {
    let code = code.trim();
    if code.is_empty() {
        return Ok(());
    }
    let mut rooms = cached();
    let before = rooms.len();
    rooms.retain(|room| room.code != code);
    if rooms.len() != before {
        persist(&rooms)?;
    }
    Ok(())
}

/// 清空全部历史房间，立即落盘。
pub fn clear() -> Result<(), String> {
    persist(&[])
}

#[derive(Debug, Serialize, Deserialize)]
struct LongPayload {
    #[serde(rename = "v")]
    version: u8,
    #[serde(rename = "n")]
    network: String,
    #[serde(rename = "s")]
    secret: String,
    #[serde(rename = "r")]
    relays: Vec<String>,
    #[serde(rename = "m", default = "default_ip_mode")]
    ip_mode: String,
    #[serde(rename = "g", default)]
    name: String,
}

fn default_ip_mode() -> String {
    "dhcp".to_string()
}

fn decode_long(code: &str) -> Result<LongPayload, String> {
    let body = code
        .trim()
        .strip_prefix(LONG_PREFIX)
        .ok_or_else(|| "完整房间码格式不正确".to_string())?;
    let raw = URL_SAFE_NO_PAD
        .decode(body)
        .map_err(|_| "完整房间码内容无法解码，可能复制时缺了字符".to_string())?;
    let mut decoder = ZlibDecoder::new(raw.as_slice());
    let mut json = Vec::new();
    decoder
        .read_to_end(&mut json)
        .map_err(|err| format!("完整房间码解压失败：{err}"))?;
    let payload: LongPayload =
        serde_json::from_slice(&json).map_err(|_| "完整房间码内容不完整".to_string())?;
    if payload.version != 1 {
        return Err(format!("暂不支持的房间码版本 {}", payload.version));
    }
    if payload.network.is_empty() || payload.secret.is_empty() {
        return Err("完整房间码缺少网络名或密钥".to_string());
    }
    Ok(payload)
}

/// 短码格式：`SLUG-XXXX`。
pub fn is_short_code(code: &str) -> bool {
    let code = code.trim();
    let Some((slug, tail)) = code.rsplit_once('-') else {
        return false;
    };
    let valid = |text: &str, min: usize, max: usize| {
        text.len() >= min && text.len() <= max && text.chars().all(|c| c.is_ascii_alphanumeric())
    };
    valid(slug, 3, 16) && valid(tail, 4, 4)
}

fn remember(room: &Room) {
    let mut rooms = cached();
    rooms.retain(|item| item.code != room.code);
    rooms.push(room.clone());
    rooms.sort_by(|a, b| b.last_used.cmp(&a.last_used));
    rooms.truncate(CACHE_LIMIT);
    write_cache(&rooms);
}

/// 注册短码到解析服务。
fn register(room: &Room, base: &str, token: Option<&str>) -> Result<(), crate::http::HttpError> {
    let payload = serde_json::json!({
        "code": room.code,
        "network_name": room.network,
        "network_secret": room.secret,
        "relays": room.relays,
        "ip_mode": "dhcp",
        "name": room.name,
    });
    crate::http::post_json(base, "/r", token, &payload, REMOTE_TIMEOUT).map(|_| ())
}

/// 网络名统一用 `lanlink-` 前缀，便于中继白名单只放行 LanLink 自己的网络。
/// 后缀沿用房间码里的随机段，保证每个房间的网络名仍然唯一。
fn network_from(code: &str) -> String {
    format!(
        "lanlink-{}",
        code.rsplit('-').next().unwrap_or("0").to_lowercase()
    )
}

/// 从房间名提取可放进短码的 ASCII 标识；纯中文名回退为 `ROOM`。
fn ascii_slug(name: &str) -> String {
    let mut slug: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>()
        .to_ascii_uppercase();
    slug.retain(|c| c.is_ascii_alphanumeric());
    if slug.len() < 3 {
        slug = "ROOM".to_string();
    }
    slug
}

fn random_tail() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut seed = nanos ^ ((std::process::id() as u64) << 17);
    let mut tail = String::new();
    for _ in 0..4 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        tail.push(CODE_ALPHABET[(seed >> 33) as usize % CODE_ALPHABET.len()] as char);
    }
    tail
}

/// 由房间码推导密钥（FNV-1a 64），保证两端一致且无需另传密钥。
fn derive_secret(code: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in format!("lanlink-room-v1:{code}").bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn cached() -> Vec<Room> {
    let Some(path) = cache_path() else {
        return Vec::new();
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn write_cache(rooms: &[Room]) {
    let _ = persist(rooms);
}

/// 把房间列表写入缓存文件并交还原文件属主；历史房间的删除 / 清空依赖它持久化。
fn persist(rooms: &[Room]) -> Result<(), String> {
    let Some(path) = cache_path() else {
        return Err("无法定位房间缓存文件".to_string());
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|err| format!("无法创建缓存目录：{err}"))?;
    }
    let json = serde_json::to_string_pretty(rooms).map_err(|err| format!("序列化房间失败：{err}"))?;
    fs::write(&path, json).map_err(|err| format!("写入房间缓存失败：{err}"))?;
    crate::config::hand_back(&path);
    Ok(())
}

fn cache_path() -> Option<PathBuf> {
    Some(crate::config::data_dir().join("rooms.json"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_names_use_lanlink_prefix() {
        let network = network_from("ROOM-8K3M");
        assert_eq!(network, "lanlink-8k3m");
        // 只包含小写字母、数字和连字符，避免 EasyTier 网络名非法
        assert!(network
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
    }

    fn sample_room(relays: Vec<String>) -> Room {
        Room {
            code: "ROOM-8K3M".to_string(),
            name: "测试房间".to_string(),
            network: network_from("ROOM-8K3M"),
            secret: derive_secret("ROOM-8K3M"),
            role: "host".to_string(),
            relays,
            kind: "long".to_string(),
            shared: false,
            created_at: 0,
            last_used: 0,
        }
    }

    /// 把房间缓存放进临时目录，避免单测写到真实的用户数据目录。
    fn isolated_cache() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lanlink-rooms-test-{}", std::process::id()));
        crate::config::set_override(dir.clone(), None);
        dir
    }

    /// 全新安装（本地 relays 为空）也要能凭房间码里的中继加入。
    #[test]
    fn join_without_local_relays_uses_record_relays() {
        isolated_cache();
        let host = sample_room(vec!["tcp://relay.example.com:11010".to_string()]);
        let code = export_long(&host).expect("导出长码");

        let joined = join(&code, None, None).expect("本地没配中继也应能加入");
        assert_eq!(joined.relays, host.relays);
        assert_eq!(joined.network, host.network);
        assert_eq!(joined.role, "member");
    }

    /// 只有房间码里真的没有中继时才报缺失，不再提「去配置中继」。
    #[test]
    fn join_reports_missing_relays_in_code() {
        isolated_cache();
        let host = sample_room(Vec::new());
        let code = export_long(&host).expect("导出长码");

        let err = join(&code, None, None).unwrap_err();
        assert!(err.contains("没有中继信息"), "实际错误：{err}");
    }

    /// 加入方的房间用记录里的中继，不被本地设置覆盖。
    #[test]
    fn lookup_keeps_record_relays_for_member() {
        isolated_cache();
        let host = sample_room(vec!["tcp://relay.example.com:11010".to_string()]);
        let code = export_long(&host).expect("导出长码");
        let joined = join(&code, None, None).expect("加入应成功");

        let again = lookup(
            &joined.code,
            vec!["tcp://local.example.com:11010".to_string()],
            None,
            None,
        )
        .expect("缓存命中");
        assert_eq!(again.relays, host.relays);
    }

    /// 本地自测：先起服务，再跑
    /// `cargo test local_resolver_selfcheck -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn local_resolver_selfcheck() {
        let base = "http://127.0.0.1:18080";
        let room = create(
            "客户端自测",
            vec!["tcp://127.0.0.1:11010".to_string()],
            Some(base.to_string()),
            None,
        )
        .expect("创建应成功");
        println!("创建：{} shared={}", room.code, room.shared);
        assert!(room.shared, "房间码应已注册到解析服务");

        // 模拟另一台机器：换个 baseURL，仅凭短码解析
        let resolved = RemoteResolver {
            base_url: Some("http://localhost:18080".to_string()),
            token: None,
        }
        .resolve(&room.code)
        .expect("远端解析应成功");
        println!(
            "解析：{} / {} / {:?}",
            resolved.name, resolved.network, resolved.relays
        );
        assert_eq!(resolved.network, room.network);
        assert_eq!(resolved.secret, room.secret);

        let missing = RemoteResolver {
            base_url: Some(base.to_string()),
            token: None,
        }
        .resolve("NOPE99");
        println!("404 分类：{:?}", missing.as_ref().err());
        assert!(missing.unwrap_err().contains("没有房间码"));
    }
}
