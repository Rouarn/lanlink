mod config;
mod core;
mod elevate;
mod http;
mod rooms;
mod settings;
mod single;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{Manager, State};

use crate::core::{LogLine, Status, Tunnel};
use crate::rooms::Room;
use crate::settings::Settings;

struct AppState {
    tunnel: Mutex<Tunnel>,
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
/// 因此用 async 命令放到工作线程，避免冻住主线程（窗口会白屏）。
#[tauri::command]
async fn get_status(state: State<'_, AppState>) -> Result<Status, String> {
    let mut status = state.tunnel.lock().unwrap().status();
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
    let settings = Settings {
        relays,
        resolver_base_url,
    };
    settings.save()?;
    Ok(settings)
}

#[tauri::command]
fn create_room(name: String) -> Result<Room, String> {
    let settings = Settings::load();
    rooms::create(
        &name,
        settings.relays,
        settings.resolver_base_url,
        resolver_token(),
    )
}

/// 解析房间码（短码 / 完整码），用于加入前的预览。
#[tauri::command]
fn resolve_code(code: String) -> Result<Room, String> {
    let settings = Settings::load();
    rooms::resolve_code(&code, settings.resolver_base_url, resolver_token())
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
#[tauri::command]
fn connect(state: State<'_, AppState>, code: String) -> Result<Status, String> {
    let settings = Settings::load();
    if settings.relays.is_empty() {
        return Err("还没有配置中继服务器，请先在「设置」里填写".to_string());
    }
    let room = rooms::lookup(
        &code,
        settings.relays.clone(),
        settings.resolver_base_url.clone(),
        resolver_token(),
    )?;
    request_connect(&state, room, None)
}

fn request_connect(
    state: &AppState,
    room: Room,
    hostname: Option<String>,
) -> Result<Status, String> {
    if !elevate::is_elevated() {
        if ELEVATING.swap(true, Ordering::SeqCst) {
            // 上一次授权还没结束，别重复拉起实例
            let mut status = state.tunnel.lock().unwrap().status();
            status.elevating = true;
            return Ok(status);
        }

        // 先在普通用户下写入缓存（房主身份等），避免 root 身份创建用户目录文件
        rooms::touch(&room);

        let name = hostname
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(default_hostname);
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

        let mut status = state.tunnel.lock().unwrap().status();
        status.elevating = true;
        return Ok(status);
    }

    Ok(state.tunnel.lock().unwrap().start(room, hostname))
}

#[tauri::command]
fn disconnect(state: State<'_, AppState>) -> Status {
    state.tunnel.lock().unwrap().stop()
}

#[tauri::command]
fn read_logs(
    state: State<'_, AppState>,
    level: Option<String>,
    limit: Option<usize>,
) -> Vec<LogLine> {
    state
        .tunnel
        .lock()
        .unwrap()
        .read_logs(level.as_deref().unwrap_or("all"), limit.unwrap_or(400))
}

#[tauri::command]
fn clear_logs(state: State<'_, AppState>) -> Result<(), String> {
    state.tunnel.lock().unwrap().clear_logs()
}

/// 解析服务 Token 是编译进客户端的内置常量，所有远端请求统一携带。
fn resolver_token() -> Option<String> {
    Some(settings::RESOLVER_TOKEN.to_string())
}

fn default_hostname() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("lanlink-{:04x}", nanos % 0xffff)
}

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

    // 单实例：只允许一个界面进程（提权后的管理员实例会接着持有这把锁）
    if let Err(err) = single::acquire() {
        eprintln!("{err}，本次启动直接退出");
        return;
    }

    let elevated_run = args.iter().any(|arg| arg == "--elevated-run");
    let connect_on_launch = args.iter().any(|arg| arg == "--connect");
    let hostname = args
        .windows(2)
        .find(|pair| pair[0] == "--hostname")
        .map(|pair| pair[1].clone());
    let room_code = args
        .windows(2)
        .find(|pair| pair[0] == "--room")
        .map(|pair| pair[1].clone());

    tauri::Builder::default()
        .manage(AppState {
            tunnel: Mutex::new(Tunnel::new()),
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            get_status,
            get_settings,
            save_settings,
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
        .setup(move |app| {
            // 提权实例由系统授权进程拉起，窗口可能落在别的窗口后面（被遮挡时 WebView 不绘制），
            // 这里主动激活一次，保证交接后用户立刻看到界面
            if elevated_run {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_focus();
                }
            }

            if !elevated_run && !connect_on_launch {
                return Ok(());
            }

            let handle = app.handle().clone();
            let hostname = hostname.clone();
            let room_code = room_code.clone();
            // 等窗口创建完成后再连，便于前端立即看到「连接中」状态
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(600));
                let settings = Settings::load();
                let Some(code) = room_code else {
                    eprintln!("缺少 --room 参数，跳过自动连接");
                    return;
                };
                match rooms::lookup(
                    &code,
                    settings.relays,
                    settings.resolver_base_url,
                    resolver_token(),
                ) {
                    Ok(room) => {
                        let state = handle.state::<AppState>();
                        if let Err(err) = request_connect(&state, room, hostname) {
                            eprintln!("连接失败：{err}");
                        }
                    }
                    Err(err) => eprintln!("房间解析失败：{err}"),
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("LanLink 启动失败");
}
