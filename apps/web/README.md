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
