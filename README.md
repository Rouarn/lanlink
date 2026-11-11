# LanLink

内置 EasyTier 核心的一键局域网联机工具。Tauri v2 + Vue 3 + TypeScript。

当前阶段：P0（macOS，内置 core 拉起 → 加入房间 → 显示虚拟 IP / 对端 / 状态 → 断开清理）。

## 目录结构

```
packages/desktop/          # 桌面端（Vue UI + Tauri）
  src/                     # 前端（views / components / composables / stores / lib / styles）
  src-tauri/
    src/                   # Rust 后端（core 生命周期、提权、RPC 查询）
    icons/                 # 应用图标（由 icons/source-1024.png 生成）
packages/shared/           # 预留（跨端共享类型/常量）
packages/server/           # 房间码解析服务（Go，单文件，不参与 pnpm 安装）
packages/core/<平台>/      # 内置 easytier-core / easytier-cli（打包时拷进 Resources/core）
packages/mobile/           # 安卓用的 libeasytier.so（C ABI 薄封装；.src/ 与 .deps/ 是构建缓存）
scripts/                   # 拉取 EasyTier 源码、生成安卓图标等辅助脚本
docs/                      # 说明文档与截图
```

`packages/core/<平台>/` 使用 EasyTier v2.6.4 的二进制，按 `macos-arm64`、`windows-x64`
分目录放同名文件，代码由 `platform_dir()` 自动选择。这些二进制体积大，**不进版本库**
（见 `.gitignore`）：本地构建需要自己放进去，GitHub Actions 会按目标平台从 EasyTier 官方 release 下载。

出包只覆盖 macOS（Apple 芯片）与 Windows x64，不再提供 Intel Mac 与 Windows arm64 的产物。

`pnpm-workspace.yaml` 只列举 `packages/desktop`、`packages/shared`，
不用通配符，避免把 Go 工程和二进制目录扫进 pnpm 安装。

## 环境要求

- Node.js 20+ 与 pnpm
- Rust（rustup 安装，`~/.cargo/bin` 在 PATH 中）
- Go 1.21+（只有构建查码服务 `pnpm build:server` 时需要）
- macOS：Xcode Command Line Tools

## 安装依赖

```bash
pnpm install
```

## 常用命令

| 目的 | 命令 | 说明 |
| --- | --- | --- |
| 开发跑窗口 | `pnpm dev` | 进 Tauri 开发模式，改代码热更新 |
| 打正式包 | `pnpm build` | 产出 `.app` + `.dmg` |
| 打调试包 | `pnpm build:debug` | 编译更快，产物同目录 |
| 只校验类型/编译 | `pnpm check` | 前端 `vue-tsc` + `vite build`，再跑 `cargo check` |
| 构建查码服务 | `pnpm build:server` | 交叉编译到 `linux/amd64`，产物 `packages/server/dist/lanlink-rooms-linux-amd64` |
| 构建查码服务（ARM） | `pnpm build:server:arm64` | 交叉编译到 `linux/arm64`，产物 `packages/server/dist/lanlink-rooms-linux-arm64` |
| 透传 Tauri CLI | `pnpm tauri:cli <子命令>` | 例如 `pnpm tauri:cli info`，必须自己带子命令 |

产物在 `packages/desktop/src-tauri/target/<profile>/bundle/`。

只想在浏览器里调 UI（不含 Rust 能力，**不会出 Tauri 窗口**）：
`pnpm -F @lanlink/desktop dev`。

## 构建安装包（GitHub Actions）

`.github/workflows/build.yml` **只在打版本标签时自动构建出包**，普通提交不会触发流水线：

| 平台 | 构建机 / 目标 | 产物文件名（`<version>` 取自标签） |
| --- | --- | --- |
| macOS arm64 | `macos-15` / `aarch64-apple-darwin` | `LanLink-<version>-macos-arm64.dmg` |
| Windows x64 | `windows-latest` / `x86_64-pc-windows-msvc` | `LanLink-<version>-windows-x64-setup.exe` |

### 怎么发一版

```bash
# 1. 确保要发的代码已经在 main 上
git checkout main && git pull

# 2. 打标签（形如 v0.1.1，必须匹配 v* 才会触发）
git tag v0.1.1

# 3. 推标签（两个远端都要推，CI 跑在 GitHub 上）
git push origin v0.1.1
git push github v0.1.1
```

