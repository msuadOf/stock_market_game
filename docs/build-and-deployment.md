# 构建与部署

现行产品选择见 [ADR-0027](decisions/0027-runtime-deployment-and-build-targets.md)。
一份 UI 在启动时选择本地或远程宿主；部署服务是原生 Rust 程序，不用 Node.js
或 Vite preview 承载生产页面。游戏交易与日终存档规则不因部署方式改变。

## 构建目标

在仓库根目录选择目标，Linux/macOS 使用 shell，Windows 使用 batch：

```sh
./scripts/build.sh desktop --jobs 8
./scripts/build.sh webui --jobs 8
./scripts/build.sh webui-server --jobs 8
./scripts/build.sh server --jobs 8
```

```bat
scripts\build.bat desktop --jobs 8
scripts\build.bat webui --jobs 8
scripts\build.bat webui-server --jobs 8
scripts\build.bat server --jobs 8
```

| 目标 | 制品 | 建议启动方式（服务显式传参） |
|---|---|---|
| `desktop` | Tauri 原生安装包/应用包 | 应用启动后选择本地或远程 |
| `webui` | 原生服务程序 + `webui/` 前端资源 | `--services webui`：仅分发 WebUI；浏览器可选本地或远程引擎 |
| `webui-server` | 原生服务程序 + 同一份 `webui/` 前端资源 | `--services all`：WebUI 和游戏 Server 同时提供 |
| `server` | 纯 Server 原生程序 | `--services server`：只提供游戏 API/WS，不提供 WebUI |

产物位于 `target/build-artifacts/<目标>/`，不是源码目录中的开发服务。
先查看规划可以使用 `--dry-run`；它不编译，也不证明对应平台制品已经可运行。
编译不会默认运行完整回归；CI 的回归门禁仍独立保留。

已有制品目录默认拒绝覆盖或删除，避免混入旧资源或改写用户文件。重复构建可选择
制品根目录内的新目录，例如：

```sh
./scripts/build.sh server --jobs 8 --output target/build-artifacts/server-next
```

### 构建机与部署机

- `server` 的 shell/batch 构建入口在调用 Node 前分流，只执行 Rust 构建与制品整理，
  不安装 pnpm，不生成 WASM，不要求前端或 Tauri 工具链。
  Linux/macOS 的进程树监督和无覆盖发布需要 GNU timeout/mv；macOS 使用
  coreutils 的 gtimeout/gmv。
  Windows 入口使用系统 PowerShell。它们是构建机工具，不是部署服务的依赖。
- 其他目标的构建机需要 Node、仓库固定的 Corepack pnpm、Rust、wasm-pack 及固定
  WASM nightly；`desktop` 另需 Tauri CLI 和平台开发依赖。
  构建前先检查 `cargo tauri --version`；缺少子命令时需先准备 Tauri 2 CLI，
  不把 `cargo build` 成功视为安装包已经生成。
- 部署成品不要求安装 Node、pnpm 或 Rust。原生程序仍需要目标操作系统的正常
  运行库；桌面应用需要平台 WebView 等 Tauri 运行依赖，不能把“无 Node”说成
  “无任何操作系统依赖”。
- 制品按对应操作系统原生构建。Linux 原生桌面为 deb/rpm/AppImage，Windows 为
  MSI/NSIS，macOS 为 app/dmg。签名、公证和公开发布不由本脚本自动完成。
  [既有矩阵](../scripts/desktop/build-matrix.mjs) 的实验性交叉路线仍保留原边界。

构建时从 `rustc -vV` 读取真实主机 target，并显式传给 Cargo/Tauri，防止全局
`build.target` 配置将新程序写到另一个目录却误发布旧缓存。Server/Web 复用独立
Cargo 缓存；Desktop 暂使用新 Cargo 目录确保安装包来源，尚未复用引擎编译缓存。
监督覆盖构建、发布和本次临时目录清理，总上限 300000ms；超时或清理无法确认
时明确失败，不冒充成功。大型 Desktop 目录清理时限仍须在真实打包环境核验。

## 部署服务的三种启动方式

将整个部署目录复制到对应系统，保留可执行程序和 `webui/` 的相对位置。
默认资源目录相对于可执行程序解析，不取决于终端当前工作目录。

