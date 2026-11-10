//! 房间内「开放游戏」宣告通道。
//!
//! 设计取舍见 `docs/open-games.md`：固定 UDP 端口、只向对端虚拟 IP 单播、
//! 载荷不自报 IP，接收端用真实源地址校验并拼装端点。

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};

use crate::lan;

/// 开放游戏宣告的固定 UDP 端口。
pub const GAME_PORT: u16 = 4449;
/// 本通道的协议版本。
const VERSION: u32 = 1;
const CHANNEL: &str = "open-games";
const SEND_INTERVAL: Duration = Duration::from_secs(2);
const TTL: Duration = Duration::from_secs(18);
/// 空快照连续出现多少次才清空，避免游戏重启时列表闪烁。
const EMPTY_CLEAR: u32 = 3;
const TICK: Duration = Duration::from_millis(200);
const MAX_PACKET: usize = 4096;
const MAX_GAMES: usize = 16;
const MAX_NAME_CHARS: usize = 48;
/// LanLink 自己占用的端口，不允许出现在任何游戏端点里。
const RESERVED_PORTS: [u16; 3] = [lan::MC_PORT, 4448, GAME_PORT];

// ---------------------------------------------------------------------------
// 本机游戏表
// ---------------------------------------------------------------------------

/// 用户手动登记的一条开放游戏。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualGame {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub port: u16,
}

/// 本机当前对外宣告的一条游戏。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalGame {
    pub port: u16,
    pub name: String,
    pub source: String,
}

/// 本机游戏来源：`lan.rs` 写入《我的世界》自动发现，设置页写入手动登记。
#[derive(Debug, Default)]
pub struct LocalGamesState {
    minecraft: HashMap<u16, String>,
    manual: Vec<ManualGame>,
}

pub type LocalGames = Arc<Mutex<LocalGamesState>>;

impl LocalGamesState {
    pub fn with_manual(games: Vec<ManualGame>) -> Self {
        let mut state = Self::default();
        state.set_manual(games);
        state
    }

    pub fn set_manual(&mut self, games: Vec<ManualGame>) {
        self.manual = normalize_manual(&games);
    }

    pub fn set_minecraft(&mut self, worlds: Vec<(u16, String)>) {
        let mut next = HashMap::new();
        for (port, motd) in worlds {
            if port == 0 || RESERVED_PORTS.contains(&port) {
                continue;
            }
            next.insert(port, clean_name(&motd, "我的世界"));
        }
        self.minecraft = next;
    }

    pub fn clear_minecraft(&mut self) {
        self.minecraft.clear();
    }

    /// 对外宣告用的合并快照：同端口时自动发现的世界名更准确，覆盖手动登记。
    pub fn snapshot(&self) -> Vec<LocalGame> {
        let mut games: HashMap<u16, LocalGame> = HashMap::new();
        for game in &self.manual {
            games.insert(
                game.port,
                LocalGame {
                    port: game.port,
                    name: game.name.clone(),
                    source: "manual".to_string(),
                },
            );
        }
        for (port, name) in &self.minecraft {
            games.insert(
                *port,
                LocalGame {
                    port: *port,
                    name: name.clone(),
                    source: "minecraft".to_string(),
                },
            );
        }
        let mut games: Vec<LocalGame> = games.into_values().collect();
        games.sort_by_key(|game| game.port);
        // 单包上限：端口升序保留前 MAX_GAMES 条，避免超长包被整体丢弃
        games.truncate(MAX_GAMES);
        games
    }
}

/// 过滤非法端口、重复端口，并统一名称格式。
pub fn normalize_manual(games: &[ManualGame]) -> Vec<ManualGame> {
    let mut normalized: Vec<ManualGame> = Vec::new();
    for game in games {
        if game.port == 0 || RESERVED_PORTS.contains(&game.port) {
            continue;
        }
        if normalized.iter().any(|item| item.port == game.port) {
            continue;
        }
        normalized.push(ManualGame {
            name: clean_name(&game.name, "局域网游戏"),
            port: game.port,
        });
    }
    normalized.sort_by_key(|game| game.port);
    normalized
}

fn clean_name(raw: &str, fallback: &str) -> String {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let name: String = collapsed.chars().take(MAX_NAME_CHARS).collect();
    if name.is_empty() {
        fallback.to_string()
    } else {
        name
    }
}

