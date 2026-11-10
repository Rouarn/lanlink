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
packages/mobile/           # 移动端占位，暂不实现
docs/                      # 说明文档与截图
```

`packages/core/<平台>/` 使用 EasyTier v2.6.4 的二进制，按 `macos-arm64`、`macos-x64`、
`windows-x64` 分目录放同名文件，代码由 `platform_dir()` 自动选择。这些二进制体积大，**不进版本库**
（见 `.gitignore`）：本地构建需要自己放进去，GitHub Actions 会按目标平台从 EasyTier 官方 release 下载。

`pnpm-workspace.yaml` 只列举 `packages/desktop`、`packages/shared`、`packages/mobile`，
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

`.github/workflows/build.yml` 在 Actions 页面手动触发（`workflow_dispatch`），或推 `v*` 标签时自动跑：

| 平台 | 构建机 / 目标 | 产物文件名 |
| --- | --- | --- |
| macOS arm64 | `macos-latest` / `aarch64-apple-darwin` | `LanLink-0.1.0-macos-arm64.dmg` |
| macOS x64 | `macos-15-intel` / `x86_64-apple-darwin` | `LanLink-0.1.0-macos-x64.dmg` |
| Windows x64 | `windows-latest` / `x86_64-pc-windows-msvc` | `LanLink-0.1.0-windows-x64-setup.exe` |

- easytier 二进制不进仓库，workflow 会按上表平台从 EasyTier 官方 release 下载
  （`easytier-macos-aarch64-v2.6.4.zip`、`easytier-macos-x86_64-v2.6.4.zip`、
  `easytier-windows-x86_64-v2.6.4.zip`）并放到 `packages/core/<平台>/`；
- macOS 走 Ad Hoc 签名（`APPLE_SIGNING_IDENTITY="-"`，无需证书），Windows 不签名，仅供自用；
- Android 目前是空壳（`packages/mobile`），本次不出包；
- 构建好的安装包在对应 Actions 运行的 Artifacts 里，保留 14 天。

## 说明

- EasyTier 创建 TUN 网卡需要管理员权限。首次点「连接」会弹系统授权框，
  授权后应用以管理员身份重新启动并自动连接（与官方 GUI 相同做法）：
  macOS 用 `AuthorizationExecuteWithPrivileges`，Windows 用 `ShellExecuteExW` + `runas`
  （UAC 点「否」会提示「你取消了管理员授权，本次未连接」，原窗口继续可用）。
- 中继服务器在「设置 → 运行环境」里由用户自己填写，程序不内置任何默认地址；
  未填写时创建/加入不可用。网络名、密钥随房间生成，不在代码里写死；
  网络名统一为 `lanlink-xxxx`，中继白名单可以只放行 `lanlink-*`。
- 短码解析服务有一个内置默认地址（常量在 `src-tauri/src/settings.rs` 的
  `DEFAULT_RESOLVER_BASE_URL`），新装即可解析短码；设置页可改成自己的地址并一键恢复默认。
  地址支持 `http://`（本地调试）与 `https://`（rustls 握手、默认校验证书，没有跳过校验的开关）。
  部署细节见 `docs/room-code-service.md`。长码（`LL1.` 开头）自包含，不依赖该服务。
- 日志写入 `/tmp/lanlink/core.log`，「日志」页可查看尾部内容。
- 提权与单实例的细节见 `docs/privilege.md`。
