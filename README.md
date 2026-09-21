# 📈 Stock Market Game

> 一个浏览器优先的股票模拟经营游戏。从开盘钟声到收盘线，模拟市场、构建策略、管理风险。
>
> **A browser-first stock market simulation game.** Trade, strategize, and manage risk in a simulated market.

[![Status: Pre-alpha](https://img.shields.io/badge/status-pre--alpha-orange)]()
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Test-Driven](https://img.shields.io/badge/dev-TDD-success)](docs/testing.md)

并行引擎候选的资源、事件与失败边界见 [ADR-0017](docs/decisions/0017-escrow-parallel-tick.md)。
业务拒单会显式返回；不变量失败丢弃整 tick 的私有结果。panic 属于进程级故障，不承诺现场恢复。
存档 v2 明确拒绝旧格式，不提供静默迁移。确定性、历史语料、性能和宿主验收是独立门禁，
不得用一个运行采集 PASS 宣称全部完成；尚无最终实测报告时不承诺提速倍数、满核或绝对无死锁。

---

## 🎯 项目愿景

一款**可玩、可学、可扩展**的股票市场模拟游戏：

- 🌐 **Web 优先** — 打开浏览器即玩，零安装
- 🔌 **后端可选** — 单机也能完整游玩；接上后端即可联机 / 持久化 / 远程部署
- 🖥️ **桌面版** — 通过 Tauri 打包为原生应用

### 路线图（Roadmap）

| 阶段 | 目标 | 后端 | 状态 |
|------|------|------|------|
| **Stage 1** | 纯前端单机可玩（本地/文件存档） | ❌ 不需要 | ✅ 已实现 |
| **Stage 2** | 前后端分离，权威后端可选 | ✅ 可选 | 🧪 可运行，继续完善鉴权 |
| **Stage 3** | Tauri 桌面应用 | ❌ 本地直连 engine | 🧪 可运行 |

> 详细范围见 [`docs/roadmap.md`](docs/roadmap.md)。

---

## 🚧 当前状态：**可玩预发布版**

仓库已经包含 Rust 权威引擎、React Web UI、Web Worker/WASM、Axum 远程服务和
Tauri 桌面壳。交易规则以中国大陆 A 股为基线，已实现范围和刻意未模拟的规则见
[`docs/trading-rules.md`](docs/trading-rules.md)。

**当前能力：**

- ✅ 订单簿撮合、集合竞价、涨跌停、T+1、资金与持仓占用、费用和存档校验
- ✅ 同一 React 应用通过 `EngineHost` 运行于 WASM Worker、远程 Axum 或 Tauri
- ✅ 本地存档、文件存档、远程/桌面原子恢复
- ✅ Rust/TypeScript 单元与集成测试、Rust→TS 自动类型同步、Playwright 浏览器 E2E，Windows + Ubuntu CI

## 本地开发

要求 Node 24.18、pnpm 11.19、Rust 1.96.1；版本文件已放在仓库根目录。

首次克隆后必须先生成被 `.gitignore` 排除的 `apps/web/wasm-pkg`。Windows 运行
`scripts\wasm-build.bat`，Linux/macOS 运行 `./scripts/wasm-build.sh`；脚本会安装前端依赖、
以固定 nightly 构建 WASM、复制绑定产物并构建 Web。随后可运行：

```bash
pnpm test
pnpm lint
pnpm types:check
pnpm test:e2e
pnpm dev
```

单独执行 `pnpm build` 只重建前端，要求上述 WASM 产物已经存在。远程模式设置
`VITE_ENGINE_HOST=remote` 和
`VITE_REMOTE_BASE_URL=http://127.0.0.1:3000`，并另行运行 `cargo run -p server`。

### Ubuntu/Debian 桌面开发与无头测试依赖

在 Ubuntu 或 Debian 上编译 Tauri 桌面应用，还需要系统级 GTK/WebKitGTK 开发包和
`pkg-config`。下面的包名覆盖当前 Tauri 2、Wry 和 WebKitGTK 依赖，以及本项目
Task 35 的 pkg-config 探测中缺失的 GLib、GIO、GDK、Cairo、Pango、ATK、GDK-Pixbuf、
libsoup 3 和 JavaScriptCoreGTK 开发文件：

```bash
sudo apt update && sudo apt install -y \
  pkg-config libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libglib2.0-dev \
  libcairo2-dev libpango1.0-dev libatk1.0-dev libgdk-pixbuf-2.0-dev \
  libsoup-3.0-dev libjavascriptcoregtk-4.1-dev
```

这些是编译桌面壳及运行项目真实 Tauri actor/IPC 或桌面 UI 测试路径的系统前置条件，
不是 Rust 或 Cargo 依赖，也不能替代应用测试成功。发行版版本不同，软件包名称和可用
版本可能会变化。遇到 native 依赖错误时，可用下面的检查确认 `pkg-config` 能找到对应
的元数据：

```bash
pkg-config --modversion \
  glib-2.0 gobject-2.0 gio-2.0 gdk-3.0 cairo pango atk \
  gdk-pixbuf-2.0 libsoup-3.0 javascriptcoregtk-4.1
```

Linux 无头验证优先使用真实 Wayland compositor。Weston headless backend 适用于 CI 或无显示
服务器环境，不替代日常桌面使用的 Wayland compositor，也不保证所有 compositor 的行为完全一致：

```bash
sudo apt install -y weston wayland-protocols wayland-utils libgl1-mesa-dri libegl1
export XDG_RUNTIME_DIR="$(mktemp -d)"
chmod 700 "$XDG_RUNTIME_DIR"
weston --backend=headless-backend.so --socket=stock-game-wayland &
export WAYLAND_DISPLAY=stock-game-wayland
GDK_BACKEND=wayland WEBKIT_DISABLE_COMPOSITING_MODE=1 \
  cargo run -p stock-market-game --features simulation-diagnostics
```

确认 socket 可用后，可用 `XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" WAYLAND_DISPLAY="$WAYLAND_DISPLAY"`
运行 `wayland-info`。测试完成后终止 Weston 并删除这个临时运行目录。

Xvfb 是尽力而为的 X11 compatibility fallback，不是唯一 Linux 图形验证依据：

```bash
xvfb-run -a cargo test -p stock-market-game --features simulation-diagnostics
```

在已完成前端构建并准备好测试命令后，可将实际命令放在 `xvfb-run -a` 后执行，例如：

```bash
xvfb-run -a cargo test -p stock-market-game --features simulation-diagnostics
```

本节只记录环境前置条件，不表示这些包已经安装，也不表示 Tauri 编译或无头测试已经
通过。

### 桌面 3×3 构建矩阵

桌面构建矩阵脚本可以先规划 Tauri 安装包路线，再决定是否执行构建。Linux/macOS 使用
shell launcher，Windows 使用 batch launcher：

```bash
./scripts/desktop/build-matrix.sh --dry-run --host linux --target all
```

```bat
scripts\desktop\build-matrix.bat --dry-run --host windows --target all
```

其中 `--host` 只能与 `--dry-run` 一起使用，表示模拟主机进行规划，不会改变真实构建主机，
也不会执行构建。`--target` 可选 `linux`、`macos`、`windows` 或 `all`。正常模式必须只选择
一个 target，例如 `./scripts/desktop/build-matrix.sh --target linux`；不能在正常模式使用
`--target all`。

矩阵中的平台边界如下：

- Linux 原生构建生成 `deb`、`rpm` 和 `appimage`。
- macOS 原生构建生成 `app` 和 `dmg`，签名与公证仍须在 macOS 上完成。
- Windows 原生构建生成 `msi` 和 `nsis`；MSI 仅限 Windows 原生路线，并需要 Windows 原生 WiX/VBScript 支持。
- Linux 或 macOS 到 Windows 仅支持受限的 NSIS 实验性路线，使用 `cargo-xwin`，不能生成 MSI，
  也不会自动产生已签名的制品。
- Linux 到 macOS、Windows 到 macOS、Windows 到 Linux、macOS 到 Linux 均不支持。

正常模式的依赖、跨平台路线的额外工具，以及签名和公证要求，请参阅
[`scripts/desktop/README.md`](scripts/desktop/README.md)。本节的 dry-run 只用于查看规划，
不表示在 Ubuntu 上已经构建了 macOS 或 Windows 制品。

---

## 🤝 参与贡献

本项目由 AI（Claude）与人类协作开发。无论你是人类还是 AI agent，请先阅读：

1. **[`CLAUDE.md`](CLAUDE.md)** / **[`AGENTS.md`](AGENTS.md)** — 协作守则（**必读**）
2. **[`docs/principles.md`](docs/principles.md)** — 工程原则
3. **[`docs/testing.md`](docs/testing.md)** — TDD 工作流
4. **[`docs/decisions/`](docs/decisions/)** — 架构决策记录（决策前的上下文都在这）
5. **[`docs/git/AGENTS.md`](docs/git/AGENTS.md)** — Agent 处理 Git 初始化或日常 Git 流程时的必读指令

**核心铁律：**
- 🧪 **测试先行** — 先写失败测试，再写实现（TDD）
- 🛡️ **防御式编程** — 错误必须显式暴露给用户，绝不静默吞掉
- 📐 **不静默 fallback** — 宁可让程序崩溃并展示细节，也不要悄悄用默认值掩盖问题

---

## 📜 许可证

[MIT](LICENSE) © 2026 msuad

许可证已由 [ADR-0007](docs/decisions/0007-three-deployment-frontend-framework.md) 确认为 MIT。