fn normalize_source(raw: &str) -> String {
    match raw {
        "minecraft" => "minecraft".to_string(),
        "manual" => "manual".to_string(),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// 报文
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct Advertise {
    lanlink: u32,
    #[serde(default)]
    channel: String,
    #[serde(default)]
    network: String,
    #[serde(default)]
    games: Vec<WireGame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WireGame {
    port: u16,
    #[serde(default)]
    name: String,
    #[serde(default)]
    source: String,
}

fn parse_advertise(bytes: &[u8]) -> Option<Advertise> {
    let message: Advertise = serde_json::from_slice(bytes).ok()?;
    if message.lanlink != VERSION || message.channel != CHANNEL {
        return None;
    }
    Some(message)
}

// ---------------------------------------------------------------------------
// 对端游戏表
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct RemoteGame {
    name: String,
    source: String,
}

#[derive(Debug)]
struct PeerState {
    games: HashMap<u16, RemoteGame>,
    empty_streak: u32,
    seen: Instant,
}

#[derive(Debug, Default)]
struct RemoteTable {
    peers: HashMap<Ipv4Addr, PeerState>,
}

/// 接收端看到的对端游戏。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteGameSnapshot {
    pub ip: Ipv4Addr,
    pub port: u16,
    pub name: String,
    pub source: String,
}

impl RemoteTable {
    fn count_for(&self, ip: Ipv4Addr) -> usize {
        self.peers.get(&ip).map(|state| state.games.len()).unwrap_or(0)
    }

    /// 最近 `window` 内还在发宣告的对端（列表为空也算在线）。
    ///
    /// 用来区分「对端主动关掉了世界」和「对端的宣告被网络吞了」：前者不能留转发，
    /// 后者必须留，否则列表里的条目会跟着 UDP 抖动一起闪。
    fn live(&self, now: Instant, window: Duration) -> Vec<Ipv4Addr> {
        let mut peers: Vec<Ipv4Addr> = self
            .peers
            .iter()
            .filter(|(_, state)| now.saturating_duration_since(state.seen) < window)
            .map(|(ip, _)| *ip)
            .collect();
        peers.sort_unstable();
        peers
    }

    /// 应用一条完整快照，返回更新后该对端的游戏数量。
    fn apply(&mut self, ip: Ipv4Addr, games: Vec<WireGame>, now: Instant) -> usize {
        let state = self.peers.entry(ip).or_insert_with(|| PeerState {
            games: HashMap::new(),
            empty_streak: 0,
            seen: now,
        });
        state.seen = now;

        if games.is_empty() {
            state.empty_streak = state.empty_streak.saturating_add(1);
            if state.empty_streak >= EMPTY_CLEAR {
                state.games.clear();
            }
            return state.games.len();
        }

        state.empty_streak = 0;
        let mut next = HashMap::new();
        for game in games.into_iter().take(MAX_GAMES) {
            if game.port == 0 || RESERVED_PORTS.contains(&game.port) {
                continue;
            }
            next.insert(
                game.port,
                RemoteGame {
                    name: clean_name(&game.name, "局域网游戏"),
                    source: normalize_source(&game.source),
                },
            );
        }
        state.games = next;
        state.games.len()
    }

    fn sweep(&mut self, now: Instant) -> Vec<Ipv4Addr> {
        let mut gone = Vec::new();
        self.peers.retain(|ip, state| {
            let alive = now.saturating_duration_since(state.seen) < TTL;
            if !alive {
                gone.push(*ip);
            }
            alive
        });
        gone.sort_unstable();
        gone
    }

    fn snapshot(&self) -> Vec<RemoteGameSnapshot> {
        let mut games = Vec::new();
        for (ip, state) in &self.peers {
            for (port, game) in &state.games {
                games.push(RemoteGameSnapshot {
                    ip: *ip,
                    port: *port,
                    name: game.name.clone(),
                    source: game.source.clone(),
                });
            }
        }
        games.sort_by(|a, b| a.ip.cmp(&b.ip).then(a.port.cmp(&b.port)));
        games
    }
}

// ---------------------------------------------------------------------------
// 中继
// ---------------------------------------------------------------------------

struct Shared {
    network: String,
    log: PathBuf,
    stop: AtomicBool,
    port: u16,
    self_ips: Vec<Ipv4Addr>,
    /// 对端虚拟 IP：由 `core` 统一轮询 `easytier-cli peer` 后写入。
    peers: Arc<Mutex<Vec<Ipv4Addr>>>,
    local: LocalGames,
    remote: Mutex<RemoteTable>,
    logged: Mutex<HashSet<String>>,
    error: Mutex<Option<String>>,
}

impl Shared {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn is_self(&self, ip: Ipv4Addr) -> bool {
        self.self_ips.contains(&ip)
    }

    fn is_peer(&self, ip: Ipv4Addr) -> bool {
        if self.is_self(ip) {
            return false;
        }
        let peers = self.peers.lock().unwrap();
        if peers.contains(&ip) {
            return true;
        }
        // 刚连上时 peer 列表可能还没刷新出来，用私有网段兜底。
        peers.is_empty() && lan::is_private(ip)
    }

    fn log(&self, level: &str, message: &str) {
        let line = format!(
            "{} {} lanlink::games: {}\n",
            lan::timestamp(),
            level,
            message
        );
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&self.log) {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn log_once(&self, key: &str, level: &str, message: &str) {
        if self.logged.lock().unwrap().insert(key.to_string()) {
            self.log(level, message);
        }
    }

    fn fail(&self, message: &str) {
        *self.error.lock().unwrap() = Some(message.to_string());
        self.log_once("fatal", "ERROR", message);
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// 开放游戏宣告中继。`Drop` 即停止并释放 4449。
pub struct GameRelay {
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
}

impl GameRelay {
    pub fn start(
        network: String,
        log: PathBuf,
        peers: Arc<Mutex<Vec<Ipv4Addr>>>,
        local: LocalGames,
    ) -> Result<Self, String> {
        Self::start_with_port(network, log, peers, local, GAME_PORT)
    }

    /// `port` 仅用于测试（避免与真实运行的客户端抢占 4449）。
    fn start_with_port(
        network: String,
        log: PathBuf,
        peers: Arc<Mutex<Vec<Ipv4Addr>>>,
        local: LocalGames,
        port: u16,
    ) -> Result<Self, String> {
        let socket = bind(port).map_err(|err| format!("UDP {port} 无法监听：{err}"))?;
        let bound = socket
            .local_addr()
            .map(|addr| addr.port())
            .unwrap_or(port);
        let self_ips = lan::local_ipv4s();

        let shared = Arc::new(Shared {
            network,
            log,
            stop: AtomicBool::new(false),
            port: bound,
            self_ips,
            peers,
            local,
            remote: Mutex::new(RemoteTable::default()),
            logged: Mutex::new(HashSet::new()),
            error: Mutex::new(None),
        });

        shared.log(
            "INFO",
            &format!(
                "开放游戏宣告已启动（UDP {}，每 {} 秒发送一次，网络 {}）",
                shared.port,
                SEND_INTERVAL.as_secs(),
                shared.network
            ),
        );

        let loop_shared = shared.clone();
        let handle = thread::spawn(move || relay_loop(loop_shared, socket));

        Ok(Self {
            shared,
            handle: Some(handle),
        })
    }

    pub fn alive(&self) -> bool {
        !self.shared.stopped()
            && self
                .handle
                .as_ref()
                .map(|handle| !handle.is_finished())
                .unwrap_or(false)
    }

    pub fn error(&self) -> Option<String> {
        if let Some(error) = self.shared.error.lock().unwrap().clone() {
            return Some(error);
        }
        if !self.shared.stopped()
            && self
                .handle
                .as_ref()
                .map(|handle| handle.is_finished())
                .unwrap_or(false)
        {
            return Some("开放游戏宣告线程意外退出（可能发生 panic），已停止".to_string());
        }
        None
    }

    pub fn snapshot(&self) -> Vec<RemoteGameSnapshot> {
        self.shared.remote.lock().unwrap().snapshot()
    }

    /// 最近 `window` 内还在发宣告的对端虚拟 IP（列表为空也算）。
    pub fn live_peers(&self, window: Duration) -> Vec<Ipv4Addr> {
        self.shared.remote.lock().unwrap().live(Instant::now(), window)
    }

    pub fn stop(&mut self) {
        if self.shared.stopped() && self.handle.is_none() {
            return;
        }
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.shared.log("INFO", "开放游戏宣告已停止");
    }
}

impl Drop for GameRelay {
    fn drop(&mut self) {
        self.stop();
    }
}

fn bind(port: u16) -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)).into())?;
    let socket: UdpSocket = socket.into();
    socket.set_nonblocking(true)?;
    Ok(socket)
}

fn relay_loop(shared: Arc<Shared>, socket: UdpSocket) {
    let mut buf = [0u8; MAX_PACKET];
    let mut last_send: Option<Instant> = None;
    let mut last_sweep = Instant::now();

    while !shared.stopped() {
        // 1) 收报文（非阻塞，一次排空队列）
        loop {
            match socket.recv_from(&mut buf) {
                Ok((len, source)) => {
                    let SocketAddr::V4(source) = source else {
                        continue;
                    };
                    let ip = *source.ip();
                    if !shared.is_peer(ip) {
                        continue;
                    }
                    let Some(message) = parse_advertise(&buf[..len]) else {
                        continue;
                    };
                    if message.network != shared.network {
                        continue;
                    }

                    let mut remote = shared.remote.lock().unwrap();
                    let before = remote.count_for(ip);
                    let after = remote.apply(ip, message.games, Instant::now());
                    drop(remote);

                    if before == 0 && after > 0 {
                        shared.log("INFO", &format!("对端 {ip} 开放了 {after} 个游戏"));
                    } else if before > 0 && after == 0 {
                        shared.log("INFO", &format!("对端 {ip} 没有开放的游戏了"));
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => {
                    shared.fail(&format!("读取开放游戏报文失败：{err}"));
                    break;
                }
            }
        }

        if shared.stopped() {
            break;
        }

        // 2) 周期发送完整快照
        if last_send
            .map(|at| at.elapsed() >= SEND_INTERVAL)
            .unwrap_or(true)
        {
            send_advertise(&shared, &socket);
            last_send = Some(Instant::now());
        }

        // 3) 摘除静默对端
        if last_sweep.elapsed() >= Duration::from_secs(1) {
            last_sweep = Instant::now();
            for ip in shared.remote.lock().unwrap().sweep(Instant::now()) {
                shared.log(
                    "INFO",
                    &format!(
                        "对端 {ip} 超过 {} 秒没有宣告，已从开放游戏列表移除",
                        TTL.as_secs()
                    ),
                );
            }
        }

        thread::sleep(TICK);
    }
}

fn send_advertise(shared: &Shared, socket: &UdpSocket) {
    let games: Vec<WireGame> = shared
        .local
        .lock()
        .unwrap()
        .snapshot()
        .into_iter()
        .map(|game| WireGame {
            port: game.port,
            name: game.name,
            source: game.source,
        })
        .collect();

    let Ok(payload) = serde_json::to_vec(&Advertise {
        lanlink: VERSION,
        channel: CHANNEL.to_string(),
        network: shared.network.clone(),
        games,
    }) else {
        return;
    };
    if payload.len() > MAX_PACKET {
        shared.log_once(
            "payload-too-large",
            "WARN",
            &format!("开放游戏快照 {} 字节，超过 {MAX_PACKET} 上限，本次不发送", payload.len()),
        );
        return;
    }

    let peers = shared.peers.lock().unwrap().clone();
    for peer in peers {
        if shared.is_self(peer) {
            continue;
        }
        if let Err(err) = socket.send_to(&payload, SocketAddr::from((peer, GAME_PORT))) {
            shared.log_once(
                &format!("send-{peer}"),
                "WARN",
                &format!("向对端 {peer}:{GAME_PORT} 发送开放游戏失败：{err}"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Instant {
        Instant::now()
    }

    fn wire(port: u16, name: &str) -> WireGame {
        WireGame {
            port,
            name: name.to_string(),
            source: "manual".to_string(),
        }
    }

    #[test]
    fn advertise_round_trips_and_filters_version() {
        let payload = serde_json::to_vec(&Advertise {
            lanlink: VERSION,
            channel: CHANNEL.to_string(),
            network: "lanlink-test".to_string(),
            games: vec![wire(25565, "World")],
        })
        .unwrap();

        let parsed = parse_advertise(&payload).unwrap();
        assert_eq!(parsed.network, "lanlink-test");
        assert_eq!(parsed.games.len(), 1);

        assert!(parse_advertise(
            br#"{"lanlink":2,"channel":"open-games","network":"lanlink-test","games":[]}"#
        )
        .is_none());
        assert!(parse_advertise(
            br#"{"lanlink":1,"channel":"other","network":"lanlink-test","games":[]}"#
        )
        .is_none());
        assert!(parse_advertise(b"not json").is_none());
    }

    #[test]
    fn manual_entries_are_normalized() {
        let games = normalize_manual(&[
            ManualGame {
                name: String::new(),
                port: 27015,
            },
            ManualGame {
                name: "重复端口".to_string(),
                port: 27015,
            },
            ManualGame {
                name: "占用协议端口".to_string(),
                port: GAME_PORT,
            },
            ManualGame {
                name: "  CS   1.6  ".to_string(),
                port: 27016,
            },
        ]);

        assert_eq!(
            games,
            vec![
                ManualGame {
                    name: "局域网游戏".to_string(),
                    port: 27015,
                },
                ManualGame {
                    name: "CS 1.6".to_string(),
                    port: 27016,
                },
            ]
        );
    }

    #[test]
    fn local_snapshot_prefers_minecraft_for_same_port() {
        let mut state = LocalGamesState::with_manual(vec![
            ManualGame {
                name: "手动世界".to_string(),
                port: 25565,
            },
            ManualGame {
                name: "CS".to_string(),
                port: 27015,
            },
        ]);
        state.set_minecraft(vec![(25565, "Steve - World".to_string()), (25566, String::new())]);

        let snapshot = state.snapshot();
        assert_eq!(snapshot.len(), 3);
        assert_eq!(snapshot[0].port, 25565);
        assert_eq!(snapshot[0].name, "Steve - World");
        assert_eq!(snapshot[0].source, "minecraft");
        assert_eq!(snapshot[1].port, 25566);
        assert_eq!(snapshot[1].name, "我的世界");
        assert_eq!(snapshot[2].port, 27015);
        assert_eq!(snapshot[2].source, "manual");
    }

    #[test]
    fn empty_advertisement_clears_after_three_times() {
        let ip = Ipv4Addr::new(10, 126, 126, 2);
        let start = now();
        let mut table = RemoteTable::default();

        assert_eq!(table.apply(ip, vec![wire(25565, "World")], start), 1);
        assert_eq!(
            table.apply(ip, vec![], start + Duration::from_secs(2)),
            1
        );
        assert_eq!(
            table.apply(ip, vec![], start + Duration::from_secs(4)),
            1
        );
        assert_eq!(
            table.apply(ip, vec![], start + Duration::from_secs(6)),
            0
        );
    }

    #[test]
    fn non_empty_advertisement_replaces_previous_games() {
        let ip = Ipv4Addr::new(10, 126, 126, 3);
        let start = now();
        let mut table = RemoteTable::default();

        table.apply(ip, vec![wire(25565, "旧世界")], start);
        table.apply(ip, vec![wire(25566, "新世界")], start + Duration::from_secs(2));

        assert_eq!(
            table.snapshot(),
            vec![RemoteGameSnapshot {
                ip,
                port: 25566,
                name: "新世界".to_string(),
                source: "manual".to_string(),
            }]
        );
    }

    #[test]
    fn sweep_removes_silent_peer_after_ttl() {
        let ip = Ipv4Addr::new(10, 126, 126, 4);
        let start = now();
        let mut table = RemoteTable::default();
        table.apply(ip, vec![wire(25565, "World")], start);

        assert!(table
            .sweep(start + TTL - Duration::from_secs(1))
            .is_empty());
        assert_eq!(table.sweep(start + TTL), vec![ip]);
        assert!(table.snapshot().is_empty());
    }

    #[test]
    fn reserved_ports_from_peers_are_ignored() {
        let ip = Ipv4Addr::new(10, 126, 126, 5);
        let mut table = RemoteTable::default();
        let games = vec![
            wire(lan::MC_PORT, "组播"),
            wire(4448, "同步"),
            wire(GAME_PORT, "宣告"),
            wire(27015, "合法"),
        ];

        assert_eq!(table.apply(ip, games, now()), 1);
        assert_eq!(table.snapshot()[0].port, 27015);
    }

    #[test]
    fn relay_releases_port_on_stop() {
        let local = Arc::new(Mutex::new(LocalGamesState::default()));
        let peers = Arc::new(Mutex::new(Vec::new()));
        let log = std::env::temp_dir().join("lanlink-games-test.log");
        let mut relay =
            GameRelay::start_with_port("lanlink-test".to_string(), log, peers, local, 0).unwrap();
        let port = relay.shared.port;
        assert!(relay.alive());

        relay.stop();

        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)).unwrap();
        drop(socket);
    }
}
