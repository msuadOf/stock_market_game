# macOS 本地 WebUI 构建与部署记录

- 日期：2026-10-05。
- 初始 `git status --short` 为空，构建 agent 不修改 UI、engine 或交易规则。
- 主机：macOS 26.6.2（25G83），arm64，10 个 logical CPU；Node v26.10.0。
- 依据：`AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/build-and-deployment.md`、ADR-0027。
- 部署边界：Rust/Axum 仅提供回环 WebUI；浏览器本地模式通过 WASM Worker 运行 engine。没有公开部署、push 或原生 Desktop 打包。

## 工具准备

默认 PATH 缺少 Rust、Corepack、wasm-pack 和 GNU coreutils。已安装 Rust 1.96.1、WASM nightly-2026-09-05（rust-src / wasm32-unknown-unknown）以及 Homebrew coreutils。

Corepack 安装在 `.tmp/macos-webui-tools/node_modules/.bin/`。通过它执行 `pnpm install --frozen-lockfile`，实际使用 pnpm 11.19.0，98 个 packages 安装成功，退出码 0。Homebrew wasm-pack 为 0.15.0；CI 固定 0.13.1，官方该版本没有 macOS arm64 asset，因此另以 `cargo install wasm-pack --version 0.13.1 --locked --jobs 10 --root .tmp/macos-webui-tools/wasm-pack` 编译项目匹配版本。

所有工具安装与构建长命令使用仓库 `run-long-validation.mjs 300000` 或内置 300000ms 外部进程树监督。构建显式传 `--jobs 10`。工具安装和原生服务编译并行使用独立 Cargo 输出目录；实测进程树同时存在多个活跃 rustc，每进程约 30–45% CPU，没有以单核串行方式运行长任务。

## 原生服务编译

```sh
PATH="/Users/asuad/.cargo/bin:$PATH" CARGO_TARGET_DIR="$PWD/target/build-cache/webui" node scripts/run-long-validation.mjs 300000 -- cargo build --locked --release -p server --bin server --jobs 10 --target aarch64-apple-darwin --features web-ui
```

日志：`native-build.log`。此步骤先准备与正式 webui target 相同的 Cargo 缓存，实际编译成功（4m04s，退出码 0）；最终仍执行正式产品构建发布制品。

## 最终状态

首轮 `frontend-build.log` 中 WASM release 编译成功（4m06s），但 wasm-pack 随后需要从源码安装 wasm-bindgen-cli 0.2.93。该轮总进程 elapsed 超过 5m33s，Node supervisor 没有按预期在 300000ms 终止；根因尚未确定，不把该轮列为通过。构建 agent 手动 SIGKILL 整组进程，停止后仅短暂留下等待系统 reap 的 rustc zombie。重跑额外使用 GNU `gtimeout --kill-after=1s 299s` 监督，保持 300000ms 总上限；复用已有 Cargo 缓存后成功完成 CLI 安装、WASM threading 检查、生产 WASM 导出检查、TypeScript 与 Vite。重跑日志为 `frontend-build-retry.log`，退出码 0。没有修改仓库 deadline 代码或放宽限制。

```sh
PATH="$PWD/.tmp/macos-webui-tools/wasm-pack/bin:$PWD/.tmp/macos-webui-tools/node_modules/.bin:/Users/asuad/.cargo/bin:$PATH" gtimeout --kill-after=1s 299s node scripts/frontend-build.mjs --jobs 10 --package-manager corepack
PATH="$PWD/.tmp/macos-webui-tools/wasm-pack/bin:$PWD/.tmp/macos-webui-tools/node_modules/.bin:/Users/asuad/.cargo/bin:$PATH" gtimeout --kill-after=1s 299s ./scripts/build.sh webui --jobs 10 --frontend-dist apps/web/dist --output target/build-artifacts/webui-macos-20261005
```

正式产品构建成功，日志为 `product-build.log`，native cache 编译耗时 0.26s。制品为 `target/build-artifacts/webui-macos-20261005/`，包括 Mach-O arm64 `server`、同目录 `webui/` 与 LICENSE。最终 UI 更新若发生在该次 Vite 构建之后，需要重新构建并发布新的输出目录，不覆盖已有制品。

WebUI 已以 detached 进程启动：PID 30227，监听 `127.0.0.1:3000`，URL 为 <http://127.0.0.1:3000/>。日志为 `webui-service.log`，PID 记录为 `webui-service.pid`。第一次即时 curl 早于监听完成而连接失败；紧接着的独立 curl 成功，真实 HTTP 200，COOP 为 `same-origin`，COEP 为 `require-corp`。入口 JS/CSS 和 10,533,307 bytes WASM 并行 HTTP 检查全部通过（`static-resource-check.json`），WASM Content-Type 为 `application/wasm`。

重启时先通过 `lsof -nP -iTCP:3000 -sTCP:LISTEN` 核对进程属于本次 `server`，再终止该 PID 并执行：

```sh
/Users/asuad/workplace/stock_market_game/target/build-artifacts/webui-macos-20261005/server --services webui --bind 127.0.0.1:3000
```

此前核验的是构建、服务监听与静态资源，不是完整游戏回归。Codex 内置浏览器真实游戏交互和 UI 视觉验收由主 agent 继续完成；本地模式在浏览器 WASM Worker 执行，不提供游戏 Server API。

## 浏览器复核后的最终更新

主 agent 完成内置浏览器启动验收后调整 MarketGrid 代码列 minWidth、侧栏个股短标签与 aria-label。最终 TypeScript 与 Vite 构建成功，正式产品发布至 `target/build-artifacts/webui-macos-20261005-final3/`，日志为 `frontend-final2-build.log` 与 `product-final3-build.log`。中间 final2 发布早于 Vite 完成，没有包含最新 aria-label，未用它替换线上服务；final3 在 Vite 完成后独立发布，已核验生成的 `render-app-DJrYPhBf.js` 包含 `个股走势图与盘口`。

当前服务 PID 为 **31070**，URL 仍为 <http://127.0.0.1:3000/>，真实 HTTP 200。重启命令改为：

```sh
/Users/asuad/workplace/stock_market_game/target/build-artifacts/webui-macos-20261005-final3/server --services webui --bind 127.0.0.1:3000
```

主 agent 发现首次从 2030-01-01 推进至下一日时 notice 显示日终存档不能包含未处理的日内请求，已暂停游戏并作为既有运行风险保留；本构建 agent 未修改 engine。用户要求编译结束后关闭本 sol agent，后续浏览器复核与风险处理交由主 agent。

## 同花顺桌面层次新版

最新制品为 `target/build-artifacts/webui-macos-20261005-terminal/`，当前 PID **34605**。旧 final3 服务已核对命令后停止。启动命令：

```sh
/Users/asuad/workplace/stock_market_game/target/build-artifacts/webui-macos-20261005-terminal/server --services webui --bind 127.0.0.1:3000
```

构建与回归结果见 `desktop-navigation.md`，新日志为 `terminal-production-build.log`、`product-terminal-build.log`、`e2e-terminal-static-final.log`。网页只监听回环地址，没有对外发布。

## 当前开发服务（2026-10-05）

当前 localhost:3000 使用 Vite 开发服务并支持 HMR；前文静态服务 PID 与制品路径是历史验证记录。进程 PID 仅在本地工作文件记录，不提交。
