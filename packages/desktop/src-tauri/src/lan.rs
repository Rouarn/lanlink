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
//!    监听 socket 绑 `0.0.0.0:4445` 并开 `SO_REUSEPORT`，再逐网卡加入组播组——
//!    绑具体网卡地址在 macOS 上收不到组播，不开 `SO_REUSEPORT` 则与原版客户端互相挤占端口。
//! 2. **同步**：谁在本机开了什么，交给「开放游戏」宣告通道（`games.rs`，UDP 4449）
//!    在房间成员之间交换——它已经覆盖自动发现与手动登记，这里不再自带同步协议，
//!    避免同一个世界建出两个转发端口。`core` 每次状态轮询把对端快照写进来，
//!    由 `bridge_loop` 对账。
//! 3. **造条目**：对端开放的每个世界，在本机建一个 TCP 转发
//!    `0.0.0.0:<本地空闲端口> → <对端虚拟 IP>:<世界端口>`，再每 1.5 秒向本机
//!    `224.0.2.60:4445` 播一条伪造公告，AD 指向本地端口。
//!    原版「多人游戏」列表就会刷出条目；点进去走的是 LAN 条目，才有 MC-170460
//!    的离线认证豁免（手动「添加服务器」填 IP 会报「无效会话」）。
//!
//! 伪造公告的源地址是发送 socket 绑定的本机网卡地址（而不是中继进程随便选的地址），
//! 本地转发监听的又是 `0.0.0.0`，因此客户端按源地址发起的连接一定能落到转发上。
//! 某些环境（屏蔽组播的虚拟网卡等）组播发不出去时，再从回环地址补发一份单播，
//! 源地址会变成 `127.0.0.1`——转发同样绑在 `0.0.0.0`，从回环连进来也能通。
//! 只在组播失败时补发，不做无条件双发：双发会让同一个世界在列表里出现两条。
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

use socket2::{Domain, Protocol, Socket, Type};

use crate::games::LocalGames;

/// 原版局域网发现的组播地址（IPv4）。
pub const MC_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 2, 60);
/// 原版局域网发现的端口。
pub const MC_PORT: u16 = 4445;
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
/// 连对端世界端口的超时兜底：拿不到对端延迟时用这个值。
const CONNECT_FALLBACK: Duration = Duration::from_secs(2);
/// 连对端世界端口的超时下限（同一物理局域网也要够用）。
const CONNECT_MIN: Duration = Duration::from_secs(1);
/// 连对端世界端口的超时上限：再慢就不值得让客户端干等，直接断开并写清原因。
const CONNECT_MAX: Duration = Duration::from_secs(8);

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
// 对端世界（来自「开放游戏」宣告通道）
// ---------------------------------------------------------------------------

/// 房间成员开放的一个世界，用来在本机建 TCP 转发并伪造公告。
///
/// 数据由 `core` 从 `games::GameRelay::snapshot()` 写入：那里已经校验过源地址、
/// 过滤过保留端口，并且只会包含**对端**，所以这里不需要再判断是不是自己的世界。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteWorld {
    pub ip: Ipv4Addr,
    pub port: u16,
    pub name: String,
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

/// 本机**物理**网卡的网段 `(地址, 掩码)`。
///
/// 用来判断一条原生公告是不是真从物理局域网来的。EasyTier 的 utun / TUN / TAP
/// 不能算：从 overlay 过来的公告源地址是对端虚拟 IP，那条路本身不稳，不能因为
/// 它出现过一次就不建转发。
fn local_lans() -> Vec<(Ipv4Addr, Ipv4Addr)> {
    let mut lans = Vec::new();
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return lans;
    };
    for interface in interfaces {
        let if_addrs::IfAddr::V4(v4) = interface.addr else {
            continue;
        };
        if v4.ip.is_loopback() || v4.ip.is_unspecified() || v4.ip.is_link_local() {
            continue;
        }
        let name = interface.name.to_ascii_lowercase();
        if name.starts_with("utun")
            || name.starts_with("tun")
            || name.starts_with("tap")
            || name.contains("easytier")
            || name.contains("wintun")
            || name.contains("vethernet")
        {
            continue;
        }
        if !lans.contains(&(v4.ip, v4.netmask)) {
            lans.push((v4.ip, v4.netmask));
        }
    }
    lans
}

/// `ip` 是否落在本机某个物理局域网网段里。
fn in_lan(ip: Ipv4Addr, lans: &[(Ipv4Addr, Ipv4Addr)]) -> bool {
    let ip = u32::from(ip);
    lans.iter()
        .any(|(local, mask)| ip & u32::from(*mask) == u32::from(*local) & u32::from(*mask))
}

/// 绑定 `0.0.0.0:4445`（组播组在各网卡上另行加入）。
///
/// 为什么绑通配地址而不是各网卡地址：macOS 上绑到具体地址的 socket 收不到目的地址
/// 是组播的报文（实测绑 `192.168.x.x:4445` 一条也收不到，绑 `0.0.0.0:4445` 正常）。
/// 为什么还要 `SO_REUSEPORT`：macOS 只认 `SO_REUSEPORT` 共享监听端口；原版客户端的
/// `MulticastSocket` 同样开了它，两边才能同时监听 4445。否则先绑定的一方会把对方挤掉，
/// 原版日志里会出现 `Unable to start LAN server detection`，且我们自己也收不到公告。
fn bind_receiver() -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, MC_PORT)).into())?;
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

/// 从 `easytier-cli peer` 的 `ipv4` 字段里取出地址：
/// 可能带 `/24` 前缀，也可能为空或 `-`。
pub(crate) fn parse_ipv4(raw: &str) -> Option<Ipv4Addr> {
    raw.split('/').next()?.trim().parse::<Ipv4Addr>().ok()
}