```sh
# Linux/macOS；Windows 将 ./server 替换为 server.exe
./server --services webui --bind 127.0.0.1:3000
./server --services server --bind 127.0.0.1:3000
./server --services all --bind 127.0.0.1:3000
```

含 WebUI 的两个目标使用同一种服务程序；省略 `--services` 时默认 `all`，并不因
产物目录命名为 `webui` 就变成仅 WebUI。要限制服务面，按上表显式传入模式。
纯 Server 程序省略该参数时默认 `server`。

含 WebUI 的程序支持三种启动方式。纯 `server` 制品只支持 `--services server`，
选择另外两种模式会明确失败，不悄悄变成仅后端。仅 WebUI 模式不提供游戏 API/WS，
访问页面的浏览器选择本地后端时，游戏在该浏览器的 Worker 执行。

默认保持回环监听；需要其他电脑访问时显式指定监听地址，例如
`--bind 0.0.0.0:3000`。`0.0.0.0` 是监听地址，不是供浏览器填写的 Server 地址；
浏览器应访问部署电脑的真实地址。也可使用既有
`STOCK_MARKET_GAME_SERVER_BIND_ADDR` 环境变量，显式 `--bind` 优先。
自定义资源目录使用 `--web-root <目录>`，并通过 `--help` 查看当前参数。

静态资源缺失、非法参数、端口占用或文件读取错误必须明确失败；不以空页面、假成功
或另一种服务模式掩盖异常。未知 API/WS 路径与缺失资源不会返回 HTML 入口冒充成功。

## 启动时选择引擎

- Desktop 的本地模式使用捆绑的原生引擎，不要求启动 HTTP Server。
- 浏览器本地模式使用访问者电脑上的 WASM 多线程 Worker，不在 WebUI 部署机模拟。
- 远程模式填写 Server 的 HTTP(S) 基地址；REST 请求和对应的 WS/WSS 通道由现有
`RemoteHost` 建立。地址可包含反向代理路径前缀，不包含账号密码、查询或片段。
- 生产脚本剔除构建机的宿主、地址和旧令牌环境变量；生产连接不采用 DEV 的
  `VITE_REMOTE_TOKEN` 覆盖新 Server 返回的会话凭据，不新增账号或认证产品。
- 正常启动先选择宿主，再读取一次日终快速档并建立会话；地址与模式不写入游戏档。
  运行中不提供无缝宿主迁移。失败或取消加载后重新选择也不能暗中重读快速槽。
- 生产构建不再靠 `VITE_ENGINE_HOST` 或 `VITE_REMOTE_BASE_URL` 固定模式和地址；
  开发环境值只能作为启动表单初值。明确的 E2E 构建路径不属于生产能力。

## 协议与浏览器约束

原生服务直接提供 HTTP + WS。HTTPS + WSS 可通过 TLS 反向代理接入，代理必须
正确转发 WebSocket Upgrade，并保留静态页面的 COOP/COEP 响应头。本轮没有
新增证书签发、账号系统、代理程序或公网认证网关，也不声称完成公网安全验收。

浏览器 WASM 多线程要求安全上下文、跨源隔离和 SharedArrayBuffer；静态服务
返回 `Cross-Origin-Opener-Policy: same-origin` 与
`Cross-Origin-Embedder-Policy: require-corp`，但响应头不能将普通局域网 HTTP
变成安全上下文。浏览器认可的回环 HTTP 或可信 HTTPS 可用于本地模式；不满足
条件时必须明确提示，不能偷偷降级为单线程或切换远程。

远程模式不依赖本地 WASM 多线程条件，但仍受浏览器证书、混合内容和跨源规则约束。
HTTP(S) 页面对应 WS/WSS 通道，不代表可以绕过浏览器限制。

## 验证范围

单元/契约短测验证参数规划、无 Node 的纯 Server 路径、静态路由、宿主启动选择和
错误边界。实际 Windows/macOS 安装包、平台运行库、签名、公证及网络部署需要在
对应环境验证；不得把 Linux 的 dry-run 或短测冒充三平台发行验收。
HTTPS/WSS 反向代理与浏览器实际 WASM/SAB 游戏启动本轮未验证；现有短测检查
原生服务的隔离响应头、静态资源以及启动策略边界，不冒充浏览器运行验收。
