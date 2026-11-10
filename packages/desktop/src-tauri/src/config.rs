//! 运行期固定参数。
//!
//! 这里不放任何用户的服务器地址：中继、网络名、密钥全部来自设置或房间码。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const RPC_PORTAL: &str = "127.0.0.1:15888";

/// 提权实例沿用发起者的数据目录与文件属主，避免 root 把用户数据写到 /var/root。
static OVERRIDE: Mutex<Option<Override>> = Mutex::new(None);

struct Override {
    dir: PathBuf,
    owner: Option<(u32, u32)>,
}

/// 由 `--data-dir` / `--owner` 参数设置（只在提权实例里调用）。
pub fn set_override(dir: PathBuf, owner: Option<(u32, u32)>) {
    *OVERRIDE.lock().unwrap() = Some(Override { dir, owner });
}

/// 用户数据目录（settings.json / rooms.json 所在）。
pub fn data_dir() -> PathBuf {
    if let Some(current) = OVERRIDE.lock().unwrap().as_ref() {
        return current.dir.clone();
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("com.xiaji.lanlink");
        }
    }
    std::env::temp_dir().join("lanlink")
}

/// root 实例写完用户数据后，把文件交还给原用户，否则用户实例之后无法再写。
pub fn hand_back(path: &Path) {
    #[cfg(unix)]
    {
        let owner = OVERRIDE.lock().unwrap().as_ref().and_then(|item| item.owner);
        let Some((uid, gid)) = owner else { return };
        if unsafe { libc::geteuid() } != 0 {
            return;
        }
        use std::os::unix::ffi::OsStrExt;
        let Ok(raw) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return;
        };
        unsafe {
            libc::chown(raw.as_ptr(), uid, gid);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}
