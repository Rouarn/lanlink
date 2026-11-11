const COMMANDS: &[&str] = &[
    "prepare_vpn",
    "start_vpn",
    "stop_vpn",
    "get_vpn_status",
    "take_tun_fd",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).android_path("android").build();
}
