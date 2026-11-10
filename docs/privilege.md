# 提权、单实例与「每次都要输密码」

本文说明 LanLink 在 macOS 上启动 EasyTier core 的进程模型，以及为什么每次连接都要输一次管理员密码。
结论在最后一节。

## 一、为什么要提权

EasyTier core 需要创建虚拟网卡（utun）并改路由，普通用户权限不够，必须 root。
macOS 上 Tauri 应用不能用 `sudo`，只能用 Authorization Services：

- `elevate::relaunch_as_root()` 通过 `AuthorizationExecuteWithPrivileges`
  以 root 身份重新启动**同一个可执行文件**，并带上 `--elevated-run` 等参数；
- 子进程由系统的 `security_authtrampoline` 拉起，父进程只是等它启动；
- 启动后紧接着 `AuthorizationFree(auth_ref, kAuthorizationFlagDestroyRights)`
  立即销毁这次授权（见 `macos.rs`），不会留下可复用的凭证。

## 二、进程模型（当前实现：启动即提权）

主窗口出现之前就解决权限（`lib.rs` 的 `elevate_at_launch`）：

1. 启动时先看 `geteuid()`；已经是 root（或带 `--elevated-run`）就继续正常起界面；
2. 不是 root：`relaunch_as_root` 立刻请求管理员授权，带上
   `--elevated-run --data-dir <用户数据目录> --owner <uid:gid>` 以 root 重开自己；
3. 拉起成功后当前进程**立即退出**，不会先显示一个普通权限窗口；
4. 用户点「取消」/授权失败：`osascript` 弹窗说明「没有管理员权限无法创建虚拟网卡」，随即退出。

因此不存在「没权限的窗口先用一半、再重开一个有权限的」这条路径；
普通权限进程在创建任何窗口（NSApplication）之前就退出了，**不会产生 Dock 图标**，
系统里始终只有一个界面进程、一个 Dock 图标。

### 只有一个 Dock 图标与一个 core

- 提权发生的时机提前到窗口创建之前，用户态进程从不进入 Dock，
  不会出现「两个 LanLink 图标、其中一个没有运行小圆点」；
- 单实例锁保证任何时候只有一个 App 进程持有锁，第二个实例启动后发现锁被占就直接 `return`；
- core 的 pid 写在 `<数据目录>/logs/core.pid`（macOS：
  `~/Library/Application Support/com.xiaji.lanlink/logs/core.pid`，Windows：
  `%LOCALAPPDATA%\com.xiaji.lanlink\logs\core.pid`），连新 core 前会先回收 15888 与 11010-11013
  上的旧 core 进程，避免「两个 core 抢同一份 peer id」。

### root 实例的数据目录

root 的 `$HOME` 是 `/var/root`，直接读默认路径会读不到用户自己的 `rooms.json` / `settings.json`。
所以提权时显式传：

```text
--elevated-run --data-dir <用户数据目录> --owner <uid:gid>
```

写回文件后再把属主 `chown` 回原用户（`config::hand_back`），目录本身也一并交还，
避免用户态下次启动读不了自己的配置。
日志文件同理：core 以 root 创建 `<数据目录>/logs/core.log` 后会把文件收紧到 0600、
目录收紧到 0700，再把属主交还登录用户——权限比旧版的 0666 更紧（日志里有中继地址和虚拟 IP），
但属主仍是当前用户，点「清空日志」不会因为没写权限而失败。

Windows 的数据目录是 `%LOCALAPPDATA%\com.xiaji.lanlink`（旧版本在 `%TEMP%\lanlink`）。
升级后首次启动会把旧目录里的 `settings.json` / `rooms.json` 一次性复制过来：
只补缺失文件、不覆盖已有配置，失败只记日志，不影响启动。

## 三、为什么每次启动都要输密码

`AuthorizationExecuteWithPrivileges` 需要一个 `AuthorizationRef`：

- 我们每次启动都在**新进程**里新建 AuthorizationRef，用完立即 `DestroyRights`；
- 系统对「获得 root 权利」的授权默认只缓存约 5 分钟（`kAuthorizationFlagDefaults` 的 right 缓存），
  且以 `security_authtrampoline` 拉起子进程后，凭证不跨进程继承；
