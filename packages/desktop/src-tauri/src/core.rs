//! easytier-core sidecar 的生命周期管理与状态查询。
//!
//! core 以独立进程方式运行（LGPL 合规、崩溃可隔离），状态通过
//! `easytier-cli -o json` 从本地 RPC 端口读取。
//!
//! 连接状态机：idle → connecting → connected，异常时进入 failed 或 reconnecting。

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::broadcast;
use crate::config;
use crate::games;
use crate::lan;
use crate::proc;
use crate::rooms::Room;
use crate::settings::Settings;

const LOG_TAIL_LINES: usize = 60;
const LOG_TAIL_BYTES: u64 = 48 * 1024;
const LOG_READ_BYTES: u64 = 512 * 1024;
const MAX_LINE_CHARS: usize = 500;
const START_TIMEOUT: Duration = Duration::from_secs(20);
const PORT_RELEASE_TIMEOUT: Duration = Duration::from_secs(3);
/// 局域网发现中继启动失败后的重试间隔
const LAN_RETRY_INTERVAL: Duration = Duration::from_secs(30);
/// 对端列表（`easytier-cli peer`）的最小刷新间隔。
///
/// 界面按 1–4 秒轮询状态，这里再做一次节流：稳态下每分钟最多 20 次进程创建，
/// 局域网发现与广播转发都读这份快照，不再各自起子进程。
const PEER_REFRESH: Duration = Duration::from_secs(3);
/// 单次 easytier-cli 超过这个耗时就在日志里记一笔（Windows 真机上用来量化进程创建开销）
const SLOW_CLI: Duration = Duration::from_millis(80);

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

#[derive(Debug, Clone, Default, Serialize)]
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
    /// 提权失败的提示（由后台线程写入，前端取一次后清空）
    pub elevate_error: Option<String>,
    pub log_tail: Vec<String>,
    pub peers: Vec<PeerInfo>,
    /// 设置里的「局域网发现」开关（默认开启）
    pub lan_enabled: bool,
    /// 局域网发现中继是否正在运行
    pub lan_relay: bool,
    /// 中继启动/运行失败的原因
    pub lan_error: Option<String>,
    /// 房间内「开放游戏」列表（本机 + 对端）
    pub open_games: Vec<OpenGame>,
    /// 开放游戏宣告通道是否正在运行
    pub games_relay: bool,
    /// 宣告通道启动/运行失败的原因
    pub games_error: Option<String>,
}

/// 房间内一条可复制的开放游戏地址。
#[derive(Debug, Clone, Serialize)]
pub struct OpenGame {
    /// 游戏名或《我的世界》世界名
    pub name: String,
    /// 开游戏的人（房间内设备名）
    pub hostname: String,
    /// 端点 IP：对端用真实源 IP，本机用虚拟 IP
    pub ipv4: String,
    pub port: u16,
    /// `minecraft` / `manual` / 空字符串
    pub source: String,
    /// 是否本机开放
    pub mine: bool,
}