/// 对端虚拟 IP：剥掉 `/24` 前缀，并排除回环 / 未指定地址。
pub(crate) fn parse_peer_ip(raw: &str) -> Option<Ipv4Addr> {
    parse_ipv4(raw).filter(|ip| !ip.is_unspecified() && !ip.is_loopback())
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
///
/// 条目存废只看宣告：宣告在就播，宣告过期（`games.rs` 的 18 秒 TTL）就撤；
/// 这里不探测对端可达性，也不因为一次客户端连接失败而停播。
struct Forward {
    port: u16,
    motd: String,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Forward {
    fn start(
        target: Ipv4Addr,
        target_port: u16,
        motd: String,
        log: PathBuf,
        latency: Arc<Mutex<HashMap<Ipv4Addr, u32>>>,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));

        let flag = stop.clone();
        let log_path = log.clone();
        // 每条连接的结果都要能查：客户端那边只会显示「无法连接至服务器 / 连接中断」，
        // 到底是「没连到我们」还是「我们连不上对端」，只能靠这里的日志区分。
        let logged_accept = Arc::new(AtomicBool::new(false));
        let handle = thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, addr)) => {
                        // macOS 上 accept 出来的 socket 会继承监听 socket 的非阻塞标志
                        // （Linux 不会）。不显式改回阻塞，`pipe` 里第一次 `read` 立刻
                        // 拿到 `WouldBlock`，被当成对端断开，客户端就会看到
                        // 「无法连接至服务器 / 连接中断」。
                        if stream.set_nonblocking(false).is_err() {
                            continue;
                        }
                        let _ = stream.set_nodelay(true);
                        if !logged_accept.swap(true, Ordering::Relaxed) {
                            log_to(
                                &log_path,
                                "INFO",
                                &format!(
                                    "本地转发端口 {port} 收到连接（来自 {addr}），开始连接对端 {target}:{target_port}"
                                ),
                            );
                        }
                        let log = log_path.clone();
                        let latency = latency.clone();
                        // 每条连接单独一个线程：MC 是长连接，互不阻塞
                        thread::spawn(move || {
                            pipe(stream, target, target_port, port, &log, &latency)
                        });
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

/// 单次出向连接的超时：按对端延迟推导，`max(1s, 4 × 延迟 + 1s)`，封顶 8 秒。
///
/// 经中继的对端 RTT 常在上百毫秒，固定 600ms 的握手超时会把本来能通的连接判死；
/// 拿不到延迟时退回 2 秒。只放宽「等多久」，不做重试。
fn connect_timeout(lat_ms: Option<u32>) -> Duration {
    match lat_ms {
        Some(ms) => Duration::from_millis(u64::from(ms) * 4 + 1000).clamp(CONNECT_MIN, CONNECT_MAX),
        None => CONNECT_FALLBACK,
    }
}

/// 把连接失败翻译成可读原因：RST、超时、不可达要能区分。
fn connect_failure(err: &io::Error) -> String {
    match err.kind() {
        io::ErrorKind::ConnectionRefused => "对端拒绝连接（RST）".to_string(),
        io::ErrorKind::TimedOut => "连接超时".to_string(),
        _ => match err.raw_os_error() {
            Some(60) => "连接超时（ETIMEDOUT）".to_string(),
            Some(65) => "目标不可达（EHOSTUNREACH）".to_string(),
            Some(51) => "网络不可达（ENETUNREACH）".to_string(),
            _ => format!("连接失败：{err}"),
        },
    }
}

/// 双向转发一条连接，任一端断开就收尾。
///
/// 出向连接只试一次：失败就把原因（RST / 超时 / 不可达）和耗时写进日志并断开客户端，
/// 不重试——重试只会把「连不上」拖成几秒假死，掩盖真正的原因。
///
/// 传进来的 `client` 必须是阻塞 socket（见 accept 处：macOS 会继承非阻塞标志），
/// 否则第一次读不到数据就会被当成断开，长连接只走完第一批报文就半关对端。
fn pipe(
    client: TcpStream,
    target: Ipv4Addr,
    target_port: u16,
    local_port: u16,
    log: &std::path::Path,
    latency: &Mutex<HashMap<Ipv4Addr, u32>>,
) {
    let addr = SocketAddr::from((target, target_port));
    let lat_ms = latency.lock().unwrap().get(&target).copied();
    let limit = connect_timeout(lat_ms);
    let at = Instant::now();
    let server = match TcpStream::connect_timeout(&addr, limit) {
        Ok(stream) => {
            log_to(
                log,
                "INFO",
                &format!(
                    "本地转发端口 {local_port} 已连上对端 {target}:{target_port}（耗时 {} ms）",
                    at.elapsed().as_millis()
                ),
            );
            stream
        }
        Err(err) => {
            log_to(
                log,
                "WARN",
                &format!(
                    "本地转发端口 {local_port} 连接对端 {target}:{target_port} 失败：{}（耗时 {} ms，上限 {} ms），已断开客户端",
                    connect_failure(&err),
                    at.elapsed().as_millis(),
                    limit.as_millis()
                ),
            );
            return;
        }
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
    /// 房间名：远端 MOTD 为空时作为伪造公告的 MOTD
    name: String,
    log: PathBuf,
    stop: AtomicBool,
    self_ips: Vec<Ipv4Addr>,
    /// 本机正在开的世界（由组播监听线程写入）
    local: Mutex<WorldTracker>,
    /// 对端开放的世界（由 `core` 从开放游戏宣告通道同步写入）
    remote: Mutex<Vec<RemoteWorld>>,
    /// 物理局域网里别人（不一定在房间内）的原生公告：(端口, 来源 IP) → 最近收到时间。
    /// 用来判断某个对端世界在本机所在局域网里本来就能被发现——能发现就不再建转发、
    /// 伪造公告，否则同一个世界会因为「原生一条 + 伪造一条」在列表里出现两次。
    ///
    /// 只认**物理网段**的来源：从 overlay 过来的公告（源地址是对端虚拟 IP）不算，
    /// 那条路本身不稳定，不能因为它出现过就不要转发。
    native: Mutex<HashMap<(u16, Ipv4Addr), Instant>>,
    /// 本机物理网卡的网段 `(地址, 掩码)`，用来判断一条原生公告是不是真的从
    /// 物理局域网来的。EasyTier 的 utun / TUN 不算物理网卡。
    lans: Vec<(Ipv4Addr, Ipv4Addr)>,
    /// 本机伪造公告的签名 (本地转发端口, 网线上的 MOTD)，收到回灌时据此过滤
    fake_announcements: Mutex<HashSet<(u16, String)>>,
    /// 当前要播的伪造公告（供发送线程读取）
    announce: Mutex<Vec<Announcement>>,
    /// 对端虚拟 IP：由 `core` 统一轮询 `easytier-cli peer` 后写入，中继自己不再起子进程
    peers: Arc<Mutex<Vec<Ipv4Addr>>>,
    /// 对端虚拟 IP → 最近一次 peer 快照里的 RTT（毫秒）：只用来推导出向连接的超时
    latency: Arc<Mutex<HashMap<Ipv4Addr, u32>>>,
    /// 本机游戏表：把自动发现的世界端口交给「开放游戏」宣告通道
    local_games: LocalGames,
    /// 已经打过日志的失败点，避免每 1.5 秒刷屏
    logged: Mutex<HashSet<String>>,
}

impl Shared {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn is_self(&self, ip: Ipv4Addr) -> bool {
        self.self_ips.contains(&ip)
    }

    /// 是不是自己伪造的公告：按 (端口, MOTD) 识别。
    ///
    /// 只按端口会把真实的本机世界误判掉——转发端口由系统随机分配，可能撞上原版
    /// 客户端正在用的端口；加上 MOTD 后与真实公告的区分是确定的。
    fn is_fake(&self, port: u16, motd: &str) -> bool {
        self.fake_announcements
            .lock()
            .unwrap()
            .contains(&(port, motd.to_string()))
    }

    fn log(&self, level: &str, message: &str) {
        log_to(&self.log, level, message);
    }

    fn log_once(&self, key: &str, level: &str, message: &str) {
        if self.logged.lock().unwrap().insert(key.to_string()) {
            self.log(level, message);
        }
    }
}

/// 追加一行日志。转发线程也要写（它拿不到 `Shared`），所以单独抽出来。
fn log_to(path: &std::path::Path, level: &str, message: &str) {
    let line = format!("{} {} lanlink::lan: {}\n", timestamp(), level, message);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// 局域网发现中继。`Drop` 即停播并释放所有端口。
pub struct LanRelay {
    shared: Arc<Shared>,
    handles: Vec<JoinHandle<()>>,
}

impl LanRelay {
    /// 启动中继。`name` 为房间名，`peers` 是 `core` 维护的对端虚拟 IP 快照，
    /// `latency` 是同一份快照里的对端 RTT（毫秒），只用来推导出向连接超时。
    ///
    /// 所有 socket 先绑定成功再起线程，关键能力失败就整体返回 `Err`，不留半开状态。
    pub fn start(
        name: String,
        log: PathBuf,
        peers: Arc<Mutex<Vec<Ipv4Addr>>>,
        latency: Arc<Mutex<HashMap<Ipv4Addr, u32>>>,
        local_games: LocalGames,
    ) -> Result<Self, String> {
        let self_ips = local_ipv4s();
        if self_ips.is_empty() {
            return Err("找不到可用网卡地址，无法开启局域网发现".to_string());
        }

        let shared = Arc::new(Shared {
            name,
            log,
            stop: AtomicBool::new(false),
            self_ips: self_ips.clone(),
            local: Mutex::new(WorldTracker::default()),
            remote: Mutex::new(Vec::new()),
            native: Mutex::new(HashMap::new()),
            lans: local_lans(),
            fake_announcements: Mutex::new(HashSet::new()),
            announce: Mutex::new(Vec::new()),
            peers,
            latency,
            local_games,
            logged: Mutex::new(HashSet::new()),
        });

        // 1) 组播监听：单个 socket 绑通配地址，再逐网卡加入组播组
        let receiver = match bind_receiver() {
            Ok(socket) => socket,
            Err(err) => return Err(format!("无法监听 {MC_GROUP}:{MC_PORT}：{err}")),
        };
        let mut joined = Vec::new();
        let mut join_errors = Vec::new();
        for ip in &self_ips {
            match receiver.join_multicast_v4(&MC_GROUP, ip) {
                Ok(()) => joined.push(*ip),
                Err(err) => {
                    join_errors.push(format!("{ip}: {err}"));
                    shared.log(
                        "WARN",
                        &format!("网卡 {ip} 无法加入 {MC_GROUP}:{MC_PORT}：{err}"),
                    );
                }
            }
        }
        if joined.is_empty() {
            return Err(format!(
                "无法加入组播组 {MC_GROUP}:{MC_PORT}（{}）",
                join_errors.join("；")
            ));
        }

        // 2) 伪造公告的发送 socket
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
        {
            let shared = shared.clone();
            handles.push(thread::spawn(move || recv_loop(shared, receiver)));
        }
        {
            // 对端世界 → 本地 TCP 转发 + 伪造公告
            let shared = shared.clone();
            handles.push(thread::spawn(move || bridge_loop(shared)));
        }
        {
            let shared = shared.clone();
            handles.push(thread::spawn(move || fake_loop(shared, senders)));
        }

        shared.log(
            "INFO",
            &format!(
                "局域网发现中继已启动（本机地址 {}）",
                self_ips
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );

        Ok(Self { shared, handles })
    }

    /// 更新「对端开放的世界」：`core` 每次状态轮询从开放游戏通道取快照后写入。
    ///
    /// 这就是转发的唯一存废依据：宣告在快照里就建/留，不在了就撤。
    /// 快照本身已经带 18 秒 TTL（`games.rs`），这里不做任何探测或宽限。
    pub fn set_remote_worlds(&self, worlds: Vec<RemoteWorld>) {
        *self.shared.remote.lock().unwrap() = worlds;
    }

    /// 线程是否都还在跑：既看停止标志，也看 `JoinHandle`——
    /// 线程 panic 后标志仍是「运行中」，只看标志会让界面一直显示正常。
    pub fn alive(&self) -> bool {
        !self.shared.stopped() && !any_finished(&self.handles)
    }

    /// 致命错误原因（正常运行时为 `None`）。
    pub fn error(&self) -> Option<String> {
        if !self.shared.stopped() && any_finished(&self.handles) {
            return Some("局域网发现线程意外退出（可能发生 panic），已停止".to_string());
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
        self.shared.announce.lock().unwrap().clear();
        self.shared.fake_announcements.lock().unwrap().clear();
        self.shared.remote.lock().unwrap().clear();
        self.shared.local_games.lock().unwrap().clear_minecraft();
        self.shared.log("INFO", "局域网发现中继已停止");
    }
}

impl Drop for LanRelay {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 组播监听：只认本机网卡发出的公告，伪造的回灌按 (端口, MOTD) 过滤。
fn recv_loop(shared: Arc<Shared>, socket: UdpSocket) {
    let mut buf = [0u8; MAX_PACKET];
    let mut last_sweep = Instant::now();
    while !shared.stopped() {
        match socket.recv_from(&mut buf) {
            Ok((len, source)) => {
                let SocketAddr::V4(source) = source else {
                    continue;
                };
                if !shared.is_self(*source.ip()) {
                    // 物理局域网里别人的原生公告：不属于本机，不能当本机世界同步出去；
                    // 但要记下来——如果某个房间成员的世界本来就能这样被发现，就不要再
                    // 为它建转发和伪造公告，否则列表里会出现两条。
                    if let Some(announcement) = parse_announcement(&buf[..len]) {
                        shared
                            .native
                            .lock()
                            .unwrap()
                            .insert((announcement.port, *source.ip()), Instant::now());
                    }
                    continue;
                }
                let Some(announcement) = parse_announcement(&buf[..len]) else {
                    continue;
                };
                if shared.is_fake(announcement.port, &announcement.motd) {
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
                            "检测到本机世界：端口 {}，MOTD「{}」",
                            announcement.port, announcement.motd
                        ),
                    );
                }
                publish_local_worlds(&shared);
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(err) => shared.log_once(
                "recv",
                "WARN",
                &format!("读取组播失败：{err}"),
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
            publish_local_worlds(&shared);
        }
    }
}

/// 把本机自动发现的世界同步给「开放游戏」宣告通道。
fn publish_local_worlds(shared: &Shared) {
    let worlds: Vec<(u16, String)> = shared
        .local
        .lock()
        .unwrap()
        .active()
        .into_iter()
        .map(|world| (world.port, world.motd))
        .collect();
    shared.local_games.lock().unwrap().set_minecraft(worlds);
}

/// 按「对端开放的世界」维护 TCP 转发与伪造公告。
///
/// 数据源是开放游戏宣告通道（`core` 每秒写入一次），不再自己收发同步报文：
/// 自动发现与手动登记的端口走同一条路，同一世界也只会有一个转发端口。
fn bridge_loop(shared: Arc<Shared>) {
    let mut forwards: HashMap<(Ipv4Addr, u16), Forward> = HashMap::new();
    let mut last_sweep = Instant::now();

    while !shared.stopped() {
        if last_sweep.elapsed() >= Duration::from_secs(1) {
            last_sweep = Instant::now();
            let remote = shared.remote.lock().unwrap().clone();
            let native = {
                let mut native = shared.native.lock().unwrap();
                let now = Instant::now();
                native.retain(|_, seen| now.saturating_duration_since(*seen) < WORLD_TTL);
                native.clone()
            };
            reconcile(&shared, &remote, &native, &mut forwards);
        }
        thread::sleep(TICK);
    }

    // 先摘掉伪造公告，再关转发端口：断开房间后条目应尽快消失
    shared.announce.lock().unwrap().clear();
    shared.fake_announcements.lock().unwrap().clear();
    forwards.clear();
}

/// 让转发与伪造公告跟上当前的对端世界集合。
///
/// 唯一依据是宣告：快照里有就建 / 留（本地端口沿用），没有就撤。
/// 不探测对端是否可达、不做宽限——可达性只在客户端真的连进来时才验证。
/// 唯一的例外是这个对端世界在本机物理局域网里已被原生发现：此时不建转发，
/// 否则同一个世界会在列表里出现两条。
fn reconcile(
    shared: &Shared,
    remote: &[RemoteWorld],
    native: &HashMap<(u16, Ipv4Addr), Instant>,
    forwards: &mut HashMap<(Ipv4Addr, u16), Forward>,
) {
    let peers = shared.peers.lock().unwrap().clone();
    // 只把「不含任何对端虚拟 IP」的网段当成物理局域网：Windows 上的 EasyTier 适配器
    // 名字不固定，光靠网卡名可能滤不掉，这里再用 peer 虚拟 IP 兜一层，避免把
    // overlay 过来的公告误判成「物理局域网原生」而反过来不建转发。
    let lans: Vec<(Ipv4Addr, Ipv4Addr)> = shared
        .lans
        .iter()
        .copied()
        .filter(|lan| !peers.iter().any(|peer| in_lan(*peer, std::slice::from_ref(lan))))
        .collect();
    let mut wanted: HashSet<(Ipv4Addr, u16)> = HashSet::new();
    let mut native_keys: HashSet<(Ipv4Addr, u16)> = HashSet::new();

    for world in remote {
        if shared.is_self(world.ip) {
            continue;
        }
        let key = (world.ip, world.port);
        if natively_visible(native, world, &lans) {
            // 同一个物理局域网里：原版客户端自己就能收到对方的公告，再伪造一条会让
            // 列表里出现两个同名世界（IP 不同，原版按 IP:端口 去重，两条都留下）
            native_keys.insert(key);
            shared.log_once(
                &format!("native-{}-{}", world.ip, world.port),
                "INFO",
                &format!(
                    "对端 {}:{} 的世界在本机物理局域网里已被原生发现，不再建本地转发",
                    world.ip, world.port
                ),
            );
            continue;
        }
        wanted.insert(key);
    }

    for world in remote {
        if shared.is_self(world.ip) {
            continue;
        }
        let key = (world.ip, world.port);
        if !wanted.contains(&key) || forwards.contains_key(&key) {
            continue;
        }
        let motd = if world.name.trim().is_empty() {
            shared.name.clone()
        } else {
            world.name.clone()
        };
        match Forward::start(
            world.ip,
            world.port,
            motd,
            shared.log.clone(),
            shared.latency.clone(),
        ) {
            Ok(forward) => {
                shared
                    .fake_announcements
                    .lock()
                    .unwrap()
                    .insert((forward.port, announced_motd(&forward.motd, forward.port)));
                shared.log(
                    "INFO",
                    &format!(
                        "对端 {} 的世界端口 {} 已就绪：本机转发 端口 {}",
                        world.ip, world.port, forward.port
                    ),
                );
                forwards.insert(key, forward);
            }
            Err(err) => shared.log_once(
                &format!("fwd-{}-{}", world.ip, world.port),
                "WARN",
                &format!(
                    "为 {}:{} 建立本地转发失败：{err}",
                    world.ip, world.port
                ),
            ),
        }
    }

    // 宣告不在快照里了就立刻撤：快照本身已带 18 秒 TTL，能走到这里说明对端
    // 已经 18 秒没再宣告这个世界；世界重开换了端口会自然换成另一条新条目。
    let remove: Vec<((Ipv4Addr, u16), &'static str)> = forwards
        .keys()
        .filter(|key| !wanted.contains(key))
        .map(|key| {
            let reason = if native_keys.contains(key) {
                "本机已能原生发现该世界"
            } else {
                "对端宣告已过期（18 秒没有更新）"
            };
            (*key, reason)
        })
        .collect();

    for (key, reason) in remove {
        if let Some(forward) = forwards.remove(&key) {
            shared
                .fake_announcements
                .lock()
                .unwrap()
                .remove(&(forward.port, announced_motd(&forward.motd, forward.port)));
            shared.log(
                "INFO",
                &format!(
                    "对端 {}:{} 移除本地转发端口 {}：{reason}",
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

/// 伪造公告在网线上实际的 MOTD。
///
/// `build_announcement` 会清掉报文标记、折叠空白，回灌后被解析出来的也是清洗过的
/// 形态；登记与比对必须用同一个形态，否则自己伪造的公告会被当成本机世界。
fn announced_motd(motd: &str, port: u16) -> String {
    parse_announcement(build_announcement(motd, port).as_bytes())
        .map(|announcement| announcement.motd)
        .unwrap_or_default()
}

/// 这个对端世界在本机所在物理局域网里是否本来就能被发现。
///
/// 判定依据只有两条：**端口一致** + **公告来源落在我本机的物理网段**。
/// 不看 MOTD：名字会被 `games` 折叠空白、截断到 48 字符，拿它做匹配既不可靠，
/// 也会在名字对不上时漏判（结果就是同一个世界出两条）。
///
/// 端口是世界打开时随机分配的，同一物理网段里撞到同一个随机端口的概率极低，
/// 再叠加「源地址必须在我自己的物理网段」，足以认定这是同一个世界。
/// 从 overlay 过来的公告（源地址是对端虚拟 IP，不在物理网段里）不在此列：
/// 那条路本身不稳定，不能因为它出现过就不要转发。
fn natively_visible(
    native: &HashMap<(u16, Ipv4Addr), Instant>,
    world: &RemoteWorld,
    lans: &[(Ipv4Addr, Ipv4Addr)],
) -> bool {
    native
        .iter()
        .any(|((port, source), _)| *port == world.port && in_lan(*source, lans))
}

/// 是否已有线程结束（正常运行时只可能是「被迫停止」或 panic）。
fn any_finished(handles: &[JoinHandle<()>]) -> bool {
    handles.iter().any(|handle| handle.is_finished())
}

/// 每 1.5 秒播一次伪造公告，让原版「多人游戏」列表刷出条目。
///
/// 主路径是组播，**所有**网卡都发不出去时才从回环地址单播一份给本机 `4445`。
///
/// 单播兜底必须按「全部失败」判断，不能按单个网卡判断：虚拟网卡（EasyTier 的 utun）
/// 发组播一定失败，如果逐网卡兜底，物理网卡成功的同时回环还会补发一份，
/// 原版按 `IP:端口` 去重，列表里就会出现两条同一个世界（一条 `192.168.x.x`，
/// 一条 `127.0.0.1`）。
fn fake_loop(shared: Arc<Shared>, senders: Vec<(Ipv4Addr, UdpSocket)>) {
    let loopback = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).ok();
    let mut last: Option<Instant> = None;
    while !shared.stopped() {
        let list = shared.announce.lock().unwrap().clone();
        let due = last.map(|at| at.elapsed() >= ANNOUNCE_INTERVAL).unwrap_or(true);
        if due && !list.is_empty() {
            last = Some(Instant::now());
            for announcement in &list {
                let payload = build_announcement(&announcement.motd, announcement.port);
                let mut sent = false;
                for (ip, socket) in &senders {
                    match socket
                        .send_to(payload.as_bytes(), SocketAddr::from((MC_GROUP, MC_PORT)))
                    {
                        Ok(_) => sent = true,
                        Err(err) => shared.log_once(
                            &format!("fake-{ip}"),
                            "WARN",
                            &format!("向 {MC_GROUP}:{MC_PORT} 发送伪造公告失败（网卡 {ip}）：{err}"),
                        ),
                    }
                }
                if sent {
                    continue;
                }
                let Some(loopback) = &loopback else {
                    continue;
                };
                if let Err(err) = loopback.send_to(
                    payload.as_bytes(),
                    SocketAddr::from((Ipv4Addr::LOCALHOST, MC_PORT)),
                ) {
                    shared.log_once(
                        "fake-loopback",
                        "WARN",
                        &format!("单播伪造公告到 127.0.0.1:{MC_PORT} 失败：{err}"),
                    );
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

    /// 4445 是进程外的独占资源：几个用例并行绑定同一个组播端口时会互相抢包。
    /// 用到 4445 的用例先拿这把锁，串行跑。
    static MC_LOCK: Mutex<()> = Mutex::new(());

    fn mc_lock() -> std::sync::MutexGuard<'static, ()> {
        MC_LOCK.lock().unwrap_or_else(|err| err.into_inner())
    }

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

    /// 节点信息里的 `ipv4_addr` 同样带 `/24`：剥掉后复制出来的地址才可直接使用。
    #[test]
    fn parse_ipv4_strips_cidr_prefix() {
        assert_eq!(
            parse_ipv4("10.126.126.1/24"),
            Some(Ipv4Addr::new(10, 126, 126, 1))
        );
        assert_eq!(
            parse_ipv4("10.126.126.1"),
            Some(Ipv4Addr::new(10, 126, 126, 1))
        );
        assert_eq!(parse_ipv4(""), None);
        assert_eq!(parse_ipv4("-"), None);
    }

    /// 线程异常退出（panic）必须能被识别，否则界面会一直显示「运行中」。
    #[test]
    fn finished_thread_is_detected() {
        let running = thread::spawn(|| {
            thread::sleep(Duration::from_millis(200));
        });
        assert!(!any_finished(std::slice::from_ref(&running)));

        let done = thread::spawn(|| {});
        let deadline = Instant::now() + Duration::from_secs(2);
        while !done.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(any_finished(std::slice::from_ref(&done)));
    }

    /// 端到端验证采集链路：按原版节奏向 `224.0.2.60:4445` 播一条公告，
    /// 中继必须把它认成本机世界并写进「开放游戏」的本机游戏表。
    #[test]
    fn relay_detects_local_multicast_announcement() {
        let _mc = mc_lock();
        let log = std::env::temp_dir().join(format!(
            "lanlink-lan-detect-{}-{:?}.log",
            std::process::id(),
            std::thread::current().id()
        ));
        let peers = Arc::new(Mutex::new(Vec::new()));
        let local_games = Arc::new(Mutex::new(crate::games::LocalGamesState::default()));
        let relay = match LanRelay::start(
            "测试房间".to_string(),
            log,
            peers,
            Arc::new(Mutex::new(HashMap::new())),
            local_games.clone(),
        ) {
            Ok(relay) => relay,
            Err(err) => {
                // CI 容器里可能不允许绑定组播：跳过，不算失败
                eprintln!("本机无法绑定 {MC_GROUP}:{MC_PORT}，跳过采集链路验证：{err}");
                return;
            }
        };

        let port = 25599;
        let payload = build_announcement("Steve - Test", port);
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut sent = false;
        let mut detected = false;
        while Instant::now() < deadline {
            // 原版每 1.5 秒播一次：这里边播边等，网卡列表中途变化也不影响
            for ip in local_ipv4s() {
                if let Ok(socket) = bind_sender(ip) {
                    sent |= socket
                        .send_to(payload.as_bytes(), SocketAddr::from((MC_GROUP, MC_PORT)))
                        .is_ok();
                }
            }
            if local_games
                .lock()
                .unwrap()
                .snapshot()
                .iter()
                .any(|game| game.port == port && game.source == "minecraft")
            {
                detected = true;
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        drop(relay);
        assert!(sent, "本机没有可用于发送组播的网卡");
        assert!(detected, "3 秒内没有从组播公告里认出本机世界");
    }

    #[test]
    fn private_range_check() {
        assert!(is_private(Ipv4Addr::new(10, 126, 126, 1)));
        assert!(is_private(Ipv4Addr::new(192, 168, 1, 2)));
        assert!(is_private(Ipv4Addr::new(172, 16, 0, 1)));
        assert!(is_private(Ipv4Addr::new(100, 64, 0, 1)));
        assert!(!is_private(Ipv4Addr::new(8, 8, 8, 8)));
    }

    /// 原生可见判定：只看「端口一致 + 来源在我自己的物理网段」，不看 MOTD。
    #[test]
    fn native_visibility_requires_port_and_physical_source() {
        let lans = [
            (Ipv4Addr::new(192, 168, 31, 115), Ipv4Addr::new(255, 255, 255, 0)),
        ];
        let world = RemoteWorld {
            ip: Ipv4Addr::new(10, 126, 126, 1),
            port: 61779,
            name: "xiaji - 测试".to_string(),
        };

        // 同端口 + 物理局域网来源：原生能发现，不再伪造
        let mut native = HashMap::new();
        native.insert((61779u16, Ipv4Addr::new(192, 168, 31, 155)), Instant::now());
        assert!(natively_visible(&native, &world, &lans));

        // 名字完全不同也不影响判定（不按 MOTD 匹配）
        let renamed = RemoteWorld {
            name: "完全不同的名字".to_string(),
            ..world.clone()
        };
        assert!(natively_visible(&native, &renamed, &lans));

        // 端口不同：不是同一个世界
        assert!(!natively_visible(
            &native,
            &RemoteWorld {
                port: 61780,
                ..world.clone()
            },
            &lans
        ));

        // 来源是对端虚拟 IP（经 overlay 过来）：不算物理局域网原生，仍要伪造
        let mut overlay = HashMap::new();
        overlay.insert((61779u16, Ipv4Addr::new(10, 126, 126, 1)), Instant::now());
        assert!(!natively_visible(&overlay, &world, &lans));

        // 没有物理网卡时不会误判
        assert!(!natively_visible(&native, &world, &[]));
    }

    /// 桥接链路：对端世界 → 本地转发端口 → 伪造公告；撤掉后端口必须释放。
    ///
    /// 对端用本机回环上的 echo 服务代替（只需验证「连上转发能通到目标端口」），
    /// 同时用一个独立的 4445 监听确认伪造公告确实播了出去。
    #[test]
    fn bridge_forwards_remote_world_and_releases_port() {
        let _mc = mc_lock();
        let log = std::env::temp_dir().join(format!(
            "lanlink-lan-bridge-{}-{:?}.log",
            std::process::id(),
            std::thread::current().id()
        ));
        let peers = Arc::new(Mutex::new(Vec::new()));
        let local_games = Arc::new(Mutex::new(crate::games::LocalGamesState::default()));
        let relay = match LanRelay::start(
            "测试房间".to_string(),
            log,
            peers,
            Arc::new(Mutex::new(HashMap::new())),
            local_games.clone(),
        ) {
            Ok(relay) => relay,
            Err(err) => {
                eprintln!("本机无法绑定 {MC_GROUP}:{MC_PORT}，跳过桥接验证：{err}");
                return;
            }
        };

        // 目标世界：本机回环上起一个 echo 服务
        let echo = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("echo 监听失败");
        let echo_port = echo.local_addr().unwrap().port();
        let _echo_thread = thread::spawn(move || {
            for stream in echo.incoming().flatten() {
                thread::spawn(move || {
                    let mut stream = stream;
                    let mut buf = [0u8; 64];
                    while let Ok(n) = stream.read(&mut buf) {
                        if n == 0 || stream.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                });
            }
        });

        // 建转发不再等待任何探测：宣告一到就建、就播
        relay.set_remote_worlds(vec![RemoteWorld {
            ip: Ipv4Addr::LOCALHOST,
            port: echo_port,
            name: "Test World".to_string(),
        }]);

        // 1) 转发端口出现
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut forward_port = 0;
        while Instant::now() < deadline {
            let port = relay
                .shared
                .announce
                .lock()
                .unwrap()
                .first()
                .map(|announcement| announcement.port);
            if let Some(port) = port {
                forward_port = port;
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        assert_ne!(forward_port, 0, "5 秒内没有为对端世界建起本地转发");

        // 2) 连上转发能真的通到目标世界，而且连接要能持续（不能转发完第一批就半关）
        let mut client =
            TcpStream::connect((Ipv4Addr::LOCALHOST, forward_port)).expect("连接本地转发失败");
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client.write_all(b"ping").expect("写入转发失败");
        let mut echoed = [0u8; 4];
        client.read_exact(&mut echoed).expect("没有收到转发回来的数据");
        assert_eq!(&echoed, b"ping");
        // macOS 上 accept 出的 socket 会继承非阻塞标志：转发线程把第一批数据送出去后
        // 紧接着再 read 会拿到 WouldBlock，如果被当成 EOF 就会半关对端，长连接直接断。
        thread::sleep(Duration::from_millis(150));
        client.write_all(b"pong").expect("第二次写入转发失败");
        let mut echoed = [0u8; 4];
        client
            .read_exact(&mut echoed)
            .expect("转发在第一批数据之后把连接断开了");
        assert_eq!(&echoed, b"pong");

        // 3) 独立的 4445 监听必须收到指向该转发端口的伪造公告
        let mut captured = false;
        if let Ok(sniffer) = bind_receiver() {
            let mut joined = false;
            for ip in local_ipv4s() {
                joined |= sniffer.join_multicast_v4(&MC_GROUP, &ip).is_ok();
            }
            if joined {
                let deadline = Instant::now() + Duration::from_secs(3);
                let mut buf = [0u8; MAX_PACKET];
                while Instant::now() < deadline {
                    if let Ok((len, _)) = sniffer.recv_from(&mut buf) {
                        if let Some(announcement) = parse_announcement(&buf[..len]) {
                            if announcement.port == forward_port
                                && announcement.motd == "Test World"
                            {
                                captured = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
        assert!(captured, "3 秒内没有在本机组播上收到伪造公告");

        // 4) 自己伪造的公告不能被当成本机世界
        assert!(
            !local_games
                .lock()
                .unwrap()
                .snapshot()
                .iter()
                .any(|game| game.port == forward_port),
            "伪造公告被当成了本机世界"
        );

        // 5) 对端宣告过期（快照清空）后，转发端口必须立刻撤掉
        relay.set_remote_worlds(Vec::new());
        let deadline = Instant::now() + Duration::from_secs(5);
        let released = loop {
            if TcpListener::bind((Ipv4Addr::UNSPECIFIED, forward_port)).is_ok() {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            thread::sleep(Duration::from_millis(50));
        };
        assert!(released, "对端世界消失后本地转发端口没有释放");

        // 6) 本机能原生发现的世界不再建转发（否则同一个世界会出现两条）
        // 原生公告的来源必须是「本机物理网段里的另一台机器」，这里用本机物理地址代替。
        let native_port: u16 = 63111;
        let physical = relay.shared.lans.first().map(|(ip, _)| *ip);
        if let Some(source) = physical {
            relay
                .shared
                .native
                .lock()
                .unwrap()
                .insert((native_port, source), Instant::now());
            relay.set_remote_worlds(vec![RemoteWorld {
                ip: Ipv4Addr::LOCALHOST,
                port: native_port,
                name: "xiaji - 测试".to_string(),
            }]);
            thread::sleep(Duration::from_millis(2500));
            assert!(
                relay.shared.announce.lock().unwrap().is_empty(),
                "能原生发现的世界不该再建转发"
            );
        }
    }

    #[test]
    fn forward_releases_its_port_on_drop() {
        let forward = Forward::start(
            Ipv4Addr::new(10, 126, 126, 9),
            25565,
            "World".to_string(),
            PathBuf::from("/dev/null"),
            Arc::new(Mutex::new(HashMap::new())),
        )
        .expect("分配本地端口失败");
        let port = forward.port;
        assert_ne!(port, 0);
        assert_ne!(port, MC_PORT);

        drop(forward);
        // 停止后端口必须能重新绑定，说明没有残留监听
        TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).expect("转发停止后端口没有释放");
    }

    /// 出向连接超时按 peer 快照里的延迟推导：直连 1 秒、经中继放宽、封顶 8 秒。
    #[test]
    fn connect_timeout_scales_with_peer_latency() {
        assert_eq!(connect_timeout(None), CONNECT_FALLBACK);
        assert_eq!(connect_timeout(Some(0)), CONNECT_MIN);
        assert_eq!(connect_timeout(Some(128)), Duration::from_millis(1512));
        assert_eq!(connect_timeout(Some(30_000)), CONNECT_MAX);
    }

    fn wait_announce_port(relay: &LanRelay, timeout: Duration) -> u16 {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(port) = relay
                .shared
                .announce
                .lock()
                .unwrap()
                .first()
                .map(|announcement| announcement.port)
            {
                return port;
            }
            thread::sleep(Duration::from_millis(50));
        }
        0
    }

    /// 条目存废只看宣告：目标端口没开也要立刻建转发、立刻播；
    /// 客户端连进来时才去连对端，失败要写清是拒绝 / 超时 / 不可达。
    #[test]
    fn forward_is_announced_even_if_target_port_is_closed() {
        let _mc = mc_lock();
        let log = std::env::temp_dir().join(format!(
            "lanlink-lan-closed-{}-{:?}.log",
            std::process::id(),
            std::thread::current().id()
        ));
        let relay = match LanRelay::start(
            "测试房间".to_string(),
            log.clone(),
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(crate::games::LocalGamesState::default())),
        ) {
            Ok(relay) => relay,
            Err(err) => {
                eprintln!("本机无法绑定 {MC_GROUP}:{MC_PORT}，跳过关闭端口验证：{err}");
                return;
            }
        };

        // 先占一个端口再放掉，确保它没人监听
        let closed = {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            listener.local_addr().unwrap().port()
        };
        relay.set_remote_worlds(vec![RemoteWorld {
            ip: Ipv4Addr::LOCALHOST,
            port: closed,
            name: "Closed".to_string(),
        }]);
        let port = wait_announce_port(&relay, Duration::from_secs(5));
        assert_ne!(port, 0, "宣告在的时候，即使目标端口没开也要立刻播条目");

        // 连进来：连不上对端就必须断开客户端，而不是挂住或留一个死连接
        let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("连接本地转发失败");
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut buf = [0u8; 8];
        match client.read(&mut buf) {
            Ok(read) => assert_eq!(read, 0, "连不上对端时应关闭客户端连接"),
            Err(err) => assert_eq!(
                err.kind(),
                io::ErrorKind::ConnectionReset,
                "连不上对端时应断开客户端，而不是挂住：{err}"
            ),
        }

        // 失败原因要具体，不能只说「连接中断」
        let body = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(
            body.contains("失败：对端拒绝连接（RST）"),
            "日志里没有写清失败原因：{body}"
        );
        assert!(
            body.contains("已断开客户端"),
            "日志里没有说明客户端已被断开：{body}"
        );
        assert!(
            body.contains(&format!("上限 {} ms", CONNECT_FALLBACK.as_millis())),
            "日志里没有带上超时上限：{body}"
        );
    }

    /// 同一个世界活着的期间本地端口不能变（客户端会缓存这个地址）；
    /// 宣告过期后立刻撤，端口随之释放。
    #[test]
    fn forward_port_is_stable_while_announced_and_released_after_expiry() {
        let _mc = mc_lock();
        let log = std::env::temp_dir().join(format!(
            "lanlink-lan-stable-{}-{:?}.log",
            std::process::id(),
            std::thread::current().id()
        ));
        let relay = match LanRelay::start(
            "测试房间".to_string(),
            log,
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(crate::games::LocalGamesState::default())),
        ) {
            Ok(relay) => relay,
            Err(err) => {
                eprintln!("本机无法绑定 {MC_GROUP}:{MC_PORT}，跳过端口稳定性验证：{err}");
                return;
            }
        };

        let echo = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("echo 监听失败");
        let echo_port = echo.local_addr().unwrap().port();
        let _echo_thread = thread::spawn(move || {
            for stream in echo.incoming().flatten() {
                drop(stream);
            }
        });
        let world = RemoteWorld {
            ip: Ipv4Addr::LOCALHOST,
            port: echo_port,
            name: "Stable".to_string(),
        };

        relay.set_remote_worlds(vec![world.clone()]);
        let port = wait_announce_port(&relay, Duration::from_secs(5));
        assert_ne!(port, 0, "宣告到达后应立刻建转发");

        // 持续续上同一份宣告：端口不能换
        let until = Instant::now() + Duration::from_millis(2500);
        while Instant::now() < until {
            relay.set_remote_worlds(vec![world.clone()]);
            thread::sleep(Duration::from_millis(200));
        }
        assert_eq!(
            relay.shared.announce.lock().unwrap().first().map(|a| a.port),
            Some(port),
            "宣告续上时不应更换本地端口"
        );
        assert!(
            TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).is_err(),
            "宣告还在时端口不应被释放"
        );

        // 宣告过期：立刻撤条目、释放端口
        relay.set_remote_worlds(Vec::new());
        let deadline = Instant::now() + Duration::from_secs(5);
        let released = loop {
            if TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).is_ok() {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            thread::sleep(Duration::from_millis(50));
        };
        assert!(released, "宣告过期后本地转发端口没有释放");
        assert!(
            relay.shared.announce.lock().unwrap().is_empty(),
            "宣告过期后不应再播条目"
        );
    }
}
