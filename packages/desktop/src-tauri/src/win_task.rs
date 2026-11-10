//! Windows「计划任务静默提权」。
//!
//! Windows 没有 macOS 那种授权一次长期有效的机制，唯一能免掉每次 UAC 的做法是
//! 注册一个「以最高权限运行」的计划任务，之后由任务启动自身。
//!
//! 任务定义（XML 注册，避免 `/TR` 的多层引号在路径含空格时出错）：
//! - 不设任何触发器（不会开机/登录自启），只允许按需 `schtasks /Run`；
//! - 账户：当前用户，`InteractiveToken` + `RunLevel=HighestAvailable`；
//! - 动作：`<安装目录>\LanLink.exe --elevated-run --from-task`，工作目录为 exe 所在目录，
//!   保证 core 的「exe 同级 core/<平台>」查找不受影响；
//! - 不隐藏任务，用户可以在任务计划程序里自行查看与删除。
//!
//! 所有失败都转成可读错误交回调用方，由上层退回「点连接再提权」，不在这里弹阻塞框。

use std::ffi::OsStr;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONQUESTION, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_YESNO,
};

/// 计划任务名（任务计划程序里显示为 `\LanLink`）。
pub const TASK_NAME: &str = "LanLink";

fn schtasks(args: &[&str]) -> Result<String, String> {
    let output = crate::proc::command("schtasks")
        .args(args)
        .output()
        .map_err(|err| format!("无法执行 schtasks：{err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        return Ok(stdout);
    }
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    Err(if detail.is_empty() {
        format!("schtasks 返回失败（退出码 {:?}）", output.status.code())
    } else {
        detail
    })
}

/// 计划任务是否存在。
pub fn task_exists() -> bool {
    schtasks(&["/Query", "/TN", TASK_NAME]).is_ok()
}

/// 按需启动任务（任务内部是高权限，因此不会弹 UAC）。
pub fn run() -> Result<(), String> {
    schtasks(&["/Run", "/TN", TASK_NAME]).map(|_| ())
}

/// 删除任务；任务本来就不存在也视为成功。
pub fn remove() -> Result<(), String> {
    if !task_exists() {
        return Ok(());
    }
    schtasks(&["/Delete", "/TN", TASK_NAME, "/F"]).map(|_| ())
}

/// 注册任务：覆盖已有同名任务。
pub fn install(exe: &Path) -> Result<(), String> {
    let xml = task_xml(exe)?;
    let path = std::env::temp_dir().join(format!("lanlink-task-{}.xml", std::process::id()));
    write_utf16(&path, &xml)?;
    let result = schtasks(&[
        "/Create",
        "/TN",
        TASK_NAME,
        "/XML",
        &path.display().to_string(),
        "/F",
    ]);
    let _ = fs::remove_file(&path);
    result.map(|_| ())
}

/// 任务 XML。所有路径都做 XML 转义，工作目录取 exe 所在目录。
fn task_xml(exe: &Path) -> Result<String, String> {
    let exe = exe
        .to_str()
        .ok_or_else(|| "可执行文件路径包含非 Unicode 字符".to_string())?;
    let dir = Path::new(exe)
        .parent()
        .and_then(|dir| dir.to_str())
        .ok_or_else(|| "无法定位安装目录".to_string())?;

    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    let user = std::env::var("USERNAME").unwrap_or_default();
    if user.is_empty() {
        return Err("无法获取当前 Windows 用户名".to_string());
    }
    let account = if domain.is_empty() {
        user
    } else {
        format!("{domain}\\{user}")
    };

    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>LanLink：以最高权限静默启动，免去每次 UAC 授权。</Description>
    <URI>\{name}</URI>
  </RegistrationInfo>
  <Triggers />
  <Principals>
    <Principal id="Author">
      <UserId>{account}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
      <Arguments>--elevated-run --from-task</Arguments>
      <WorkingDirectory>{dir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#,
        name = TASK_NAME,
        account = xml_escape(&account),
        exe = xml_escape(exe),
        dir = xml_escape(dir),
    ))
}

/// `schtasks /XML` 要求 Unicode（UTF-16LE + BOM）编码的任务定义。
fn write_utf16(path: &Path, text: &str) -> Result<(), String> {
    let mut bytes = vec![0xff, 0xfe];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(path, bytes).map_err(|err| format!("无法写入任务定义：{err}"))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 首次启动的一次性说明：返回 true 表示用户同意请求授权。
pub fn confirm_first_time() -> bool {
    let text = "LanLink 创建虚拟网卡需要管理员权限。\n\n\
要现在授权一次，并创建一个计划任务吗？之后启动 LanLink 将不再询问 UAC。\n\n\
选择「否」也可以正常使用，只是每次点「连接」时才请求授权，且随时可以在设置里开启。";
    ask(text, MB_YESNO | MB_ICONQUESTION | MB_SETFOREGROUND) == IDYES
}

/// 非阻塞式提示（只在设置页主动操作失败时用，启动阶段不弹）。
pub fn warn(message: &str) {
    ask(message, MB_OK | MB_ICONWARNING | MB_SETFOREGROUND);
}

fn ask(text: &str, flags: u32) -> i32 {
    let text = to_wide(OsStr::new(text));
    let caption = to_wide(OsStr::new("LanLink"));
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            flags,
        )
    }
}

fn to_wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}
