//! easytier-core sidecar 的生命周期管理与状态查询。
//!
//! core 以独立进程方式运行（LGPL 合规、崩溃可隔离），状态通过
//! `easytier-cli -o json` 从本地 RPC 端口读取。
//!
//! 连接状态机：idle → connecting → connected，异常时进入 failed 或 reconnecting。

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::config;
use crate::proc;
use crate::rooms::Room;

const LOG_TAIL_LINES: usize = 60;
const LOG_TAIL_BYTES: u64 = 48 * 1024;
const LOG_READ_BYTES: u64 = 512 * 1024;
const MAX_LINE_CHARS: usize = 500;
const START_TIMEOUT: Duration = Duration::from_secs(20);
const PORT_RELEASE_TIMEOUT: Duration = Duration::from_secs(3);

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
}

impl Tunnel {
    pub fn new() -> Self {
        let log_dir = log_dir();
        let _ = fs::create_dir_all(&log_dir);
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
        };
        // 上次以 root 运行留下的日志文件：这次启动就顺手把权限交还给登录用户
        tunnel.relax_log_permissions();
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
    /// 启动 core 时（以及每次启动 App 时）把权限与属主交还给当前登录用户。
    fn relax_log_permissions(&self) {
        let path = self.log_path();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(&path) {
                let mut permissions = meta.permissions();
                if permissions.mode() & 0o666 != 0o666 {
                    permissions.set_mode(0o666);
                    let _ = fs::set_permissions(&path, permissions);
                }
            }
        }
        config::hand_back(&path);
    }

    fn pid_path(&self) -> PathBuf {
        self.log_dir.join("core.pid")
    }

    /// 加入房间。端口仍被上次残留的 core 占用时，先清理再启动。
    pub fn start(&mut self, room: Room, hostname: Option<String>) -> Status {
        if self.child.is_some() {
            return self.status();
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
        }
        self.status()
    }

    pub fn stop(&mut self) -> Status {
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
            let code = code.map(|c| c.to_string()).unwrap_or_else(|| "signal".to_string());
            self.state = State::Failed;
            self.error = Some(format!("core 进程已退出（状态码 {code}）"));
            let hint = hint_from_log(&self.tail_log());
            self.hint = hint.as_ref().map(|hint| hint.text.clone());

            let port_conflict = hint.map(|hint| hint.kind == HintKind::PortConflict).unwrap_or(false);
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

        // 2. 读取节点与对端信息
        if status.running {
            if let Some(node) = self.run_cli_json("node") {
                status.ipv4 = node
                    .get("ipv4_addr")
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned);
                status.hostname = node
                    .get("hostname")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                status.version = node
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
            }

            if let Some(peers) = self.run_cli_json("peer").and_then(|v| v.as_array().cloned()) {
                let mut peer_count = 0u32;
                let mut direct_count = 0u32;
                for peer in &peers {
                    let cost = peer.get("cost").and_then(|v| v.as_str()).unwrap_or_default();
                    if cost.eq_ignore_ascii_case("local") {
                        continue;
                    }
                    peer_count += 1;
                    let proto = peer
                        .get("tunnel_proto")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    if !proto.is_empty() && proto != "-" {
                        direct_count += 1;
                    }
                    status.peers.push(PeerInfo {
                        hostname: peer
                            .get("hostname")
                            .and_then(|v| v.as_str())
                            .unwrap_or("未知节点")
                            .to_string(),
                        ipv4: peer
                            .get("ipv4")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-")
                            .to_string(),
                        cost: cost.to_string(),
                        proto: proto.to_string(),
                    });
                }
                status.peer_count = peer_count;
                status.direct_peer_count = direct_count;
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

        status.state = self.state;
        status.error = self.error.clone();
        status.hint = self.hint.clone();
        status.room = self.room.as_ref().map(|room| room.code.clone());
        status.room_name = self.room.as_ref().map(|room| room.name.clone());
        status.role = self.room.as_ref().map(|room| room.role.clone());
        status.network = self.room.as_ref().map(|room| room.network.clone());
        status
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
    /// 现在提权启动时会把日志权限/属主交还给登录用户（见 `relax_log_permissions`），
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

        let _ = fs::create_dir_all(&self.log_dir);
        let log_path = self.log_path();
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|err| format!("无法打开日志文件 {}：{err}", log_path.display()))?;
        let log_err = log
            .try_clone()
            .map_err(|err| format!("无法复制日志句柄：{err}"))?;
        // 以管理员身份运行时新建的日志属于 root，交还给登录用户，避免之后清空被拒
        self.relax_log_permissions();

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
        let core_path = core.display().to_string();
        let mut victims: Vec<u32> = Vec::new();

        for holder in port_holders() {
            if holder.pid != me && holder.command.contains(&core_path) {
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

    fn run_cli_json(&self, subcommand: &str) -> Option<serde_json::Value> {
        let cli = self.cli.as_ref()?;
        let output = proc::command(cli)
            .arg("-p")
            .arg(config::RPC_PORTAL)
            .arg("-o")
            .arg("json")
            .arg(subcommand)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        serde_json::from_slice(&output.stdout).ok()
    }

    fn tail_log(&self) -> Vec<String> {
        let path = self.log_path();
        let Ok(mut file) = File::open(&path) else {
            return Vec::new();
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len > LOG_TAIL_BYTES {
            let _ = file.seek(SeekFrom::Start(len - LOG_TAIL_BYTES));
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

fn full_command(pid: u32, fallback: &str) -> String {
    proc::command("ps")
        .args(["-o", "command=", "-p", &pid.to_string()])
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn pgrep_pids(core: &Path) -> Vec<u32> {
    let Ok(output) = proc::command("pgrep").arg("-f").arg(core).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect()
}

fn signal(pid: u32, sig: i32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, sig);
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, sig);
    }
}

fn wait_ports_free(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if port_holders().is_empty() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(120));
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

fn log_dir() -> PathBuf {
    #[cfg(unix)]
    {
        PathBuf::from("/tmp/lanlink")
    }
    #[cfg(windows)]
    {
        std::env::temp_dir().join("lanlink")
    }
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
}
