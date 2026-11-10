//! 运行期固定参数。
//!
//! 这里不放任何用户的服务器地址：中继、网络名、密钥全部来自设置或房间码。

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once};

pub const RPC_PORTAL: &str = "127.0.0.1:15888";

/// 应用数据目录名（与 bundle identifier 一致，便于用户找到与卸载）。
const APP_DIR: &str = "com.xiaji.lanlink";

/// 旧版本在 `%TEMP%` 下用的目录名。
#[cfg(windows)]
const LEGACY_DIR: &str = "lanlink";

/// 提权实例沿用发起者的数据目录与文件属主，避免 root 把用户数据写到 /var/root。
static OVERRIDE: Mutex<Option<Override>> = Mutex::new(None);

/// 旧目录迁移只做一次（`data_dir()` 会被高频调用，不能每次都扫描）。
static MIGRATED: Once = Once::new();

struct Override {
    dir: PathBuf,
    owner: Option<(u32, u32)>,
}

/// 由 `--data-dir` / `--owner` 参数设置（只在提权实例里调用）。
pub fn set_override(dir: PathBuf, owner: Option<(u32, u32)>) {
    *OVERRIDE.lock().unwrap() = Some(Override { dir, owner });
}

/// 用户数据目录（settings.json / rooms.json / logs 所在）。
pub fn data_dir() -> PathBuf {
    if let Some(current) = OVERRIDE.lock().unwrap().as_ref() {
        return current.dir.clone();
    }
    let dir = default_data_dir();
    MIGRATED.call_once(|| migrate_once(&dir));
    dir
}

fn default_data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join(APP_DIR);
        }
    }
    #[cfg(windows)]
    {
        // 旧版本把数据放在 %TEMP%\lanlink，升级后统一收到 %LOCALAPPDATA%：
        // 临时目录会被清理工具删掉，也可能被别的用户复用。
        if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
            return PathBuf::from(local).join(APP_DIR);
        }
    }
    std::env::temp_dir().join(LEGACY_DIR_OR_FALLBACK)
}

/// 拿不到平台标准目录时的兜底名（Windows 上同时就是旧目录名）。
const LEGACY_DIR_OR_FALLBACK: &str = "lanlink";

#[cfg(windows)]
fn migrate_once(dir: &Path) {
    let legacy = std::env::temp_dir().join(LEGACY_DIR);
    if let Err(err) = migrate_dir(&legacy, dir) {
        // 迁移失败不能影响启动：用户在旧目录里的数据仍可用，只是这次没搬过来
        eprintln!("[lanlink] 旧数据目录迁移失败（{err}），本次仍使用新目录启动");
        log_line_to(
            &dir.join("logs").join("core.log"),
            "WARN",
            "lanlink::config",
            &format!("从 {} 迁移用户数据失败：{err}", legacy.display()),
        );
    }
}

#[cfg(not(windows))]
fn migrate_once(_dir: &Path) {}