impl Default for State {
    fn default() -> Self {
        State::Idle
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HintKind {
    PortConflict,
    Secret,
    Relay,
    Other,
}

struct Hint {
    kind: HintKind,
    text: String,
}

/// 一次 `easytier-cli peer` 查询的结果快照。
///
/// 查询失败（超时、core 还没就绪）时保留上一次快照，界面不会瞬间被清空。
struct PeerSnapshot {
    peers: Vec<PeerInfo>,
    count: u32,
    direct: u32,
}

pub struct Tunnel {
    core: Option<PathBuf>,
    cli: Option<PathBuf>,
    log_dir: PathBuf,
    child: Option<Child>,
    state: State,
    room: Option<Room>,
    hostname: Option<String>,
    started_at: Option<Instant>,
    error: Option<String>,
    hint: Option<String>,
    retried: bool,
    /// 本次 spawn core 之前的日志文件长度：判断退出原因时只解析这之后新写入的内容，
    /// 避免上一次会话残留的旧行（例如旧的端口冲突）误导结论。
    log_mark: u64,
    /// 《我的世界》局域网发现中继：只在房间连上期间存在
    lan: Option<lan::LanRelay>,
    lan_enabled: bool,
    lan_error: Option<String>,
    /// 启动失败后的重试时间，避免每秒重试刷日志
    lan_retry_at: Option<Instant>,
    /// 连上之后的节点信息（ipv4 / hostname / version）：这三项在子进程存活期间不会变，
    /// 缓存后每次状态查询可以少起一个 easytier-cli 进程（Windows 上进程创建很贵）。
    node_cache: Option<(Option<String>, Option<String>, Option<String>)>,
    /// 上一次记录「CLI 太慢」日志的时间，避免每次轮询都写一行
    last_slow_cli: Option<Instant>,
    /// 上一次 CLI 调用失败（超时 / 启动失败）的日志时间
    last_cli_error: Option<Instant>,
    /// 对端虚拟 IP 快照：局域网发现与广播转发共用，避免各自轮询 easytier-cli
    peer_ips: Arc<Mutex<Vec<Ipv4Addr>>>,
    /// 对端列表快照（界面用），查询失败时保留上一次
    peer_cache: Option<PeerSnapshot>,
    /// 上一次查询对端列表的时间（节流到 `PEER_REFRESH`）
    peer_at: Option<Instant>,
    /// 通用 UDP 广播转发（CS 1.6 / 魔兽 / 帝国时代这类老游戏）
    bc: Option<broadcast::BroadcastRelay>,
    broadcast_ports: Vec<u16>,
    bc_retry_at: Option<Instant>,
    /// 房间内「开放游戏」宣告：只在房间连上期间存在，和局域网发现开关无关
    games: Option<games::GameRelay>,
    games_error: Option<String>,
    games_retry_at: Option<Instant>,
    /// 本机开放游戏表（自动发现 + 手动登记），由 lan 中继与设置命令共同维护
    local_games: games::LocalGames,
}

impl Tunnel {
    pub fn new() -> Self {
        let log_dir = config::log_dir();
        let _ = config::create_private_dir(&log_dir);
        let settings = Settings::load();
        let (core, cli) = resolve_binaries();
        match &core {
            Some(path) => eprintln!("[lanlink] easytier-core: {}", path.display()),
            None => eprintln!("[lanlink] 未找到 easytier-core，已查找：{:?}", candidate_dirs()),
        }
        let tunnel = Self {
            core,
            cli,
            log_dir,
            child: None,
            state: State::Idle,
            room: None,
            hostname: None,
            started_at: None,
            error: None,
            hint: None,
            retried: false,
            log_mark: 0,
            lan: None,
            lan_enabled: settings.lan_discovery,
            lan_error: None,
            lan_retry_at: None,
            node_cache: None,
            last_slow_cli: None,
            last_cli_error: None,
            peer_ips: Arc::new(Mutex::new(Vec::new())),
            peer_cache: None,
            peer_at: None,
            bc: None,
            broadcast_ports: broadcast::normalize_ports(&settings.broadcast_ports),
            bc_retry_at: None,
            games: None,
            games_error: None,
            games_retry_at: None,
            local_games: Arc::new(Mutex::new(games::LocalGamesState::with_manual(
                settings.manual_games.clone(),
            ))),
        };
        // 上次以 root 运行留下的日志文件：这次启动就顺手把权限与属主打回登录用户
        tunnel.secure_log_permissions();
        tunnel
    }

    pub fn core_path(&self) -> Option<String> {
        self.core.as_ref().map(|p| p.display().to_string())
    }

    pub fn cli_path(&self) -> Option<String> {
        self.cli.as_ref().map(|p| p.display().to_string())
    }

    fn log_path(&self) -> PathBuf {
        self.log_dir.join("core.log")
    }

    /// core 以 root 运行时日志文件属于 root，普通用户清空不了。
    /// 启动 core 时（以及每次启动 App 时）把权限收紧到 0600、属主交还给当前登录用户。
    ///
    /// 日志里有中继地址、虚拟 IP 与本机路网信息：不能像旧版本那样给 0666，
    /// 也不能放在所有用户可读的 `/tmp`。
    fn secure_log_permissions(&self) {
        let path = self.log_path();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(&path) {
                let mut permissions = meta.permissions();
                if permissions.mode() & 0o777 != 0o600 {
                    permissions.set_mode(0o600);
                    let _ = fs::set_permissions(&path, permissions);
                }
            }
        }
        config::hand_back(&path);
        // 目录本身也要交还：root 建的 0700 目录会让普通用户之后读不到日志
        config::hand_back(&self.log_dir);
    }

    fn pid_path(&self) -> PathBuf {
        self.log_dir.join("core.pid")
    }

    /// 加入房间。端口仍被上次残留的 core 占用时，先清理再启动。
    pub fn start(&mut self, room: Room, hostname: Option<String>) -> Status {
        // 已有 core 在跑：连接中的直接回报状态；失败残留的先收掉再重启，
        // 否则「再试一次」只会永远返回同一个失败状态（等于按钮没反应）。
        if self.child.is_some() {
            if matches!(
                self.state,
                State::Connecting | State::Reconnecting | State::Connected
            ) {
                return self.status();
            }
            self.stop();
        }

        let Some(core) = self.core.clone() else {
            self.state = State::Failed;
            self.error = Some(format!(
                "未找到内置 easytier-core，请检查 packages/core/{} 目录",
                platform_dir()
            ));
            return self.status();
        };

        self.room = Some(room);
        self.hostname = hostname.clone();
        self.error = None;
        self.hint = None;
        self.retried = false;
        self.lan_error = None;
        self.lan_retry_at = None;
        self.games = None;
        self.games_error = None;
        self.games_retry_at = None;
        self.local_games.lock().unwrap().clear_minecraft();
        self.node_cache = None;
        self.peer_cache = None;
        self.peer_at = None;
        self.peer_ips.lock().unwrap().clear();

        if port_in_use() || !port_holders().is_empty() {
            self.state = State::Reconnecting;
        }
        let blockers = self.reclaim_ports(&core);
        if !blockers.is_empty() {
            self.state = State::Failed;
            self.error = Some(blocker_message(&blockers));
            self.hint = Some(
                "这些进程不是本应用启动的，未自动结束（也可能是 launchd / 看门狗反复拉起）。结束它们或更换中继后重试。"
                    .to_string(),
            );
            return self.status();
        }

        self.state = State::Connecting;
        self.started_at = Some(Instant::now());
        if let Err(err) = self.spawn(hostname) {
            self.state = State::Failed;
            self.error = Some(format!("启动 core 失败：{err}"));
            self.hint = Some("请确认内置的 easytier-core 有可执行权限。".to_string());
        } else {
            // 缺 DLL / 架构不匹配这类失败发生在加载阶段，进程会立刻退出。
            // 先在本地观察一小会儿，让第一次返回的状态就带上真实原因，
            // 而不是前端先看到「连接中」、再白等一轮轮询与 CLI 探测。
            self.wait_quick_exit();
        }
        self.status()
    }

    /// 启动后短暂观察子进程，捕捉加载阶段就退出的情况（缺 DLL、架构不匹配等）。
    /// 最多等 300ms，正常启动不受影响。
    fn wait_quick_exit(&mut self) {
        const QUICK_EXIT_WINDOW: Duration = Duration::from_millis(300);
        let started = Instant::now();
        while started.elapsed() < QUICK_EXIT_WINDOW {
            let exited = match self.child.as_mut() {
                Some(child) => !matches!(child.try_wait(), Ok(None)),
                None => true,
            };
            if exited {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn stop(&mut self) -> Status {
        // 先停中继：断开房间后要立刻停播并关掉本地转发端口
        self.lan = None;
        self.bc = None;
        self.games = None;
        self.games_error = None;
        self.games_retry_at = None;
        self.local_games.lock().unwrap().clear_minecraft();
        self.node_cache = None;
        // 对端快照属于上一次会话，不能带到下一次连接
        self.peer_cache = None;
        self.peer_at = None;
        self.peer_ips.lock().unwrap().clear();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(core) = self.core.clone() {
            // 覆盖 kill 掉外壳但仍残留的进程，顺带释放端口
            self.reclaim_ports(&core);
        }
        let _ = fs::remove_file(self.pid_path());

        self.state = State::Idle;
        self.started_at = None;
        self.error = None;
        self.hint = None;
        self.retried = false;
        // 上一次会话的中继失败原因不能留到下次连接
        self.lan_error = None;
        self.lan_retry_at = None;
        self.bc_retry_at = None;
        // 断开即清空本次房间：界面不该再拿到上一轮的房间码/角色/注册结果，
        // 也就不会把「没有数据」当成「注册失败」继续提示。
        self.room = None;
        self.hostname = None;
        self.status()
    }

    pub fn status(&mut self) -> Status {
        let mut status = Status {
            elevated: crate::elevate::is_elevated(),
            ..Default::default()
        };
        status.log_tail = self.tail_log();

        // 1. 探测子进程，处理退出与端口冲突的一键重试
        let mut exit_code: Option<Option<i32>> = None;
        match self.child.as_mut() {
            None => {}
            Some(child) => match child.try_wait() {
                Ok(None) => {
                    status.running = true;
                    status.core_pid = Some(child.id());
                }
                Ok(Some(code)) => exit_code = Some(code.code()),
                Err(err) => {
                    self.error = Some(format!("无法读取 core 进程状态：{err}"));
                    self.state = State::Failed;
                }
            },
        }

        if let Some(code) = exit_code {
            self.child = None;
            self.node_cache = None;
            let (message, exit_hint) = describe_exit(code);
            self.state = State::Failed;
            self.error = Some(message);
            // 只解析本次 spawn 之后新写入的日志：加载期秒退时本次一行都没有，
            // 读旧行会把上一次会话的端口冲突之类误当成本次原因。
            let log_hint = hint_from_log(&self.tail_log_from(Some(self.log_mark)));
            self.hint = exit_hint
                .map(str::to_string)
                .or_else(|| log_hint.as_ref().map(|hint| hint.text.clone()));

            let port_conflict = log_hint
                .map(|hint| hint.kind == HintKind::PortConflict)
                .unwrap_or(false);
            if port_conflict && !self.retried {
                self.retried = true;
                self.state = State::Reconnecting;
                self.error = None;
                let blockers = self
                    .core
                    .clone()
                    .map(|core| self.reclaim_ports(&core))
                    .unwrap_or_default();
                if !blockers.is_empty() {
                    self.state = State::Failed;
                    self.error = Some(blocker_message(&blockers));
                    self.hint =
                        Some("清理后端口仍被占用，占用详情见上；结束这些进程后重试。".to_string());
                } else if self.spawn(self.hostname.clone()).is_ok() {
                    self.state = State::Connecting;
                    self.started_at = Some(Instant::now());
                    self.hint = None;
                }
            }
        }

        // 2. 读取节点与对端信息（查询失败时保留上一次快照，界面不会瞬间清空）
        if status.running {
            // 连上之后 ipv4/hostname/version 在子进程存活期间不会变：
            // 命中缓存就不再起 easytier-cli，稳态下每次轮询只剩 peer 一次进程创建。
            if self.state == State::Connected {
                if let Some((ipv4, hostname, version)) = self.node_cache.clone() {
                    status.ipv4 = ipv4;
                    status.hostname = hostname;
                    status.version = version;
                }
            }
            if status.ipv4.is_none() {
                if let Some(node) = self.run_cli_json("node") {
                    status.ipv4 = node
                        .get("ipv4_addr")
                        .and_then(|v| v.as_str())
                        .filter(|v| !v.is_empty())
                        .map(normalize_ipv4);
                    status.hostname = node
                        .get("hostname")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned);
                    status.version = node
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned);
                    self.node_cache = Some((
                        status.ipv4.clone(),
                        status.hostname.clone(),
                        status.version.clone(),
                    ));
                }
            }

            self.refresh_peers();
            if let Some(snapshot) = &self.peer_cache {
                status.peers = snapshot.peers.clone();
                status.peer_count = snapshot.count;
                status.direct_peer_count = snapshot.direct;
            }
        }

        // 3. 状态推进
        match self.state {
            State::Connecting | State::Reconnecting if status.running => {
                if status.ipv4.is_some() {
                    self.state = State::Connected;
                } else if self
                    .started_at
                    .map(|t| t.elapsed() > START_TIMEOUT)
                    .unwrap_or(false)
                {
                    self.state = State::Failed;
                    self.error = Some(format!(
                        "{} 秒内未获取到虚拟 IP",
                        START_TIMEOUT.as_secs()
                    ));
                    self.hint = Some(
                        "可能是中继服务器不可达，或房间密钥不匹配。请看日志页确认具体原因。"
                            .to_string(),
                    );
                }
            }
            State::Failed if status.ipv4.is_some() => {
                // 超时后延迟连上的情况
                self.state = State::Connected;
                self.error = None;
                self.hint = None;
            }
            _ => {}
        }

        // 4. 局域网发现中继：只在房间真正连上后运行，断开即停
        self.sync_lan();
        status.lan_enabled = self.lan_enabled;
        status.lan_relay = self.lan.is_some();
        // 中继自身没起来（组播端口绑不上等）时才有值
        status.lan_error = self.lan_error.clone();

        // 4.1 房间内开放游戏：与局域网发现开关无关，连上就跑（手动登记也能用）
        self.sync_games();
        status.games_relay = self.games.is_some();
        status.games_error = self
            .games_error
            .clone()
            .or_else(|| self.games.as_ref().and_then(|relay| relay.error()));

        // 4.2 把对端开放的游戏交给局域网中继：在本地起 TCP 转发并伪造《我的世界》
        // 公告，原版「多人游戏」列表才会刷出条目。数据只走宣告通道，不再另发同步报文，
        // 所以自动发现与手动登记的世界表现一致，同一个世界也只会有一个转发端口。
        if let Some(lan) = &self.lan {
            let worlds: Vec<lan::RemoteWorld> = self
                .games
                .as_ref()
                .map(|relay| relay.snapshot())
                .unwrap_or_default()
                .into_iter()
                .map(|game| lan::RemoteWorld {
                    ip: game.ip,
                    port: game.port,
                    name: game.name,
                })
                .collect();
            lan.set_remote_worlds(worlds);
        }

        status.state = self.state;
        status.error = self.error.clone();
        status.hint = self.hint.clone();
        status.room = self.room.as_ref().map(|room| room.code.clone());
        status.room_name = self.room.as_ref().map(|room| room.name.clone());
        status.role = self.room.as_ref().map(|room| room.role.clone());
        status.network = self.room.as_ref().map(|room| room.network.clone());
        status.open_games = self.open_games(&status);
        status
    }

    /// 按 `PEER_REFRESH` 节流地刷新对端列表。
    ///
    /// 局域网发现与广播转发都读这份快照，稳态下整个进程每分钟只起 20 次
    /// `easytier-cli`（旧版本 status + 两个中继各 5 秒一次，约 44–54 次）。
    /// 查询失败时保留上一次快照，避免界面上的 peer 列表瞬间清空。
    fn refresh_peers(&mut self) {
        if let Some(at) = self.peer_at {
            if at.elapsed() < PEER_REFRESH {
                return;
            }
        }
        self.peer_at = Some(Instant::now());
        let Some(value) = self.run_cli_json("peer") else {
            return;
        };
        let Some(peers) = value.as_array() else {
            return;
        };

        let mut info = Vec::new();
        let mut ips: Vec<Ipv4Addr> = Vec::new();
        let mut count = 0u32;
        let mut direct = 0u32;
        for peer in peers {
            let cost = peer.get("cost").and_then(|v| v.as_str()).unwrap_or_default();
            if cost.eq_ignore_ascii_case("local") {
                continue;
            }
            count += 1;
            let proto = peer
                .get("tunnel_proto")
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            if !proto.is_empty() && proto != "-" {
                direct += 1;
            }
            let raw_ip = peer.get("ipv4").and_then(|v| v.as_str()).unwrap_or("-");
            if let Some(ip) = lan::parse_peer_ip(raw_ip) {
                if !ips.contains(&ip) {
                    ips.push(ip);
                }
            }
            info.push(PeerInfo {
                hostname: peer
                    .get("hostname")
                    .and_then(|v| v.as_str())
                    .unwrap_or("未知节点")
                    .to_string(),
                ipv4: normalize_ipv4(raw_ip),
                cost: cost.to_string(),
                proto: proto.to_string(),
            });
        }

        *self.peer_ips.lock().unwrap() = ips;
        self.peer_cache = Some(PeerSnapshot {
            peers: info,
            count,
            direct,
        });
    }

    /// 开关「局域网发现」：立刻生效（关掉时马上停播并释放端口）。
    pub fn set_lan_discovery(&mut self, enabled: bool) -> Status {
        self.lan_enabled = enabled;
        self.lan_error = None;
        self.lan_retry_at = None;
        if !enabled {
            self.lan = None;
        }
        self.status()
    }

    /// 设置通用 UDP 广播转发端口表（空数组 = 关闭），立即生效。
    pub fn set_broadcast_ports(&mut self, ports: Vec<u16>) -> Status {
        self.broadcast_ports = broadcast::normalize_ports(&ports);
        self.bc = None;
        self.bc_retry_at = None;
        self.status()
    }

    /// 设置「开放游戏」手动登记表，下一次宣告周期生效。
    pub fn set_manual_games(&mut self, games: Vec<games::ManualGame>) -> Status {
        self.local_games.lock().unwrap().set_manual(games);
        self.status()
    }

    /// 让开放游戏宣告通道跟上连接状态：连上即启动，断开即停。
    fn sync_games(&mut self) {
        let connected = self.state == State::Connected && self.room.is_some();
        if !connected {
            self.games = None;
            self.games_error = None;
            self.games_retry_at = None;
            return;
        }

        if self
            .games
            .as_ref()
            .map(|relay| relay.alive())
            .unwrap_or(false)
        {
            self.games_error = None;
            return;
        }

        if let Some(relay) = self.games.take() {
            self.games_error = relay.error();
            if self.games_error.is_some() {
                self.games_retry_at = Some(Instant::now() + LAN_RETRY_INTERVAL);
            }
        }
        let cooling = self
            .games_retry_at
            .map(|at| Instant::now() < at)
            .unwrap_or(false);
        if cooling {
            return;
        }

        let Some(network) = self.room.as_ref().map(|room| room.network.clone()) else {
            return;
        };
        match games::GameRelay::start(
            network,
            self.log_path(),
            self.peer_ips.clone(),
            self.local_games.clone(),
        ) {
            Ok(relay) => {
                self.games = Some(relay);
                self.games_error = None;
                self.games_retry_at = None;
            }
            Err(err) => {
                eprintln!("[lanlink] 开放游戏宣告启动失败：{err}");
                self.note(
                    "WARN",
                    &format!(
                        "开放游戏宣告启动失败：{err}（{} 秒后重试）",
                        LAN_RETRY_INTERVAL.as_secs()
                    ),
                );
                self.games_error = Some(err);
                self.games_retry_at = Some(Instant::now() + LAN_RETRY_INTERVAL);
            }
        }
    }

    /// 组装房间内开放游戏列表：本机在前，对端按 IP + 端口排序。
    ///
    /// 端点 IP 对远端一律取真实源地址（`games::RemoteGameSnapshot::ip`），
    /// 与报文里可能出现的任何地址声明无关。
    fn open_games(&self, status: &Status) -> Vec<OpenGame> {
        // 宣告通道没起来时，本机游戏并没有真正播出去，不能假装「已开放」
        let Some(relay) = &self.games else {
            return Vec::new();
        };

        let mut games = Vec::new();
        if let Some(ipv4) = status.ipv4.as_deref().filter(|ip| !ip.is_empty()) {
            let hostname = status
                .hostname
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or("这台电脑");
            for game in self.local_games.lock().unwrap().snapshot() {
                games.push(OpenGame {
                    name: game.name,
                    hostname: hostname.to_string(),
                    ipv4: ipv4.to_string(),
                    port: game.port,
                    source: game.source,
                    mine: true,
                });
            }
        }

        let names: HashMap<&str, &str> = status
            .peers
            .iter()
            .map(|peer| (peer.ipv4.as_str(), peer.hostname.as_str()))
            .collect();
        for game in relay.snapshot() {
            let ipv4 = game.ip.to_string();
            let hostname = names
                .get(ipv4.as_str())
                .map(|name| name.trim())
                .filter(|name| !name.is_empty() && *name != "未知节点")
                .unwrap_or("未命名设备");
            games.push(OpenGame {
                name: game.name,
                hostname: hostname.to_string(),
                ipv4,
                port: game.port,
                source: game.source,
                mine: false,
            });
        }
        games
    }

    /// 让中继跟上连接状态：连上才启动，断开/关闭开关时立即停。
    fn sync_lan(&mut self) {
        let connected = self.state == State::Connected && self.room.is_some();

        // 1. 《我的世界》局域网发现：独立开关，关掉它不影响广播转发
        if !(self.lan_enabled && connected) {
            self.lan = None;
        } else {
            let lan_alive = self.lan.as_ref().map(|relay| relay.alive()).unwrap_or(false);
            if !lan_alive {
                if let Some(relay) = self.lan.take() {
                    // 线程因致命错误退出：记下原因，重建时带退避
                    self.lan_error = relay.error();
                    if self.lan_error.is_some() {
                        self.lan_retry_at = Some(Instant::now() + LAN_RETRY_INTERVAL);
                    }
                }
                let cooling = self
                    .lan_retry_at
                    .map(|at| Instant::now() < at)
                    .unwrap_or(false);
                if !cooling {
                    if let Some(name) = self.room.as_ref().map(|room| room.name.clone()) {
                        match lan::LanRelay::start(
                            name,
                            self.log_path(),
                            self.peer_ips.clone(),
                            self.local_games.clone(),
                        ) {
                            Ok(relay) => {
                                self.lan = Some(relay);
                                self.lan_error = None;
                                self.lan_retry_at = None;
                            }
                            Err(err) => {
                                eprintln!("[lanlink] 局域网发现中继启动失败：{err}");
                                self.lan_error = Some(err);
                                self.lan_retry_at = Some(Instant::now() + LAN_RETRY_INTERVAL);
                            }
                        }
                    }
                }
            }
        }

        // 2. 通用 UDP 广播转发（端口表为空即关闭）
        if !(connected && !self.broadcast_ports.is_empty()) {
            self.bc = None;
        } else if !self.bc.as_ref().map(|relay| relay.alive()).unwrap_or(false) {
            if let Some(relay) = self.bc.take() {
                // 线程因致命错误退出时把原因写进日志，避免广播转发静默失效
                if let Some(err) = relay.error() {
                    self.note("WARN", &format!("UDP 广播转发已停止：{err}"));
                    self.bc_retry_at = Some(Instant::now() + LAN_RETRY_INTERVAL);
                }
            }
            let cooling = self
                .bc_retry_at
                .map(|at| Instant::now() < at)
                .unwrap_or(false);
            if !cooling {
                match broadcast::BroadcastRelay::start(
                    &self.broadcast_ports,
                    self.log_path(),
                    self.peer_ips.clone(),
                ) {
                    Ok(relay) => {
                        self.bc = Some(relay);
                        self.bc_retry_at = None;
                    }
                    Err(err) => {
                        eprintln!("[lanlink] UDP 广播转发启动失败：{err}");
                        self.note(
                            "WARN",
                            &format!("UDP 广播转发启动失败：{err}（{} 秒后重试）", LAN_RETRY_INTERVAL.as_secs()),
                        );
                        self.bc_retry_at = Some(Instant::now() + LAN_RETRY_INTERVAL);
                    }
                }
            }
        }
    }

    /// 读取日志，`level` 取 all/info/warn/error，`limit` 为返回的最大行数。
    pub fn read_logs(&self, level: &str, limit: usize) -> Vec<LogLine> {
        let path = self.log_path();
        let Ok(mut file) = File::open(&path) else {
            return Vec::new();
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len > LOG_READ_BYTES {
            let _ = file.seek(SeekFrom::Start(len - LOG_READ_BYTES));
        }
        let mut buf = Vec::new();
        let _ = file.read_to_end(&mut buf);
        let text = String::from_utf8_lossy(&buf);

        let mut lines: Vec<LogLine> = text
            .split(['\n', '\r'])
            .filter(|line| !line.trim().is_empty())
            .map(parse_log_line)
            .filter(|line| match level {
                "warn" => line.level == "WARN",
                "error" => line.level == "ERROR",
                "info" => matches!(line.level.as_str(), "INFO" | "WARN" | "ERROR"),
                _ => true,
            })
            .collect();

        if lines.len() > limit {
            lines = lines.split_off(lines.len() - limit);
        }
        lines
    }

    /// 清空日志：截断 `core.log` 本身，而不是只清界面。
    ///
    /// 早先版本里 core 以管理员身份创建日志，文件属于 root、权限 644，
    /// 普通窗口 `truncate` 会被拒（EACCES）——这正是「点清空没反应」的原因。
    /// 现在提权启动时会把日志权限/属主交还给登录用户（见 `secure_log_permissions`），
    /// 但历史遗留的 root 文件仍要能清：日志目录归登录用户所有，删掉旧文件再建一个空的即可。
    pub fn clear_logs(&self) -> Result<(), String> {
        let path = self.log_path();
        match truncate_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                // 旧文件属于 root：删除后重建（此时 core 未运行，不会丢正在写的内容）。
                match fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(remove) if remove.kind() == std::io::ErrorKind::NotFound => {}
                    Err(remove) => {
                        return Err(format!(
                            "无法清空日志：{} 的属主是管理员（core 以 root 运行时创建），\
                             当前窗口既不能截断也不能删除（{remove}）。\
                             连接一次（以管理员身份启动）后再清空即可。",
                            path.display()
                        ))
                    }
                }
                truncate_file(&path).map_err(|err| {
                    format!(
                        "无法清空日志 {}：{err}。连接一次（以管理员身份启动）后再清空即可。",
                        path.display()
                    )
                })
            }
            Err(err) => Err(format!("无法清空日志 {}：{err}", path.display())),
        }
    }

