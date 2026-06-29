# ADR-0005: 统一引擎 + 三端同构部署（推翻串行阶段模型）

- **状态 (Status):** accepted
- **日期 (Date):** 2026-06-29
- **决策者 (Deciders):** msuad + Claude
- **修订关系：** 细化 [ADR-0002](0002-engine-rust-wasm.md) / [ADR-0003](0003-backend-rust.md) / [ADR-0004](0004-frontend-state-redux-toolkit.md)；解决 [open-questions.md](../open-questions.md) Q8、Q9。

## 上下文 (Context)

原架构（ADR-0002/0003/0004）采用**串行阶段模型**：Stage 1 纯前端单机（本地存档）→ Stage 2 前后端分离（后端可选）→ Stage 3 Tauri 桌面。三阶段各自一套写法，复用靠「engine 是纯逻辑」这一条维系。

msuad 2026-06-29 重新定调：**一份 engine + 一份「宿主无关的应用层」，三端是「engine 跑在哪」的部署形态切换，三端真正同时构建、代码最大程度复用**。核心动机：
- 纯前端单机 = 后端的 Rust 逻辑编译进 WASM 跑在浏览器；
- 前后端分离的 web = 同一份引擎跑在后端、前端调 API；
- Tauri 桌面 = 复用同一套「前后端」代码打包。
后两者尤其复用度高，Tauri 桌面可很快落地。

同时，msuad 对**市场模型**给出根本性定调（见 Decision §3）：NPC（散户/机构/游资）与玩家抽象为**同一种「账户」**，共享同一个盘口（orderbook）、看同一份行情、**互相撮合交易**——即「**撮合驱动价格**」的真实市场模型，而非参考游戏 `ref/模拟股市.html` 的「价格随机游走 + 玩家按现价即时成交」。这意味着未来后端部署到公网时，要支持**多账户（多个前端）连同一后端、各自操作**的联机形态——本次为该形态留好接口与设计余地，但**不在本阶段实现联机**。

约束（铁律）：
- engine 无 I/O、无定时器、无副作用、无全局可变状态。
- TDD：市场随机性必须可注入种子，可断言确定性。
- 防御式：所有外部输入（配置、存档、网络）边界校验 + 显式错误。

## 决策 (Decision)

### §1 一份引擎，三端同构

```
        ┌──────────────────────────────────────────┐
        │  engine crate (纯 Rust 逻辑)              │
        │  money / config / account / orderbook /  │  ← 三端共用，TDD 主战场
        │  market / session+save                   │
        │  + 宿主无关的「应用层」：                  │
        │    Simulator(step loop)、会话、序列化/   │
        │    校验、事件流契约                        │
        └─────────┬───────────────┬─────────────┬──┘
   编WASM │        │ 直接依赖       │ Tauri 直接依赖
   ① 纯前端单机    ② Web 分离        ③ Tauri 桌面
   WASM→Web Worker  Rust 后端进程     engine 在 Tauri
   LocalStorage存档 服务器存档         本地文件存档
   离线可玩         联机（多账户预留）  单机离线
```

**三端的「应用层契约」是同一份**：`step(session) -> (session', events)`，推进一个 tick；玩家动作是请求。引擎**不知道**自己跑在哪种宿主里。

### §2 统一账户模型（NPC = 玩家同构）

- **`Account` 是唯一的「参与者」抽象**：现金(`Money`) + 持仓 + 下单策略。NPC（散户 `retail_0..n` / 机构 `inst_0..n` / 游资 `hot_0..n`）与玩家（`player_0..n`）**是同一种对象**，区别只在「下单策略」：NPC = 算法 AI（种子化随机驱动），玩家 = UI 点击。
- 下单、撮合、持仓代码对**所有账户完全一致**。
- `AccountId` / `AccountKind`(retail/inst/hot/player) 贯穿事件、请求、存档。

### §3 共享盘口 + 撮合驱动价格

- **orderbook 全局唯一**（每只股票一个）：所有账户挂单混在一起，**玩家挂的卖单能被 NPC 吃，反之亦然**——与真实股市一致。
- **价格内生**：买一/卖一价由挂单撮合产生，成交在买卖价交叉处。**废弃** ref 的「价格随机游走」模型。
- 全订单簿撮合：限价单、部分成交、挂单队列、**价格-时间优先**（同价位先挂先成交）。

### §4 模拟循环 + 事件流（解决 Q8 / Q9）

- **种子化 PRNG 存入 `Session`**（`SplitMix64`，可序列化、确定性）。用途从原「行情随机」迁移到 **「NPC 下单决策的随机」**（价格已由撮合内生，不再外生随机）。新局从熵取种；测试注入固定种子可断言精确的 NPC 行为 + 撮合结果；存档存 RNG 状态可重放。→ 满足 TDD 可断言性。
- engine **无定时器**：`step(session) -> (session', events)` 推进一个 tick（含 NPC 决策挂单 → 撮合 → 结算）。交易日边界（集合竞价 / 连续竞价 / 收盘 `lp` 重置）按 tick 计数在状态里追踪。
- 倍速 2x…720x = **宿主调用 `step` 的频率**（引擎不感知时间）。
- **T+0 / T+1 可配置**（`GameConfig` 开关）：持仓带「当日买入锁定份额」字段，日终清算时按配置解锁。