/// 把旧数据目录里的数据文件复制到新目录。
///
/// - 只补新目录里缺失的文件，已经存在的绝不覆盖（用户可能已经在新版本里改过设置）；
/// - 旧目录不存在视为无事可做；
/// - 不删除旧文件，失败时用户还能从旧目录找回数据。
#[cfg_attr(not(windows), allow(dead_code))]
pub fn migrate_dir(from: &Path, to: &Path) -> io::Result<()> {
    if !from.is_dir() {
        return Ok(());
    }
    let mut copied = false;
    for name in ["settings.json", "rooms.json"] {
        let source = from.join(name);
        let target = to.join(name);
        if source.is_file() && !target.exists() {
            fs::create_dir_all(to)?;
            fs::copy(&source, &target)?;
            copied = true;
        }
    }
    if copied {
        eprintln!(
            "[lanlink] 已把用户数据从 {} 迁移到 {}",
            from.display(),
            to.display()
        );
    }
    Ok(())
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

// ---------------------------------------------------------------------------
// 日志与文件写入
// ---------------------------------------------------------------------------

/// 日志目录：跟随用户数据目录，不再放 `/tmp`（会被清理，且多用户可读）。
pub fn log_dir() -> PathBuf {
    data_dir().join("logs")
}

/// core / 中继 / 设置共用的日志文件。
pub fn log_path() -> PathBuf {
    log_dir().join("core.log")
}

/// 往 `core.log` 追加一行本应用自己的日志（与 easytier 的格式一致）。
pub fn log_line(level: &str, module: &str, message: &str) {
    log_line_to(&log_path(), level, module, message);
}

/// 指定日志文件的写入口（迁移早期还不能调用 `data_dir()`，用它避免重入）。
pub fn log_line_to(path: &Path, level: &str, module: &str, message: &str) {
    if let Some(dir) = path.parent() {
        let _ = create_private_dir(dir);
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // 日志里可能有中继地址、虚拟 IP：只给当前用户读写
        options.mode(0o600);
    }
    let Ok(mut file) = options.open(path) else {
        return;
    };
    let _ = writeln!(file, "{} {} {module}: {message}", crate::lan::timestamp(), level);
}

/// 建目录并收紧到 0700（日志、数据文件都在里面）。
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(dir)?.permissions();
        if permissions.mode() & 0o777 != 0o700 {
            permissions.set_mode(0o700);
            fs::set_permissions(dir, permissions)?;
        }
    }
    Ok(())
}

/// `.tmp` + rename 原子写：进程被强杀也不会留下写了一半的 JSON。
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        create_private_dir(dir)?;
    }
    let mut raw = path.as_os_str().to_owned();
    raw.push(".tmp");
    let temp = PathBuf::from(raw);

    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    {
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        // 先落盘再 rename：断电时宁可旧文件完整，也不要新文件是半截
        file.sync_all()?;
    }
    fs::rename(&temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{migrate_dir, write_atomic};
    use std::fs;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lanlink-config-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 迁移只补缺失文件：已有设置不能被旧目录覆盖，旧目录不存在也不能报错。
    #[test]
    fn migrate_copies_only_missing_files() {
        let root = scratch("migrate");
        let legacy = root.join("legacy");
        let current = root.join("current");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("settings.json"), "旧设置").unwrap();
        fs::write(legacy.join("rooms.json"), "旧房间").unwrap();

        migrate_dir(&legacy, &current).expect("迁移应成功");
        assert_eq!(
            fs::read_to_string(current.join("settings.json")).unwrap(),
            "旧设置"
        );

        // 新目录里已有的文件必须保留
        fs::write(current.join("settings.json"), "新设置").unwrap();
        fs::write(legacy.join("settings.json"), "又一份旧设置").unwrap();
        migrate_dir(&legacy, &current).expect("重复迁移应成功");
        assert_eq!(
            fs::read_to_string(current.join("settings.json")).unwrap(),
            "新设置",
            "已有文件不能被旧目录覆盖"
        );

        // 旧目录不存在：无事可做，但不能报错（否则会拖垮启动）
        migrate_dir(&root.join("missing"), &current).expect("旧目录不存在应视为成功");

        let _ = fs::remove_dir_all(&root);
    }

    /// 原子写：写完不留 `.tmp`，重复写覆盖旧内容。
    #[test]
    fn atomic_write_replaces_file_without_leftovers() {
        let dir = scratch("atomic");
        let path = dir.join("settings.json");
        write_atomic(&path, b"first").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "first");
        assert!(!dir.join("settings.json.tmp").exists(), "不能留下临时文件");

        write_atomic(&path, b"second").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");

        let entries = fs::read_dir(&dir).unwrap().count();
        assert_eq!(entries, 1, "目录里只应剩下目标文件");

        let _ = fs::remove_dir_all(&dir);
    }
}
