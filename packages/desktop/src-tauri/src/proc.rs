//! 启动外部子进程的统一入口。
//!
//! Windows 上从 GUI 进程 spawn 控制台程序（easytier-core、easytier-cli、schtasks、
//! tasklist）会弹出黑色控制台窗口；连接后前端每 1.5s 轮询状态，
//! 每次都 spawn 两次 easytier-cli，于是窗口不停闪。
//! 所有外部命令都从这里创建，由这里统一加 `CREATE_NO_WINDOW`，
//! 避免新增调用点时漏掉——也不依赖 `windows_subsystem` 或打包隐藏来兜底。
//! 标准输出/错误的去向仍由调用方决定（core 仍重定向到日志文件）。

use std::ffi::OsStr;
use std::process::Command;

/// Windows：不为子进程创建控制台窗口。
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Windows：让子进程以「低于正常」的优先级运行。
///
/// easytier-cli 每次查询都要新起一个进程，Windows 上进程创建本身很贵；
/// 降一档优先级可以避免它和前台界面抢 CPU（界面掉帧、切换页面发闷）。
#[cfg(windows)]
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// 构造子进程命令；Windows 下保证不弹出控制台窗口，并以低优先级运行。
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = Command::new(program);
        cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
        cmd
    }
    #[cfg(not(windows))]
    {
        Command::new(program)
    }
}
