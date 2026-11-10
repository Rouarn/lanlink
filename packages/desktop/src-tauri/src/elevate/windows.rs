//! 基于 `ShellExecuteExW` + `runas` 动词的提权实现。
//!
//! 与 macOS 分支保持同样的语义：弹出系统授权（UAC）后拉起新的管理员实例，
//! 拉起后立即返回，由父进程轮询 pid 文件、确认新实例存活后退出。
//! 用户在 UAC 对话框点「否」时 `ShellExecuteExW` 返回 `ERROR_CANCELLED`，
//! 这里转成明确的错误文案回传给界面。

use std::ffi::{c_void, OsStr};
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED, HANDLE};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// 当前进程是否已经拿到管理员令牌。
pub fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation: TOKEN_ELEVATION = zeroed();
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut TOKEN_ELEVATION as *mut c_void,
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

/// 以管理员身份重新拉起当前可执行文件；UAC 被取消时返回可读错误。
pub fn relaunch_as_root(extra_args: &[String]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("无法定位可执行文件：{e}"))?;
    let exe_w = to_wide(exe.as_os_str());

    let params = extra_args
        .iter()
        .map(|arg| quote_arg(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let params_w = to_wide(OsStr::new(&params));
    let verb_w = to_wide(OsStr::new("runas"));

    let com_ready = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) } >= 0;

    let result = unsafe {
        let mut info: SHELLEXECUTEINFOW = zeroed();
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb_w.as_ptr();
        info.lpFile = exe_w.as_ptr();
        info.lpParameters = params_w.as_ptr();
        info.nShow = SW_SHOWNORMAL;

        if ShellExecuteExW(&mut info) != 0 {
            // 不等待新实例退出：与 macOS 分支一致，父进程靠 pid 文件判断交接完成
            if !info.hProcess.is_null() {
                CloseHandle(info.hProcess);
            }
            Ok(())
        } else {
            let err = GetLastError();
            if err == ERROR_CANCELLED {
                Err("你取消了管理员授权，本次未连接".to_string())
            } else {
                Err(format!("无法以管理员身份启动：错误码 {err}"))
            }
        }
    };

    if com_ready {
        unsafe { CoUninitialize() };
    }
    result
}

fn to_wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

/// 按 `CommandLineToArgvW` 的规则给单个参数加引号，避免路径里有空格时被拆开。
fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }

    let mut out = String::from("\"");
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => {
                backslashes += 1;
                out.push('\\');
            }
            '"' => {
                // 引号前的反斜杠要成对翻倍，再转义引号本身
                for _ in 0..=backslashes {
                    out.push('\\');
                }
                out.push('"');
                backslashes = 0;
            }
            _ => {
                backslashes = 0;
                out.push(ch);
            }
        }
    }
    // 结尾的反斜杠会被当作转义符，同样需要翻倍
    for _ in 0..backslashes {
        out.push('\\');
    }
    out.push('"');
    out
}
