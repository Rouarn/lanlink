mod broadcast;
mod config;
mod core;
mod elevate;
mod games;
mod http;
mod lan;
mod proc;
mod rooms;
mod settings;
mod single;
#[cfg(windows)]
mod win_task;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{Manager, State};

use crate::core::{LogLine, Status, Tunnel};
use crate::rooms::Room;
use crate::settings::Settings;

struct AppState {
    /// 用 `Arc` 包一层，命令层才能把锁与阻塞操作整体搬进 `spawn_blocking`，
    /// 保持命令签名不变的同时不冻住主线程。
    tunnel: Arc<Mutex<Tunnel>>,
}

/// 提权过程中（等待管理员实例接管）避免重复拉起。
static ELEVATING: AtomicBool = AtomicBool::new(false);
/// 后台线程里提权失败的原因，交给 `get_status` 带回前端。
static ELEVATE_ERROR: Mutex<Option<String>> = Mutex::new(None);

#[derive(Serialize)]
struct AppInfo {
    elevated: bool,
    core_path: String,
    cli_path: String,
    rpc_portal: String,
    platform: String,
    /// 内置解析服务地址（用户没改过时设置里就是它，界面用它实现「恢复默认」）
    default_resolver_base_url: String,
}

#[tauri::command]
fn app_info(state: State<'_, AppState>) -> AppInfo {
    let tunnel = state.tunnel.lock().unwrap();
    AppInfo {
        elevated: elevate::is_elevated(),
        core_path: tunnel.core_path().unwrap_or_else(|| "未找到".to_string()),
        cli_path: tunnel.cli_path().unwrap_or_else(|| "未找到".to_string()),
        rpc_portal: config::RPC_PORTAL.to_string(),
        platform: std::env::consts::OS.to_string(),
        default_resolver_base_url: settings::DEFAULT_RESOLVER_BASE_URL.to_string(),
    }
}

/// 状态查询要读日志、跑 easytier-cli，可能被启动流程短暂阻塞，
/// 因此整个查询放到阻塞线程池，不占用异步运行时线程（窗口不会白屏）。
#[tauri::command]
async fn get_status(state: State<'_, AppState>) -> Result<Status, String> {
    let tunnel = state.tunnel.clone();
    let mut status = tauri::async_runtime::spawn_blocking(move || tunnel.lock().unwrap().status())
        .await
        .map_err(|err| format!("状态查询失败：{err}"))?;
    if let Some(err) = ELEVATE_ERROR.lock().unwrap().take() {
        status.elevate_error = Some(err);
    }
    Ok(status)
}

#[tauri::command]
fn get_settings() -> Settings {
    Settings::load()
}

/// 保存中继与解析服务配置。返回保存后的设置；前端负责提示「需要重连生效」。
/// 解析服务 Token 内置在客户端，不属于用户配置，因此不经过设置结构。
#[tauri::command]
fn save_settings(relays: String, resolver_base_url: Option<String>) -> Result<Settings, String> {
    let relays = settings::normalize_relays(&relays)?;
    // 允许保存空列表：表示「清除中继配置」，此后创建/加入会被前端拦住
    let resolver_base_url = resolver_base_url
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
        .map(|url| url.trim_end_matches('/').to_string());
    // 中继与解析地址之外的状态（Windows 静默提权标记）必须原样保留
    let existing = Settings::load();
    let mut settings = Settings::new(relays, resolver_base_url);
    settings.silent_elevate = existing.silent_elevate;
    settings.silent_prompted = existing.silent_prompted;
    settings.lan_discovery = existing.lan_discovery;
    settings.broadcast_ports = existing.broadcast_ports.clone();
    settings.manual_games = existing.manual_games.clone();
    settings.save()?;
    Ok(settings)
}

/// 开关《我的世界》局域网发现中继：立刻落设置，连接中会立刻停/起。
#[tauri::command]
fn set_lan_discovery(state: State<'_, AppState>, enabled: bool) -> Result<Status, String> {
    let mut settings = Settings::load();
    settings.lan_discovery = enabled;
    settings.save()?;
    Ok(state.tunnel.lock().unwrap().set_lan_discovery(enabled))
}

