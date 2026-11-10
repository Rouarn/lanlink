//! 《我的世界》Java 版「对局域网开放」跨虚拟网互通。
//!
//! # 为什么不是「把组播报文转给对端」
//!
//! 原版客户端与服务端的局域网发现只走组播 `224.0.2.60:4445`，EasyTier 的 TUN
//! 不转发组播；而把报文直接单播到对端 `4445` 也不行——原版客户端在这个端口上
//! 只有一个 `MulticastSocket`，同端口的单播归属不确定，而且「自己伪造的报文」
//! 与「真实公告」混在一起后源地址会变成中继进程的地址，客户端会连错 IP。
//!
//! # 采用的做法（与 Terracotta 相同）
//!
//! <https://github.com/burningtnt/Terracotta>，参考 `src/mc/scanning.rs`、
//! `src/mc/fakeserver.rs`、`src/controller/rooms/scaffolding/room.rs`：
//!
//! 1. **探端口**：监听 `224.0.2.60:4445`，解析原版公告
//!    `[MOTD]<motd>[/MOTD][AD]<端口>[/AD]`（UTF-8，约每 1.5 秒一次）。
//!    报文里**没有 IP**，原版客户端用的是 UDP 源地址；本机只认源地址属于本机网卡的公告，
//!    避免把物理局域网里邻居的世界当成自己的。超过 5 秒没再收到视为世界已关闭。
//! 2. **同步**：把「本机世界端口 + MOTD」通过虚拟网内的 UDP 心跳（`4448`）告诉房间成员。
//!    这项同步是本项目补充的最小协议——EasyTier 只提供节点/对端信息，没有应用层数据通道；
//!    报文是 `{"lanlink":1,"worlds":[{"port":25565,"motd":"..."}]}`，每个成员只会发给
//!    `easytier-cli peer` 里出现的对端虚拟 IP，不走公网。
//! 3. **造条目**：收到别人的端口后，在本机建一个 TCP 转发
//!    `0.0.0.0:<本地空闲端口> → <对端虚拟 IP>:<世界端口>`，再每 1.5 秒向本机
//!    `224.0.2.60:4445` 播一条伪造公告，AD 指向本地端口。
//!    原版「多人游戏」列表就会刷出条目；点进去走的是 LAN 条目，才有 MC-170460
//!    的离线认证豁免（手动「添加服务器」填 IP 会报「无效会话」）。
//!
//! 伪造公告的源地址是发送 socket 绑定的本机网卡地址（而不是中继进程随便选的地址），
//! 本地转发监听的又是 `0.0.0.0`，因此客户端按源地址发起的连接一定能落到转发上。
//!
//! # 协议版本
//!
//! 组播地址/端口与报文格式在 1.21.x 未变化：Yarn 的 `LanServerDetector` 仍只 join
//! IPv4 `224.0.2.60:4445`，`LanServerPinger` 仍按上面的格式每 1.5 秒播一次。
//! IPv6 组地址 `[FF75:230::60]:4445` 是 Terracotta 的兼容性补充，原版客户端并不监听，
//! 因此这里只处理 IPv4 这一个地址（不做全量广播抓取）。
//!
//! # 生命周期
//!
//! 由 `core::Tunnel` 在房间连上后启动、断开时 `stop()`；所有线程都在 200ms 内响应停止，
//! 转发端口随之释放。启动失败会带原因返回并写日志，不静默。

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};

use crate::proc;

/// 原版局域网发现的组播地址（IPv4）。
pub const MC_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 2, 60);
/// 原版局域网发现的端口。
pub const MC_PORT: u16 = 4445;
/// 房间成员之间同步「谁在开世界」用的端口，只走虚拟网。
const SYNC_PORT: u16 = 4448;
/// 同步报文的协议版本，未来改结构时用来丢弃旧报文。
const SYNC_VERSION: u32 = 1;
/// 原版每 1.5 秒播一次公告，伪造公告保持同样节奏。
const ANNOUNCE_INTERVAL: Duration = Duration::from_millis(1500);
/// 超过这个时间没再收到某个世界（或某个成员的心跳），视为它的世界已关闭。
const WORLD_TTL: Duration = Duration::from_secs(5);
/// 单个 socket 的读超时 / 主循环步长，决定停止时的响应速度。
const TICK: Duration = Duration::from_millis(200);
/// 单个公告报文的最大长度（原版只有几十字节，留足余量）。
const MAX_PACKET: usize = 2048;
/// 报文没写端口时的默认值（原版「对局域网开放」默认端口）。
const DEFAULT_PORT: u16 = 25565;

