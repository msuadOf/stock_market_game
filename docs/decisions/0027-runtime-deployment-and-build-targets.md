# ADR-0027：运行时选择宿主与独立构建目标

- 状态：accepted（用户于 2026-10-02 确认）
- 日期：2026-10-02
- 关联：ADR-0005、ADR-0007、ADR-0010、ADR-0025

## 用户决定

Desktop 和 WebUI 的同一份成品在启动时选择本地或远程模式，并填写远程 Server
地址，不分别编译两种模式。部署后的 WebUI 服务与 Server 不依赖 Node.js 或 Rust
工具链；额外提供纯 Server 构建目标，不安装或编译前端，也不要求 Node.js。
保留仅启动 WebUI、仅启动 Server 和同时启动二者的能力。性能优先，不扩大账号、
公网部署或网络环境适配的产品范围。

## 决策

1. 构建目标为 `desktop`、`webui`、`webui-server`、`server`。Desktop 继续使用
   Tauri 原生引擎；WebUI 的本地模式继续在访问者浏览器的 WASM Worker 执行引擎。
   WebUI 服务只分发前端资源，不冒充访问者的游戏后端。
2. 生产 UI 在建立游戏宿主和载入日终档前显示启动选择。远程模式复用 REST/WS
   `RemoteHost`，显式传入用户填写的 HTTP(S) Server 基地址。模式选择仅在启动
   阶段进行，不实现运行中的无缝宿主迁移，不改日终保存和启动一次读取的契约。
   测试构建仍允许明确的 E2E 启动路径，生产构建不能获得该测试能力。
3. 使用现有 Rust/Axum 服务承载生产静态 WebUI，不用 Vite preview 或 Node 服务
   作为部署进程。Cargo feature `web-ui` 控制静态服务能力；含 WebUI 的部署包携带
   可执行程序和 `webui/` 资源目录，按可执行程序位置解析默认资源路径。
   启动参数 `--services webui|server|all` 分别只提供静态资源、只提供游戏 API/WS、
   或提供二者。纯 Server 未编入 WebUI 功能，明确拒绝其他服务模式。
4. 原生服务使用 HTTP + WS；HTTPS + WSS 可通过 TLS 反向代理接入，不在本轮
   新增证书签发、账号体系或认证网关。保留已有会话凭据校验，不把编译产物
   宣称为已完成公网安全验收。监听地址可显式配置，默认保持回环地址。
5. 静态服务提供 WASM 多线程需要的 COOP/COEP 响应头。浏览器安全上下文与
   SharedArrayBuffer 是浏览器限制，不受“协议任选”覆盖：其他电脑访问普通 HTTP
   时可能无法使用本地 WASM 多线程。必须在创建 Worker 前明确报告原因和解决
   方式，不静默改为单线程、主线程、远程宿主或修改交易结果。
6. 纯 Server 可直接通过无 Node 的 shell/batch 入口执行 Cargo。其他构建目标
   在构建机需要 Node、固定 pnpm、Rust 及对应工具；这些不是部署机依赖。
   目标独立输出，不要求纯 Server 准备 WASM、前端资源或桌面系统依赖。
7. Windows、Linux、macOS 制品优先在对应系统原生构建。支持三种运行平台不
   等于支持从当前 Linux 主机生成全部平台安装包；既有实验性交叉构建路线不扩大。
8. 编译与完整回归分开，保留 CI 的回归门禁；本轮只执行代表性短测、定向编译和
   独立 diff 复核。脚本显式配置并行资源和失败退出，不把 dry-run 当成真实制品。

## 边界

2026-10-02 用户追加分发决定：GitHub Actions 在 Linux、Windows、macOS 原生编译；
Apple/Windows 暂不做发行者签名与公证。Windows Desktop 提供 MSI/NSIS 与便携 ZIP，
Linux Desktop 提供 DEB/RPM/AppImage 与便携 ZIP，macOS 提供 app/DMG。
纯 Server 保持单文件 CLI；WebUI Server 为 CLI 加同目录静态资源，另行生成 ZIP/tar.gz。
只上传 Actions artifacts，不因构建要求自动创建公开 Release。前端在同一轮 Actions
构建一次供 UI 产品复用，纯 Server 的编译任务不依赖前端或 Node 安装。

同一份权威 engine、A 股交易约束、会话隔离、单位、个人策略、存档格式和日终保存
时机均不改变。浏览器中运行的本地局相互独立，远程客户端不会因此共享一个玩家
账户；本轮不新增多人同局产品。
