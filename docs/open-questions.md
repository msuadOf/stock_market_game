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

**✅ 已解决（2026-06-29）：pnpm workspace。** 本机已装 pnpm 11.9；`pnpm-workspace.yaml` 已就位。详见 [ADR-0007](decisions/0007-three-deployment-frontend-framework.md) §7。

---

### Q5. 前端状态管理方案？

**✅ 已解决（2026-06-28）：Redux Toolkit。** 详见 [ADR-0004](decisions/0004-frontend-state-redux-toolkit.md)。

---

## 🟡 非阻塞型（Stage 1 可延后，但值得早想）

### Q6. UI 语言 / i18n 策略？
- 纯中文界面？中文优先 + 英文 i18n？还是一开始就内置 i18n 框架？
- 倾向：中文优先，但代码层面用 i18n key（避免硬编码字符串），便于后期补英文。

### Q7. 存档与持久化的范围？
- Stage 1 存什么？仅"当前游戏进度"，还是含"成就/历史交易记录"？
- 单存档还是多存档槽？是否需要"云同步"占位（为 Stage 2）？

### Q8. 市场模拟的确定性？
- 市场行情是否需要"可回放/可复现"（便于测试 + 公平）？
- 若需要，随机数必须可注入种子（呼应 TDD：测试要能断言确定性结果）。

**✅ 已解决（2026-06-29）：种子化 PRNG（SplitMix64）存入 Session，可注入、可序列化、可重放。** 详见 [ADR-0005](decisions/0005-unified-engine-three-deployments.md) §4。
> 注意：随 ADR-0005 定调为「撮合驱动价格」，RNG 的用途从原「行情随机」迁移到「**NPC 下单决策的随机**」。

### Q9. 游戏的"核心玩法循环"边界？
- 第一版（Stage 1）最小可玩 = 哪些功能？（买卖、行情、持仓、盈亏？是否含事件/新闻、止盈止损、多市场？）
- 这决定了 engine 第一批要 TDD 的模块清单。

**✅ 已解决（2026-06-29）：tick 步进 + 宿主驱动；全订单簿撮合；T+0/T+1 可配置；统一账户（NPC=玩家同构）+ 共享盘口撮合驱动价格。** 详见 [ADR-0005](decisions/0005-unified-engine-three-deployments.md)。下一批 engine 模块（按依赖序）：account → orderbook → market → session/save。

### Q10. 视觉风格与设计系统？
- 极简文字 / 数据图表 / 卡通拟物？是否需要设计系统（色板、组件库）？
- 影响 UI 测试与组件结构。

---

### Q11. NPC（散户 / 机构 / 游资）的 AI 行为模型？ ⭐ 新增（阻塞 market/account 的 NPC 部分）

ADR-0005 定调「统一账户 + 撮合驱动价格」后，NPC 是**主动挂单的 AI 参与者**（与玩家平权进同一 orderbook）。但三类 NPC 各自的**策略算法尚未敲定**：

- 散户（retail）：追涨杀跌？噪音交易？受市场情绪驱动？
- 机构（inst）：大单、方向性、可能护盘/砸盘？拆单？
- 游资（hot）：短线投机、拉抬/打压、快进快出？

**✅ 已解决（2026-06-29）：策略为独立模块 + Strategy trait + 每实例独立参数 + 可插拔扩展。** 首批三策略：散户=ZI噪音、机构=基本面价值(隐藏公允价V轨道+机构各异目标价)、游资=动量。玩家不走 Strategy。详见 [ADR-0006](decisions/0006-npc-strategy-module.md)。
> 研究基础：市场微观结构 + agent-based 模拟文献（ZI 泊松模型、基本面/趋势双因子、噪声交易者）。
> 后续每加新策略 = 新增 trait 实现 + 单测 + 工厂注册，不改现有代码。

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
| Q9 核心玩法循环 | tick步进 + 全订单簿撮合 + T+0/T1可配 + 统一账户 | [ADR-0005](decisions/0005-unified-engine-three-deployments.md) |
| Q11 NPC AI 行为 | 独立策略模块 + Strategy trait + 每实例参数 + 可插拔 | [ADR-0006](decisions/0006-npc-strategy-module.md) |

（其余问题解决时，继续在此登记。）