// ---------------------------------------------------------------------------
// 报文
// ---------------------------------------------------------------------------

/// 一条局域网公告：MOTD + 端口。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    pub motd: String,
    pub port: u16,
}

/// 解析 `[MOTD]<motd>[/MOTD][AD]<端口>[/AD]`。
///
/// 规则按原版 `LanServerDetector` 实现：
/// - 必须先有 `[MOTD]...[/MOTD]`，否则不是公告；
/// - MOTD 里可能原样带着 `[AD]` 字样，所以只在 `[/MOTD]` 之后再找端口块；
/// - `[AD]` 缺失或不是数字时按默认端口 25565 处理（不去猜别的值）。
pub fn parse_announcement(bytes: &[u8]) -> Option<Announcement> {
    let text = String::from_utf8_lossy(bytes);
    parse_announcement_text(&text)
}

fn parse_announcement_text(text: &str) -> Option<Announcement> {
    let head = text.find("[MOTD]")? + "[MOTD]".len();
    let rest = &text[head..];
    let end = rest.find("[/MOTD]")?;
    let motd = rest[..end].trim().to_string();
    let tail = &rest[end + "[/MOTD]".len()..];
    let port = parse_port(tail).unwrap_or(DEFAULT_PORT);
    Some(Announcement { motd, port })
}

fn parse_port(tail: &str) -> Option<u16> {
    let start = tail.find("[AD]")? + "[AD]".len();
    let rest = &tail[start..];
    let end = rest.find("[/AD]").unwrap_or(rest.len());
    rest[..end].trim().parse::<u16>().ok().filter(|p| *p > 0)
}

/// 组装一条公告。MOTD 里的控制字符与标记会被清掉，避免把报文弄坏。
pub fn build_announcement(motd: &str, port: u16) -> String {
    let cleaned = motd
        .replace("[MOTD]", " ")
        .replace("[/MOTD]", " ")
        .replace("[AD]", " ")
        .replace("[/AD]", " ");
    // 折叠空白、清掉控制字符：既避免把报文结构弄坏，也避免日志里出现奇怪字符
    let cleaned = cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let motd: String = cleaned.chars().take(256).collect();
    format!("[MOTD]{motd}[/MOTD][AD]{port}[/AD]")
}

// ---------------------------------------------------------------------------
// 世界表（TTL）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct SeenWorld {
    motd: String,
    seen: Instant,
}

/// 按端口记录「最近还有公告」的世界，5 秒不更新即摘除。
///
/// 同一端口重复公告只刷新时间（去重），不会重复上报。
#[derive(Debug, Default)]
struct WorldTracker {
    worlds: HashMap<u16, SeenWorld>,
}

impl WorldTracker {
    /// 记录一次公告，返回该端口是否是这次新出现的（便于只打一条日志）。
    fn observe(&mut self, announcement: &Announcement, now: Instant) -> bool {
        let fresh = !self.worlds.contains_key(&announcement.port);
        self.worlds.insert(
            announcement.port,
            SeenWorld {
                motd: announcement.motd.clone(),
                seen: now,
            },
        );
        fresh
    }

    /// 摘除过期条目，返回被摘掉的端口。
    fn sweep(&mut self, now: Instant, ttl: Duration) -> Vec<u16> {
        let mut expired = Vec::new();
        self.worlds.retain(|port, world| {
            let alive = now.saturating_duration_since(world.seen) < ttl;
            if !alive {
                expired.push(*port);
            }
            alive
        });
        expired.sort_unstable();
        expired
    }

    fn active(&self) -> Vec<Announcement> {
        let mut worlds: Vec<Announcement> = self
            .worlds
            .iter()
            .map(|(port, world)| Announcement {
                motd: world.motd.clone(),
                port: *port,
            })
            .collect();
        worlds.sort_by_key(|a| a.port);
        worlds
    }
}

