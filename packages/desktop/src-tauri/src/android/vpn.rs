//! 与系统 VpnService 的桥接。
//!
//! 流程（与官方 App 一致，但由 Rust 侧驱动，不依赖前端事件）：
//! 1. `prepare_vpn` 触发系统授权弹窗（首次连接）；
//! 2. `start_vpn` 启动前台服务，Kotlin 侧 `Builder.establish()` 建卡；
//! 3. Rust 轮询 `take_tun_fd` 取走 fd；
//! 4. 调用方把 fd 交给 `libeasytier.so`，并把地址/路由写回状态。

use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_vpnservice::models::{StartVpnRequest, VoidRequest};
use tauri_plugin_vpnservice::VpnserviceExt;

/// 取 fd 的轮询上限：建卡本身是同步的，留 5 秒兜底。
const FD_TIMEOUT: Duration = Duration::from_secs(5);
const FD_POLL: Duration = Duration::from_millis(100);

fn not_granted() -> String {
    "你拒绝了 VPN 授权，无法创建虚拟网卡。请重新连接并在系统弹窗中允许。".to_string()
}

/// 授权并建卡，返回 tun fd。
pub fn start(
    app: &AppHandle,
    addresses: &[String],
    routes: &[String],
    dns: Option<String>,
) -> Result<i32, String> {
    let vpn = app.vpnservice();

    let prepared = vpn
        .prepare_vpn(VoidRequest {})
        .map_err(|err| format!("请求 VPN 授权失败：{err}"))?;
    if prepared.granted == Some(false) {
        return Err(not_granted());
    }

    let started = vpn
        .start_vpn(StartVpnRequest {
            ipv4_addr: addresses.first().cloned(),
            ipv4_addrs: Some(addresses.to_vec()),
            routes: Some(routes.to_vec()),
            // dns 允许为空：Kotlin 侧用 `dns?.let` 判断，不会写进 Builder。
            dns,
            // 必须是空数组而不是 null：Kotlin 的 @InvokeArg 反射会把 JSON null
            // 写进非空字段，后续 getClass() 直接 NPE。
            disallowed_applications: Some(Vec::new()),
            mtu: Some(1500),
        })
        .map_err(|err| format!("启动虚拟网卡失败：{err}"))?;
    if let Some(err) = started.error_msg {
        if err == "need_prepare" {
            return Err(not_granted());
        }
        return Err(format!("启动虚拟网卡失败：{err}"));
    }

    let deadline = std::time::Instant::now() + FD_TIMEOUT;
    loop {
        let response = vpn
            .take_tun_fd(VoidRequest {})
            .map_err(|err| format!("读取虚拟网卡 fd 失败：{err}"))?;
        if response.fd > 0 {
            return Ok(response.fd);
        }
        if std::time::Instant::now() >= deadline {
            let _ = vpn.stop_vpn(VoidRequest {});
            return Err("系统没有在预期时间内建立虚拟网卡，请重试".to_string());
        }
        std::thread::sleep(FD_POLL);
    }
}

/// 关闭系统虚拟网卡（失败不影响主流程）。
pub fn stop(app: &AppHandle) {
    let _ = app.vpnservice().stop_vpn(VoidRequest {});
}
