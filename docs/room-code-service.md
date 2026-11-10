# 房间码解析服务（packages/server）

给 LanLink 客户端提供「短码 → 完整房间配置」查询的极简服务：一个 Go 单文件、只用标准库、
数据落地成一个 JSON 文件。

- 不参与联机流量，EasyTier 仍然走中继 / P2P；
- 完整房间码（`LL1.` 开头）自包含，不依赖本服务，服务挂了也能用；
- 服务默认只监听 `127.0.0.1:18080`，对外由 Nginx **整站转发**（`location /` → `proxy_pass http://127.0.0.1:18080`），
  路径原样透传，不要额外加 `/api/lanlink` 之类的前缀。

## 客户端内置的默认地址（部署后只改这一处）

客户端默认就用内置地址解析短码，全新安装不需要先手填：

```rust
// packages/desktop/src-tauri/src/settings.rs
pub const DEFAULT_RESOLVER_BASE_URL: &str = "https://lanlink.woyuxiaji.cn";
```

- 这是**全项目唯一出处**，前端通过 `app_info.default_resolver_base_url` 读取，不要另写常量；
- 地址**不带** `/api/lanlink` 前缀（整站转发、路径透传），接口路径固定为 `POST {base}/r`、`GET {base}/r/{code}`；
- 服务端开启 `X-LanLink-Token` 校验，Token 作为内置常量（`RESOLVER_TOKEN`）编译进客户端，
  注册与查询都会携带，不在设置页暴露；
- 设置页允许用户改成自己的地址，改完以用户的为准；填成空会回落到默认值；
- 中继没有默认值，只能由查码服务下发或用户手填。

## 客户端协议与错误分类

- **协议**：`http://` 与 `https://` 都支持。`https` 由 `rustls` 完成握手，**默认校验证书**
  （根证书来自 `webpki-roots`，不读取系统钥匙串）。客户端**没有**任何跳过校验的开关
  （没有 `danger_accept_invalid_certs` 之类），自签名证书会被拒绝；本地调试请直接用 `http://127.0.0.1:18080`。
  如果服务端用私有 CA 签发证书，需要把该 CA 加进根证书列表或换公共证书。
- **错误分类**：DNS 解析失败 / 连接超时 / TLS 证书问题 / HTTP 4xx-5xx 分别有不同提示，
  证书问题会明确说「这是服务器证书的问题，不是网络不通」。

短码与长码的边界：

| 输入 | 行为 |
| --- | --- |
| `ROOM-XXXX` 短码 | 先查本机缓存；命中直接用。未命中走 `GET {base}/r/{code}`，成功则拿到网络名、密钥与中继 |
| 解析服务不可达 / 502 / 404 | 报错并提示「可以让对方发完整房间码，长码不依赖解析服务」 |
| `LL1.` 长码 | 就地解压解码，完全不发网络请求，断网也能连 |

## 目录结构

```text
lanlink/
└── packages/
    ├── server/
    │   ├── go.mod
    │   └── main.go        # 单文件服务，只用标准库
    └── ...
docs/room-code-service.md
```

## 一、编译（单二进制）

本机直接跑（开发）：

```bash
cd packages/server
go run .
```

交叉编译到服务器（x86_64 Linux）：

```bash
GOOS=linux GOARCH=amd64 CGO_ENABLED=0 go build -trimpath -ldflags "-s -w" -o lanlink-rooms .
```

也可以直接用根目录脚本（在 macOS 上即可跑通，产物落在 `packages/server/dist/`）：

```bash
pnpm build:server        # → packages/server/dist/lanlink-rooms-linux-amd64（约 6.6 MB）
pnpm build:server:arm64  # → packages/server/dist/lanlink-rooms-linux-arm64（约 6.1 MB）
```

二进制为静态链接单文件，可直接 `scp` 到服务器，不依赖 Go 运行时。