// ---------------------------------------------------------------------------
// 同步报文
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct SyncMessage {
    lanlink: u32,
    #[serde(default)]
    worlds: Vec<SyncWorld>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SyncWorld {
    port: u16,
    #[serde(default)]
    motd: String,
}

/// 解析对端心跳；版本不符或格式错误都返回 `None`（对端可能来自更旧的版本）。
fn parse_sync(bytes: &[u8]) -> Option<SyncMessage> {
    let message: SyncMessage = serde_json::from_slice(bytes).ok()?;
    if message.lanlink != SYNC_VERSION {
        return None;
    }
    Some(message)
}

// ---------------------------------------------------------------------------
// 网络工具
// ---------------------------------------------------------------------------

/// 枚举本机 IPv4（跳过回环 / 链路本地），用作组播绑定与源地址。
pub(crate) fn local_ipv4s() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return ips;
    };
    for interface in interfaces {
        let if_addrs::IfAddr::V4(v4) = interface.addr else {
            continue;
        };
        if v4.ip.is_loopback() || v4.ip.is_unspecified() || v4.ip.is_link_local() {
            continue;
        }
        if !ips.contains(&v4.ip) {
            ips.push(v4.ip);
        }
    }
    ips
}

/// 绑定 `ip:4445` 并加入组播组。每个网卡一个 socket，原版按网卡播公告。
fn bind_receiver(ip: Ipv4Addr) -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&SocketAddr::from((ip, MC_PORT)).into())?;
    socket.join_multicast_v4(&MC_GROUP, &ip)?;
    let socket: UdpSocket = socket.into();
    socket.set_read_timeout(Some(TICK))?;
    Ok(socket)
}

/// 发公告用的 socket：绑定到具体网卡，这样报文源地址就是该网卡地址，
/// 客户端会按这个地址连回本地转发。
fn bind_sender(ip: Ipv4Addr) -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&SocketAddr::from((ip, 0)).into())?;
    socket.set_multicast_loop_v4(true)?;
    socket.set_multicast_ttl_v4(4)?;
    Ok(socket.into())
}

fn bind_sync_socket() -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, SYNC_PORT)).into())?;
    let socket: UdpSocket = socket.into();
    socket.set_nonblocking(true)?;
    Ok(socket)
}

/// `easytier-cli peer` 里的 `ipv4` 可能带 `/24` 前缀，也可能为空或 `-`。
fn parse_peer_ip(raw: &str) -> Option<Ipv4Addr> {
    let head = raw.split('/').next()?.trim();
    head.parse::<Ipv4Addr>()
        .ok()
        .filter(|ip| !ip.is_unspecified() && !ip.is_loopback())
}

/// 私有网段判断：`easytier-cli` 暂时拿不到对端列表时的兜底，
/// 只接受内网地址，不会把公网来源当成房间成员。
pub(crate) fn is_private(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1])
}

// ---------------------------------------------------------------------------
// TCP 端口转发
// ---------------------------------------------------------------------------

/// `0.0.0.0:<本地端口>` → `<对端虚拟 IP>:<世界端口>` 的 TCP 转发。
///
/// 监听在 `0.0.0.0` 而不是 `127.0.0.1`：伪造公告的源地址是网卡地址，
/// 客户端会连 `网卡地址:端口`，只听回环会接不到。
struct Forward {
    port: u16,
    motd: String,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Forward {
    fn start(target: Ipv4Addr, target_port: u16, motd: String) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));

        let flag = stop.clone();
        let handle = thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nodelay(true);
                        // 每条连接单独一个线程：MC 是长连接，互不阻塞
                        thread::spawn(move || pipe(stream, target, target_port));
                    }
                    Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(50)),
                }
            }
            // listener 在这里 drop，端口立即释放
        });

        Ok(Self {
            port,
            motd,
            stop,
            handle: Some(handle),
        })
    }
}

impl Drop for Forward {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            // 最多等一个 50ms 的轮询间隔：断开房间时要立刻释放端口
            let _ = handle.join();
        }
    }
}

/// 双向转发一条连接，任一端断开就收尾。
fn pipe(client: TcpStream, target: Ipv4Addr, target_port: u16) {
    let Ok(server) = TcpStream::connect_timeout(
        &SocketAddr::from((target, target_port)),
        Duration::from_secs(5),
    ) else {
        return;
    };
    let _ = server.set_nodelay(true);
    let (Ok(mut client_read), Ok(mut server_write)) = (client.try_clone(), server.try_clone())
    else {
        return;
    };

    let upstream = thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match client_read.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if server_write.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
            }
        }
        // 客户端半关后要让对端也收到 EOF，否则 MC 会一直等
        let _ = server_write.shutdown(std::net::Shutdown::Write);
    });

    let mut client = client;
    let mut server = server;
    let mut buf = [0u8; 8192];
    loop {
        match server.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if client.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
        }
    }
    let _ = client.shutdown(std::net::Shutdown::Write);
    let _ = upstream.join();
}

