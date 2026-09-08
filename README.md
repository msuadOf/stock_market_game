# 📈 Stock Market Game

> 一个浏览器优先的股票模拟经营游戏。从开盘钟声到收盘线，模拟市场、构建策略、管理风险。
>
> **A browser-first stock market simulation game.** Trade, strategize, and manage risk in a simulated market.

[![Status: Pre-alpha](https://img.shields.io/badge/status-pre--alpha-orange)]()
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Test-Driven](https://img.shields.io/badge/dev-TDD-success)](docs/testing.md)

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
