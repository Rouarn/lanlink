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

## 二、进程模型（当前实现）

点「连接」后：

1. 用户态实例把待连接参数（房间码、主机名、数据目录、原用户 uid:gid）拼成命令行；
2. 释放单实例锁（`/tmp/lanlink-instance.lock` 的 flock），让提权实例能拿到；
3. 后台线程调用 `relaunch_as_root`，另起一个线程轮询 `/tmp/lanlink-instance.pid`；
4. root 实例拿到锁并把 pid 写进 pid 文件后，用户态实例检测到「已有继任者」，
   等 150ms 干净退出（不留在 Dock、不留后台）；
5. root 实例接管界面，接管后启动 core，全局只有一份 core。

失败路径：如果 120s 内没有继任者（用户取消授权 / 授权失败），用户态实例重新拿回锁，
把错误写进 `ELEVATE_ERROR`，前端刷新状态时展示出来。

### 只有一个 Dock 图标与一个 core

- 选的是「提权实例接管界面、原实例退出」，不是两个窗口并存；
- 单实例锁保证任何时候只有一个 App 进程持有锁，第二个实例启动后发现锁被占就直接 `return`；
- core 的 pid 写在 `/tmp/lanlink/core.pid`，连新 core 前会先回收 15888 与 11010-11013
  上的旧 core 进程，避免「两个 core 抢同一份 peer id」。

### root 实例的数据目录

root 的 `$HOME` 是 `/var/root`，直接读默认路径会读不到用户自己的 `rooms.json` / `settings.json`。
所以提权时显式传：

```text
--elevated-run --room <code> --hostname <name> --data-dir <用户数据目录> --owner <uid:gid>
```

写回文件后再把属主 `chown` 回原用户（`config::hand_back`），避免用户态下次启动读不了自己的配置。
日志文件同理：core 以 root 创建 `/tmp/lanlink/core.log` 后会 chmod 666 并交还属主，
否则普通用户点「清空日志」会因为没写权限而失败。

## 三、为什么每次连接都要输密码

`AuthorizationExecuteWithPrivileges` 需要一个 `AuthorizationRef`：

- 我们每次连接都在**新进程**里新建 AuthorizationRef，用完立即 `DestroyRights`；
- 系统对「获得 root 权利」的授权默认只缓存约 5 分钟（`kAuthorizationFlagDefaults` 的 right 缓存），
  且以 `security_authtrampoline` 拉起子进程后，凭证不跨进程继承；
- 因此：5 分钟内连续连接不会再问密码；超过这个窗口、或重启过 App，就会重新弹框。
  这是系统行为，不是 LanLink 故意每次索要。

实测：授权过一次后 5 分钟内反复连接不弹框；等到凭证过期再连，密码框会重新出现。

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