// ---------------------------------------------------------------------------
// 中继
// ---------------------------------------------------------------------------

/// 线程之间共享的状态。
struct Shared {
    /// easytier-cli 与 RPC 端口：心跳线程自己查对端，不依赖界面是否在轮询
    cli: Option<PathBuf>,
    portal: String,
    /// 房间名：远端 MOTD 为空时作为伪造公告的 MOTD
    name: String,
    log: PathBuf,
    stop: AtomicBool,
    self_ips: Vec<Ipv4Addr>,
    /// 本机正在开的世界（由组播监听线程写入）
    local: Mutex<WorldTracker>,
    /// 本机伪造公告用的本地端口，收到回灌的公告时据此过滤
    fake_ports: Mutex<HashSet<u16>>,
    /// 当前要播的伪造公告（供发送线程读取）
    announce: Mutex<Vec<Announcement>>,
    /// 对端虚拟 IP（心跳线程每 5 秒刷新一次）
    peers: Mutex<Vec<Ipv4Addr>>,
    /// 已经打过日志的失败点，避免每 1.5 秒刷屏
    logged: Mutex<HashSet<String>>,
    /// 致命错误（线程因此退出），由 `core` 读取后展示
    error: Mutex<Option<String>>,
}

impl Shared {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn is_self(&self, ip: Ipv4Addr) -> bool {
        self.self_ips.contains(&ip)
    }

    fn is_fake(&self, port: u16) -> bool {
        self.fake_ports.lock().unwrap().contains(&port)
    }

    fn is_peer(&self, ip: Ipv4Addr) -> bool {
        let peers = self.peers.lock().unwrap();
        if peers.contains(&ip) {
            return true;
        }
        // 刚连上时 peer 列表可能还没刷新出来，用私有网段兜底
        peers.is_empty() && is_private(ip) && !self.is_self(ip)
    }