### §5 联机多账户的「设计钩子」（现在留、日后接，不在本阶段实现联机）

为公网联机（多前端连同一后端、各自账户操作）预留：
1. **`GameSession` 抽象**（一用户一 `GameState` + 一 orderbook 集合 + 一 RNG）。engine **严禁全局可变状态 / 单例 / `thread_local`**，状态必须由调用方按 session 显式持有。
2. **`SessionId` / `AccountId` 贯穿**事件 / 请求 / 存档，类型上从第一天就存在（纯前端版隐式单 session）。
3. 后端框架钉死 **Axum（tokio）**——天然 async 多连接，日后多会话并发**零协议返工**。
4. WS 握手 token 同时承担「鉴权」与「→ SessionId 解析」（联机时只多一步映射）。

### §6 协议：WS + REST 双通道（公网可走，韧性内建）

依据对同花顺/通达信/LSEG 的研究，采用真实交易软件通用模式——**双通道分离**：

| 通道 | 用途 | 协议 |
|------|------|------|
| **推送** | 行情流、成交事件、状态变更 | **`wss://`** WebSocket 持久全双工（本阶段 JSON 载荷；日后嫌慢可换二进制如 MsgPack/FlatBuffers，YAGNI 先不引入） |
| **请求** | 下单/撤单/查询/登录/存档 | **HTTPS REST** 请求/响应 |

**公网 WS 韧性契约**（公网长连接必踩，从设计内建而非事后补丁）：
- **心跳 ping-pong**（后端 ~30s 发 ping，客户端不回 pong 即重连）——防中间设备杀空闲连接。
- **事件流带单调递增 `seq` 序号** + **状态可随时快照**——断线重连后按 seq 续传 / 拉快照对齐。
- **`wss://` + TLS 证书**（明文 `ws://` 在公网会被拦截/注入）。
- **握手鉴权 token**（query 或首条消息），裸连拒绝。
- 部署文档须写清反向代理（Nginx/Caddy）的 WS 透传配置；注意 CDN（如 Cloudflare 免费版）对 WS 连接数/频率的限制。

### §7 三端持久化

engine 负责**统一序列化**（`GameState` → JSON）+ **schema 校验**（脏数据显式报错，绝不静默吞）；**存哪由宿主决定**：纯前端 = LocalStorage/IndexedDB，分离版 = 服务器，Tauri = 本地文件。

## 备选方案 (Alternatives Considered)

- **A. 维持串行阶段模型（原 ADR-0002/0003/0004）** — 简单、循序渐进；但三阶段三套写法，复用靠人工，且与 msuad「三端真正同时做、最大复用」诉求冲突。不选。
- **B. 三端各写一份引擎** — 否决：违背「单一权威引擎」核心价值，同步成本随玩法增长失控。
- **C. 价格沿用 ref 的随机游走 + 即时成交** — 最简单、TDD 最顺；但与 msuad 「NPC=玩家同构、共享盘口、撮合驱动、和真实股市一致」定调根本冲突。不选。
- **D. 现在就实现完整联机（账户系统/并发隔离/跨账户 PvP）** — 工程量过大且多处依赖未定的玩法决策。不选：本阶段只留接口（§5），联机日后接。

## 后果 (Consequences)

- **正面：**
  - 一份引擎跑遍三端，复用最大化；Tauri 桌面因复用前后端代码可快速落地。
  - 统一账户模型 + 撮合驱动，行为更贴近真实股市，且为公网联机天然铺路。
  - 种子化 RNG 让 NPC 行为/撮合结果可精确单测、可重放。
  - 公网 WS 韧性从设计内建，避免事后返工。
- **负面 / 代价：**
  - **三端真正同时做 + 全订单簿撮合 + NPC AI + 统一账户 = 数月体量**，非数日。
  - 全订单簿撮合远重于 ref 的即时成交，是首个 `trade`/`orderbook` 模块的主要工作量。
  - 推翻 ref 的价格模型 → market 模块从零设计（无现成参照）。
- **后续需要做的：**
  - 更新 [open-questions.md](../open-questions.md)：Q8/Q9 标已解决；**新增开放问题「NPC AI 行为模型」**（散户/机构/游资各自策略算法未定，AI 不擅自定）。
  - 标注 ADR-0002/0003/0004「被 ADR-0005 细化」。
  - 下一批 engine 模块（按依赖序）：**account → orderbook → market → session/save**。
  - 引入新依赖时（wasm-bindgen / tokio / Axum 等）逐个评估，核心依赖补 ADR。
  - NPC AI 策略定调后，决定 `Account` 的「策略」接口形态（trait + 配置注入）。

## 关联 (Related)

- 细化：[ADR-0002](0002-engine-rust-wasm.md)（engine=Rust）、[ADR-0003](0003-backend-rust.md)（后端=Rust）、[ADR-0004](0004-frontend-state-redux-toolkit.md)（RTK）。
- 解决：[open-questions.md](../open-questions.md) Q8（市场确定性）、Q9（核心玩法循环）。
- 配套：[architecture.md](../architecture.md)、[tech-stack.md](../tech-stack.md)、[`money` 设计](../superpowers/specs/2026-06-29-money-fixed-point-design.md)、[`GameConfig` 设计](../superpowers/specs/2026-06-29-gameconfig-design.md)。
