//! Android 端的 Tunnel 实现。
//!
//! 与桌面端（spawn `easytier-core` 子进程 + 调 `easytier-cli`）不同：
//! 这里直接调用同包内的 `libeasytier.so`，由系统 VpnService 建卡后把 fd 交给库。
//! 桌面的 `core.rs` 不参与 Android 编译，两边互不影响。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::android::{ffi, vpn};
use crate::games;
use crate::rooms::Room;
use crate::settings::Settings;

/// 实例名固定：与 FFI 侧 `lanlink_et_attach_tun` 的第一个参数对应。
const INSTANCE: &str = "lanlink";
/// 等待 DHCP 下发虚拟 IP 的上限
const ADDRESS_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const LOG_LIMIT: usize = 400;
/// 对端列表的最小刷新间隔（界面按 1–4 秒轮询状态，这里再节流一次）。
const PEER_REFRESH: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Idle,
    Connecting,
    Connected,
    Reconnecting,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeerInfo {
    pub hostname: String,
    pub ipv4: String,
    pub cost: String,
    pub proto: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub level: String,
    pub time: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenGame {
    pub name: String,
    pub hostname: String,
    pub ipv4: String,
    pub port: u16,
    pub source: String,
    pub mine: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub state: State,
    pub running: bool,
    pub elevated: bool,
    pub elevating: bool,
    pub ipv4: Option<String>,
    pub hostname: Option<String>,
    pub peer_count: u32,
    pub direct_peer_count: u32,
    pub version: Option<String>,
    pub core_pid: Option<u32>,
    pub error: Option<String>,
    pub hint: Option<String>,
    pub room: Option<String>,
    pub room_name: Option<String>,
    pub role: Option<String>,
    pub network: Option<String>,
    pub elevate_error: Option<String>,
    pub log_tail: Vec<String>,
    pub peers: Vec<PeerInfo>,
    pub lan_enabled: bool,
    pub lan_relay: bool,
    pub lan_error: Option<String>,
    pub open_games: Vec<OpenGame>,
    pub games_relay: bool,
    pub games_error: Option<String>,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            state: State::Idle,
            running: false,
            elevated: true,
            elevating: false,
            ipv4: None,
            hostname: None,
            peer_count: 0,
            direct_peer_count: 0,
            version: None,
            core_pid: None,
            error: None,
            hint: None,
            room: None,
            room_name: None,
            role: None,
            network: None,
            elevate_error: None,
            log_tail: Vec::new(),
            peers: Vec::new(),
            lan_enabled: false,
            lan_relay: false,
            lan_error: None,
            open_games: Vec::new(),
            games_relay: false,
            games_error: None,
        }
    }
}

pub struct Tunnel {
    /// VpnService 插件需要 AppHandle；由 `run()` 在 setup 阶段注入。
    app: Mutex<Option<tauri::AppHandle>>,
    state: State,
    room: Option<Room>,
    hostname: Option<String>,
    error: Option<String>,
    hint: Option<String>,
    ipv4: Option<String>,
    peers: Vec<PeerInfo>,
    peer_count: u32,
    direct_peer_count: u32,
    /// 上一次刷新对端列表的时间（节流到 `PEER_REFRESH`）
    peer_at: Option<Instant>,
    started_at: Option<Instant>,
    logs: Vec<LogLine>,
    lan_enabled: bool,
    manual_games: Vec<games::ManualGame>,
    /// 已交给 EasyTier 的 tun fd；> 0 表示虚拟网卡已挂上
    vpn_fd: i32,
}

impl Tunnel {
    pub fn new() -> Self {
        let settings = Settings::load();
        Self {
            app: Mutex::new(None),
            state: State::Idle,
            room: None,
            hostname: None,
            error: None,
            hint: None,
            ipv4: None,
            peers: Vec::new(),
            peer_count: 0,
            direct_peer_count: 0,
            peer_at: None,
            started_at: None,
            logs: Vec::new(),
            lan_enabled: settings.lan_discovery,
            manual_games: settings.manual_games,
            vpn_fd: 0,
        }
    }

    pub fn set_app(&self, app: tauri::AppHandle) {
        *self.app.lock().unwrap() = Some(app);
    }

    /// Android 上没有外部可执行文件：界面上显示库名，便于排查。
    pub fn core_path(&self) -> Option<String> {
        Some("libeasytier.so（内置）".to_string())
    }

    pub fn cli_path(&self) -> Option<String> {
        None
    }