    fn spawn(&mut self, hostname: Option<String>) -> Result<(), String> {
        let Some(core) = self.core.clone() else {
            return Err("缺少 easytier-core".to_string());
        };
        let Some(room) = self.room.clone() else {
            return Err("尚未选择房间".to_string());
        };

        let _ = config::create_private_dir(&self.log_dir);
        let log_path = self.log_path();
        let mut log_options = OpenOptions::new();
        log_options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            log_options.mode(0o600);
        }
        let log = log_options
            .open(&log_path)
            .map_err(|err| format!("无法打开日志文件 {}：{err}", log_path.display()))?;
        let log_err = log
            .try_clone()
            .map_err(|err| format!("无法复制日志句柄：{err}"))?;
        // 以管理员身份运行时新建的日志属于 root：收紧到 0600 并交还给登录用户
        self.secure_log_permissions();

        let mut cmd = proc::command(&core);
        cmd.arg("--network-name")
            .arg(&room.network)
            .arg("--network-secret")
            .arg(&room.secret);
        for relay in &room.relays {
            cmd.arg("-p").arg(relay);
        }
        cmd.arg("--dhcp")
            .arg("--rpc-portal")
            .arg(config::RPC_PORTAL);
        if let Some(name) = hostname.as_deref().filter(|name| !name.is_empty()) {
            cmd.arg("--hostname").arg(name);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err));

        // 记下 spawn 前的日志长度：本次的退出原因只看这之后新写入的内容
        self.log_mark = fs::metadata(&log_path).map(|meta| meta.len()).unwrap_or(0);
        let child = cmd.spawn().map_err(|err| err.to_string())?;
        let pid = child.id();
        self.child = Some(child);
        let _ = fs::write(self.pid_path(), pid.to_string());
        Ok(())
    }

    /// 回收被残留 core 占用的端口。
    ///
    /// 只结束命令行里包含内置 core 绝对路径的进程（以及 pid 文件记录的进程），
    /// 其它占用者只报告、不擅自杀掉。返回清理后仍然占用端口的进程列表。
    fn reclaim_ports(&self, core: &Path) -> Vec<PortHolder> {
        let me = std::process::id();
        let mut victims: Vec<u32> = Vec::new();

        for holder in port_holders() {
            if holder.pid != me && is_our_core(&holder.command, core) {
                victims.push(holder.pid);
            }
        }
        if let Ok(raw) = fs::read_to_string(self.pid_path()) {
            if let Ok(pid) = raw.trim().parse::<u32>() {
                if pid != me {
                    victims.push(pid);
                }
            }
        }
        victims.extend(pgrep_pids(core));
        victims.sort_unstable();
        victims.dedup();
        victims.retain(|pid| *pid != me);

        if !victims.is_empty() {
            for pid in &victims {
                signal(*pid, 15);
            }
            if !wait_ports_free(PORT_RELEASE_TIMEOUT) {
                for pid in &victims {
                    signal(*pid, 9);
                }
                wait_ports_free(Duration::from_secs(2));
            }
            let _ = fs::remove_file(self.pid_path());
        }

        port_holders()
    }

    fn run_cli_json(&mut self, subcommand: &str) -> Option<serde_json::Value> {
        let cli = self.cli.clone()?;
        let started = Instant::now();
        let mut cmd = proc::command(&cli);
        cmd.arg("-p")
            .arg(config::RPC_PORTAL)
            .arg("-o")
            .arg("json")
            .arg(subcommand);
        // 一定要有超时：Windows 上 CLI 偶尔会被 Defender 卡住，无限等待会把轮询拖死
        let output = match proc::run_with_timeout(cmd, proc::CLI_TIMEOUT) {
            Ok(output) => output,
            Err(err) => {
                // 失败沿用上一次快照；日志最多 30 秒一条，避免超时时刷屏
                if self
                    .last_cli_error
                    .map(|at| at.elapsed() >= Duration::from_secs(30))
                    .unwrap_or(true)
                {
                    self.last_cli_error = Some(Instant::now());
                    self.note(
                        "WARN",
                        &format!("easytier-cli {subcommand} 调用失败：{err}（沿用上一次状态）"),
                    );
                }
                return None;
            }
        };
        // 量化进程创建开销：真机上超过阈值就写进日志（最多 10 秒一条，避免刷屏）
        let elapsed = started.elapsed();
        if elapsed >= SLOW_CLI
            && self
                .last_slow_cli
                .map(|at| at.elapsed() >= Duration::from_secs(10))
                .unwrap_or(true)
        {
            self.last_slow_cli = Some(Instant::now());
            self.note(
                "WARN",
                &format!(
                    "easytier-cli {subcommand} 单次耗时 {}ms（含进程创建）",
                    elapsed.as_millis()
                ),
            );
        }
        if !output.status.success() {
            return None;
        }
        serde_json::from_slice(&output.stdout).ok()
    }

    /// 往 core.log 追加一行本应用自己的日志（与 easytier 的格式一致，日志页可直接看到）。
    fn note(&self, level: &str, message: &str) {
        use std::io::Write;
        let line = format!("{} {} lanlink::core: {}\n", lan::timestamp(), level, message);
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path())
        {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn tail_log(&self) -> Vec<String> {
        self.tail_log_from(None)
    }

    /// 读取日志尾部；`from` 有值时只读取该偏移之后写入的内容（本次 spawn 之后）。
    fn tail_log_from(&self, from: Option<u64>) -> Vec<String> {
        let path = self.log_path();
        let Ok(mut file) = File::open(&path) else {
            return Vec::new();
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let tail_start = len.saturating_sub(LOG_TAIL_BYTES);
        let start = match from {
            Some(offset) if offset >= len => return Vec::new(),
            Some(offset) if offset > tail_start => offset,
            _ => tail_start,
        };
        if start > 0 {
            let _ = file.seek(SeekFrom::Start(start));
        }
        let mut buf = Vec::new();
        let _ = file.read_to_end(&mut buf);
        let text = String::from_utf8_lossy(&buf);
        let lines: Vec<String> = text
            .split(['\n', '\r'])
            .filter(|line| !line.trim().is_empty())
            .map(|line| truncate(&strip_ansi(line), MAX_LINE_CHARS))
            .collect();
        lines
            .into_iter()
            .rev()
            .take(LOG_TAIL_LINES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn port_in_use() -> bool {
    let Ok(addr) = config::RPC_PORTAL.parse::<SocketAddr>() else {
        return false;
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(150)).is_ok()
}

/// 占用端口的进程（只保留真正在监听的）。
#[derive(Debug, Clone)]
struct PortHolder {
    pid: u32,
    port: u16,
    command: String,
}

fn listen_ports() -> Vec<u16> {
    let mut ports = vec![11010u16, 11011, 11012, 11013];
    if let Some(port) = config::RPC_PORTAL.rsplit(':').next().and_then(|p| p.parse().ok()) {
        if !ports.contains(&port) {
            ports.push(port);
        }
    }
    ports
}

#[cfg(unix)]
fn port_holders() -> Vec<PortHolder> {
    let mut holders: Vec<PortHolder> = Vec::new();
    let mut cmd = proc::command("lsof");
    cmd.arg("-nP");
    for port in listen_ports() {
        cmd.arg(format!("-iTCP:{port}")).arg(format!("-iUDP:{port}"));
    }
    cmd.arg("-sTCP:LISTEN").arg("-Fpcn");
    let Ok(output) = cmd.output() else {
        return holders;
    };

    let mut pid = 0u32;
    let mut name = String::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some(tag) = line.get(0..1) else { continue };
        let value = &line[1..];
        match tag {
            "p" => pid = value.trim().parse().unwrap_or(0),
            "c" => name = value.trim().to_string(),
            "n" => {
                let port = value
                    .rsplit(':')
                    .next()
                    .and_then(|p| p.trim().parse::<u16>().ok())
                    .unwrap_or(0);
                if pid == 0 || port == 0 {
                    continue;
                }
                if holders.iter().any(|h| h.pid == pid && h.port == port) {
                    continue;
                }
                holders.push(PortHolder {
                    pid,
                    port,
                    command: full_command(pid, &name),
                });
            }
            _ => {}
        }
    }
    holders
}

/// Windows：用 `netstat -ano` 找占用 PID（系统自带，不依赖 lsof/pgrep）。
///
/// 进程全路径用 `QueryFullProcessImageNameW` 取，取不到时退回 `tasklist` 的映像名；
/// 这样「端口被占 → 只杀自己的 core」的重试逻辑在 Windows 上同样成立。
#[cfg(windows)]
fn port_holders() -> Vec<PortHolder> {
    let mut cmd = proc::command("netstat");
    cmd.arg("-ano");
    let Ok(output) = proc::run_with_timeout(cmd, proc::CLI_TIMEOUT) else {
        return Vec::new();
    };
    let mut holders = parse_netstat(&String::from_utf8_lossy(&output.stdout), &listen_ports());
    if holders.is_empty() {
        return holders;
    }
    let names = tasklist();
    for holder in &mut holders {
        holder.command = process_image(holder.pid)
            .or_else(|| names.get(&holder.pid).cloned())
            .unwrap_or_default();
    }
    holders
}

/// 解析 `netstat -ano` 的输出，只保留「本地端口在监听 / 已绑定」的行。
///
/// 格式：
/// ```text
///   TCP    0.0.0.0:15888     0.0.0.0:0      LISTENING      1234
///   UDP    0.0.0.0:4448      *:*                            5678
/// ```
/// 不依赖 LISTENING 这个英文状态词（避免本地化系统差异），改为看外部地址是否为通配。
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_netstat(text: &str, ports: &[u16]) -> Vec<PortHolder> {
    let mut holders: Vec<PortHolder> = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }
        let proto = fields[0].to_ascii_uppercase();
        let is_tcp = proto == "TCP";
        if !is_tcp && proto != "UDP" {
            continue;
        }
        let foreign = fields.get(2).copied().unwrap_or_default();
        let wildcard = matches!(foreign, "0.0.0.0:0" | "[::]:0" | "*:*");
        if is_tcp && !wildcard {
            continue;
        }
        if !is_tcp && fields.len() < 4 {
            continue;
        }
        let Some(port) = fields[1]
            .rsplit(':')
            .next()
            .and_then(|value| value.parse::<u16>().ok())
        else {
            continue;
        };
        if !ports.contains(&port) {
            continue;
        }
        let Some(pid) = fields
            .last()
            .and_then(|value| value.trim().parse::<u32>().ok())
        else {
            continue;
        };
        if pid == 0 || holders.iter().any(|h| h.pid == pid && h.port == port) {
            continue;
        }
        holders.push(PortHolder {
            pid,
            port,
            command: String::new(),
        });
    }
    holders
}

/// `tasklist` 的「PID → 映像名」映射（只在取进程全路径失败时用）。
#[cfg(windows)]
fn tasklist() -> std::collections::HashMap<u32, String> {
    let mut names = std::collections::HashMap::new();
    let mut cmd = proc::command("tasklist");
    cmd.args(["/FO", "CSV", "/NH"]);
    let Ok(output) = proc::run_with_timeout(cmd, proc::CLI_TIMEOUT) else {
        return names;
    };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.trim().trim_end_matches('"').split("\",\"").collect();
        if parts.len() < 2 {
            continue;
        }
        let name = parts[0].trim_start_matches('"').to_string();
        if let Ok(pid) = parts[parts.len() - 1].trim_matches('"').parse::<u32>() {
            names.insert(pid, name);
        }
    }
    names
}

