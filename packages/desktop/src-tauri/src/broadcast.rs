//! 通用 UDP 广播转发（按端口表），用于 CS 1.6 / 魔兽 / 帝国时代这类老游戏的局域网联机。
//!
//! 与 `lan` 的「伪造 MC 公告」不同，这里做的是**按端口的广播搬运**：
//!
//! 1. 在配置的端口上监听 UDP（`0.0.0.0:<端口>`，允许复用、允许广播）；
//! 2. 收到**本机游戏进程**发出的包（源地址是本机网卡）时，把它包成
//!    `{"lanlink":1,"port":..,"kind":..,"data":base64}` 发给房间内对端的同一端口；
//! 3. 对端收到后在本机按原端口重新广播（或按下面规则单播给本机游戏），
//!    游戏服务器/客户端就能互相看见；
//! 4. 用 `(端口, 报文哈希)` 的 2 秒缓存防止回灌成环；只转发本机发出的包，
//!    不会把物理局域网里别人家的游戏搬进房间。
//!
//! 方向判定（覆盖「服务器主动广播自己」和「先问再答」两种）：
//! - 源端口 == 监听端口：多半是服务器自己的公告/回包，记为 `Server`，对端一律重新广播；
//! - 源端口是临时端口：是客户端的查询，记为 `Client`，对端单播给本机上次出现公告的服务器地址
//!   （不知道就退化成广播）。
//!
//! 已知限制（不做硬撑）：
//! - 无法区分广播与单播目的地（`recv_from` 拿不到目的地址、也没有 IP_PKTINFO），
//!   因此同一端口上的普通单播也会被搬运；
//! - 同一端口同时有多个客户端查询时，回包只会送到「最近一次查询」的地址；
//! - 只发 `255.255.255.255`（不发各网段的定向广播），个别只认定向广播的游戏可能收不到；
//! - 监听靠 `0.0.0.0:<端口>` + `SO_REUSEADDR` 接住广播。若本机游戏自己也绑在同一端口，
//!   操作系统的分发策略可能把**单播**交给这个监听（而不是游戏），因此开着转发时
//!   建议只保留真正需要搬运的方向，用端口表逐项开关，出问题先删掉对应端口。
//!
//! 端口默认表覆盖常见老游戏；设置为空数组即整体关闭。

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};

use crate::lan;

/// 同步报文版本（与 lan 的同步无关，独立演进）。
const VERSION: u32 = 1;
/// `(端口, 哈希)` 去重窗口：够挡住自己的回灌，又不会误杀正常的连续请求。
const DEDUPE_TTL: Duration = Duration::from_secs(2);
/// 单端口读超时 / 主循环步长。
const TICK: Duration = Duration::from_millis(200);
const MAX_PACKET: usize = 4096;

/// 内置的常见老游戏局域网端口。
const DEFAULT_PORTS: &[u16] = &[
    27015, 27016, 27017, // CS 1.6 / 半条命（GoldSrc）
    6112, 6113, 6114, // 魔兽争霸 III、星际争霸
    2300, 2301, // 帝国时代 / 神话时代
    6073, 47624, // DirectPlay 辅助端口
    7777, 7778, // 虚幻竞技场
    27960, // Quake III
];

/// 内置默认端口（不包含 LanLink 自身的协议端口）。
pub fn default_ports() -> Vec<u16> {
    normalize_ports(DEFAULT_PORTS)
}

/// 规整端口表：去掉 0、以及被 lan 同步/组播与开放游戏宣告占用的端口，去重后排序。
pub fn normalize_ports(input: &[u16]) -> Vec<u16> {
    let mut ports: Vec<u16> = input
        .iter()
        .copied()
        .filter(|port| {
            *port != 0
                && *port != lan::MC_PORT
                && *port != 4448
                && *port != crate::games::GAME_PORT
        })
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Kind {
    /// 服务器侧发出（公告 / 回包），对端重新广播
    Server,
    /// 客户端侧发出（查询），对端单播给本机服务器
    Client,
}

#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    lanlink: u32,
    port: u16,
    kind: Kind,
    /// 原始报文的 base64，避免 JSON 转义破坏二进制内容
    data: String,
}

impl Envelope {
    fn encode(port: u16, kind: Kind, payload: &[u8]) -> Option<Vec<u8>> {
        serde_json::to_vec(&Envelope {
            lanlink: VERSION,
            port,
            kind,
            data: base64::engine::general_purpose::STANDARD.encode(payload),
        })
        .ok()
    }

    fn decode(bytes: &[u8]) -> Option<(u16, Kind, Vec<u8>)> {
        let envelope: Envelope = serde_json::from_slice(bytes).ok()?;
        if envelope.lanlink != VERSION {
            return None;
        }
        let data = base64::engine::general_purpose::STANDARD
            .decode(envelope.data.as_bytes())
            .ok()?;
        Some((envelope.port, envelope.kind, data))
    }
}

