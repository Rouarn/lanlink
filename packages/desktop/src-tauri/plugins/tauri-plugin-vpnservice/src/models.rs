#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoidRequest {}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartVpnRequest {
    pub ipv4_addr: Option<String>,
    pub ipv4_addrs: Option<Vec<String>>,
    pub routes: Option<Vec<String>>,
    pub dns: Option<String>,
    pub disallowed_applications: Option<Vec<String>>,
    pub mtu: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub error_msg: Option<String>,
    /// `prepare_vpn` 的结果：用户是否授予了 VPN 权限。
    pub granted: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnStatus {
    pub running: bool,
    pub ipv4_addr: Option<String>,
    pub ipv4_addrs: Option<Vec<String>>,
    pub routes: Option<Vec<String>>,
    pub dns: Option<String>,
}

/// `take_tun_fd`：取走系统建卡后拿到的 tun fd（取一次即清空，-1 表示还没有）。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeTunFdResponse {
    pub fd: i32,
}