推送标签后 GitHub Actions 会自动跑两个平台的构建，产物在**对应那次运行的 Artifacts** 里
（Actions → 选带 `v0.1.1` 标签的那次运行 → 页面底部 Artifacts），分别下载，保留 14 天。

版本号不用手动改 `tauri.conf.json`：工作流会从标签名去掉 `v` 得到版本号，
在构建前写进 runner 工作区的 `packages/desktop/src-tauri/tauri.conf.json`
（不提交回仓库），所以 app / dmg / 安装包内部的版本、以及产物文件名都与标签一致。

需要临时出包（不想打标签）时，可以在 Actions 页面手动触发 `workflow_dispatch`：
此时版本取仓库里 `tauri.conf.json` 的当前值。手动触发不属于「提交触发」，不影响上面的约定。

- easytier 二进制不进仓库，workflow 会按上表平台从 EasyTier 官方 release 下载
  （`easytier-macos-aarch64-v2.6.4.zip`、`easytier-windows-x86_64-v2.6.4.zip`）
  并放到 `packages/core/<平台>/`；
- macOS 走 Ad Hoc 签名（`APPLE_SIGNING_IDENTITY="-"`，无需证书），Windows 不签名，仅供自用；
- Windows 安装包界面为简体中文，只装文件、建快捷方式，不结束进程：早期版本可能残留的
  `core\windows-x64\WinDivert64.sys` 不会被清理，卸载后计划任务也可能残留（下次运行覆盖）；
- Android 复用桌面前端，通过 `tauri android build` 本地出 APK（首次需先跑
  `scripts/fetch-easytier.sh` 编出 `libeasytier.so`），CI 仍只出 macOS 与 Windows 安装包；
- 构建好的安装包在对应 Actions 运行的 Artifacts 里，保留 14 天。

## 说明

- EasyTier 创建 TUN 网卡需要管理员权限。
  macOS：启动即提权——打开应用就请求管理员授权，以 root 重开并接管界面，
  拒绝授权会提示原因后直接退出（macOS 用 `AuthorizationExecuteWithPrivileges`）。
  Windows：默认点击「连接」时才用 `ShellExecuteExW` + `runas` 提权，
  UAC 点「否」会提示「你取消了管理员授权，本次未连接」，原窗口继续可用；
  也可在「设置 → 提权方式」里开启计划任务静默提权（任务无触发器、不会开机自启，
  只在启动时按需运行），之后启动不再弹 UAC。
- 中继服务器在「设置 → 运行环境」里由用户自己填写，程序不内置任何默认地址；
  **创建房间**要求本地已配中继，没配会被拦下提示去配置；
  **加入房间不用配**——短码经解析服务、长码本地解出，中继直接取房间记录里的，
  房间码里没有中继时才报错。加入方也不会把码里的中继写进自己的设置。
  网络名、密钥随房间生成，不在代码里写死；
  网络名统一为 `lanlink-xxxx`，中继白名单可以只放行 `lanlink-*`。
- 短码解析服务有一个内置默认地址（常量在 `src-tauri/src/settings.rs` 的
  `DEFAULT_RESOLVER_BASE_URL`），新装即可解析短码；设置页可改成自己的地址并一键恢复默认。
  地址支持 `http://`（本地调试）与 `https://`（rustls 握手、默认校验证书，没有跳过校验的开关）。
  部署细节见 `docs/room-code-service.md`。长码（`LL1.` 开头）自包含，不依赖该服务。
- 日志写入用户数据目录下的 `logs/core.log`，「日志」页可查看尾部内容
  （macOS：`~/Library/Application Support/com.xiaji.lanlink/logs/core.log`，
  Windows：`%LOCALAPPDATA%\com.xiaji.lanlink\logs\core.log`）；
  日志目录权限 0700、文件 0600，只有当前用户可读写。
- 用户数据（`settings.json` / `rooms.json`）与日志放在同一个数据目录：
  macOS 为 `~/Library/Application Support/com.xiaji.lanlink`，
  Windows 为 `%LOCALAPPDATA%\com.xiaji.lanlink`。
  Windows 从旧版本升级后首次启动会把 `%TEMP%\lanlink` 里的 `settings.json` / `rooms.json`
  一次性复制过来——只补缺失文件、不覆盖已有配置，复制失败只记日志，不影响启动。
- 提权与单实例的细节见 `docs/privilege.md`。