/// 进程可执行文件的完整路径（Windows）。
#[cfg(windows)]
fn process_image(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut buffer = [0u16; 1024];
    let mut size = buffer.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size) };
    unsafe { CloseHandle(handle) };
    if ok == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..size as usize]))
}

/// 占用者是不是本应用启动的 core（按可执行文件路径判断，避免误杀用户自己装的 EasyTier）。
fn is_our_core(command: &str, core: &Path) -> bool {
    if command.is_empty() {
        return false;
    }
    let core = core.to_string_lossy();
    if core.is_empty() {
        return false;
    }
    #[cfg(windows)]
    {
        command.to_ascii_lowercase().contains(&core.to_ascii_lowercase())
    }
    #[cfg(not(windows))]
    {
        command.contains(core.as_ref())
    }
}

#[cfg(unix)]
fn full_command(pid: u32, fallback: &str) -> String {
    proc::command("ps")
        .args(["-o", "command=", "-p", &pid.to_string()])
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

#[cfg(unix)]
fn pgrep_pids(core: &Path) -> Vec<u32> {
    let Ok(output) = proc::command("pgrep").arg("-f").arg(core).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect()
}

#[cfg(not(unix))]
fn pgrep_pids(_core: &Path) -> Vec<u32> {
    Vec::new()
}

fn signal(pid: u32, sig: i32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, sig);
    }
    #[cfg(windows)]
    {
        // Windows 没有 Unix 信号：taskkill 不带 /F 只会给 GUI 窗口发 WM_CLOSE，
        // 控制台程序（easytier-core）收不到，所以这里直接强制结束；
        // 只对「确认是本应用 core」的 pid 调用，见 reclaim_ports。
        let _ = sig;
        let mut cmd = proc::command("taskkill");
        cmd.arg("/PID").arg(pid.to_string()).arg("/F");
        let _ = proc::run_with_timeout(cmd, proc::CLI_TIMEOUT);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (pid, sig);
    }
}