/// 设置通用 UDP 广播转发的端口表（空数组 = 关闭）；界面定稿前先只保留接口。
#[tauri::command]
fn set_broadcast_ports(state: State<'_, AppState>, ports: Vec<u16>) -> Result<Status, String> {
    let mut settings = Settings::load();
    settings.broadcast_ports = broadcast::normalize_ports(&ports);
    settings.save()?;
    Ok(state
        .tunnel
        .lock()
        .unwrap()
        .set_broadcast_ports(settings.broadcast_ports.clone()))
}

/// 设置「开放游戏」的手动登记表。只登记名称与端口，不做进程识别。
#[tauri::command]
fn set_manual_games(
    state: State<'_, AppState>,
    items: Vec<games::ManualGame>,
) -> Result<Status, String> {
    let items = games::normalize_manual(&items);
    let mut settings = Settings::load();
    settings.manual_games = items.clone();
    settings.save()?;
    Ok(state.tunnel.lock().unwrap().set_manual_games(items))
}

/// 提权方式（Windows 计划任务静默提权 / 其他平台固定启动即提权）。
#[derive(Serialize)]
struct ElevationInfo {
    platform: String,
    elevated: bool,
    /// 是否已开启计划任务静默提权
    silent: bool,
    /// 计划任务当前是否真的存在（用户可能在任务计划程序里手动删掉）
    task_present: bool,
}

#[cfg(windows)]
fn task_present() -> bool {
    win_task::task_exists()
}

#[cfg(not(windows))]
fn task_present() -> bool {
    false
}

fn current_elevation_info() -> ElevationInfo {
    ElevationInfo {
        platform: std::env::consts::OS.to_string(),
        elevated: elevate::is_elevated(),
        silent: Settings::load().silent_elevate,
        task_present: task_present(),
    }
}

#[tauri::command]
fn elevation_info() -> ElevationInfo {
    current_elevation_info()
}

/// 开启 / 关闭 Windows「计划任务静默提权」。
#[tauri::command]
fn set_silent_elevate(enabled: bool) -> Result<ElevationInfo, String> {
    #[cfg(windows)]
    return set_silent_elevate_windows(enabled);

    #[cfg(not(windows))]
    {
        let _ = enabled;
        Err("当前平台不支持计划任务静默提权".to_string())
    }
}

/// 注册任务需要管理员权限：普通权限下先用 UAC 拉起 `--setup-elevate` 实例接管，
/// 由它在启动阶段建任务、落标记；失败则保持原状，界面继续可用。
#[cfg(windows)]
fn set_silent_elevate_windows(enabled: bool) -> Result<ElevationInfo, String> {
    if enabled {
        if !elevate::is_elevated() {
            single::release();
            match elevate::relaunch_as_root(&["--setup-elevate".to_string()]) {
                Ok(()) => {
                    std::thread::spawn(|| {
                        if single::wait_for_successor(
                            Duration::from_secs(120),
                            &AtomicBool::new(false),
                        )
                        .is_some()
                        {
                            std::thread::sleep(Duration::from_millis(150));
                            std::process::exit(0);
                        }
                        // 管理员实例没接管：把锁抢回来，当前窗口继续用
                        let _ = single::acquire();
                    });
                    return Ok(current_elevation_info());
                }
                Err(err) => {
                    let _ = single::acquire();
                    return Err(format!("管理员授权失败：{err}"));
                }
            }
        }
        install_silent_task()?;
    } else {
        if !elevate::is_elevated() {
            return Err("取消静默提权需要管理员权限，请以管理员身份运行后再试".to_string());
        }
        win_task::remove()?;
        let mut settings = Settings::load();
        settings.silent_elevate = false;
        settings.save()?;
    }
    Ok(current_elevation_info())
}

/// 建计划任务并落标记；失败时标记不写，调用方负责回退。
#[cfg(windows)]
fn install_silent_task() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|err| format!("无法定位可执行文件：{err}"))?;
    win_task::install(&exe)?;
    let mut settings = Settings::load();
    settings.silent_elevate = true;
    settings.silent_prompted = true;
    settings.save()?;
    Ok(())
}

/// 创建房间要访问远端解析服务，放到阻塞线程池，避免阻塞 UI 线程。
#[tauri::command]
async fn create_room(name: String) -> Result<Room, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = Settings::load();
        rooms::create(
            &name,
            settings.relays,
            settings.resolver_base_url,
            resolver_token(),
        )
    })
    .await
    .map_err(|err| format!("创建房间失败：{err}"))?
}

