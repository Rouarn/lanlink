//! 管理员权限相关的平台适配。
//!
//! P0 采用「整个应用以管理员重开」的策略（与 EasyTier 官方 GUI 一致）：
//! 普通权限下点击连接时，以管理员身份重新拉起自身，
//! 然后当前实例退出，由管理员实例接管并自动连接。
//! macOS 用 `AuthorizationExecuteWithPrivileges`，Windows 用 `ShellExecuteExW` + `runas`。

pub fn is_elevated() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(windows)]
    {
        windows::is_elevated()
    }
}

#[cfg(target_os = "macos")]
mod macos;

#[cfg(windows)]
mod windows;

/// 以管理员权限重新拉起当前可执行文件。
/// `extra_args` 会原样传给新实例（用于标记自动连接、主机名等）。
#[cfg(target_os = "macos")]
pub fn relaunch_as_root(extra_args: &[String]) -> Result<(), String> {
    macos::relaunch_as_root(extra_args)
}

/// 启动阶段授权失败时的系统弹窗（此时还没有主窗口）。
#[cfg(target_os = "macos")]
pub fn show_fatal_alert(message: &str) {
    macos::show_fatal_alert(message)
}

#[cfg(windows)]
pub fn relaunch_as_root(extra_args: &[String]) -> Result<(), String> {
    windows::relaunch_as_root(extra_args)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn relaunch_as_root(_extra_args: &[String]) -> Result<(), String> {
    Err("当前平台暂未实现管理员提权".to_string())
}
