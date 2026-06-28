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

**Claude 倾向：** 维持 MIT。

> ⏳ 待 msuad 确认。→ 解决后产出/更新 ADR。

---

### Q4. 包管理器与 monorepo 工具？

**选项：**
- **A. npm workspace** — 零额外工具，本机已具备；功能够用。
- **B. pnpm workspace** — 更快、磁盘省（硬链接）、monorepo 体验更好；需先 `npm i -g pnpm`。

**Claude 倾向：** A 起步（降低门槛），若后续包变多 / CI 变慢再迁 pnpm。

> ⏳ 待 msuad 定夺。

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

### Q9. 游戏的"核心玩法循环"边界？
- 第一版（Stage 1）最小可玩 = 哪些功能？（买卖、行情、持仓、盈亏？是否含事件/新闻、止盈止损、多市场？）
- 这决定了 engine 第一批要 TDD 的模块清单。

### Q10. 视觉风格与设计系统？
- 极简文字 / 数据图表 / 卡通拟物？是否需要设计系统（色板、组件库）？
- 影响 UI 测试与组件结构。

---

## ✅ 已解决（参考）

| 问题 | 决策 | ADR |
|------|------|-----|
| Q1 engine 语言 | Rust → WASM | [ADR-0002](decisions/0002-engine-rust-wasm.md) |
| Q2 后端语言 | Rust | [ADR-0003](decisions/0003-backend-rust.md) |
| Q5 前端状态管理 | Redux Toolkit | [ADR-0004](decisions/0004-frontend-state-redux-toolkit.md) |

（其余问题解决时，继续在此登记。）