/// 解析房间码（短码 / 完整码），用于加入前的预览。
#[tauri::command]
async fn resolve_code(code: String) -> Result<Room, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = Settings::load();
        rooms::resolve_code(&code, settings.resolver_base_url, resolver_token())
    })
    .await
    .map_err(|err| format!("解析房间码失败：{err}"))?
}

/// 导出完整房间码（自包含，可发给别人）。
#[tauri::command]
fn export_room_code(code: String) -> Result<String, String> {
    let settings = Settings::load();
    let room = rooms::lookup(
        &code,
        settings.relays,
        settings.resolver_base_url,
        resolver_token(),
    )?;
    rooms::export_long(&room)
}

#[tauri::command]
fn recent_rooms() -> Vec<Room> {
    rooms::recent()
}

/// 删除一条历史房间（重启后不再出现）。
#[tauri::command]
fn forget_room(code: String) -> Result<(), String> {
    rooms::forget(&code)
}

/// 清空全部历史房间。
#[tauri::command]
fn clear_rooms() -> Result<(), String> {
    rooms::clear()
}

/// 连接指定房间。普通权限下会以管理员重新拉起自身，然后把当前实例退出。
///
/// 中继来自房间记录：加入方不需要先在本地配置中继，全新安装也能直接连；
/// 只有创建房间（`create_room`）才要求本地已配中继。
#[tauri::command]
async fn connect(state: State<'_, AppState>, code: String) -> Result<Status, String> {
    let tunnel = state.tunnel.clone();
    // 解析房间码要发网络请求、启动 core 要等端口回收：整体放到阻塞线程池
    tauri::async_runtime::spawn_blocking(move || {
        let settings = Settings::load();
        let room = rooms::lookup(
            &code,
            settings.relays.clone(),
            settings.resolver_base_url.clone(),
            resolver_token(),
        )?;
        request_connect(&tunnel, room, None)
    })
    .await
    .map_err(|err| format!("连接失败：{err}"))?
}

fn request_connect(
    tunnel: &Arc<Mutex<Tunnel>>,
    room: Room,
    hostname: Option<String>,
) -> Result<Status, String> {
    // 只在入口算一次：提权转发给别人、自己直接启动 core，两处必须是同一个名字，
    // 否则对端看到的名称会随「当前实例是不是已经提权」而变。
    let hostname = effective_hostname(hostname);
    if !elevate::is_elevated() {
        if ELEVATING.swap(true, Ordering::SeqCst) {
            // 上一次授权还没结束，别重复拉起实例
            let mut status = tunnel.lock().unwrap().status();
            status.elevating = true;
            return Ok(status);
        }

        // 先在普通用户下写入缓存（房主身份等），避免 root 身份创建用户目录文件
        rooms::touch(&room);

        let name = hostname.unwrap_or_else(default_hostname);
        let mut args = vec![
            "--elevated-run".to_string(),
            "--room".to_string(),
            room.code.clone(),
            "--hostname".to_string(),
            name,
            // root 实例没有用户的 HOME，必须显式带上数据目录与文件属主
            "--data-dir".to_string(),
            config::data_dir().display().to_string(),
        ];
        #[cfg(unix)]
        args.extend([
            "--owner".to_string(),
            format!("{}:{}", unsafe { libc::getuid() }, unsafe { libc::getgid() }),
        ]);

        // 交棒：先把实例锁放掉，否则管理员实例会把自己当成第二个实例而退出
        single::release();

        let canceled = std::sync::Arc::new(AtomicBool::new(false));

        // 线程一：拉起管理员实例。授权对话框可能阻塞到子进程结束，因此放到后台
        let cancel_flag = canceled.clone();
        std::thread::spawn(move || {
            if let Err(err) = elevate::relaunch_as_root(&args) {
                // 授权被取消 / 失败：把锁拿回来，当前窗口继续可用
                cancel_flag.store(true, Ordering::SeqCst);
                let _ = single::acquire();
                ELEVATING.store(false, Ordering::SeqCst);
                *ELEVATE_ERROR.lock().unwrap() = Some(format!("管理员授权失败：{err}"));
            }
        });

        // 线程二：盯着 pid 文件，管理员实例一出现就立即让位。
        // 独立于授权调用，避免授权调用阻塞时旧实例被留在 Dock 和后台。
        let cancel_flag = canceled.clone();
        std::thread::spawn(move || {
            if let Some(pid) = single::wait_for_successor(Duration::from_secs(120), &cancel_flag) {
                eprintln!("管理员实例 {pid} 已接管，当前进程退出");
                std::thread::sleep(Duration::from_millis(150));
                std::process::exit(0);
            }
            if !cancel_flag.load(Ordering::SeqCst) {
                eprintln!("管理员实例未在预期时间内接管，保留当前窗口");
                let _ = single::acquire();
                ELEVATING.store(false, Ordering::SeqCst);
                *ELEVATE_ERROR.lock().unwrap() =
                    Some("管理员实例没有启动起来，请重试连接。".to_string());
            }
        });

        let mut status = tunnel.lock().unwrap().status();
        status.elevating = true;
        return Ok(status);
    }

    Ok(tunnel.lock().unwrap().start(room, hostname))
}

