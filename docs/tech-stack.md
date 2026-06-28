# 技术栈 (Tech Stack)

> 本文档记录各层的技术选型。**已敲定的标 ✅，待定的标 ⏳**（待定项见 [`open-questions.md`](open-questions.md)，决策进 ADR）。

---

## 总览

| 层 | 技术 | 状态 | 备注 |
|----|------|------|------|
| 前端框架 | **React** | ✅ 已定 | 用户指定 |
| 前端构建 | Vite | ⏳ 建议 | React 生态主流、快 |
| 前端语言 | TypeScript | ✅ 已定 | 与 engine 共享类型 |
| 状态管理 | ? | ⏳ | 见 Q5 |
| 桌面壳 | **Tauri 2** | ✅ 已定 | 本机已装 (2.11)；用户指定 |
| 游戏核心 (engine) | **TS** 或 **Rust→WASM** | ⏳ | 见 Q1（最关键决策） |
| 后端语言 | **Rust** 或 **Go** | ⏳ | 见 Q2（用户标注可后期讨论） |
| 包管理 | npm | ⏳ 建议 | 本机已具备；是否升级 pnpm 见 Q4 |
| 测试 (TS) | Vitest | ⏳ 建议 | Vite 原生、快 |
| 测试 (Rust) | `cargo test` | ✅ | 随 Rust |
| E2E | Playwright | ⏳ 建议 | 跨浏览器 |
| Lint/格式化 | ESLint + Prettier | ⏳ 建议 | |

---

## ⏳ 关键开放决策（摘要）

> 详见 [`open-questions.md`](open-questions.md)。这里只列影响技术栈的：

### Q1. engine 用 TS 还是 Rust→WASM？（**最关键**）
- **纯 TS**：engine 与前端同语言，TDD 最顺，纯前端零成本可跑，无 WASM 构建复杂度。
- **Rust→WASM**：前后端/Tauri 共用一份 Rust engine（真正的单一代码源），性能强，但 WASM 桥接与构建链更重。

### Q2. 后端用 Rust 还是 Go？
- **Rust**：与 engine/Tauri 同语言，类型与错误处理强；本机已装。
- **Go**：并发简单、部署轻（单二进制）；本机**未安装**，需评估学习/部署成本。

> 用户已标注此项"可后期讨论"。但若 Q1 选 Rust→WASM，后端大概率顺势选 Rust（共享 engine）。

### Q4. 包管理 / monorepo 工具
- npm workspace（零额外工具）vs pnpm workspace（更快、磁盘省）。

### Q5. 前端状态管理
- 轻量（Zustand）vs React 内置（useReducer + Context）vs 重型（Redux Toolkit）。
- 倾向轻量，因为 engine 已是状态转换核心，UI 状态层应薄。

---

## 已敲定的依据

- **React**：用户明确指定。
- **Tauri 2**：用户明确指定；本机已安装 `tauri-cli 2.11.3`，工具链就绪。
- **TypeScript**：前端事实标准；若 engine 用 TS 则天然共享类型。

## 待定项的决策流程

每个 ⏳ 都将通过 ADR 敲定（[`decisions/`](decisions/)）。**未敲定前不擅自引入。**
