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

单独执行 `pnpm build` 只重建前端，要求上述 WASM 产物已经存在。
远程模式在启动界面选择并填写 Server 地址，后端可另行运行 `cargo run -p server`。
开发环境的 `VITE_ENGINE_HOST=remote` 和 `VITE_REMOTE_BASE_URL` 仅作为表单初值。

### 按产品目标构建与部署

在仓库根目录执行。以下入口按当前操作系统原生编译：Linux 上生成 Linux 成品，
Windows 上生成 Windows 成品，macOS 上生成 macOS 成品，不自动跨平台编译。

| 目标 | 成品与用途 |
|---|---|
| `desktop` | 桌面应用，启动时选择捆绑的本地引擎或远程 Server |
| `webui` | 原生静态服务程序 + `webui/` 资源；使用 `--services webui` 仅提供页面 |
| `webui-server` | 原生服务程序 + `webui/` 资源；使用 `--services all` 同时提供页面和游戏 Server |
| `server` | 纯 Server 单文件 CLI（附 LICENSE），不包含静态前端 |

Linux / macOS：

```sh
./scripts/build.sh desktop
./scripts/build.sh webui
./scripts/build.sh webui-server
./scripts/build.sh server
```

Windows：

```bat
scripts\build.bat desktop
scripts\build.bat webui
scripts\build.bat webui-server
scripts\build.bat server
```

只编译浏览器静态前端（WASM + UI，不编译原生服务或桌面应用）：

```sh
./scripts/wasm-build.sh
```

Windows 使用 `scripts\wasm-build.bat`。静态前端输出为 `apps/web/dist/`；上述四个
产品目标输出为 `target/build-artifacts/<目标>/`。WebUI 的本地游戏运行在访问者浏览器
的 WASM Worker，不是在静态服务部署机上运行；同一成品也可在启动时选择远程 Server。

构建前置条件：

- 纯 `server` 构建只需 Rust/Cargo 与系统构建工具，不需要 Node、pnpm、WASM 或 Tauri。
  Linux/macOS 还需 GNU timeout/mv；macOS 可由 coreutils 提供，Windows 使用系统 PowerShell。
- UI 目标需要 Node 24.18.0 或更新版本、Corepack 与固定 pnpm 11.19.0、Rust 1.96.1、
  `nightly-2026-09-05`（含 `rust-src`、`wasm32-unknown-unknown`）和 wasm-pack 0.13.1。
- `desktop` 另需 Tauri CLI 2.12.1 与对应平台开发依赖；Linux 系统依赖见下一节。
  部署后的 WebUI/Server 不要求安装 Node 或 Rust，但仍依赖正常的操作系统运行库。

默认自动使用可用 CPU 核心；追加 `--jobs 8` 可指定并行数，`--dry-run` 只查看规划。
产品输出目录已存在时拒绝覆盖，重复编译可显式选择新目录：

```sh
./scripts/build.sh server --jobs 8 --output target/build-artifacts/server-next
```

Desktop 自动生成当前平台未签名安装包：Windows 为 MSI/NSIS，Linux 为
DEB/RPM/AppImage，macOS 为 app/DMG。便携 ZIP 和 Server/WebUI 的 ZIP、tar.gz
归档是单独打包阶段，GitHub 分发工作流会调用打包脚本并上传 Actions artifacts，
不自动创建 Release。构建入口不默认运行完整回归，CI 的测试门禁独立保留。

本地与 CI 共用仓库构建脚本；前端流程统一由 `scripts/frontend-build.mjs` 编排，
CI 只负责触发、平台矩阵、环境准备、缓存、门禁和制品传输。
完整打包命令、三种服务启动方式、平台边界与浏览器协议限制见
[构建与部署](docs/build-and-deployment.md)。

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
[构建与部署](docs/build-and-deployment.md)。本节的 dry-run 只用于查看规划，
不表示在 Ubuntu 上已经构建了 macOS 或 Windows 制品。

---

## 🤝 参与贡献

本项目由人类与 AI agent 协作开发。开工前请先阅读：

1. **[`AGENTS.md`](AGENTS.md)** — 协作守则（**必读**）
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