    pub fn start(&mut self, room: Room, hostname: Option<String>) -> Status {
        if matches!(self.state, State::Connecting | State::Connected) {
            return self.status();
        }
        self.stop();

        self.room = Some(room.clone());
        self.hostname = hostname.clone();
        self.error = None;
        self.hint = None;
        self.ipv4 = None;
        self.peers.clear();
        self.peer_count = 0;
        self.direct_peer_count = 0;
        self.peer_at = None;
        self.started_at = Some(Instant::now());

        let hostname = hostname.unwrap_or_else(|| "lanlink-android".to_string());
        self.hostname = Some(hostname.clone());
        let toml = build_config(&room, &hostname);
        self.push_log("info", format!("加入房间 {}（{}）", room.name, room.code));
        self.push_log("info", "启动内置 EasyTier 引擎");

        if let Err(err) = ffi::start(&toml) {
            self.state = State::Failed;
            self.error = Some(err.clone());
            self.push_log("error", err);
            return self.status();
        }

        self.state = State::Connecting;

        // 先等 DHCP 下发地址：VpnService 建卡时必须带地址，不能先建空卡。
        let config = match wait_for_addresses() {
            Ok(config) => config,
            Err(err) => return self.fail(err),
        };
        let (addresses, routes, dns) = ffi::tun_endpoints(&config);
        let Some(ipv4) = ffi::first_ipv4(&addresses) else {
            return self.fail("房间没有下发虚拟 IP，请检查中继地址与房间码".to_string());
        };
        self.push_log("info", format!("已分配虚拟 IP {ipv4}"));

        match self.bring_up_vpn(&addresses, &routes, dns) {
            Ok(()) => {
                self.ipv4 = Some(ipv4);
                self.state = State::Connected;
                self.push_log("info", "虚拟网卡已建立，EasyTier 已接管流量");
            }
            Err(err) => return self.fail(err),
        }

        self.status()
    }

    fn bring_up_vpn(
        &mut self,
        addresses: &[String],
        routes: &[String],
        dns: Option<String>,
    ) -> Result<(), String> {
        let app = self
            .app
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "应用尚未就绪，无法请求 VPN 权限".to_string())?;

        // 系统授权（首次会弹窗，拒绝会明确报错）
        let fd = vpn::start(&app, addresses, routes, dns)?;
        self.vpn_fd = fd;
        self.push_log("info", format!("系统虚拟网卡已建立（fd={fd}）"));

