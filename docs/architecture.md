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
├── AGENTS.md / CONTRIBUTING.md
└── ...
```

**包的依赖：** `apps/*` → `packages/engine`；`apps/*` 之间不互相依赖。
engine 是被依赖的叶子，不依赖任何 app。

## Escrow tick 与验证边界（ADR-0017）

本节描述候选实现的契约；完整语料、性能和最终宿主验收仍以独立证据为准，不由文档宣告通过。
市场 tick 采用一条生产路径。P1 由 `DecisionResourceSnapshot::seal` 一次按账户并行固定 post-P0 资源；
P2 决策影子与 P3/P4 就绪轮次承接真实计划依赖，最后统一进入 P5 收据聚合、P6 结算、
P7 派生、P8 哈希核查及 P9 `engine::commit_tick`。账户/股票可并行，同股票 FIFO 不变。
线程预算为 1 不是另一套串行引擎。

`engine::StrategyState` 保存策略权威状态；展示 profile 不另立权威。
`engine::ResVec` 的 cash（分）与 shares（股）分别守恒，聚合方程就是逐 envelope 方程之和。
P0 报价过期释放在 P1 前可见；密封批成交、撤单、拒单、竞价完成及日界释放在下一密封批才可用于分配。
提交后的公开快照立即反映余额，但 UI 快照不保证包含全部账户；全账户验证读取完整存档快照。

`engine::StepFatal` 与业务 IntentRejected 不同。失败丢弃私有影子与 outbox，
`engine::business_state_hash` 不变；`engine::session_state_hash` 允许已声明的 poison/错误元数据差异。
panic 是进程级故障，不使用 catch_unwind 冒充可恢复 tick。
`engine::CivilUpdate` 是独立事务，不推进市场 tick；它与所有 `engine::TickFrame` 共同覆盖连续 seq。
事件来源/实体/局部序号的完整映射见 ADR-0017 §3；数组位置与 seq 不表示跨实体因果。

`engine::TickCommitEvidence` 仅在成功提交后交出真实收据链、全局游标与实际竞价/日界执行次数，
不从最终账本反推收据。`engine::UpdateStreamProjector` 跨同 tick 市场帧和自然日更新维护共享 Session 序号。
`verification-harness` feature 的临时调度观察/置换不进入存档，不形成备用业务实现。
失败的禁排序实验必须有同输入启用排序成功的对照、明确错误、零部分事件及完整状态回滚；
公开 JSON 重排不是执行器扰动证据。

所有验证副本、进程临时目录及缓存位于工作区 `.tmp/`，worktree 位于 `.worktree/`。
验收绑定完整源码内容清单，包含未跟踪的必要源码；运行中哈希漂移使相关结果失效。
性能只比较条件相同的实测 ticks/sec 与进程树 RSS，单列新路径阶段 wall。Rayon 注册表容量
只表示池中配置的 worker 数，不能称为实际 runnable/active worker；实际 runnable 线程由外部
runner 从 Linux `/proc` 对整个进程树采样 `R`（running or runnable）状态。两者都不替代吞吐，
也不能单凭 CPU 百分比推断提速。有限检查不证明绝对无死锁、满核或固定提速。
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
