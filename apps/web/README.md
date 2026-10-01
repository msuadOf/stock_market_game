# Web 前端

股票模拟游戏的共享 React 前端。同一套界面通过 `EngineHost` 连接三种运行形态：

- `wasm`：浏览器 Web Worker 内运行 Rust/WASM 引擎
- `remote`：通过 REST + WebSocket 连接 Axum 服务端
- `tauri`：通过 Tauri command/event 连接桌面端 actor

## 本地运行

首次克隆后，在仓库根目录先生成未纳入 Git 的 WASM 绑定产物：Windows 运行
`scripts\wasm-build.bat`，Linux/macOS 运行 `./scripts/wasm-build.sh`。脚本会安装依赖、
构建 `apps/web-wasm`、复制 `apps/web/wasm-pkg` 并完成一次 Web 构建。之后启动开发服务：

```bash
pnpm --filter web dev
```

同一份 UI 在启动界面选择本地或远程；本地自动区分浏览器 WASM 和桌面 Tauri，
远程显式填写 Server 的 HTTP(S) 地址，对应 WS/WSS 自动派生。开发环境
`VITE_ENGINE_HOST=remote` 与 `VITE_REMOTE_BASE_URL` 仅作为表单初值。
`VITE_REMOTE_TOKEN` 只保留 DEV 行为，生产连接使用 Server 返回的会话凭据；
目标构建脚本剔除上述三个构建环境变量，不将构建机令牌写入可分发 UI。
失败/取消重新选择会等待已提交日终写入、复用首次读档源和明确的新局配置，
不无缝迁移、不降级引擎。浏览器本地多线程还要求安全上下文与 COOP/COEP。

生产部署使用 Rust 服务，不依赖 Node.js 或 Vite preview。四种构建目标和三种
服务启动方式见 [构建与部署](../../docs/build-and-deployment.md)。

### 当前会话 NPC 决策诊断（仅 DEV）

要在 Worker 当前游戏会话启用 NPC trace，先在仓库根目录构建单独的调试 WASM 包，再显式启动 Vite：

```bash
RUSTUP_TOOLCHAIN=nightly-2026-09-05 CARGO_BUILD_JOBS=8 wasm-pack build apps/web-wasm --target web --dev --out-dir ../web/wasm-diagnostics-pkg -- --features simulation-diagnostics --jobs 8
VITE_ENGINE_DIAGNOSTICS=1 corepack pnpm --filter web dev
```

调试包写入 `apps/web/wasm-diagnostics-pkg`，不在 `public` 中；Worker 只在 `import.meta.env.DEV`
且开关精确为 `1` 时加载它，并用同一 `EngineHost` 的会话设置和 seed 创建正在玩的会话。
普通开发、预览和 release 都使用 `apps/web/wasm-pkg`；release 的 `DEV` 条件为 false，不会加载或打包诊断包。

## 验证

```bash
pnpm --filter web test
pnpm --filter web lint
pnpm --filter web build
pnpm --filter web test:e2e
```

构建与 E2E 要求 `apps/web/wasm-pkg` 已由上述 WASM 构建脚本生成；E2E 会构建并启动
production preview，首次运行前执行 `pnpm --filter web exec playwright install chromium`。

交易规则由 Rust engine 统一执行，前端校验仅用于及时反馈，不能替代引擎校验。
规则范围见 [`../../docs/trading-rules.md`](../../docs/trading-rules.md)，架构见
[`../../docs/architecture.md`](../../docs/architecture.md)。
