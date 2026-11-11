//! LanLink Android 端使用的 EasyTier 动态库。
//!
//! 只导出 C ABI，供 `liblanlink_lib.so` 在运行期用 `libloading` 加载。
//! 单独成 `.so` 的目的：满足 LGPL-3.0 的可替换要求 —— 替换本文件即可换库，
//! 不需要重新编译 LanLink 自身。
//!
//! 约定：
//! - 所有 `*_buf` 参数由调用方分配，函数写入 UTF-8 并以 NUL 结尾；
//! - 返回写入的字节数（不含 NUL），缓冲区不足返回 `-1`；
//! - 失败原因用 `lanlink_et_last_error` 取（线程局部，调用后立即取）。

use std::ffi::{c_char, c_int, CStr, CString};
use std::sync::{Arc, Mutex};
use std::sync::OnceLock;

use once_cell::sync::Lazy;
use tokio::runtime::{Builder, Runtime};
use uuid::Uuid;

use easytier::common::config::{ConfigFileControl, TomlConfigLoader};
use easytier::instance::factory::{
    native_instance_manager_with_runtime, native_process_management, NativeInstanceManager,
    NativeProcessManagement,
};
use easytier_core::management::InstanceMutationHooks;

thread_local! {
    static LAST_ERROR: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn set_error(msg: impl Into<String>) {
    let msg = msg.into();
    log::error!("{msg}");
    LAST_ERROR.with(|cell| *cell.borrow_mut() = msg);
}

fn clear_error() {
    LAST_ERROR.with(|cell| cell.borrow_mut().clear());
}

fn take_error() -> String {
    LAST_ERROR.with(|cell| cell.borrow().clone())
}

/// 最近一次 `lanlink_et_attach_tun` / `lanlink_et_config` 返回的移动端网卡配置。
static TUN_CONFIG: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));

struct Hooks;

#[async_trait::async_trait]
impl InstanceMutationHooks for Hooks {
    async fn post_remove_network_instances(
        &self,
        _instance_ids: &[Uuid],
    ) -> Result<(), String> {
        Ok(())
    }
}

struct Context {
    runtime: Runtime,
    manager: Arc<NativeInstanceManager>,
    process_management: NativeProcessManagement,
}

static CONTEXT: OnceLock<Context> = OnceLock::new();

fn context() -> Result<&'static Context, String> {
    if let Some(ctx) = CONTEXT.get() {
        return Ok(ctx);
    }
    let runtime = Builder::new_multi_thread()
        .enable_all()
        .thread_name("easytier")
        .build()
        .map_err(|err| format!("创建 EasyTier 运行时失败：{err}"))?;
    let manager = Arc::new(native_instance_manager_with_runtime(runtime.handle().clone()));
    let process_management = native_process_management(manager.clone(), Arc::new(Hooks));
    let ctx = Context {
        runtime,
        manager,
        process_management,
    };
    let _ = CONTEXT.set(ctx);
    CONTEXT.get().ok_or_else(|| "EasyTier 初始化失败".to_string())
}

/// 把一段字符串写进调用方缓冲区，返回写入长度。
unsafe fn write_buf(buf: *mut c_char, len: c_int, text: &str) -> c_int {
    if buf.is_null() || len <= 0 {
        return -1;
    }
    let bytes = text.as_bytes();
    let capacity = len as usize;
    if bytes.len() + 1 > capacity {
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, bytes.len());
        *buf.add(bytes.len()) = 0;
    }
    bytes.len() as c_int
}