    fn log(&self, level: &str, message: &str) {
        let line = format!("{} {} lanlink::lan: {}\n", timestamp(), level, message);
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&self.log) {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn log_once(&self, key: &str, level: &str, message: &str) {
        if self.logged.lock().unwrap().insert(key.to_string()) {
            self.log(level, message);
        }
    }

    /// 记录致命错误并停下所有线程（`core` 会读走原因并决定是否重试）。
    fn fail(&self, key: &str, message: &str) {
        *self.error.lock().unwrap() = Some(message.to_string());
        self.log_once(key, "ERROR", message);
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// 局域网发现中继。`Drop` 即停播并释放所有端口。
pub struct LanRelay {
    shared: Arc<Shared>,
    handles: Vec<JoinHandle<()>>,
}

impl LanRelay {
    /// 启动中继。`cli` 为内置 easytier-cli，`portal` 为 RPC 地址，`name` 为房间名。
    ///
    /// 所有 socket 先绑定成功再起线程，任何一步失败都整体返回 `Err`，不留半开状态。
    pub fn start(
        cli: Option<PathBuf>,
        portal: String,
        name: String,
        log: PathBuf,
    ) -> Result<Self, String> {
        let self_ips = local_ipv4s();
        if self_ips.is_empty() {
            return Err("找不到可用网卡地址，无法开启局域网发现".to_string());
        }

        let shared = Arc::new(Shared {
            cli,
            portal,
            name,
            log,
            stop: AtomicBool::new(false),
            self_ips: self_ips.clone(),
            local: Mutex::new(WorldTracker::default()),
            fake_ports: Mutex::new(HashSet::new()),
            announce: Mutex::new(Vec::new()),
            peers: Mutex::new(Vec::new()),
            logged: Mutex::new(HashSet::new()),
            error: Mutex::new(None),
        });

        // 1) 组播监听：每个网卡一个 socket
        let mut receivers = Vec::new();
        let mut receiver_errors = Vec::new();
        for ip in &self_ips {
            match bind_receiver(*ip) {
                Ok(socket) => receivers.push((*ip, socket)),
                Err(err) => {
                    receiver_errors.push(format!("{ip}: {err}"));
                    shared.log("WARN", &format!("网卡 {ip} 无法监听 {MC_GROUP}:{MC_PORT}：{err}"));
                }
            }
        }
        if receivers.is_empty() {
            return Err(format!(
                "无法监听 {MC_GROUP}:{MC_PORT}（{}）",
                receiver_errors.join("；")
            ));
        }

        // 2) 房间内同步端口
        let sync = match bind_sync_socket() {
            Ok(socket) => socket,
            Err(err) => {
                return Err(format!(
                    "无法占用 UDP {SYNC_PORT}，房间内无法同步世界端口：{err}"
                ))
            }
        };

        // 3) 伪造公告的发送 socket
        let mut senders = Vec::new();
        for ip in &self_ips {
            match bind_sender(*ip) {
                Ok(socket) => senders.push((*ip, socket)),
                Err(err) => {
                    shared.log("WARN", &format!("网卡 {ip} 无法用于发送组播：{err}"))
                }
            }
        }
        if senders.is_empty() {
            return Err("没有可用于发送组播的本机地址".to_string());
        }

        let mut handles = Vec::new();
        for (ip, socket) in receivers {
            let shared = shared.clone();
            handles.push(thread::spawn(move || recv_loop(shared, ip, socket)));
        }
        {
            let shared = shared.clone();
            handles.push(thread::spawn(move || sync_loop(shared, sync)));
        }
        {
            let shared = shared.clone();
            handles.push(thread::spawn(move || fake_loop(shared, senders)));
        }

        shared.log(
            "INFO",
            &format!(
                "局域网发现中继已启动（本机地址 {}，同步端口 {SYNC_PORT}）",
                self_ips
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );

        Ok(Self { shared, handles })
    }

    /// 线程是否都还在跑（致命错误会让它们一起停下）。
    pub fn alive(&self) -> bool {
        !self.shared.stopped()
    }

    /// 致命错误原因（正常运行时为 `None`）。
    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().unwrap().clone()
    }

    pub fn stop(&mut self) {
        if self.shared.stopped() && self.handles.is_empty() {
            return;
        }
        self.shared.stop.store(true, Ordering::Relaxed);
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
        self.shared.announce.lock().unwrap().clear();
        self.shared.fake_ports.lock().unwrap().clear();
        self.shared.log("INFO", "局域网发现中继已停止");
    }
}

impl Drop for LanRelay {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 组播监听：只认本机网卡发出的公告，伪造的回灌按端口过滤。
fn recv_loop(shared: Arc<Shared>, ip: Ipv4Addr, socket: UdpSocket) {
    let mut buf = [0u8; MAX_PACKET];
    let mut last_sweep = Instant::now();
    while !shared.stopped() {
        match socket.recv_from(&mut buf) {
            Ok((len, source)) => {
                let SocketAddr::V4(source) = source else {
                    continue;
                };
                if !shared.is_self(*source.ip()) {
                    // 物理局域网里别人的世界：不属于本机，不能同步出去
                    continue;
                }
                let Some(announcement) = parse_announcement(&buf[..len]) else {
                    continue;
                };
                if shared.is_fake(announcement.port) {
                    // 自己伪造的公告被回灌，忽略，避免把自己当成房主
                    continue;
                }
                let fresh = shared
                    .local
                    .lock()
                    .unwrap()
                    .observe(&announcement, Instant::now());
                if fresh {
                    shared.log(
                        "INFO",
                        &format!(
                            "检测到本机世界：端口 {}，MOTD「{}」（来自网卡 {ip}）",
                            announcement.port, announcement.motd
                        ),
                    );
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(err) => shared.log_once(
                &format!("recv-{ip}"),
                "WARN",
                &format!("读取组播失败（网卡 {ip}）：{err}"),
            ),
        }

        if last_sweep.elapsed() >= Duration::from_secs(1) {
            last_sweep = Instant::now();
            for port in shared
                .local
                .lock()
                .unwrap()
                .sweep(Instant::now(), WORLD_TTL)
            {
                shared.log(
                    "INFO",
                    &format!(
                        "本机世界端口 {port} 已 {} 秒没有公告，视为已关闭",
                        WORLD_TTL.as_secs()
                    ),
                );
            }
        }
    }
}

/// 房间内同步：发本机世界端口、收对端端口、维护转发与伪造公告。
fn sync_loop(shared: Arc<Shared>, socket: UdpSocket) {
    let mut remote: HashMap<(Ipv4Addr, u16), (String, Instant)> = HashMap::new();
    let mut forwards: HashMap<(Ipv4Addr, u16), Forward> = HashMap::new();
    let mut last_peers: Option<Instant> = None;
    let mut last_send: Option<Instant> = None;
    let mut last_sweep = Instant::now();
    let mut buf = [0u8; MAX_PACKET];

    while !shared.stopped() {
        // 1) 收心跳（非阻塞，一次把队列排空）
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
                    let Some(message) = parse_sync(&buf[..len]) else {
                        continue;
                    };
                    let now = Instant::now();
                    for world in message.worlds {
                        if world.port == 0 {
                            continue;
                        }
                        remote.insert((ip, world.port), (world.motd, now));
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => {
                    // 非阻塞 socket 上出现其它错误，说明这个 socket 已经不可用：
                    // 记下原因并让中继整体停下，由 core 决定是否重试，不静默继续。
                    shared.fail("sync-recv", &format!("读取同步报文失败：{err}"));
                    break;
                }
            }
        }

        // 2) 刷新对端列表（5 秒）并周期发送本机端口
        if last_peers
            .map(|at| at.elapsed() >= Duration::from_secs(5))
            .unwrap_or(true)
        {
            let peers = probe_peers(&shared);
            *shared.peers.lock().unwrap() = peers;
            last_peers = Some(Instant::now());
        }
        if last_send
            .map(|at| at.elapsed() >= ANNOUNCE_INTERVAL)
            .unwrap_or(true)
        {
            send_sync(&shared, &socket);
            last_send = Some(Instant::now());
        }

        // 3) 过期摘除 + 重建转发
        if last_sweep.elapsed() >= Duration::from_secs(1) {
            last_sweep = Instant::now();
            let now = Instant::now();
            remote.retain(|_, (_, seen)| now.saturating_duration_since(*seen) < WORLD_TTL);
            reconcile(&shared, &remote, &mut forwards);
        }

        thread::sleep(TICK);
    }

    // 先摘掉伪造公告，再关转发端口：断开房间后条目应尽快消失
    shared.announce.lock().unwrap().clear();
    shared.fake_ports.lock().unwrap().clear();
    forwards.clear();
}

/// 让转发与伪造公告跟上当前的对端世界集合。
fn reconcile(
    shared: &Shared,
    remote: &HashMap<(Ipv4Addr, u16), (String, Instant)>,
    forwards: &mut HashMap<(Ipv4Addr, u16), Forward>,
) {
    for (&(ip, port), (motd, _)) in remote {
        if forwards.contains_key(&(ip, port)) {
            continue;
        }
        let motd = if motd.trim().is_empty() {
            shared.name.clone()
        } else {
            motd.clone()
        };
        match Forward::start(ip, port, motd) {
            Ok(forward) => {
                shared
                    .fake_ports
                    .lock()
                    .unwrap()
                    .insert(forward.port);
                shared.log(
                    "INFO",
                    &format!(
                        "对端 {ip} 的世界端口 {port} 已就绪：本机转发 端口 {}",
                        forward.port
                    ),
                );
                forwards.insert((ip, port), forward);
            }
            Err(err) => shared.log_once(
                &format!("fwd-{ip}-{port}"),
                "WARN",
                &format!("为 {ip}:{port} 建立本地转发失败：{err}"),
            ),
        }
    }

    let stale: Vec<(Ipv4Addr, u16)> = forwards
        .keys()
        .copied()
        .filter(|key| !remote.contains_key(key))
        .collect();
    for key in stale {
        if let Some(forward) = forwards.remove(&key) {
            shared.fake_ports.lock().unwrap().remove(&forward.port);
            shared.log(
                "INFO",
                &format!(
                    "对端 {}:{} 的世界已关闭，移除本地转发端口 {}",
                    key.0, key.1, forward.port
                ),
            );
        }
    }

    let mut announce: Vec<Announcement> = forwards
        .values()
        .map(|forward| Announcement {
            motd: forward.motd.clone(),
            port: forward.port,
        })
        .collect();
    announce.sort_by_key(|a| a.port);
    *shared.announce.lock().unwrap() = announce;
}

/// 把本机世界里「最近 5 秒还有公告」的端口发给房间成员。
fn send_sync(shared: &Shared, socket: &UdpSocket) {
    let worlds: Vec<SyncWorld> = shared
        .local
        .lock()
        .unwrap()
        .active()
        .into_iter()
        .map(|world| SyncWorld {
            port: world.port,
            motd: world.motd,
        })
        .collect();
    let Ok(payload) = serde_json::to_vec(&SyncMessage {
        lanlink: SYNC_VERSION,
        worlds,
    }) else {
        return;
    };
    let peers = shared.peers.lock().unwrap().clone();
    for peer in peers {
        if shared.is_self(peer) {
            continue;
        }
        if let Err(err) = socket.send_to(&payload, SocketAddr::from((peer, SYNC_PORT))) {
            shared.log_once(
                &format!("sync-send-{peer}"),
                "WARN",
                &format!("向对端 {peer}:{SYNC_PORT} 同步世界端口失败：{err}"),
            );
        }
    }
}

/// 从 `easytier-cli peer` 取对端虚拟 IP（供局域网发现与广播转发共用）。
pub(crate) fn peers_from_cli(cli: Option<&std::path::Path>, portal: &str) -> Vec<Ipv4Addr> {
    let Some(cli) = cli else {
        return Vec::new();
    };
    let Ok(output) = proc::command(cli)
        .arg("-p")
        .arg(portal)
        .arg("-o")
        .arg("json")
        .arg("peer")
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return Vec::new();
    };
    let mut peers = Vec::new();
    for peer in value.as_array().into_iter().flatten() {
        let Some(raw) = peer.get("ipv4").and_then(|v| v.as_str()) else {
            continue;
        };
        if let Some(ip) = parse_peer_ip(raw) {
            if !peers.contains(&ip) {
                peers.push(ip);
            }
        }
    }
    peers
}

/// 从 `easytier-cli peer` 取对端虚拟 IP（每 5 秒一次）。
fn probe_peers(shared: &Shared) -> Vec<Ipv4Addr> {
    peers_from_cli(shared.cli.as_deref(), &shared.portal)
}

/// 每 1.5 秒向本机组播地址播一次伪造公告，让原版「多人游戏」列表刷出条目。
fn fake_loop(shared: Arc<Shared>, senders: Vec<(Ipv4Addr, UdpSocket)>) {
    let mut last: Option<Instant> = None;
    while !shared.stopped() {
        let list = shared.announce.lock().unwrap().clone();
        let due = last.map(|at| at.elapsed() >= ANNOUNCE_INTERVAL).unwrap_or(true);
        if due && !list.is_empty() {
            last = Some(Instant::now());
            for announcement in &list {
                let payload = build_announcement(&announcement.motd, announcement.port);
                for (ip, socket) in &senders {
                    if let Err(err) =
                        socket.send_to(payload.as_bytes(), SocketAddr::from((MC_GROUP, MC_PORT)))
                    {
                        shared.log_once(
                            &format!("fake-{ip}"),
                            "WARN",
                            &format!("发送伪造公告失败（网卡 {ip}）：{err}"),
                        );
                    }
                }
            }
        }
        if list.is_empty() {
            last = None;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

// ---------------------------------------------------------------------------
// 日志时间戳（与 easytier 的日志格式对齐，便于同一份日志阅读）
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
pub(crate) fn timestamp() -> String {
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&now, &mut local);
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        local.tm_year + 1900,
        local.tm_mon + 1,
        local.tm_mday,
        local.tm_hour,
        local.tm_min,
        local.tm_sec
    )
}

#[cfg(windows)]
pub(crate) fn timestamp() -> String {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;

    let mut local: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe {
        GetLocalTime(&mut local);
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond
    )
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn timestamp() -> String {
    // 仅用于开发机上的其它平台：UTC 时间也满足日志查看器的格式
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-unix{}", "1970-01-01T00:00:00", now.as_secs())
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn announcement(motd: &str, port: u16) -> Announcement {
        Announcement {
            motd: motd.to_string(),
            port,
        }
    }

    #[test]
    fn parses_vanilla_announcement() {
        let parsed = parse_announcement(b"[MOTD]Steve - My World[/MOTD][AD]54321[/AD]").unwrap();
        assert_eq!(parsed, announcement("Steve - My World", 54321));
    }

    #[test]
    fn motd_may_contain_ad_markers() {
        // 世界名里带 [AD] 字样时，只能认 [/MOTD] 之后的端口块
        let parsed =
            parse_announcement(b"[MOTD]Steve - [AD]funny[/AD] name[/MOTD][AD]25566[/AD]").unwrap();
        assert_eq!(parsed, announcement("Steve - [AD]funny[/AD] name", 25566));
    }

    #[test]
    fn missing_or_bad_port_falls_back_to_default() {
        assert_eq!(
            parse_announcement(b"[MOTD]World[/MOTD]").unwrap(),
            announcement("World", DEFAULT_PORT)
        );
        assert_eq!(
            parse_announcement(b"[MOTD]World[/MOTD][AD]abc[/AD]").unwrap(),
            announcement("World", DEFAULT_PORT)
        );
    }

    #[test]
    fn rejects_non_announcement() {
        assert!(parse_announcement(b"").is_none());
        assert!(parse_announcement(b"garbage").is_none());
        assert!(parse_announcement(b"[MOTD]unterminated").is_none());
    }

    #[test]
    fn build_round_trips() {
        let text = build_announcement("中文世界 · test", 25565);
        assert_eq!(
            parse_announcement(text.as_bytes()).unwrap(),
            announcement("中文世界 · test", 25565)
        );
        // MOTD 里自带的标记必须被清掉，不能破坏报文结构
        let broken = build_announcement("a[/MOTD][AD]1[/AD]b", 25566);
        assert_eq!(
            parse_announcement(broken.as_bytes()).unwrap(),
            announcement("a 1 b", 25566)
        );
    }

    #[test]
    fn tracker_keeps_port_alive_until_ttl() {
        let start = Instant::now();
        let mut tracker = WorldTracker::default();
        assert!(tracker.observe(&announcement("World", 25565), start));
        // 同一端口再收到公告只刷新时间，不重复上报
        assert!(!tracker.observe(&announcement("World", 25565), start + Duration::from_secs(3)));

        let now = start + Duration::from_secs(7);
        assert!(tracker.sweep(now - Duration::from_secs(1), WORLD_TTL).is_empty());
        assert_eq!(tracker.active(), vec![announcement("World", 25565)]);

        let now = start + Duration::from_secs(9);
        assert_eq!(tracker.sweep(now, WORLD_TTL), vec![25565]);
        assert!(tracker.active().is_empty());
    }

    #[test]
    fn tracker_handles_multiple_ports() {
        let now = Instant::now();
        let mut tracker = WorldTracker::default();
        tracker.observe(&announcement("A", 25565), now);
        tracker.observe(&announcement("B", 25566), now);
        assert_eq!(
            tracker.active(),
            vec![announcement("A", 25565), announcement("B", 25566)]
        );
        // 只让 25565 续命，25566 过期
        tracker.observe(&announcement("A", 25565), now + Duration::from_secs(4));
        assert_eq!(
            tracker.sweep(now + Duration::from_secs(6), WORLD_TTL),
            vec![25566]
        );
        assert_eq!(tracker.active(), vec![announcement("A", 25565)]);
    }

    #[test]
    fn parses_peer_ipv4() {
        assert_eq!(parse_peer_ip("10.126.126.1"), Some(Ipv4Addr::new(10, 126, 126, 1)));
        assert_eq!(parse_peer_ip("10.126.126.1/24"), Some(Ipv4Addr::new(10, 126, 126, 1)));
        assert_eq!(parse_peer_ip(""), None);
        assert_eq!(parse_peer_ip("-"), None);
        assert_eq!(parse_peer_ip("127.0.0.1"), None);
        assert_eq!(parse_peer_ip("0.0.0.0"), None);
    }

    #[test]
    fn private_range_check() {
        assert!(is_private(Ipv4Addr::new(10, 126, 126, 1)));
        assert!(is_private(Ipv4Addr::new(192, 168, 1, 2)));
        assert!(is_private(Ipv4Addr::new(172, 16, 0, 1)));
        assert!(is_private(Ipv4Addr::new(100, 64, 0, 1)));
        assert!(!is_private(Ipv4Addr::new(8, 8, 8, 8)));
    }

    #[test]
    fn sync_message_round_trip() {
        let payload = serde_json::to_vec(&SyncMessage {
            lanlink: SYNC_VERSION,
            worlds: vec![SyncWorld {
                port: 25565,
                motd: "Steve - World".to_string(),
            }],
        })
        .unwrap();
        let parsed = parse_sync(&payload).unwrap();
        assert_eq!(parsed.worlds.len(), 1);
        assert_eq!(parsed.worlds[0].port, 25565);
        assert_eq!(parsed.worlds[0].motd, "Steve - World");

        // 版本不符 / 坏 JSON 一律丢弃
        assert!(parse_sync(b"{\"lanlink\":2,\"worlds\":[]}").is_none());
        assert!(parse_sync(b"not json").is_none());
    }

    #[test]
    fn forward_releases_its_port_on_drop() {
        let forward = Forward::start(Ipv4Addr::new(10, 126, 126, 9), 25565, "World".to_string())
            .expect("分配本地端口失败");
        let port = forward.port;
        assert_ne!(port, 0);
        assert_ne!(port, MC_PORT);

        drop(forward);
        // 停止后端口必须能重新绑定，说明没有残留监听
        TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).expect("转发停止后端口没有释放");
    }
}