## 二、参数

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `-addr` | `127.0.0.1:18080` | 监听地址。建议只绑 `127.0.0.1`，由 Nginx 反代，不要直接暴露公网 |
| `-data` | `./data` | 数据目录，实际文件是 `<data>/rooms.json` |
| `-token` | 空（也读环境变量 `LANLINK_TOKEN`） | 可选访问 token，填了之后所有请求必须带 `X-LanLink-Token` 头；留空表示不校验 |
| `-ttl` | `0` | 房间码有效期，Go duration 写法，例如 `720h`（30 天）；`0` 表示不过期 |
| `-relay` | 空 | 请求里没带 relays 时使用的兜底中继，例如 `tcp://1.2.3.4:11010`；必须带协议前缀 |
| `-codelen` | `6` | 自动生成的短码长度（6-8） |

> 客户端自己会带上中继，所以 `-relay` 主要是给「客户端没配中继」或第三方接入时兜底。
> 中继的协议前缀只接受 `tcp:// udp:// ws:// wss:// quic:// wg://`。

启动示例：

```bash
./lanlink-rooms \
  -addr 127.0.0.1:18080 \
  -data /www/wwwroot/lanlink-rooms/data \
  -token 换成你的随机串 \
  -ttl 720h
```

## 三、Nginx 反代

```nginx
location /api/lanlink/ {
    proxy_pass http://127.0.0.1:18080/;   # 结尾的斜杠不能省
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-LanLink-Token $http_x_lanlink_token;
    proxy_read_timeout 10s;
}
```

**`proxy_pass` 结尾的斜杠必须写**：`location /api/lanlink/` + `proxy_pass http://127.0.0.1:18080/;`
会把 `/api/lanlink/r/ROOM-8K3M` 重写成 `/r/ROOM-8K3M` 再转发给服务。
少了结尾斜杠，服务会收到 `/api/lanlink/r/...`，直接 404。
同理 `location` 那行也要以 `/` 结尾，否则前缀匹配会漏。

客户端设置里的地址就填 `https://你的域名/api/lanlink`（或 http，看你有没有配证书）。

## 四、宝塔面板启动方式

用「网站 → 通用项目管理」（或「进程守护管理器 / Supervisor」）：

1. 类型选「Go」或「其他」，启动命令填二进制绝对路径加参数：
   `/www/wwwroot/lanlink-rooms/lanlink-rooms -addr 127.0.0.1:18080 -data /www/wwwroot/lanlink-rooms/data -token xxx -ttl 720h`
2. 运行目录填 `/www/wwwroot/lanlink-rooms`，守护进程数 1（服务是单实例，多开会抢 `rooms.json`）；
3. 开机自启打开；
4. 不用开「外网映射」，反代由 Nginx 站点配置完成。

## 五、数据与备份

- 数据文件：`<data>/rooms.json`，内容是房间码数组，写盘用「临时文件 + rename」保证原子性；
- **备份**：直接复制 `rooms.json` 即可（`cp rooms.json rooms.json.$(date +%F)` 或加进面板的定时备份）；
- **丢失影响**：短码全部失效（客户端会收到 404「房间码不存在」），但**已分发的长码不受影响**——
  长码自带网络名、密钥和中继，仍然能连；房间本身也不受影响，只是别人没法再用短码查到它；
- 单文件上限 10000 个房间，超出后注册会返回错误；单个房间最多 8 条中继；
- 迁移服务器时把 `rooms.json` 一起拷走即可，无需导入命令。

## 六、接口与验证

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| `GET` | `/healthz` | 健康检查，返回 `{"ok":true,"rooms":N}` |
| `POST` | `/r` | 注册房间码，body 含 `network_name` / `network_secret` / `relays` 等 |
| `GET` | `/r/{code}` | 解析房间码，返回完整配置；404 不存在、410 已过期 |

验证命令（在本机）：

```bash
curl -s http://127.0.0.1:18080/healthz
# {"ok":true,"rooms":0}

curl -s -X POST http://127.0.0.1:18080/r \
  -H 'Content-Type: application/json' \
  -H 'X-LanLink-Token: 你的token' \
  -d '{"network_name":"demo","network_secret":"secret","relays":["tcp://1.2.3.4:11010"]}'

curl -s http://127.0.0.1:18080/r/ROOM-XXXX -H 'X-LanLink-Token: 你的token'
```

从公网验证反代是否配对（注意结尾斜杠）：

```bash
curl -s https://你的域名/api/lanlink/healthz
```

返回 `{"ok":true,...}` 说明 path 重写正确；返回 404 大概率是 `proxy_pass` 少了结尾斜杠。