        match ffi::attach_tun(INSTANCE, fd) {
            Ok(config) => {
                let (addrs, _, _) = ffi::tun_endpoints(&config);
                if let Some(ip) = ffi::first_ipv4(&addrs) {
                    self.ipv4 = Some(ip);
                }
                self.push_log("info", "EasyTier 已附加虚拟网卡");
                Ok(())
            }
            Err(err) => {
                let _ = vpn::stop(&app);
                self.vpn_fd = 0;
                Err(format!("把网卡交给 EasyTier 失败：{err}"))
            }
        }
    }

    fn fail(&mut self, message: String) -> Status {
        self.push_log("error", message.clone());
        self.error = Some(message);
        self.state = State::Failed;
        self.release();
        self.status()
    }

    /// 停掉库与系统网卡（不改动界面字段）。
    fn release(&mut self) {
        if self.vpn_fd > 0 {
            let _ = ffi::attach_tun(INSTANCE, 0);
            if let Some(app) = self.app.lock().unwrap().clone() {
                vpn::stop(&app);
            }
            self.vpn_fd = 0;
        }
        ffi::stop();
    }

    pub fn stop(&mut self) -> Status {
        self.release();
        if matches!(self.state, State::Connected | State::Connecting) {
            self.push_log("info", "已断开连接");
        }
        self.state = State::Idle;
        self.room = None;
        self.hostname = None;
        self.error = None;
        self.hint = None;
        self.ipv4 = None;
        self.peers.clear();
        self.peer_count = 0;
        self.direct_peer_count = 0;
        self.peer_at = None;
        self.started_at = None;
        self.status()
    }

    /// 按 `PEER_REFRESH` 节流地刷新对端列表。
    ///
    /// 安卓没有 `easytier-cli`，直接从进程内实例快照取；查询失败时保留上一次结果，
    /// 避免界面上对端列表瞬间清空。
    fn refresh_peers(&mut self) {
        if let Some(at) = self.peer_at {
            if at.elapsed() < PEER_REFRESH {
                return;
            }
        }
        self.peer_at = Some(Instant::now());
        let Ok(value) = ffi::peers() else {
            return;
        };
        let Some(items) = value.as_array() else {
            return;
        };

        let mut peers = Vec::new();
        let mut direct = 0u32;
        for item in items {
            let proto = item
                .get("proto")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            if !proto.is_empty() && proto != "-" {
                direct += 1;
            }
            let ipv4 = item
                .get("ipv4")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .split('/')
                .next()
                .unwrap_or("")
                .to_string();
            if ipv4.is_empty() {
                continue;
            }
            peers.push(PeerInfo {
                hostname: item
                    .get("hostname")
                    .and_then(|value| value.as_str())
                    .unwrap_or("未知节点")
                    .to_string(),
                ipv4,
                cost: item
                    .get("cost")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string(),
                proto,
            });
        }

        self.peer_count = peers.len() as u32;
        self.direct_peer_count = direct;
        self.peers = peers;
    }

    pub fn status(&mut self) -> Status {
        if self.state == State::Connected {
            self.refresh_peers();
        }

        Status {
            state: self.state,
            running: matches!(self.state, State::Connecting | State::Connected),
            elevated: true,
            elevating: false,
            ipv4: self.ipv4.clone(),
            hostname: self.hostname.clone(),
            peer_count: self.peer_count,
            direct_peer_count: self.direct_peer_count,
            version: Some("内置 EasyTier".to_string()),
            core_pid: None,
            error: self.error.clone(),
            hint: self.hint.clone(),
            room: self.room.as_ref().map(|room| room.code.clone()),
            room_name: self.room.as_ref().map(|room| room.name.clone()),
            role: self.room.as_ref().map(|room| room.role.clone()),
            network: self.room.as_ref().map(|room| room.network.clone()),
            elevate_error: None,
            log_tail: self.logs.iter().rev().take(60).map(|line| line.message.clone()).collect::<Vec<_>>().into_iter().rev().collect(),
            peers: self.peers.clone(),
            lan_enabled: self.lan_enabled,
            lan_relay: false,
            // 平台限制不是故障：前端据平台显示中性提示，不当红色错误。
            lan_error: if self.lan_enabled {
                Some("当前平台暂不支持局域网发现与 UDP 广播转发，可用虚拟 IP 直连".to_string())
            } else {
                None
            },
            open_games: Vec::new(),
            games_relay: false,
            games_error: None,
        }
    }

    pub fn set_lan_discovery(&mut self, enabled: bool) -> Status {
        self.lan_enabled = enabled;
        self.push_log(
            "info",
            if enabled {
                "当前平台暂不支持局域网广播发现，已跳过".to_string()
            } else {
                "已关闭局域网发现".to_string()
            },
        );
        self.status()
    }

    pub fn set_broadcast_ports(&mut self, ports: Vec<u16>) -> Status {
        if !ports.is_empty() {
            self.push_log("info", "当前平台暂不支持 UDP 广播转发，端口设置已忽略");
        }
        self.status()
    }

    pub fn set_manual_games(&mut self, items: Vec<games::ManualGame>) -> Status {
        self.manual_games = items;
        self.status()
    }

    pub fn read_logs(&self, level: &str, limit: usize) -> Vec<LogLine> {
        self.logs
            .iter()
            .filter(|line| level == "all" || line.level == level)
            .rev()
            .take(limit.min(LOG_LIMIT))
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    pub fn clear_logs(&self) -> Result<(), String> {
        // 日志是内存里的诊断信息，界面清空即可；下一次连接会重新开始记录。
        Ok(())
    }

    fn push_log(&mut self, level: &str, message: impl Into<String>) {
        self.logs.push(LogLine {
            level: level.to_string(),
            time: timestamp(),
            message: message.into(),
        });
        if self.logs.len() > LOG_LIMIT {
            let drop = self.logs.len() - LOG_LIMIT;
            self.logs.drain(0..drop);
        }
    }
}

/// 轮询 FFI，直到拿到虚拟地址或超时。
fn wait_for_addresses() -> Result<serde_json::Value, String> {
    let deadline = Instant::now() + ADDRESS_TIMEOUT;
    let mut last_error: Option<String> = None;
    loop {
        match ffi::config() {
            Ok(config) => {
                let (addresses, _, _) = ffi::tun_endpoints(&config);
                if !addresses.is_empty() {
                    return Ok(config);
                }
            }
            Err(err) => last_error = Some(err),
        }
        if Instant::now() >= deadline {
            return Err(match last_error {
                Some(err) => format!("等待虚拟 IP 超时：{err}"),
                None => "等待虚拟 IP 超时：房间没有分配地址（检查中继与房间码）".to_string(),
            });
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// 生成 EasyTier 的 TOML 配置。
///
/// 与桌面端命令行参数一一对应：`--network-name/--network-secret/--dhcp/--peers/--hostname`。
fn build_config(room: &Room, hostname: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("instance_name = \"{INSTANCE}\"\n"));
    out.push_str(&format!("hostname = \"{}\"\n", escape(hostname)));
    out.push_str("dhcp = true\n");
    out.push_str("listeners = [\"tcp://0.0.0.0:11010\", \"udp://0.0.0.0:11010\"]\n");
    out.push_str("\n[network_identity]\n");
    out.push_str(&format!("network_name = \"{}\"\n", escape(&room.network)));
    out.push_str(&format!("network_secret = \"{}\"\n", escape(&room.secret)));
    for relay in &room.relays {
        out.push_str("\n[[peer]]\n");
        out.push_str(&format!("uri = \"{}\"\n", escape(relay)));
    }
    out
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// 日志时间（本地时区格式化交给系统，这里只输出 HH:MM:SS）。
fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let day = secs % 86_400;
    format!("{:02}:{:02}:{:02}", day / 3600, (day % 3600) / 60, day % 60)
}