unsafe fn c_str(ptr: *const c_char) -> Result<String, String> {
    if ptr.is_null() {
        return Err("参数为空".to_string());
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map(|s| s.to_owned())
        .map_err(|err| format!("参数不是合法 UTF-8：{err}"))
}

fn resolve_instance(name: &str) -> Result<Uuid, String> {
    let ctx = context()?;
    easytier_core::management::resolve_optional_instance_by_name(ctx.manager.as_ref(), name)
        .map_err(|err| err.to_string())?
        .map(|instance| instance.instance_id())
        .ok_or_else(|| format!("找不到实例 {name}"))
}

fn mobile_tun_config_json() -> Result<String, String> {
    let ctx = context()?;
    #[cfg(target_os = "android")]
    {
        let ids: Vec<Uuid> = ctx
            .manager
            .instance_ids()
            .into_iter()
            .filter(|id| ctx.manager.instance(*id).is_some())
            .collect();
        let config = ctx.runtime.block_on(easytier::instance::factory::get_mobile_tun_config(
            ctx.manager.as_ref(),
            &ids,
        ));
        return serde_json::to_string(&config).map_err(|err| err.to_string());
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = ctx;
        Err("移动端网卡配置仅在 Android 上可用".to_string())
    }
}

/// 启动一个网络实例，`toml` 为 EasyTier TOML 配置文本。
#[no_mangle]
pub unsafe extern "C" fn lanlink_et_start(toml: *const c_char) -> c_int {
    clear_error();
    let text = match unsafe { c_str(toml) } {
        Ok(value) => value,
        Err(err) => {
            set_error(err);
            return -1;
        }
    };
    let cfg = match TomlConfigLoader::new_from_str(&text) {
        Ok(cfg) => cfg,
        Err(err) => {
            set_error(format!("配置解析失败：{err}"));
            return -1;
        }
    };
    let ctx = match context() {
        Ok(ctx) => ctx,
        Err(err) => {
            set_error(err);
            return -1;
        }
    };
    match ctx.runtime.block_on(
        ctx.process_management
            .run_owned_network_instance(cfg, ConfigFileControl::STATIC_CONFIG),
    ) {
        Ok(_instance_id) => 0,
        Err(err) => {
            set_error(format!("启动实例失败：{err:#}"));
            -1
        }
    }
}

/// 停止全部由本库启动的实例。
#[no_mangle]
pub unsafe extern "C" fn lanlink_et_stop() -> c_int {
    clear_error();
    let ctx = match context() {
        Ok(ctx) => ctx,
        Err(err) => {
            set_error(err);
            return -1;
        }
    };
    match ctx
        .runtime
        .block_on(ctx.process_management.retain_owned_network_instances_by_name(Vec::new()))
    {
        Ok(_result) => 0,
        Err(err) => {
            set_error(format!("停止实例失败：{err:#}"));
            -1
        }
    }
}

/// 把 Android VpnService 建好的 tun fd 交给实例；`fd <= 0` 表示解绑。
///
/// 成功时把网卡配置（地址/路由/DNS）存起来，随后用 `lanlink_et_tun_config` 取。
#[no_mangle]
pub unsafe extern "C" fn lanlink_et_attach_tun(name: *const c_char, fd: c_int) -> c_int {
    clear_error();
    let name = match unsafe { c_str(name) } {
        Ok(value) => value,
        Err(err) => {
            set_error(err);
            return -1;
        }
    };
    let instance_id = match resolve_instance(&name) {
        Ok(id) => id,
        Err(err) => {
            set_error(err);
            return -1;
        }
    };
    let ctx = match context() {
        Ok(ctx) => ctx,
        Err(err) => {
            set_error(err);
            return -1;
        }
    };

    #[cfg(target_os = "android")]
    {
        let result = ctx.runtime.block_on(
            easytier::instance::factory::attach_mobile_tun_fd(
                ctx.manager.as_ref(),
                fd,
                Some(&[instance_id]),
            ),
        );
        match result {
            Ok(config) => {
                let json = serde_json::to_string(&config).unwrap_or_else(|_| "{}".to_string());
                log::info!("attach tun fd={fd} config={json}");
                *TUN_CONFIG.lock().unwrap() = json;
                0
            }
            Err(err) => {
                set_error(format!("附加虚拟网卡失败：{err:#}"));
                -1
            }
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = instance_id;
        let _ = fd;
        let _ = ctx;
        set_error("附加虚拟网卡仅在 Android 上可用");
        -1
    }
}

/// 取最近一次附加网卡时下发的配置（JSON），未附加时返回 `{}`。
#[no_mangle]
pub unsafe extern "C" fn lanlink_et_tun_config(buf: *mut c_char, len: c_int) -> c_int {
    clear_error();
    let json = TUN_CONFIG.lock().unwrap().clone();
    let json = if json.is_empty() { "{}".to_string() } else { json };
    unsafe { write_buf(buf, len, &json) }
}

/// 查询运行中实例的网卡配置（还没建卡时用它拿地址，再建 VpnService）。
#[no_mangle]
pub unsafe extern "C" fn lanlink_et_config(buf: *mut c_char, len: c_int) -> c_int {
    clear_error();
    match mobile_tun_config_json() {
        Ok(json) => unsafe { write_buf(buf, len, &json) },
        Err(err) => {
            set_error(err);
            -1
        }
    }
}

/// 查询运行中实例的信息（JSON 对象：实例名 → 状态）。
#[no_mangle]
pub unsafe extern "C" fn lanlink_et_collect(buf: *mut c_char, len: c_int) -> c_int {
    clear_error();
    let ctx = match context() {
        Ok(ctx) => ctx,
        Err(err) => {
            set_error(err);
            return -1;
        }
    };
    let infos = match ctx.manager.collect_network_infos_sync() {
        Ok(infos) => infos,
        Err(err) => {
            set_error(format!("读取实例状态失败：{err:#}"));
            return -1;
        }
    };
    let mut map = serde_json::Map::new();
    for (id, value) in infos.iter() {
        let Some(instance) = ctx.manager.instance(*id) else {
            continue;
        };
        map.insert(
            instance.instance_name().to_owned(),
            serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
        );
    }
    let json = serde_json::Value::Object(map).to_string();
    unsafe { write_buf(buf, len, &json) }
}

/// 查询对端列表（JSON 数组），字段与桌面端 `PeerInfo` 对齐：
/// `[{hostname, ipv4, cost, proto, lat_ms, loss_rate}]`。
///
/// 安卓没有 `easytier-cli` 可执行文件，这里直接从进程内实例快照取，
/// 口径参照 `easytier-cli peer`（cost/延迟/协议去重规则一致）。
#[no_mangle]
pub unsafe extern "C" fn lanlink_et_peers(buf: *mut c_char, len: c_int) -> c_int {
    clear_error();
    let ctx = match context() {
        Ok(ctx) => ctx,
        Err(err) => {
            set_error(err);
            return -1;
        }
    };
    let infos = match ctx.manager.collect_network_infos_sync() {
        Ok(infos) => infos,
        Err(err) => {
            set_error(format!("读取对端列表失败：{err:#}"));
            return -1;
        }
    };

    let mut peers = Vec::new();
    for value in infos.values() {
        for pair in value.peer_route_pairs.iter() {
            let route = pair.route.clone().unwrap_or_default();
            let ipv4 = route
                .ipv4_addr
                .clone()
                .and_then(|ip| ip.address)
                .map(|ip| ip.to_string())
                .unwrap_or_default();
            // 没有虚拟 IP 的路由（例如只做中继的节点）不进对端列表。
            if ipv4.is_empty() {
                continue;
            }
            // 与 CLI 一致：直连看连接 RTT，经中继看路径延迟。
            let lat_ms = if route.cost == 1 {
                pair.get_latency_ms().unwrap_or(0.0)
            } else {
                route.path_latency_latency_first() as f64
            };
            let hostname = if route.hostname.is_empty() {
                "未知节点".to_string()
            } else {
                route.hostname.clone()
            };
            peers.push(serde_json::json!({
                "hostname": hostname,
                "ipv4": ipv4,
                "cost": easytier::utils::string::cost_to_str(route.cost),
                "proto": pair.get_conn_protos().unwrap_or_default().join(","),
                "lat_ms": lat_ms,
                "loss_rate": pair.get_loss_rate().unwrap_or(0.0),
            }));
        }
    }

    #[cfg(target_os = "android")]
    log::info!("lanlink_et_peers: {} 个对端", peers.len());
    let json = serde_json::Value::Array(peers).to_string();
    unsafe { write_buf(buf, len, &json) }
}

/// 取当前线程最后一次失败原因。
#[no_mangle]
pub unsafe extern "C" fn lanlink_et_last_error(buf: *mut c_char, len: c_int) -> c_int {
    let msg = take_error();
    unsafe { write_buf(buf, len, &msg) }
}

/// 释放本库分配的字符串（当前实现不向外分配，保留给未来版本）。
#[no_mangle]
pub unsafe extern "C" fn lanlink_et_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn lanlink_et_init_log() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("easytier"),
        );
    });
}

#[cfg(not(target_os = "android"))]
#[no_mangle]
pub extern "C" fn lanlink_et_init_log() {}
