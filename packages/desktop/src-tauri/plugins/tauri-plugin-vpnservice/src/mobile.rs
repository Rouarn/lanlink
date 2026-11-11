use serde::de::DeserializeOwned;
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::models::*;

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "com.plugin.vpnservice";

// initializes the Kotlin plugin class
pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> crate::Result<Vpnservice<R>> {
    #[cfg(target_os = "android")]
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "VpnServicePlugin")?;
    Ok(Vpnservice(handle))
}

/// Access to the vpnservice APIs.
///
/// 注意命令名：`PluginHandle.invoke` 用 **Kotlin 方法名** 查表
/// （`commands[method.name]`），所以这里必须写 camelCase 的 Kotlin 方法名，
/// 写成 snake_case 会报 `No command xxx found for plugin ...`。
pub struct Vpnservice<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Vpnservice<R> {
    pub fn prepare_vpn(&self, payload: VoidRequest) -> crate::Result<Status> {
        self.0
            .run_mobile_plugin("prepareVpn", payload)
            .map_err(Into::into)
    }

    pub fn start_vpn(&self, payload: StartVpnRequest) -> crate::Result<Status> {
        self.0
            .run_mobile_plugin("startVpn", payload)
            .map_err(Into::into)
    }

    pub fn stop_vpn(&self, payload: VoidRequest) -> crate::Result<Status> {
        self.0
            .run_mobile_plugin("stopVpn", payload)
            .map_err(Into::into)
    }

    pub fn get_vpn_status(&self, payload: VoidRequest) -> crate::Result<VpnStatus> {
        self.0
            .run_mobile_plugin("getVpnStatus", payload)
            .map_err(Into::into)
    }

    /// 取走 VpnService 建好的 tun fd；返回 -1 表示还没建好。
    pub fn take_tun_fd(&self, payload: VoidRequest) -> crate::Result<TakeTunFdResponse> {
        self.0
            .run_mobile_plugin("takeTunFd", payload)
            .map_err(Into::into)
    }
}