fn wait_ports_free(timeout: Duration) -> bool {
    // Windows 上一次 port_holders() 要起 netstat + tasklist 两个进程，
    // 轮询间隔放大到 400ms，避免「回收端口」这一下自己又变成进程风暴。
    #[cfg(windows)]
    const STEP: Duration = Duration::from_millis(400);
    #[cfg(not(windows))]
    const STEP: Duration = Duration::from_millis(120);

    let deadline = Instant::now() + timeout;
    loop {
        if port_holders().is_empty() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(STEP);
    }
}

fn blocker_message(holders: &[PortHolder]) -> String {
    let mut items: Vec<String> = Vec::new();
    for holder in holders {
        let command = truncate(&holder.command, 90);
        let item = format!("端口 {} 被 pid {} 占用：{}", holder.port, holder.pid, command);
        if !items.contains(&item) {
            items.push(item);
        }
    }
    items.join("；")
}

/// 把子进程退出码翻译成给人看的原因。返回 (错误文案, 额外提示)。
///
/// 原始状态码始终保留在文案里（界面「技术信息」直接展示这段），
/// 方便排查时对照 Windows NTSTATUS。
fn describe_exit(code: Option<i32>) -> (String, Option<&'static str>) {
    const RUNTIME_HINT: &str =
        "网络组件缺少运行库，通常是安装包不完整：请重新安装最新版本；若仍失败，把这条技术信息发给开发者。";
    let Some(raw) = code else {
        return ("core 进程已退出（状态码 signal）".to_string(), None);
    };
    let (reason, hint) = match raw as u32 {
        0xC0000135 => ("核心缺少运行库，无法启动", Some(RUNTIME_HINT)),
        0xC0000139 => ("核心依赖的运行库版本不匹配，无法启动", Some(RUNTIME_HINT)),
        0xC000007B => (
            "核心文件损坏或架构不匹配，无法启动",
            Some("安装包可能不完整或被安全软件拦截：请重新安装最新版本。"),
        ),
        0xC0000142 => ("核心运行库初始化失败，无法启动", Some(RUNTIME_HINT)),
        0xC0000005 => ("核心启动时崩溃（访问违例）", None),
        _ => return (format!("core 进程已退出（状态码 {raw}）"), None),
    };
    (
        format!("{reason}（状态码 {raw} / 0x{:08X}）", raw as u32),
        hint,
    )
}

