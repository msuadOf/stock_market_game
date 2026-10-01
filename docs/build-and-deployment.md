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
  MSI/NSIS，macOS 为 app/dmg。此次明确使用 Tauri `--no-sign`，不进行发行者签名、
  公证或公开发布；Windows MSI 使用 `zh-CN`（936 代码页）以支持现有中文产品名。
  固定 CLI 的原生 MacOsBundle/Dmg 路径本身已跳过二进制补丁；macOS 另显式传
  `--no-binary-patching` 作为防御性配置，不宣称修复了已复现的 ARM 启动缺陷。
  Apple 链接器为 ARM 程序生成的必要 ad-hoc 标记不属于 Developer ID 发行签名或公证。
  当前没有 updater，不需要每种 bundle 的更新器识别补丁。
  [既有矩阵](../scripts/desktop/build-matrix.mjs) 的实验性交叉路线仍保留原边界。

构建时从 `rustc -vV` 读取真实主机 target，并显式传给 Cargo/Tauri，防止全局
`build.target` 配置将新程序写到另一个目录却误发布旧缓存。Server/Web 复用独立
Cargo 缓存。
Desktop 同样复用独立 Cargo 编译缓存，每轮先清除对应原生 target 的旧 bundle，
再由本轮成功的 Tauri 命令重新生成，不能把缓存中的旧安装包发布为新制品。
监督覆盖构建、发布和本次临时目录清理，总上限 300000ms；超时或清理无法确认
时明确失败，不冒充成功。大型 Desktop 目录清理时限仍须在真实打包环境核验。

### GitHub 三平台未签名分发

`.github/workflows/distributions.yml` 在 push、PR 或手动调度时执行：

- Ubuntu 24.04 / Windows Server 2022 / macOS 15 原生 runner；根据 `rustc -vV`
  标注实际 target 与架构，不声称一个原生包支持其他架构，也不生成 universal 包。
- 纯 Server 三平台任务独立于前端，使用原有无 Node shell/batch 编译入口。
  Actions runner 自带 Node 仅用于编译后的短 smoke 与归档工具；Server 编译与部署
  不依赖 Node，纯 Server 任务不安装 Node、pnpm、WASM 或 Tauri。
- Desktop / WebUI Server 共用本次 workflow 的一份生产前端；固定 WASM nightly、
  wasm-pack 0.13.1 与 pnpm 11.19.0，原生 Desktop 下载官方 Tauri CLI 2.12.1。
  不跨 workflow 缓存前端；各原生产品只复用平台/架构隔离的 Cargo 编译缓存。
- 编译失败同样保存已完成的 Cargo 缓存，但失败制品不打包、不上传。所有编译和
  归档命令继续受各阶段的 300000ms 进程树上限约束，不通过放宽期限掩盖冷构建失败。
  Desktop 先以 `--compile-only` 在独立受限阶段编译原生桌面，再在另一受限阶段
  校验编译缓存并生成安装包；两者使用相同 Tauri feature graph、配置和 native target。
  不用裸 `engine --lib` 预热冒充整个桌面依赖图，避免前端、SDK 冷编译及打包下载
  叠加在同一条命令内耗尽时限。编译阶段不清除 bundle、不发布制品。
  当前仅发布 Desktop，Rust 壳库只生成供桌面可执行程序/测试链接的 `rlib`，不额外
  链接未使用的 mobile FFI `staticlib`/`cdylib`；Windows 冷链接不能通过输出三份
  重复引擎或放宽时限解决。未来如需原生移动宿主，须重新定义其 FFI 产物契约。
- 原有 `ci.yml` 的回归、Clippy、lint 与 E2E 门禁保留；分发 workflow 不额外执行
  完整回归，不将“打包成功”当作游戏回归或 GUI 安装验收。

| 产品 | Actions 下载产物（包含大小及 SHA-256 manifest） |
|---|---|
| Windows Desktop | MSI、NSIS 安装程序、免安装 ZIP（exe + LICENSE） |
| Linux Desktop | DEB、RPM、AppImage、免安装 ZIP（AppImage + LICENSE） |
| macOS Desktop | DMG、保留应用权限/内部相对链接的 app ZIP 和 tar.gz |
| 三平台纯 Server | ZIP / tar.gz，仅 `server[.exe]` 单文件 CLI 与 LICENSE |
| 三平台 WebUI Server | ZIP / tar.gz，CLI + 同目录 `webui/` 静态资源 + LICENSE |

产物只上传为 Actions artifacts，不创建 Release、不推送标签、不配置签名凭据。
未签名包可能被 SmartScreen/Gatekeeper 提示或拦截；便携版仍依赖正常操作系统
运行库，Windows 需要 WebView2，Linux AppImage 的 FUSE/提取运行能力和 Linux
发行版运行库兼容性须在部署机确认，不承诺无系统依赖。

本地已生成制品可独立打包，输出 `target/distributions/NAME` 必须尚不存在：

```sh
node scripts/run-long-validation.mjs 300000 -- node scripts/package-distributions.mjs webui-server --input target/build-artifacts/webui-server --output target/distributions/webui-server --target x86_64-unknown-linux-gnu
```

归档阶段不编译，显式拒绝目标/主机架构不符、缺失安装格式、空文件、外部链接与
输出覆盖。Unix ZIP/tar.gz 保存可执行权限；macOS 应用只允许内部相对链接。
macOS ZIP 使用系统 BSD tar/libarchive，显式写 UTF-8 文件名，不依赖 Apple 旧 zip
不支持的 `-UN=UTF8` 选项；Linux 使用 Info-ZIP，Windows 使用 PowerShell。
其他系统须替换为真实 native target，不通过改参数伪装成交叉编译。

CI 的原生阶段使用 `--frontend-dist target/ci-frontend` 避免反复编译相同 UI。
此选项只适用于 UI 目标，要求工作区内无符号链接的完整生产目录，原生构建前
检查 HTML 和发布 WASM；不会绕过 WASM 私有诊断导出检查。普通构建不传此选项
仍自行安装依赖和生成前端。传入其他自建目录的源码一致性由调用者负责。

未签名开关和 MSI 语言依据 Tauri 官方固定版本的
[build CLI](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.1/crates/tauri-cli/src/build.rs)、
[macOS bundler](https://github.com/tauri-apps/tauri/blob/tauri-v2.9.5/crates/tauri-bundler/src/bundle/macos/app.rs)
与 [WiX 语言映射](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.1/crates/tauri-bundler/src/bundle/windows/msi/languages.json)。

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
