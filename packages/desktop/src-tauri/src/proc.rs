//! 启动外部子进程的统一入口。
//!
//! Windows 上从 GUI 进程 spawn 控制台程序（easytier-core、easytier-cli、schtasks、
//! tasklist）会弹出黑色控制台窗口；连接后前端每 1.5s 轮询状态，
//! 每次都 spawn 两次 easytier-cli，于是窗口不停闪。
//! 所有外部命令都从这里创建，由这里统一加 `CREATE_NO_WINDOW`，
//! 避免新增调用点时漏掉——也不依赖 `windows_subsystem` 或打包隐藏来兜底。
//! 标准输出/错误的去向仍由调用方决定（core 仍重定向到日志文件）。

use std::ffi::OsStr;
use std::io;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// `easytier-cli` 单次调用的上限。
///
/// Windows 上进程创建 + Defender 扫描可能卡到秒级；超过这个时间就杀掉子进程，
/// 绝不能让状态轮询无限等待（否则界面会一直停在「连接中」）。
pub const CLI_TIMEOUT: Duration = Duration::from_secs(3);

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

/// 带超时地执行命令并收集输出。
///
/// 超时（或子进程异常）时先 `kill` 再 `wait`，确保不会留下卡死的 `easytier-cli`；
/// 调用方拿到 `Err` 后应保留上一次的快照，而不是把界面清空。
pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> io::Result<Output> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("命令超过 {}ms 未返回，已结束", timeout.as_millis()),
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(err);
            }
        }
    }
    child.wait_with_output()
}

#[cfg(test)]
mod tests {
    use super::{command, run_with_timeout};
    use std::time::{Duration, Instant};

    #[test]
    fn returns_output_of_a_fast_command() {
        let mut cmd = command("echo");
        cmd.arg("hello");
        let output = run_with_timeout(cmd, Duration::from_secs(2)).expect("echo 应当成功");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
    }

    /// 超时必须杀进程并返回错误，而不是一直等下去。
    #[cfg(unix)]
    #[test]
    fn kills_the_child_when_timeout_is_hit() {
        let mut cmd = command("sleep");
        cmd.arg("10");
        let started = Instant::now();
        let result = run_with_timeout(cmd, Duration::from_millis(200));
        let error = result.expect_err("超时应返回错误");
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "超时后必须立即返回，实际耗时 {:?}",
            started.elapsed()
        );
    }
}