/// 从日志尾部归纳出可读的失败原因。
fn hint_from_log(lines: &[String]) -> Option<Hint> {
    for line in lines.iter().rev().take(80) {
        let lower = line.to_lowercase();
        if lower.contains("address already in use") {
            return Some(Hint {
                kind: HintKind::PortConflict,
                text: format!(
                    "本地端口 {} 被上一次未退出的 core 占用，已自动清理并重试。",
                    config::RPC_PORTAL
                ),
            });
        }
        if lower.contains("private_key") || lower.contains("invalid private key") {
            return Some(Hint {
                kind: HintKind::Secret,
                text: "房间握手失败：密钥不匹配，请确认房间码。".to_string(),
            });
        }
        if lower.contains("network_name") && lower.contains("mismatch") {
            return Some(Hint {
                kind: HintKind::Secret,
                text: "网络名与房间不匹配，请确认房间码。".to_string(),
            });
        }
        if lower.contains("relay") && (lower.contains("failed") || lower.contains("error")) {
            return Some(Hint {
                kind: HintKind::Relay,
                text: "无法连接中继服务器，请检查本机网络或中继是否在线。".to_string(),
            });
        }
        if lower.contains("permission denied") {
            return Some(Hint {
                kind: HintKind::Other,
                text: "权限不足，创建虚拟网卡需要以管理员身份运行。".to_string(),
            });
        }
    }
    None
}