/// 断开要等 core 退出、回收端口，可能要几秒：放到阻塞线程池，界面保持可响应。
///
/// 用 `AppHandle`（拥有所有权）而不是借用参数：异步命令里借用参数要求返回 `Result`，
/// 这里保持前端拿到的返回值不变。
#[tauri::command]
async fn disconnect(app: tauri::AppHandle) -> Status {
    let tunnel = app.state::<AppState>().tunnel.clone();
    tauri::async_runtime::spawn_blocking(move || tunnel.lock().unwrap().stop())
        .await
        .unwrap_or_else(|err| {
            eprintln!("断开失败（工作线程异常）：{err}");
            Status::default()
        })
}

/// 读日志是磁盘 IO，放到阻塞线程池。
#[tauri::command]
async fn read_logs(
    app: tauri::AppHandle,
    level: Option<String>,
    limit: Option<usize>,
) -> Vec<LogLine> {
    let tunnel = app.state::<AppState>().tunnel.clone();
    tauri::async_runtime::spawn_blocking(move || {
        tunnel
            .lock()
            .unwrap()
            .read_logs(level.as_deref().unwrap_or("all"), limit.unwrap_or(400))
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
fn clear_logs(state: State<'_, AppState>) -> Result<(), String> {
    state.tunnel.lock().unwrap().clear_logs()
}

/// 解析服务 Token 是编译进客户端的内置常量，所有远端请求统一携带。
fn resolver_token() -> Option<String> {
    Some(settings::RESOLVER_TOKEN.to_string())
}

/// 决定传给 EasyTier 的 `--hostname`。
///
/// - 命令行显式带 `--hostname`（提权转发）时以它为准，只做去空白；
/// - 否则 macOS 取用户可读的设备名（`config::device_name`），
///   避免把 `gethostname()` 返回的 DHCP IP（本机实测就是 `192.168.31.115`）
///   当成主机名，导致对端列表里只显示 IP；
/// - Windows 保持原状：返回 `None`，由 EasyTier 自己取系统计算机名（`Redmi-G-Pro`）。
fn effective_hostname(hostname: Option<String>) -> Option<String> {
    hostname
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .or_else(config::device_name)
}

fn default_hostname() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("lanlink-{:04x}", nanos % 0xffff)
}

/// macOS：启动即提权。
///
/// 在创建任何窗口之前判断权限：不是 root 就立刻请求管理员授权并用 root 重开自己，
/// 当前进程随即退出；授权被取消或失败则明确告知原因后退出，不进入「无权限也能用一半」的状态。
#[cfg(target_os = "macos")]
fn elevate_at_launch(args: &[String]) {
    if args.iter().any(|arg| arg == "--elevated-run") || elevate::is_elevated() {
        return;
    }

    // 开发态默认跳过启动即提权：
    // `tauri dev` 跑的是 target/debug 下的裸可执行文件，走授权会让当前进程先退出，
    // dev 会话随之结束（Vite 也被带走），root 实例没有 dev server 可加载，窗口就不出现。
    // 连接时的提权路径不受影响（点「连接」仍按原有方式请求授权）。
    // 判据用 `tauri::is_dev()`（即未启用 `custom-protocol`），而不是 `debug_assertions`：
    // `tauri build --debug` 产出的 .app 已经内嵌前端资源，属于可分发产物，必须启动即提权。
    // 要在 dev 里验证真实的启动提权，设 LANLINK_FORCE_ELEVATE=1。
    if tauri::is_dev() && std::env::var_os("LANLINK_FORCE_ELEVATE").is_none() {
        eprintln!("[lanlink] 开发模式：跳过启动即提权（验证请设 LANLINK_FORCE_ELEVATE=1）");
        return;
    }

    // 已经有实例在跑（root 实例持有 /tmp 下的锁）：不再重复索要授权
    if single::acquire().is_err() {
        return;
    }
    // 马上要拉起 root 实例，锁必须先放掉，否则它会把自己当成第二个实例而退出
    single::release();

    let mut extra = vec![
        "--elevated-run".to_string(),
        "--data-dir".to_string(),
        config::data_dir().display().to_string(),
        "--owner".to_string(),
        // root 实例没有用户的 HOME，必须显式带上数据目录与文件属主
        format!("{}:{}", unsafe { libc::getuid() }, unsafe { libc::getgid() }),
    ];
    // 兼容从命令行带进来的连接参数（正常情况下启动时没有这些）
    for name in ["--room", "--hostname"] {
        if let Some(value) = args
            .windows(2)
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].clone())
        {
            extra.push(name.to_string());
            extra.push(value);
        }
    }

    match elevate::relaunch_as_root(&extra) {
        Ok(()) => std::process::exit(0),
        Err(err) => {
            elevate::show_fatal_alert(&format!(
                "LanLink 需要管理员权限才能创建虚拟网卡，没有权限无法启动。{err}"
            ));
            std::process::exit(1);
        }
    }
}

