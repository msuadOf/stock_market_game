# Web 前端

股票模拟游戏的共享 React 前端。同一套界面通过 `EngineHost` 连接三种运行形态：

- `wasm`：浏览器 Web Worker 内运行 Rust/WASM 引擎（默认）
- `remote`：通过 REST + WebSocket 连接 Axum 服务端
- `tauri`：通过 Tauri command/event 连接桌面端 actor

## 本地运行

首次克隆后，在仓库根目录先生成未纳入 Git 的 WASM 绑定产物：Windows 运行
`scripts\wasm-build.bat`，Linux/macOS 运行 `./scripts/wasm-build.sh`。脚本会安装依赖、
构建 `apps/web-wasm`、复制 `apps/web/wasm-pkg` 并完成一次 Web 构建。之后启动开发服务：

```bash
pnpm --filter web dev
```

通过 `VITE_ENGINE_HOST=wasm|remote|tauri` 选择宿主；远端模式还可用
`VITE_REMOTE_BASE_URL` 指定服务地址，`VITE_REMOTE_TOKEN` 指定会话令牌。

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