fn parse_log_line(raw: &str) -> LogLine {
    let line = truncate(&strip_ansi(raw), MAX_LINE_CHARS);
    let mut parts = line.split_whitespace();

    let mut time = String::new();
    let mut level = String::new();
    let mut rest_start = 0usize;

    if let Some(first) = parts.next() {
        if first.len() >= 19 && first.as_bytes().get(4) == Some(&b'-') && first.contains('T') {
            // 2026-11-10T08:11:26.247609+08:00 → 08:11:26
            time = first.get(11..19).unwrap_or_default().to_string();
            rest_start = line.find(first).map(|i| i + first.len()).unwrap_or(0);
            if let Some(second) = parts.next() {
                let upper = second.to_ascii_uppercase();
                if matches!(upper.as_str(), "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR") {
                    level = upper;
                    rest_start = line
                        .find(second)
                        .map(|i| i + second.len())
                        .unwrap_or(rest_start);
                }
            }
        }
    }

    let message = line.get(rest_start..).unwrap_or(&line).trim().to_string();
    LogLine {
        level,
        time,
        message: if message.is_empty() {
            line.trim().to_string()
        } else {
            message
        },
    }
}

fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(ch);
    }
    out
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

fn platform_dir() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "macos-arm64"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "macos-x64"
    }
    #[cfg(target_os = "windows")]
    {
        "windows-x64"
    }
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        target_os = "windows"
    )))]
    {
        "unsupported"
    }
}

/// `easytier-cli` 的 `ipv4_addr` / `ipv4` 形如 `10.126.126.1/24`：
/// 去掉前缀后缀，复制出来就是可以直接粘贴使用的地址。
fn normalize_ipv4(raw: &str) -> String {
    lan::parse_ipv4(raw)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| raw.to_string())
}

/// 把文件截断为 0 字节（不存在则创建）。
fn truncate_file(path: &Path) -> std::io::Result<()> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map(|_| ())
}

fn binary_names() -> (&'static str, &'static str) {
    #[cfg(windows)]
    {
        ("easytier-core.exe", "easytier-cli.exe")
    }
    #[cfg(not(windows))]
    {
        ("easytier-core", "easytier-cli")
    }
}

fn resolve_binaries() -> (Option<PathBuf>, Option<PathBuf>) {
    let (core_name, cli_name) = binary_names();
    for dir in candidate_dirs() {
        let core = dir.join(core_name);
        if core.is_file() {
            let cli = dir.join(cli_name);
            return (Some(core), cli.is_file().then_some(cli));
        }
    }
    (None, None)
}

