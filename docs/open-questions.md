# 开放问题 (Open Questions)

> 这里是**尚未敲定**的关键技术决策。每解决一个 → 写一条 ADR（[`decisions/`](decisions/)）→ 在此标记为已解决。
>
> AI 协作铁律：**未敲定前，AI 不得擅自为这些问题定方向。** 触及时须上报人类讨论。

---

## 🔴 阻塞型（影响骨架搭建，需优先定）

### Q1. 游戏核心引擎 (engine) 的实现语言？—— **最关键**

**✅ 已解决（2026-06-28）：Rust 编译为 WASM。** 详见 [ADR-0002](decisions/0002-engine-rust-wasm.md)。

---

### Q2. 后端语言：Rust 还是 Go？

**✅ 已解决（2026-06-28）：Rust。** 详见 [ADR-0003](decisions/0003-backend-rust.md)。

---

### Q3. 开源许可证？

当前 `LICENSE` 暂用 **MIT**（宽松、生态友好、适合游戏）。
备选：Apache-2.0（含专利条款）、GPL（强 copyleft，限制闭源衍生）。

**✅ 已解决（2026-06-29）：维持 MIT。** 详见 [ADR-0007](decisions/0007-three-deployment-frontend-framework.md) §7。

---

### Q4. 包管理器与 monorepo 工具？

**选项：**
- **A. npm workspace** — 零额外工具，本机已具备；功能够用。
- **B. pnpm workspace** — 更快、磁盘省（硬链接）、monorepo 体验更好；需先 `npm i -g pnpm`。

**✅ 已解决（2026-06-29）：pnpm workspace。** 仓库通过 `packageManager` 固定 pnpm 11.19.0；`pnpm-workspace.yaml` 已就位。详见 [ADR-0007](decisions/0007-three-deployment-frontend-framework.md) §7。

---

### Q5. 前端状态管理方案？

**✅ 已解决（2026-06-28）：Redux Toolkit。** 详见 [ADR-0004](decisions/0004-frontend-state-redux-toolkit.md)。

---

## 🟡 非阻塞型（Stage 1 可延后，但值得早想）

### Q6. UI 语言 / i18n 策略？

**✅ 首发范围已解决（2026-06-29）：全中文界面，不引入 i18n 框架。** 详见
[ADR-0007](decisions/0007-three-deployment-frontend-framework.md) §2。未来何时增加第二语言仍是产品层开放项，
届时必须先补 ADR，不能把当前硬编码中文误称为“已具备国际化”。

### Q7. 存档与持久化的范围？

**✅ Stage 1 范围已落地：** 一个浏览器快速存档槽 + JSON 文件导入/导出，内容为可确定性恢复的
权威 `SaveSlot`。存档先经过边界校验，再由 Rust engine 深度验证并原子恢复。

多存档槽、成就/完整交易历史、数据库持久化和云同步仍属于 Stage 2 产品决策，当前没有占位式承诺。

### Q8. 市场模拟的确定性？
- 市场行情是否需要"可回放/可复现"（便于测试 + 公平）？
- 若需要，随机数必须可注入种子（呼应 TDD：测试要能断言确定性结果）。

**✅ 已解决（2026-06-29）：种子化 PRNG（SplitMix64）存入 Session，可注入、可序列化、可重放。** 详见 [ADR-0005](decisions/0005-unified-engine-three-deployments.md) §4。
> 注意：随 ADR-0005 定调为「撮合驱动价格」，RNG 的用途从原「行情随机」迁移到「**NPC 下单决策的随机**」。

### Q9. 游戏的"核心玩法循环"边界？
- 第一版（Stage 1）最小可玩 = 哪些功能？（买卖、行情、持仓、盈亏？是否含事件/新闻、止盈止损、多市场？）
- 这决定了 engine 第一批要 TDD 的模块清单。