/// 稳定的报文哈希（FNV-1a）。只用于 2 秒内去重，不需要密码学强度。
fn hash(port: u16, payload: &[u8]) -> (u16, u64) {
    let mut value: u64 = 0xcbf2_9ce4_8422_2325;
    value ^= u64::from(port);
    value = value.wrapping_mul(0x100_0000_01b3);
    for byte in payload {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x100_0000_01b3);
    }
    (port, value)
}

struct Shared {
    log: PathBuf,
    stop: AtomicBool,
    self_ips: Vec<Ipv4Addr>,
    /// 对端虚拟 IP：由 `core` 统一轮询 `easytier-cli peer` 后写入
    peers: Arc<Mutex<Vec<Ipv4Addr>>>,
    seen: Mutex<HashMap<(u16, u64), Instant>>,
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
        let peers = self.peers.lock().unwrap();
        peers.contains(&ip) || (peers.is_empty() && lan::is_private(ip) && !self.is_self(ip))
    }

    /// 返回 `true` 表示这个报文最近已经处理过（应当丢弃）。
    fn seen(&self, port: u16, payload: &[u8]) -> bool {
        let now = Instant::now();
        let key = hash(port, payload);
        let mut seen = self.seen.lock().unwrap();
        seen.retain(|_, at| now.saturating_duration_since(*at) < DEDUPE_TTL);
        seen.insert(key, now).is_some()
    }

    fn log(&self, level: &str, message: &str) {
        let line = format!("{} {} lanlink::bcast: {}\n", lan::timestamp(), level, message);
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
        self.log("ERROR", message);
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// 单个端口的转发状态。
struct Port {
    port: u16,
    socket: UdpSocket,
    /// 本机最近一次听到的「服务器」地址（源端口 == 监听端口）
    server: Option<SocketAddrV4>,
}

/// 广播转发器。`Drop` 即停止并释放所有端口。
pub struct BroadcastRelay {
    shared: Arc<Shared>,
    handles: Vec<JoinHandle<()>>,
}

impl BroadcastRelay {
    pub fn start(
        ports: &[u16],
        log: PathBuf,
        peers: Arc<Mutex<Vec<Ipv4Addr>>>,
    ) -> Result<Self, String> {
        let ports = normalize_ports(ports);
        if ports.is_empty() {
            return Err("没有配置任何广播转发端口".to_string());
        }
        let self_ips = lan::local_ipv4s();
        let shared = Arc::new(Shared {
            log,
            stop: AtomicBool::new(false),
            self_ips,
            peers,
            seen: Mutex::new(HashMap::new()),
            logged: Mutex::new(HashSet::new()),
            error: Mutex::new(None),
        });

        let mut states = Vec::new();
        for port in &ports {
            match bind(*port) {
                Ok(socket) => states.push(Port {
                    port: *port,
                    socket,
                    server: None,
                }),
                Err(err) => shared.log(
                    "WARN",
                    &format!("端口 {port} 无法监听，已跳过：{err}"),
                ),
            }
        }
        if states.is_empty() {
            return Err("所有配置的广播端口都无法监听".to_string());
        }

        let mut handles = Vec::new();
        for state in states {
            let shared = shared.clone();
            handles.push(thread::spawn(move || port_loop(shared, state)));
        }

        shared.log(
            "INFO",
            &format!(
                "UDP 广播转发已启动，端口：{}",
                ports
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
        Ok(Self { shared, handles })
    }

    /// 线程是否都还在跑：`JoinHandle` 已结束（panic 或异常退出）也算不存活。
    pub fn alive(&self) -> bool {
        !self.shared.stopped() && !self.handles.iter().any(|handle| handle.is_finished())
    }

    pub fn error(&self) -> Option<String> {
        if let Some(error) = self.shared.error.lock().unwrap().clone() {
            return Some(error);
        }
        if !self.shared.stopped() && self.handles.iter().any(|handle| handle.is_finished()) {
            return Some("UDP 广播转发线程意外退出（可能发生 panic），已停止".to_string());
        }
        None
    }

    pub fn stop(&mut self) {
        if self.shared.stopped() && self.handles.is_empty() {
            return;
        }
        self.shared.stop.store(true, Ordering::Relaxed);
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
        self.shared.log("INFO", "UDP 广播转发已停止");
    }
}

impl Drop for BroadcastRelay {
    fn drop(&mut self) {
        self.stop();
    }
}

fn bind(port: u16) -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)).into())?;
    socket.set_broadcast(true)?;
    let socket: UdpSocket = socket.into();
    socket.set_nonblocking(true)?;
    Ok(socket)
}

