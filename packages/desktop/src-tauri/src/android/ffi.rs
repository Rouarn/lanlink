//! 对 `libeasytier.so` 的动态加载封装。
//!
//! 该库单独打包进 APK 的 `lib/arm64-v8a/`，运行期用 `libloading` 按文件名加载：
//! 不静态链进 `liblanlink_lib.so`，以满足 LGPL-3.0 的可替换要求。

use std::ffi::{c_char, c_int, CStr, CString};
use std::sync::OnceLock;

use libloading::Library;

type StartFn = unsafe extern "C" fn(*const c_char) -> c_int;
type StopFn = unsafe extern "C" fn() -> c_int;
type AttachFn = unsafe extern "C" fn(*const c_char, c_int) -> c_int;
type BufFn = unsafe extern "C" fn(*mut c_char, c_int) -> c_int;

struct Api {
    /// 必须持有：库被卸载后函数指针立刻失效。
    _lib: Library,
    start: StartFn,
    stop: StopFn,
    attach_tun: AttachFn,
    tun_config: BufFn,
    config: BufFn,
    peers: BufFn,
    last_error: BufFn,
}

unsafe impl Send for Api {}
unsafe impl Sync for Api {}

static API: OnceLock<Result<Api, String>> = OnceLock::new();

fn load() -> Result<Api, String> {
    unsafe {
        let lib = Library::new("libeasytier.so")
            .map_err(|err| format!("加载 libeasytier.so 失败：{err}"))?;
        // 让库内的日志走 logcat，真机排查时不用再改库。
        if let Ok(init_log) = lib.get::<unsafe extern "C" fn()>(b"lanlink_et_init_log\0") {
            init_log();
        }
        let start = *lib
            .get::<StartFn>(b"lanlink_et_start\0")
            .map_err(|err| format!("libeasytier.so 缺少 lanlink_et_start：{err}"))?;
        let stop = *lib
            .get::<StopFn>(b"lanlink_et_stop\0")
            .map_err(|err| format!("libeasytier.so 缺少 lanlink_et_stop：{err}"))?;
        let attach_tun = *lib
            .get::<AttachFn>(b"lanlink_et_attach_tun\0")
            .map_err(|err| format!("libeasytier.so 缺少 lanlink_et_attach_tun：{err}"))?;
        let tun_config = *lib
            .get::<BufFn>(b"lanlink_et_tun_config\0")
            .map_err(|err| format!("libeasytier.so 缺少 lanlink_et_tun_config：{err}"))?;
        let config = *lib
            .get::<BufFn>(b"lanlink_et_config\0")
            .map_err(|err| format!("libeasytier.so 缺少 lanlink_et_config：{err}"))?;
        let peers = *lib
            .get::<BufFn>(b"lanlink_et_peers\0")
            .map_err(|err| format!("libeasytier.so 缺少 lanlink_et_peers：{err}"))?;
        let last_error = *lib
            .get::<BufFn>(b"lanlink_et_last_error\0")
            .map_err(|err| format!("libeasytier.so 缺少 lanlink_et_last_error：{err}"))?;
        Ok(Api {
            _lib: lib,
            start,
            stop,
            attach_tun,
            tun_config,
            config,
            peers,
            last_error,
        })
    }
}

fn api() -> Result<&'static Api, String> {
    match API.get_or_init(load) {
        Ok(api) => Ok(api),
        Err(err) => Err(err.clone()),
    }
}

/// 取本库记录的最后一次错误（FFI 返回失败时立即调用）。
fn last_error(api: &Api) -> String {
    let mut buf = vec![0u8; 512];
    let len = unsafe { (api.last_error)(buf.as_mut_ptr() as *mut c_char, buf.len() as c_int) };
    if len <= 0 {
        return "未知错误".to_string();
    }
    String::from_utf8_lossy(&buf[..len as usize]).into_owned()
}

fn read_buf(api: &Api, f: BufFn, capacity: usize) -> Result<String, String> {
    let mut buf = vec![0u8; capacity];
    let len = unsafe { f(buf.as_mut_ptr() as *mut c_char, buf.len() as c_int) };
    if len < 0 {
        return Err(last_error(api));
    }
    Ok(String::from_utf8_lossy(&buf[..len as usize]).into_owned())
}

fn parse_json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or(serde_json::Value::Null)
}

/// 启动实例（TOML 配置）。
pub fn start(toml: &str) -> Result<(), String> {
    let api = api()?;
    let text = CString::new(toml).map_err(|_| "配置里不能有 NUL 字符".to_string())?;
    let code = unsafe { (api.start)(text.as_ptr()) };
    if code == 0 {
        Ok(())
    } else {
        Err(last_error(api))
    }
}

/// 停止全部实例。
pub fn stop() {
    if let Ok(api) = api() {
        unsafe {
            (api.stop)();
        }
    }
}

/// 把 tun fd 交给实例（`fd <= 0` 表示解绑），返回网卡配置。
pub fn attach_tun(name: &str, fd: i32) -> Result<serde_json::Value, String> {
    let api = api()?;
    let name = CString::new(name).map_err(|_| "实例名非法".to_string())?;
    let code = unsafe { (api.attach_tun)(name.as_ptr(), fd) };
    if code == 0 {
        read_buf(api, api.tun_config, 8192).map(|text| parse_json(&text))
    } else {
        Err(last_error(api))
    }
}

/// 读取运行中实例的网卡配置（地址 / 路由 / DNS）。
pub fn config() -> Result<serde_json::Value, String> {
    let api = api()?;
    read_buf(api, api.config, 8192).map(|text| parse_json(&text))
}

/// 读取对端列表（`[{hostname, ipv4, cost, proto, lat_ms, loss_rate}]`）。
pub fn peers() -> Result<serde_json::Value, String> {
    let api = api()?;
    read_buf(api, api.peers, 64 * 1024).map(|text| parse_json(&text))
}

/// 从网卡配置 JSON 里取出地址、路由、DNS。
pub fn tun_endpoints(config: &serde_json::Value) -> (Vec<String>, Vec<String>, Option<String>) {
    let strings = |key: &str| -> Vec<String> {
        config
            .get(key)
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };
    let addresses = strings("addresses");
    let routes = strings("routes");
    let dns = config
        .get("dns")
        .and_then(|value| value.as_str())
        .map(|s| s.to_string());
    (addresses, routes, dns)
}

/// 取第一段 IPv4 地址（去掉掩码）。
pub fn first_ipv4(addresses: &[String]) -> Option<String> {
    addresses
        .first()
        .map(|addr| addr.split('/').next().unwrap_or(addr).to_string())
}

/// 保证 `CStr` 指针有效（供单元内调试使用）。
#[allow(dead_code)]
pub fn cstr_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr).to_str().ok().map(|s| s.to_owned()) }
}