fn candidate_dirs() -> Vec<PathBuf> {
    let platform = platform_dir();
    let mut dirs = Vec::new();

    // 打包态优先：安装后的 app 必须用包内的 core，不能落到构建机上的仓库路径
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            #[cfg(target_os = "macos")]
            {
                // macOS 的 bundle 资源在 Contents/Resources：LanLink.app/Contents/Resources/core/<平台>
                dirs.push(
                    exe_dir
                        .join("..")
                        .join("Resources")
                        .join("core")
                        .join(platform),
                );
                dirs.push(
                    exe_dir
                        .join("..")
                        .join("Resources")
                        .join("resources")
                        .join("core")
                        .join(platform),
                );
            }
            // Windows 的 resource_dir 就是 exe 所在目录：<安装目录>/core/<平台>
            #[cfg(windows)]
            {
                dirs.push(exe_dir.join("core").join(platform));
                dirs.push(exe_dir.join("resources").join("core").join(platform));
            }
        }
    }

    // 开发态兜底：仓库里的 packages/core/<platform>（tauri dev 时没有包内资源）
    dirs.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("core")
            .join(platform),
    );

    dirs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// 端口回收自测：先人为让 easytier-core 占住 15888/11010，再执行清理。
    /// `cargo test reclaim_smoke -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn reclaim_smoke() {
        let core = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("core")
            .join(platform_dir())
            .join("easytier-core");
        assert!(core.exists(), "缺少 core 二进制：{}", core.display());

        let before = port_holders();
        println!(
            "清理前占用：{:?}",
            before.iter().map(|h| (h.pid, h.port)).collect::<Vec<_>>()
        );
        assert!(!before.is_empty(), "请先人为占住端口");

        let tunnel = Tunnel::new();
        let blockers = tunnel.reclaim_ports(&core);
        if blockers.is_empty() {
            println!("清理结果：端口已全部释放");
        } else {
            println!("清理结果：{}", blocker_message(&blockers));
        }
        println!("端口空闲：{}", wait_ports_free(Duration::from_secs(1)));

        assert!(blockers.is_empty(), "{}", blocker_message(&blockers));
    }

    /// 清空日志要真的把文件截断成 0 字节，而不是只清界面。
    #[test]
    fn clear_logs_truncates_the_file() {
        let dir = std::env::temp_dir().join(format!("lanlink-clear-logs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("core.log");
        fs::write(&log, "旧日志一行\n".repeat(200)).unwrap();
        assert!(fs::metadata(&log).unwrap().len() > 0);

        let mut tunnel = Tunnel::new();
        tunnel.log_dir = dir.clone();
        tunnel.clear_logs().expect("清空应当成功");
        assert_eq!(
            fs::metadata(&log).unwrap().len(),
            0,
            "日志文件应被截断为 0 字节"
        );

        // 文件还不存在时也要能成功（create(true)），不能因此报错。
        fs::remove_file(&log).unwrap();
        tunnel.clear_logs().expect("文件不存在时也应成功");
        assert_eq!(fs::metadata(&log).unwrap().len(), 0);

        let _ = fs::remove_dir_all(&dir);
    }

    /// 无写权限时必须给出可读提示，不能静默失败。
    #[cfg(unix)]
    #[test]
    fn clear_logs_reports_when_it_cannot_clear() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("lanlink-clear-logs-ro-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("core.log");
        fs::write(&log, "root 写的日志\n").unwrap();
        // 文件只读 + 目录只读：既不能截断也不能删除，模拟最坏情况。
        fs::set_permissions(&log, fs::Permissions::from_mode(0o444)).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();

        let mut tunnel = Tunnel::new();
        tunnel.log_dir = dir.clone();
        let result = tunnel.clear_logs();

        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o755));
        let _ = fs::set_permissions(&log, fs::Permissions::from_mode(0o644));

        // 以 root 身份跑测试时仍能截断成功，此时跳过断言，避免误报。
        if let Err(message) = result {
            assert!(message.contains("管理员"), "提示应说明是权限问题：{message}");
            assert!(
                fs::metadata(&log).unwrap().len() > 0,
                "失败时不能把文件清掉"
            );
        }

        let _ = fs::remove_dir_all(&dir);
    }

    /// 历史遗留的 root 日志（普通用户截断不了）也要能清：删掉旧文件再重建。
    #[cfg(unix)]
    #[test]
    fn clear_logs_falls_back_to_recreate() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("lanlink-clear-logs-recreate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("core.log");
        fs::write(&log, "旧日志\n".repeat(50)).unwrap();
        // 只读文件（目录仍可写）：截断会 EACCES，删除+重建应当成功。
        fs::set_permissions(&log, fs::Permissions::from_mode(0o444)).unwrap();

        let mut tunnel = Tunnel::new();
        tunnel.log_dir = dir.clone();
        tunnel.clear_logs().expect("应退回删除重建并成功");
        assert_eq!(fs::metadata(&log).unwrap().len(), 0, "日志文件应被清空");

        let _ = fs::remove_dir_all(&dir);
    }

    /// 节点信息里的地址带 `/24`，界面复制出来的必须是可直接使用的纯地址。
    #[test]
    fn normalizes_node_ipv4() {
        assert_eq!(normalize_ipv4("10.126.126.1/24"), "10.126.126.1");
        assert_eq!(normalize_ipv4("10.126.126.1"), "10.126.126.1");
        // 解析不了就原样保留，至少不要显示别的东西
        assert_eq!(normalize_ipv4("-"), "-");
    }

    /// Windows 的端口占用解析：只认监听/绑定行，且按端口过滤、按 PID 去重。
    #[test]
    fn parses_windows_netstat_output() {
        let sample = "\
活动连接

  协议  本地地址          外部地址        状态           PID
  TCP    0.0.0.0:15888         0.0.0.0:0              LISTENING       4321
  TCP    [::]:15888            [::]:0                 LISTENING       4321
  TCP    192.168.1.5:53211      10.0.0.9:443           ESTABLISHED     9999
  TCP    127.0.0.1:11010        127.0.0.1:60000        ESTABLISHED     7777
  UDP    0.0.0.0:27015          *:*                                    5555
  UDP    [::]:11010             *:*                                    5555
  TCP    0.0.0.0:445            0.0.0.0:0              LISTENING       4
";
        let holders = parse_netstat(sample, &[15888, 11010, 27015]);
        let got: Vec<(u32, u16)> = holders.iter().map(|h| (h.pid, h.port)).collect();
        assert_eq!(got, vec![(4321, 15888), (5555, 27015), (5555, 11010)]);
        // 外部地址不是通配的 TCP 行（已建立的连接）不能被当成占用者
        assert!(!got.iter().any(|(pid, _)| *pid == 9999 || *pid == 7777));
        // 不在关注列表里的端口忽略
        assert!(!got.iter().any(|(_, port)| *port == 445));
    }

    /// 断开后 status 不能再带本次房间信息：界面据此收起「当前房间」卡片，
    /// 也就不会因为拿不到注册结果而落到「注册失败」的兜底文案。
    #[test]
    fn stop_clears_room_identity() {
        let mut tunnel = Tunnel::new();
        // 不碰真实 core：避免测试期间误杀正在运行的 easytier-core。
        tunnel.core = None;
        tunnel.log_dir = std::env::temp_dir().join(format!("lanlink-stop-{}", std::process::id()));
        tunnel.state = State::Connected;
        tunnel.room = Some(Room {
            code: "ROOM-8K3M".to_string(),
            name: "测试房间".to_string(),
            network: "lanlink-8k3m".to_string(),
            secret: "secret".to_string(),
            role: "host".to_string(),
            relays: vec!["tcp://127.0.0.1:11010".to_string()],
            kind: "short".to_string(),
            shared: true,
            registration: "ok".to_string(),
            created_at: 0,
            last_used: 0,
        });
        tunnel.hostname = Some("host".to_string());

        let status = tunnel.stop();
        assert_eq!(status.state, State::Idle);
        assert!(status.room.is_none(), "断开后不应再报告房间码");
        assert!(status.room_name.is_none(), "断开后不应再报告房间名");
        assert!(status.role.is_none(), "断开后不应再报告身份");
        assert!(status.network.is_none(), "断开后不应再报告网络名");
    }

    /// 缺 DLL 的退出码要翻译成人话，同时保留原始码给「技术信息」。
    #[test]
    fn describe_exit_maps_missing_runtime_dll() {
        let (message, hint) = describe_exit(Some(-1073741515));
        assert!(message.contains("缺少运行库"), "应指出缺少运行库：{message}");
        assert!(message.contains("0xC0000135"), "应保留原始状态码：{message}");
        assert!(hint.unwrap_or_default().contains("重新安装"));

        // 未知退出码保持原来的技术文案，不能吞掉状态码。
        let (other, other_hint) = describe_exit(Some(1));
        assert!(other.contains("状态码 1"));
        assert!(other_hint.is_none());
    }

    /// 退出原因只看本次 spawn 之后新写入的日志，旧会话的端口冲突不能算到本次头上。
    #[test]
    fn tail_log_from_ignores_previous_session() {
        let dir = std::env::temp_dir().join(format!("lanlink-log-mark-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("core.log");
        fs::write(&log, "error: address already in use\n").unwrap();

        let mut tunnel = Tunnel::new();
        tunnel.log_dir = dir.clone();
        tunnel.log_mark = fs::metadata(&log).unwrap().len();

        assert!(
            tunnel.tail_log_from(Some(tunnel.log_mark)).is_empty(),
            "本次还没写日志时不应读到上一次会话的旧行"
        );

        fs::write(&log, "error: address already in use\n新一行\n").unwrap();
        let lines = tunnel.tail_log_from(Some(tunnel.log_mark));
        assert_eq!(lines, vec!["新一行".to_string()]);

        let _ = fs::remove_dir_all(&dir);
    }
}
