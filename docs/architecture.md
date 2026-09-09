# 架构 (Architecture)

> 本文档定义系统的分层、依赖方向、目标目录结构。
> 它是"后端可选"和"多端复用"能成立的地基。配套：[`principles.md`](principles.md)、[`tech-stack.md`](tech-stack.md)。
>
> 本文描述当前已落地结构；具体跨端协议以代码和 ADR 为准。

---

## 1. 核心设计理念：Engine 在中心，外壳可替换

```
                         ┌─────────────────────┐
                         │      game-engine     │   ← 纯逻辑核心
                         │  (规则 / 模拟 / 撮合) │      (无副作用、可序列化)
                         └──────────┬──────────┘
                                    │ 纯函数调用
            ┌───────────────┬───────┴────────┬────────────────┐
            ▼               ▼                ▼                ▼
       ┌─────────┐     ┌─────────┐      ┌──────────┐     ┌──────────┐
       │  web    │     │ server  │      │ desktop  │     │  tests   │
       │ (React) │     │(可选后端)│      │ (Tauri)  │     │ (各层)   │
       └─────────┘     └─────────┘      └──────────┘     └──────────┘
        浏览器            远程/联机         原生桌面          自动化
```

**关键点：**
- `game-engine` 是**纯逻辑**：输入"状态 + 动作"，输出"新状态 + 事件"。不碰 DOM、不碰网络、不碰定时器。
- engine 用 **Rust** 实现（[ADR-0002](decisions/0002-engine-rust-wasm.md)）：前端经 WASM 调用，后端/Tauri 直接复用同一 crate。
- 各"外壳"负责 I/O（渲染、存储、网络、定时器），把用户动作翻译成对 engine 的调用。
- 同一份 engine 能在前端、后端、Tauri、测试中**分别运行**——这是"后端可选"与"多端一致"的根本。
- React 应用层只依赖统一 `EngineHost`：命令、`HostUpdate`、存读档与 `SpeedMetrics` 的语义完全
  相同。`HostUpdate` 只包含 `baseline` 与带 seq 覆盖区间的 `delta`；`postMessage`、Tauri IPC、
  REST/WebSocket 只是适配层传输细节，不得渗透为 UI 条件分支。宿主专属能力通过
  `HostCapabilities` 显式暴露，详见 [ADR-0010](decisions/0010-unified-host-protocol-and-local-refresh.md)。

## 2. 分层与依赖方向（**无环依赖**）

```
依赖只能向下流动，绝不向上、绝不跨外壳：

   表现层 (web/desktop UI)  ──┐
   应用层 (用例编排)         ──┼──►  核心层 (game-engine)
   适配层 (存储/网络适配)    ──┘

   ❌ web 不能直接 import server 的代码
   ❌ engine 不能 import 任何 UI / I/O 库
```

| 层 | 职责 | 可依赖 | 不可依赖 |
|----|------|--------|----------|
| **核心层** engine | 游戏规则、市场模拟、撮合、组合计算 | 仅标准库 + 极少数纯逻辑库 | UI、I/O、React、网络 |
| **应用层** | 用例编排（一次"买入"涉及 engine + 存储 + 事件） | engine、适配层 | 具体框架细节 |
| **适配层** | 存储、网络、平台 API 的封装（实现接口） | engine 的接口定义 | 表现层 |
| **表现层** | UI、渲染、交互 | 应用层、核心层的只读视图 | 直接碰存储/网络 |

## 3. "后端可选"如何实现

引擎是纯逻辑 → 同一份代码可放三处：

| 模式 | engine 运行在 | 存储 | 联机 |
|------|--------------|------|------|
| **Stage 1 纯前端** | 浏览器 Web Worker（Rust/WASM） | LocalStorage / JSON 文件 | 无 |
| **Stage 2 单机+后端** | 服务端（Rust engine actor） | 服务端会话 / JSON 存档 | 可选 |
| **Stage 2 权威后端** | 服务端 | 服务端数据库 | 是（权威状态在后端） |
| **Stage 3 桌面** | Tauri 进程（同份 engine） | 本地文件 / 复用后端 | 可选 |

> 关键设计：**存储与联机是"适配层"的可替换实现**。换后端 = 换一个适配器，不动核心。
> 这要求核心层对存储/网络只依赖**接口**，不依赖具体实现（依赖倒置）。

## 4. 状态与持久化

- 游戏状态是**可序列化的纯数据**（JSON 友好），不含函数、不含类实例的隐藏状态。
- 持久化通过**单一数据访问层**（[`principles.md`](principles.md) 原则 4）进行：
  - 接口定义在适配层（`loadState` / `saveState`）。
  - 实现可替换：LocalStorage（前端）/ 文件或 DB（后端 / 桌面）。
- 每次读取都做 **schema 校验**（[`error-handling.md`](error-handling.md) §5），脏数据 → 显式报错而非静默吞。

## 5. 目标目录结构（monorepo）

> 选 monorepo 是因为多端共享同一个 engine，必须放同一仓库。

```
stock_market_game/
├── apps/
│   ├── web/                 # 前端 (React + Vite + Redux Toolkit)，经 wasm 调用 engine
│   ├── web-wasm/            # wasm-bindgen 适配 crate；把 engine 暴露给 Web Worker
│   ├── server/              # 可选后端 (Rust，Stage 2 起)
│   └── desktop/             # Tauri 桌面壳 (复用 web + engine crate)
├── packages/
│   ├── engine/              # 游戏核心逻辑 (Rust crate, 纯逻辑；不含宿主绑定)
│   └── engine-gpu/          # GPU 能力探测/实验管线；权威计算仍为 CPU
├── docs/                    # 你在这里的子树
├── .github/                 # CI / 协作模板
├── CLAUDE.md / AGENTS.md / CONTRIBUTING.md
└── ...
```

**包的依赖：** `apps/*` → `packages/engine`；`apps/*` 之间不互相依赖。
engine 是被依赖的叶子，不依赖任何 app。
Rust 侧用 cargo workspace 管理 `packages/engine`、`packages/engine-gpu`、`apps/web-wasm`、
`apps/server` 和 `apps/desktop/src-tauri`；前端用 pnpm workspace 管理 `apps/web`。
`apps/web-wasm` 是 engine 与浏览器之间的绑定适配层，不在核心 crate 内引入 Web API。
两种 workspace 并存。

## 6. 数据流（一个"买入"操作的例子）

```
用户点击"买入"
  → [表现层] React 组件 dispatch 一个意图
  → [应用层] EngineHost.submitIntent(intent)
  → [适配层] WASM Worker / REST / Tauri invoke
  → [核心层] GameSession.step() → Event[]
  → [应用层] 三种宿主统一交付 HostUpdate（baseline / delta）
  → [表现层] Redux 消费同一种更新批次并重渲染
  任一步失败 → 抛出带上下文的错误 → UI 显式展示（绝不静默）
```

## 7. 待定（与 ADR / 开放问题联动）

- [x] engine 实现语言：**Rust → WASM** ✅ [ADR-0002](decisions/0002-engine-rust-wasm.md)
- [x] 后端语言：**Rust** ✅ [ADR-0003](decisions/0003-backend-rust.md)
- [x] 包管理：pnpm workspace（ADR-0007）
- [x] monorepo 工具：Cargo workspace + pnpm workspace
- [x] 联机协议：REST 命令/快照 + WebSocket 事件（ADR-0005）

> 已敲定的进 ADR（[`decisions/`](decisions/)）；未敲定的进开放问题清单，**不擅自拍板**。
