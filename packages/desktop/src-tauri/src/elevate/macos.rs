//! 基于 `AuthorizationExecuteWithPrivileges` 的提权实现。
//!
//! 思路与 EasyTier 官方 GUI 的 `elevate/macos.rs` 一致（该文件为 MIT，
//! 版权归 Luis Liu，参考自 sudo-prompt by Joran Dirk Greef），
//! 区别是本实现不等待子进程退出，拉起后立即返回，父进程自行退出。

use std::ffi::CString;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::FromRawFd;
use std::ptr;

use libc::{c_char, SHUT_WR};
use security_framework_sys::authorization::{
    errAuthorizationSuccess, kAuthorizationFlagDefaults, kAuthorizationFlagDestroyRights,
    AuthorizationCreate, AuthorizationExecuteWithPrivileges, AuthorizationFree, AuthorizationRef,
};

/// 启动阶段还没有任何窗口，授权被取消 / 失败时用系统弹窗把原因说清楚，再退出。
pub fn show_fatal_alert(message: &str) {
    let text = message.replace('\\', "\\\\").replace('"', "\\\"").replace(['\n', '\r'], " ");
    let script = format!("display alert \"LanLink 无法启动\" message \"{text}\" as critical");
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status();
}

pub fn relaunch_as_root(extra_args: &[String]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("无法定位可执行文件：{e}"))?;
    let exe = CString::new(exe.as_os_str().as_bytes())
        .map_err(|_| "可执行文件路径包含非法字符".to_string())?;

    let args: Vec<CString> = extra_args
        .iter()
        .map(|arg| CString::new(arg.as_bytes()).map_err(|_| "启动参数包含非法字符".to_string()))
        .collect::<Result<_, _>>()?;
    let mut argv: Vec<*const c_char> = args.iter().map(|arg| arg.as_ptr()).collect();
    argv.push(ptr::null());

    unsafe {
        let mut auth_ref: AuthorizationRef = ptr::null_mut();
        if AuthorizationCreate(
            ptr::null(),
            ptr::null(),
            kAuthorizationFlagDefaults,
            &mut auth_ref,
        ) != errAuthorizationSuccess
        {
            return Err("创建授权对象失败".to_string());
        }

        let mut pipe: *mut libc::FILE = ptr::null_mut();
        let rc = AuthorizationExecuteWithPrivileges(
            auth_ref,
            exe.as_ptr(),
            kAuthorizationFlagDefaults,
            argv.as_ptr() as *const *mut c_char,
            &mut pipe,
        );
        if rc != errAuthorizationSuccess {
            AuthorizationFree(auth_ref, kAuthorizationFlagDestroyRights);
            return Err(format!("授权未通过（错误码 {rc}）"));
        }

        if !pipe.is_null() {
            let fd = libc::fileno(pipe);
            if fd >= 0 {
                // 关闭写端，让子进程的 stdin 立即收到 EOF
                libc::shutdown(fd, SHUT_WR);

                // 后台排空子进程 stdout，避免管道写满后其输出被阻塞
                let read_fd = libc::dup(fd);
                if read_fd >= 0 {
                    std::thread::spawn(move || {
                        let mut file = std::fs::File::from_raw_fd(read_fd);
                        let mut buf = [0u8; 4096];
                        while matches!(file.read(&mut buf), Ok(n) if n > 0) {}
                    });
                }
            }
            libc::fclose(pipe);
        }

        AuthorizationFree(auth_ref, kAuthorizationFlagDestroyRights);
    }

    Ok(())
}