- 因此：5 分钟内反复启动不会再问密码；超过这个窗口、或重启过电脑，就会重新弹框。
  这是系统行为，不是 LanLink 故意每次索要。

实测：授权过一次后 5 分钟内反复启动不弹框；等到凭证过期再启动，密码框会重新出现。

## 四、能不能复用授权 / 换常驻 helper

### 方案 A：复用同一个 AuthorizationRef

- 需要**同一个进程**长期存活并持有 auth_ref（`kAuthorizationFlagExtendRights` 续期），
  但当前模型是「提权实例接管界面、原实例退出」，原进程退出后 auth_ref 必然销毁；
- 即使改成「原进程常驻、只让子进程提权」，也要求原进程一直活着，
  与「关窗口即退出」的产品行为冲突，而且 auth_ref 在内存里长期有效本身是风险；
- 结论：**当前架构下不可行**，要做得先改成常驻托盘进程。

### 方案 B：常驻提权 helper（SMAppService / LaunchDaemon）

- `SMAppService.daemon` 要求应用**有开发者 ID 签名**，自签名（ad-hoc）无法注册，
  这是硬限制，不是配置问题；
- 传统 `LaunchDaemon` plist + root 可执行文件在自签名下技术上能装，但需要：
  自建安装/卸载流程、版本协商（App 升级后 helper 也要换）、
  XPC 或 socket 鉴权（否则任何本地进程都能让 helper 起 core）、代码签名校验；
- 收益是「输一次密码，长期不再输」，代价是一整套特权后台服务的安全面，
  在还没有正式签名的阶段性价比很低；
- 结论：**暂不推荐**。路线建议：先用 Developer ID 签名 + 公证 →
  再把提权范围收窄到「只启动 core」（界面不需要 root）→ 最后再考虑 helper。

## 五、已知的显示问题

WKWebView 在被其它窗口遮挡时不会绘制，`screencapture -l <windowId>` 截未激活的提权窗口
会得到一张白图——这是截图假象，不是界面没渲染。提权实例启动时会 `set_focus()` 主动激活窗口。

## 六、Windows：计划任务静默提权（可选）

Windows 没有 macOS 那种「授权一次短期内复用」的机制，唯一能免掉每次 UAC 的做法是
注册一个「以最高权限运行」的计划任务，之后由任务启动自身（`win_task.rs`）。

任务定义（XML 注册，`schtasks /Create /XML`）：

- 任务名 `LanLink`，不隐藏，用户可在任务计划程序里看到并删除；
- 不设任何触发器：**不会开机或登录自启**，只在需要时 `schtasks /Run` 按需启动；
- 账户：当前用户，`InteractiveToken` + `RunLevel=HighestAvailable`；
- 动作：`<安装目录>\LanLink.exe --elevated-run --from-task`，
  工作目录为 exe 所在目录，保证 `core/<平台>` 的相对查找不受影响。

> 升级说明：早期版本的任务带 `LogonTrigger`。任务注册用 `/F` 覆盖写，
> 而管理员实例每次启动都会调用 `install_silent_task()` 刷新定义，
> 因此旧任务会在升级后第一次以管理员身份启动时被重写成无触发器的版本，不会继续自启。

启动顺序（`lib.rs` 的 `prepare_launch_windows`）：

1. 已经是管理员（任务或 UAC 拉起）：直接起界面；带 `--setup-elevate` 时顺手建任务、落标记；
2. 未提权且已开标记：让出单实例锁 → `schtasks /Run` → 等新实例写 pid → 自己退出；
3. 未提权且从未询问过：弹一次说明框，同意则 `runas` 拉起 `--setup-elevate` 实例；
4. 任何一步失败（UAC 取消、任务被删、被组策略/杀软拦、任务没带来管理员权限）：
   只写 stderr，按普通权限继续启动，点「连接」时再走一次 UAC，界面功能不受影响。

`MultipleInstancesPolicy=IgnoreNew` 防止任务重复拉起；带 `--from-task` 的实例若仍不是
管理员，就不会再触发任务，避免进程来回拉起。卸载时由 NSIS 的
`NSIS_HOOK_PREUNINSTALL`（`windows/installer-hooks.nsh`）删除任务；程序被移动到别的目录后，
下一次以管理员身份启动会自动按新路径重写任务。