fn port_loop(shared: Arc<Shared>, mut state: Port) {
    let mut buf = [0u8; MAX_PACKET];
    while !shared.stopped() {
        loop {
            match state.socket.recv_from(&mut buf) {
                Ok((len, source)) => {
                    let SocketAddr::V4(source) = source else {
                        continue;
                    };
                    let payload = &buf[..len];

                    if let Some((port, kind, data)) = Envelope::decode(payload) {
                        // 来自对端的搬运包
                        if port != state.port || !shared.is_peer(*source.ip()) {
                            continue;
                        }
                        if shared.seen(port, &data) {
                            continue;
                        }
                        let target = match kind {
                            Kind::Client => state.server,
                            Kind::Server => None,
                        };
                        match target {
                            Some(address) => {
                                let _ = state.socket.send_to(&data, address);
                            }
                            None => broadcast(&shared, &state, &data),
                        }
                        continue;
                    }

                    // 本机游戏进程发出的包：只搬本机的，别把物理局域网里别人的游戏搬进来
                    if !shared.is_self(*source.ip()) {
                        continue;
                    }
                    if shared.seen(state.port, payload) {
                        continue;
                    }
                    let kind = if source.port() == state.port {
                        state.server = Some(source);
                        Kind::Server
                    } else {
                        Kind::Client
                    };
                    let Some(envelope) = Envelope::encode(state.port, kind, payload) else {
                        continue;
                    };
                    for peer in shared.peers.lock().unwrap().clone() {
                        if let Err(err) =
                            state.socket.send_to(&envelope, SocketAddr::from((peer, state.port)))
                        {
                            shared.log_once(
                                &format!("send-{}-{peer}", state.port),
                                "WARN",
                                &format!("向对端 {peer}:{} 转发广播失败：{err}", state.port),
                            );
                        }
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => {
                    shared.fail(&format!("端口 {} 读取失败：{err}", state.port));
                    return;
                }
            }
        }

        if shared.stopped() {
            break;
        }
        thread::sleep(TICK);
    }
}

fn broadcast(shared: &Shared, state: &Port, payload: &[u8]) {
    if let Err(err) = state
        .socket
        .send_to(payload, SocketAddr::from((Ipv4Addr::BROADCAST, state.port)))
    {
        shared.log_once(
            &format!("bcast-{}", state.port),
            "WARN",
            &format!("端口 {} 本机广播失败：{err}", state.port),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_classic_games_and_exclude_own_ports() {
        let ports = default_ports();
        assert!(ports.contains(&27015)); // CS 1.6
        assert!(ports.contains(&6112)); // 魔兽 / 星际
        assert!(ports.contains(&2300)); // 帝国时代
        assert!(!ports.contains(&lan::MC_PORT));
        assert_eq!(normalize_ports(&ports), ports);
    }

    #[test]
    fn normalize_drops_reserved_and_duplicates() {
        let ports = normalize_ports(&[
            0,
            27015,
            27015,
            lan::MC_PORT,
            4448,
            crate::games::GAME_PORT,
            6112,
        ]);
        assert_eq!(ports, vec![6112, 27015]);
        assert!(normalize_ports(&[]).is_empty());
    }

    #[test]
    fn envelope_round_trip_keeps_binary_payload() {
        let payload = [0u8, 1, 2, 255, 10, 13];
        let bytes = Envelope::encode(27015, Kind::Server, &payload).unwrap();
        let (port, kind, data) = Envelope::decode(&bytes).unwrap();
        assert_eq!(port, 27015);
        assert_eq!(kind, Kind::Server);
        assert_eq!(data, payload);
    }

    #[test]
    fn envelope_rejects_junk_and_other_versions() {
        assert!(Envelope::decode(b"not json").is_none());
        assert!(Envelope::decode(br#"{"lanlink":2,"port":1,"kind":"server","data":""}"#).is_none());
    }

    #[test]
    fn hash_is_stable_and_port_sensitive() {
        assert_eq!(hash(27015, b"x"), hash(27015, b"x"));
        assert_ne!(hash(27015, b"x"), hash(27016, b"x"));
        assert_ne!(hash(27015, b"x"), hash(27015, b"y"));
    }

    #[test]
    fn dedupe_window_expires() {
        let shared = Shared {
            log: std::env::temp_dir().join("lanlink-bcast-test.log"),
            stop: AtomicBool::new(false),
            self_ips: Vec::new(),
            peers: Arc::new(Mutex::new(Vec::new())),
            seen: Mutex::new(HashMap::new()),
            logged: Mutex::new(HashSet::new()),
            error: Mutex::new(None),
        };
        assert!(!shared.seen(27015, b"hello")); // 第一次：新的
        assert!(shared.seen(27015, b"hello")); // 立刻再来：重复
        // 手动把时间推到窗口之外，应重新视为新的
        shared
            .seen
            .lock()
            .unwrap()
            .insert(hash(27015, b"hello"), Instant::now() - DEDUPE_TTL);
        assert!(!shared.seen(27015, b"hello"));
    }

    #[test]
    fn relay_releases_ports_on_stop() {
        // 用高位端口，避免和真机上的游戏冲突
        let port = 45_123u16;
        let mut relay = BroadcastRelay::start(
            &[port],
            std::env::temp_dir().join("lanlink-bcast-test.log"),
            Arc::new(Mutex::new(Vec::new())),
        )
        .expect("启动广播转发失败");
        assert!(relay.alive());
        relay.stop();
        assert!(!relay.alive());
        // 停止后端口应能重新绑定（说明监听已释放）
        UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)).expect("端口没有释放");
    }
}