/// Windows：启动阶段的提权处理（计划任务静默提权 / 首次授权说明）。
///
/// 与 macOS 的「拒绝就退出」不同，这里任何失败都只回退到普通权限启动，
/// 界面照常可用（改中继、看日志不受影响），点「连接」时再走一次 UAC。
#[cfg(windows)]
fn prepare_launch_windows(args: &[String]) {
    let setup = args.iter().any(|arg| arg == "--setup-elevate");
    let from_task = args.iter().any(|arg| arg == "--from-task");

    // 管理员实例（计划任务或 UAC 拉起）：正常建界面；
    // `--setup-elevate` 还负责把任务与标记补上，失败也不影响本次使用
    if elevate::is_elevated() {
        if setup {
            if let Err(err) = install_silent_task() {
                eprintln!("创建计划任务失败：{err}");
                win_task::warn(&format!(
                    "LanLink 无法创建计划任务，本次仍可正常使用，但下次启动还会请求管理员授权。\n\n{err}"
                ));
            }
        } else if Settings::load().silent_elevate {
            // 程序被移动到别的目录后，任务里的路径会失效：管理员实例顺手刷新一次
            let _ = install_silent_task();
        }
        return;
    }

    // UAC 没能提到管理员：按普通权限启动，不再尝试，避免来回拉起
    if setup {
        eprintln!("管理员实例未拿到权限，本次按普通权限启动");
        return;
    }

    let settings = Settings::load();

    // 已开启静默提权：交给计划任务以最高权限启动自身
    if settings.silent_elevate {
        if !win_task::task_exists() {
            // 任务被删或创建失败：标记失效，退回「点连接时提权」
            eprintln!("计划任务已不存在，已关闭静默提权标记");
            let mut stale = settings.clone();
            stale.silent_elevate = false;
            let _ = stale.save();
            return;
        }
        // 任务已经拉起过实例但仍不是管理员（任务被降级/策略限制）：不再重复触发
        if from_task {
            eprintln!("计划任务未能提供管理员权限，本次按普通权限启动");
            return;
        }
        if single::acquire().is_err() {
            return;
        }
        // 任务里要拉起新实例，锁必须先让出来
        single::release();
        if win_task::run().is_ok()
            && single::wait_for_successor(Duration::from_secs(30), &AtomicBool::new(false)).is_some()
        {
            std::process::exit(0);
        }
        eprintln!("计划任务没有接管，本次按普通权限启动");
        return;
    }

    // 首次：只问一次，拒绝后不再打扰（设置页随时可以再开）
    if settings.silent_prompted {
        return;
    }
    if single::acquire().is_err() {
        return;
    }
    let agreed = win_task::confirm_first_time();
    let mut decided = settings.clone();
    decided.silent_prompted = true;
    let _ = decided.save();
    single::release();
    if !agreed {
        return;
    }

    match elevate::relaunch_as_root(&["--setup-elevate".to_string()]) {
        Ok(()) => {
            if single::wait_for_successor(Duration::from_secs(120), &AtomicBool::new(false)).is_some()
            {
                std::process::exit(0);
            }
            eprintln!("管理员实例没有接管，本次按普通权限启动");
        }
        Err(err) => eprintln!("首次授权失败，本次按普通权限启动：{err}"),
    }
}

