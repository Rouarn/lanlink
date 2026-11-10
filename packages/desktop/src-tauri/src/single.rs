//! 单实例守卫，以及提权时的「进程交接」。
//!
//! 普通权限实例点击连接后要换成管理员实例：先用 `release` 把锁交出去，
//! 管理员实例启动时 `acquire` 拿到锁并写下自己的 pid；原进程轮询 pid 文件，
//! 看到别的存活进程后立即退出，保证任何时刻只有一个界面进程、一个 Dock 图标。
//!
//! 锁文件放在 /tmp（sticky，普通用户与 root 都能拿到同一个文件），
//! 不用 HOME，因为提权后的进程 HOME 可能不同。

use std::fs;
use std::io::Read;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static LOCK: Mutex<Option<fs::File>> = Mutex::new(None);

fn lock_path() -> PathBuf {
    PathBuf::from("/tmp/lanlink-instance.lock")
}

fn pid_path() -> PathBuf {
    PathBuf::from("/tmp/lanlink-instance.pid")
}

/// 拿到实例锁；已被别的进程持有时返回错误。
pub fn acquire() -> Result<(), String> {
    let path = lock_path();
    let file = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
    {
        Ok(file) => file,
        // 文件由 root 实例创建时普通用户只能读，flock 在只读 fd 上同样有效
        Err(_) => fs::OpenOptions::new()
            .read(true)
            .open(&path)
            .map_err(|err| format!("无法打开单实例锁 {}：{err}", path.display()))?,
    };
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        return Err("已经有一个 LanLink 在运行".to_string());
    }
    *LOCK.lock().unwrap() = Some(file);
    let _ = fs::write(pid_path(), std::process::id().to_string());
    Ok(())
}

/// 交出实例锁（提权前调用，否则管理员实例会把自己当成第二个实例而退出）。
pub fn release() {
    if let Some(file) = LOCK.lock().unwrap().take() {
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        drop(file);
    }
}

/// 等管理员实例接管：pid 文件里出现另一个存活进程时返回它的 pid。
///
/// `canceled` 为真（授权被取消）时立刻放弃等待。
pub fn wait_for_successor(timeout: Duration, canceled: &std::sync::atomic::AtomicBool) -> Option<u32> {
    use std::sync::atomic::Ordering;
    let mine = std::process::id();
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if canceled.load(Ordering::SeqCst) {
            return None;
        }
        if let Ok(mut file) = fs::File::open(pid_path()) {
            let mut buf = String::new();
            if file.read_to_string(&mut buf).is_ok() {
                if let Ok(pid) = buf.trim().parse::<u32>() {
                    if pid != mine && is_alive(pid) {
                        return Some(pid);
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(120));
    }
    None
}

fn is_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}