**✅ 已解决（2026-06-29，2026-09-08 按 A 股基线修订）：tick 步进 + 宿主驱动；全订单簿撮合；对外固定 T+1；统一账户（NPC=玩家同构）+ 共享盘口撮合驱动价格。** 详见 [ADR-0005](decisions/0005-unified-engine-three-deployments.md)。

### Q10. 视觉风格与设计系统？

**✅ 已解决（2026-06-29）：** 亮色券商数据终端风格、Blueprint.js + AG Grid +
Lightweight Charts、桌面/移动响应式布局。详见 [ADR-0007](decisions/0007-three-deployment-frontend-framework.md)
与根目录 [`DESIGN.md`](../DESIGN.md)。

---

### Q11. NPC（散户 / 机构 / 游资）的 AI 行为模型？ ⭐ 新增（阻塞 market/account 的 NPC 部分）

ADR-0005 定调「统一账户 + 撮合驱动价格」后，NPC 是**主动挂单的 AI 参与者**（与玩家平权进同一 orderbook）。但三类 NPC 各自的**策略算法尚未敲定**：

- 散户（retail）：追涨杀跌？噪音交易？受市场情绪驱动？
- 机构（inst）：大单、方向性、可能护盘/砸盘？拆单？
- 游资（hot）：短线投机、拉抬/打压、快进快出？

**✅ 已解决（2026-06-29）：策略为独立模块 + Strategy trait + 每实例独立参数 + 可插拔扩展。** 首批三策略：散户=ZI噪音、机构=基本面价值(隐藏公允价V轨道+机构各异目标价)、游资=动量。玩家不走 Strategy。详见 [ADR-0006](decisions/0006-npc-strategy-module.md)。
> 研究基础：市场微观结构 + agent-based 模拟文献（ZI 泊松模型、基本面/趋势双因子、噪声交易者）。
> 后续每加新策略 = 新增 trait 实现 + 单测 + 工厂注册，不改现有代码。

### Q12. 封闭经济长期运行时，资金从哪里进入和退出？

当前成交严格守恒股票与交易双方资金，但佣金、过户费和印花税会持续退出参与者账户。尚未决定的
外部现金流包括企业利润与分红、基金申购赎回、居民收入、融资、回购和退市清算。任何方案都必须：

- 明确资金来源、接收方、发生频率和会计记录；
- 与企业基本面、持股和游戏事件相联系，而不是按日给 NPC 隐藏补钱；
- 保持可存档、同 seed 可重放，并允许玩家在 UI 中查到资金变化原因。

**⏳ 未解决。** 在单独 ADR 获得确认前，策略层不得承担货币发行职责。

---

## ✅ 已解决（参考）

| 问题 | 决策 | ADR |
|------|------|-----|
| Q1 engine 语言 | Rust → WASM | [ADR-0002](decisions/0002-engine-rust-wasm.md) |
| Q2 后端语言 | Rust | [ADR-0003](decisions/0003-backend-rust.md) |
| Q5 前端状态管理 | Redux Toolkit | [ADR-0004](decisions/0004-frontend-state-redux-toolkit.md) |
| Q3 许可证 | MIT | [ADR-0007](decisions/0007-three-deployment-frontend-framework.md) |
| Q4 包管理器 | pnpm workspace | [ADR-0007](decisions/0007-three-deployment-frontend-framework.md) |
| Q8 市场确定性 | 种子化 PRNG 存 Session，可重放 | [ADR-0005](decisions/0005-unified-engine-three-deployments.md) |
| Q9 核心玩法循环 | tick步进 + 全订单簿撮合 + 对外固定 T+1 + 统一账户 | [ADR-0005](decisions/0005-unified-engine-three-deployments.md) |
| Q11 NPC AI 行为 | 独立策略模块 + Strategy trait + 每实例参数 + 可插拔 | [ADR-0006](decisions/0006-npc-strategy-module.md) |
| 三宿主通信抽象 | 统一 HostUpdate 语义，保留 Worker/WS/Tauri 传输差异 | [ADR-0010](decisions/0010-unified-host-protocol-and-local-refresh.md) |

（其余问题解决时，继续在此登记。）