/// Android/iOS 上 Tauri 需要这个宏导出入口符号，桌面端不受影响。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    let arg_value = |name: &str| {
        args.windows(2)
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].clone())
    };

    // 提权实例：沿用发起者的数据目录，并把写回的文件交还原用户
    if let Some(dir) = arg_value("--data-dir").filter(|dir| !dir.trim().is_empty()) {
        let owner = arg_value("--owner").and_then(|raw| {
            let (uid, gid) = raw.split_once(':')?;
            Some((uid.parse().ok()?, gid.parse().ok()?))
        });
        config::set_override(std::path::PathBuf::from(dir), owner);
    }

    // macOS：主窗口出现之前先解决权限，任何时刻系统里只有一个界面进程
    #[cfg(target_os = "macos")]
    elevate_at_launch(&args);

    // Windows：优先用计划任务静默提权；没有标记时只问一次是否开启
    #[cfg(windows)]
    prepare_launch_windows(&args);

    // 单实例：只允许一个界面进程（提权后的管理员实例会接着持有这把锁）
    if let Err(err) = single::acquire() {
        eprintln!("{err}，本次启动直接退出");
        return;
    }

    let elevated_run = args.iter().any(|arg| arg == "--elevated-run");
    let hostname = args
        .windows(2)
        .find(|pair| pair[0] == "--hostname")
        .map(|pair| pair[1].clone());
    let room_code = args
        .windows(2)
        .find(|pair| pair[0] == "--room")
        .map(|pair| pair[1].clone());

    let app = tauri::Builder::default()
        .manage(AppState {
            tunnel: Arc::new(Mutex::new(Tunnel::new())),
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            get_status,
            get_settings,
            save_settings,
            set_lan_discovery,
            set_broadcast_ports,
            set_manual_games,
            elevation_info,
            set_silent_elevate,
            create_room,
            resolve_code,
            export_room_code,
            recent_rooms,
            forget_room,
            clear_rooms,
            connect,
            disconnect,
            read_logs,
            clear_logs
        ])
        // 点红叉等同于退出：先停 core 再退出，
        // 否则 core 会脱离界面继续占着虚拟网卡与 15888 / 11010-11013
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                stop_core(window.app_handle());
                window.app_handle().exit(0);
            }
        })
        .setup(move |app| {
            // 提权实例由系统授权进程拉起，窗口可能落在别的窗口后面（被遮挡时 WebView 不绘制），
            // 这里主动激活一次，保证交接后用户立刻看到界面
            if elevated_run {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_focus();
                }
            }

            // 只有带了 --room 的启动才自动连接（正常启动没有）
            let Some(room_code) = room_code.clone() else {
                return Ok(());
            };

            let handle = app.handle().clone();
            let hostname = hostname.clone();
            // 等窗口创建完成后再连，便于前端立即看到「连接中」状态
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(600));
                let settings = Settings::load();
                match rooms::lookup(
                    &room_code,
                    settings.relays,
                    settings.resolver_base_url,
                    resolver_token(),
                ) {
                    Ok(room) => {
                        let tunnel = handle.state::<AppState>().tunnel.clone();
                        if let Err(err) = request_connect(&tunnel, room, hostname) {
                            eprintln!("连接失败：{err}");
                        }
                    }
                    Err(err) => eprintln!("房间解析失败：{err}"),
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("LanLink 启动失败");

    // Cmd+Q / 系统退出同样要把 core 一起带走，避免孤儿 core 继续占着端口
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            stop_core(handle);
        }
    });
}

/// 退出前停掉 core：回收子进程与端口，避免留下孤儿进程。
fn stop_core(handle: &tauri::AppHandle) {
    if let Some(state) = handle.try_state::<AppState>() {
        state.tunnel.lock().unwrap().stop();
    }
}
