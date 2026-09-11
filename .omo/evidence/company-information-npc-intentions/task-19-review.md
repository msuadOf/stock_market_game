VERDICT: APPROVE

# Task 19 独立复核（大 A 语义与独立复核门禁）

- 复核对象：commit `b1f638d` `feat(engine): 扩展个人参照与具名技术信号`（10 files, +1238, 0 deletions）
- 复核人：未实施该改动的独立 subagent（本轮全部命令由复核人亲自执行）
- 计划锚点：K5 行 134–135（具名技术信号按完整日 K；个人价格记忆上限与诚实区分）
- 复核日期：2026-09-11

## 0. 命令与结果清单（全部在隔离树执行，非主工作树）

| # | 命令 | 执行位置 | exit | 结果 |
|---|------|----------|------|------|
| 1 | `cargo test -p engine --test technical_memory` | worktree `wt-review-19` @ b1f638d (detached, clean) | 0 | **33 passed / 0 failed / 0 ignored**（13 gold + 6 memory + 14 failures，与 evidence happy.txt 逐名一致） |
| 2 | `cargo test -p engine`（全量） | 同上 | 0 | **649 passed / 0 failed / 4 ignored**（24 个套件逐项核过；含 behavior 35/observations 10 原套件不变绿） |
| 3 | `cargo check --workspace` | 同上 | 101→重试遇 worktree 被外部移除 | 首次为瞬态文件系统错误（写 aho-corasick fingerprint 时 os error 3，疑似 AV 竞争）；重试时 worktree 已被外部清理（复核人未删除，遵循 MUST NOT） |
| 4 | `cargo check --workspace` | `git archive b1f638d` 导出树（与 commit 内容逐字节等价） | 101 | **唯一失败 = Tauri 桌面壳**：`tauri::generate_context!` panic，`frontendDist "../../web/dist"` 不存在。该目录被 `.gitignore`（`apps/*/dist/`）排除，任何干净检出/worktree 都没有它——环境性、先在 |
| 5 | `cargo check -p engine -p server -p web-wasm -p engine-gpu` | 导出树 | 0 | 4 个成员全绿（覆盖 workspace 中除桌面壳外全部成员） |
| 6 | `cargo check -p stock-market-game`（补 stub `apps/web/dist/index.html` 后） | 导出树 | 0 | 证明 #4 的失败 100% 归因于缺失的 gitignored 前端产物，与 b1f638d 改动无关 |
| 7 | `cargo test -p engine --lib` | 导出树 | 0 | 83 passed（附带证明见 §3-新发现） |
| 8 | `cargo test -p engine export_bindings -- --list` | 导出树 | 0 | 含 `experience::price_memory::export_bindings_{personal,stock}pricememory` |

原始输出：`C:\Users\msuad\AppData\Local\Temp\opencode\t19r-{1..8}.txt` + `t19r-diff.txt`（全量 diff）。

**关于 660 vs 649**：任务简报预期 660 含并发 bank 噪声；b1f638d 提交树中不存在任何 `tests/bank_accounting` 文件（`git ls-tree` 核实；`git show b1f638d --stat` 亦无 bank 文件）。干净树权威数字即 **649 = worker 记录的 660（共享树）− 11（并发任务未提交 bank 测试）**，与 worker evidence 的孤立树复跑完全一致。0 failed 为判据，达成。

**worktree 事件（如实记录）**：两条测试命令完成后、workspace check 进行中，`wt-review-19` 被外部移除（`Test-Path` False）。复核未丢弃任何证据：#1/#2 结果与逐套件清单已捕获；#4–#8 改在 `git archive b1f638d` 导出树（内容与 commit 树等价，无 git 元数据）完成，且额外覆盖了 worktree 阶段未能完成的 workspace check。

## 1. 大 A / 契约语义符合性与依据（AGENTS.md 问 1）

**内核契约（`strategy/technical.rs`，239 行）——符合**
- 纯数学内核：唯一依赖 `crate::Money`；不 import 日历/session/账户。窗口一律完整交易日数（SMA_SHORT_WINDOW=20 / SMA_LONG_WINDOW=60 / RSI_WINDOW=14 / ATR_WINDOW=14 常量具名）。
- 全整数分运算，i128 累加，三处除法（SMA 均值、RSI 百分比、ATR 均值）统一 `div_round_half_even`，各函数文档写明舍入规则。
- `InsufficientHistory { available, required }` 携带可用长度（SMA 19/20、59/60；RSI 14/15；ATR 14/15 均有断言）；无任何零填充/虚构趋势路径。
- 与 `accounting/amount.rs:228` 的同名函数逐行比对：算法等价（unsigned_abs、半偶进位、符号对称还原），仅 `quotient % 2 == 1` 与 `!quotient.is_multiple_of(2)` 措辞差异。

**RSI 零分母规则——精确按计划**
- 14 个收盘（13 个涨跌样本）→ 类型化拒绝 `{available:14, required:15}`；15 个全平收盘 → 精确 50 + `all_flat=true` + `valid_samples=14`。两侧对照测试就在同一用例内（`failures.rs:32-49`），另 gold.rs:118-132 全字段断言。分母为零 ⇔ Σgain=Σloss=0 ⇔ 窗口全平，数学上无歧义。
- 公式 `round_half_even(100×Σgain/(Σgain+Σloss))`，两端 /14 平均分母约去；金样 66.67→67、62.5→62（半偶）、纯涨 100/纯跌 0、均衡 50 均手算复核无误。

**Cutler（简单平均）RSI vs Wilder 递推——裁决：接受**
- 计划 K5 只说「RSI14 按完整日 K 计算」，未强制 Wilder。worker 的文档化理由（technical.rs:8-11）：引擎日 K 历史有保留上限，Wilder 递推需自序列首样本起连续平滑，截断窗口下无法确定性重建；简单平均是无状态重算，存档恢复前后逐位一致、可手算复核。这与项目确定性回放 + 手算可核的价值一致（Cutler's RSI 本身是公认变体）。ATR 同理采用简单平均并显式文档（148-151），两指标方法学一致。**接受，无保留**。

**ATR——符合**
- 需 15 根日 K（每个 TR 需前收，`window.windows(2)` 取最近 14 个 TR，不虚构首根前收；`failures.rs:52-62` 14 根→拒绝）。
- 只供风险/执行：约束写在类型文档（technical.rs:69-70）与观测字段文档（observation/technical.rs:40-41）；全库 grep 确认无任何消费者从 ATR 派生分数/方向信号（当前唯一消费者是观测层透传）。

**观测层（`observation/technical.rs`，103 行）——符合**
- 无成交日（volume==0）先结构性校验后排除；排除后样本数如实计入 `valid_sample_count`；20 日全无成交 → 各指标 `InsufficientHistory{0,req}`、观测本身 Ok（failures.rs:98-125）——「无真实已发生行情」与「缺数据」两种语义不混淆。
- 缺日 `TradingDayGap`、非递增 `NonIncreasingDay`、当日/未来 K `DailyBarNotBeforeObservation`（`>= as_of` 拒绝，无 lookahead）全部类型化且有测试；内核价格拒绝按指标独立透传（failures.rs:241-261，4 指标各断言一次）。
- 停牌建模为「存在于连续日序、volume=0、不构成样本」——与 K5「真实已发生行情」一致，未篡改 docs/trading-rules.md 范围内的任何交易制度。

**个人价格记忆（`experience/price_memory.rs`，183 行）——符合 K5 行 135 全部要点**
- 上限 = 受保护（持仓∪活跃计划，调用方组合）+ `MAX_UNHELD_WATCHLIST_STOCKS=8`（复用 experience.rs:17 既有常量）；驱逐按 `last_touched` 降序 + StockCode 稳定破同分，与既有 `prune_watchlist`（experience.rs:279-294）逐字符同构——一致性声明经核实为真。
- 首次/最近观察价、首次观察以来高低（只累计本人所见）、时间窗 [首次,最近] 分钟、来源区分（本人观察 vs 公开历史读取事件，读取记录时间戳+次数、**不触碰任何亲历锚点**）；读取从未观察过的股票 → `UnobservedStock` 类型化拒绝且状态不变（不虚构亲历）；读取刷新 recency 参与驱逐；observe/read 双向时间回拨拒绝；serde `deny_unknown_fields` 往返保持。
- 「来源」以结构化字段区分本人观察/公开读取，文档标注任务 22/25 扩展为显式来源枚举的接缝——本任务范围内足够。

**分层纪律——无违规**：kernel（纯数学，无日历/账户）→ observation（对市场时间校验+过滤）→ memory（纯个人状态）。三个新文件 import 面分别仅为 `crate::Money`、`super + crate::strategy`、`super + crate::{Money,StockCode}`；零基本面耦合（本任务在无任何基本面数据下完整成立）、无 common-V 复活、无新增共享可变状态。

## 2. 必要性与最小范围（AGENTS.md 问 2）

- 全 commit 恰好 10 文件、+1238、**0 删除**，全部落在 packages/engine 的 strategy/observation/experience 及其测试目录，无一行越界。
- 既有信号保持：`observation.rs` 仅 +10（3 行模块文档 + `mod technical` + 再导出 + 1 个新错误变体）；`experience.rs` 仅 +7（见 §4-偏差1 的计数更正）；`strategy/mod.rs` 仅 +5（mod 声明 + pub use）；`behavior.rs`、`tests/behavior.rs`、`tests/observations.rs` 完全未触碰——全量套件中 behavior 35、observations 10 原样绿。
- TDD 证据链完整：red（10 个 E0432/E0599，编译期不可伪绿）→ happy 33/33 → failure 14/14 独立过滤跑，三份 evidence 文件与我的独立复跑逐名一致。
- 复核中逐一手算金样（SMA 1050/3050、半偶 100.5→100 与 101.5→102、RSI 67/62、ATR 25/2、短跌长涨 SMA20=1065/SMA60=1285 同时成立）全部吻合——测试不是自证循环。

**结论：为 K5 所必需，范围最小。**

## 3. 遗漏边界测试 / 跨层漂移 / 不必要复杂度（AGENTS.md 问 3）

边界覆盖逐项核过：恰窗口长度、离群旧样本不入窗、RSI/ATR 超 required 长历史、16 根只取最近 14 TR、空历史、全无成交、缺日/乱序/当日/未来、非正价（带 index）、倒挂 high<low、记忆时间回拨（双操作）、非正价、未观察股票读取（+状态不变断言）、驱逐三态（保护/同分破平/读取刷新 recency）、serde 往返。**未发现遗漏的边界测试。**

**新发现（非阻断，登记给任务 29）——Rust→TS 绑定积压，task 19 新增 2 个成员**
- 仓库约定把 ts_rs 生成物提交在 `apps/web/src/types/generated/`（.cargo/config.toml 设 `TS_RS_EXPORT_DIR`），CI 有 "Check generated Rust -> TypeScript bindings" 步骤（ci.yml:104-105，`node scripts/check-generated-types.mjs`，对 untracked/modified 即 fail）。
- 实证：在 b1f638d 干净树跑 `cargo test -p engine`（lib 含 export tests）会生成 `StockPriceMemory.ts`、`PersonalPriceMemory.ts`，且 **b1f638d 未提交它们**（提交树 Test-Path False → 跑测试后 True）。
- 但这是 W3 波次性先在债务：b1f638d 处重新生成共 **24 个**未提交绑定，其中 22 个来自更早提交（Plan*/TradingPlan 系列自 6cdc219、Civil*/CivilDate 自 bf00279/16702cd、OpinionSource 自 02e484d）——即 CI 该步在 b1f638d 之前的多笔提交早已红。计划把 `pnpm types:generate`/`types:check` 的 QA 明确派给任务 29（plan 行 531-533，K7 web 接线）；task 19 遵循了波次内先例（engine 任务不同步绑定），当前 web 无任何消费者 import 这两个新类型，运行时不破坏任何东西。
- **裁决：非 task 19 缺陷、不阻断**；已按复核授权追加至 issues.md，供任务 29 清扫时一并收编（含全部 24 个文件清单的产生机制）。

**复杂度**：无越界抽象、无防御性冗余（observation.rs:67-73 的 checked_add 溢出分支实为不可达但类型化非静默，可忽略）；`div_round_half_even` 受控副本与 `matches!` 断言两项见 §4。

## 4. issues.md §"W3-Task 19 记录的问题" 逐项裁决

1. **共享树三次并发交织（如实登记）** — 接受。bank 文件不在本 commit（stat 核实）；孤立树 649/0/4 与其记录的算术吻合。中间态以 22 套件枚举作证据的处理诚实且充分。
2. **超 250 行文件的 mandated 接线增量** — 接受。`observation.rs` 实际 +10、`experience.rs` 实际 **+7**（notepad 记 +9，系笔误：git stat 为准，diff 内容为 3 行文档+空行+mod+空行+pub use）。登记中的 518→528/290→299 纯行数与我的口径（528→534 / 304→306）不完全一致，属计数口径差异；实质（两文件远超 250 纯行、增量是规格要求的最小接线、不做 mod.rs 目录化重构）成立。**计数更正入档，不改变结论。**
3. **ObservationError 无 PartialEq → 4 处 `matches!` 字段绑定断言** — 接受。逐处核过（failures.rs TradingDayGap/NonIncreasingDay/DailyBarNotBeforeObservation×2，共 4 处，与登记数一致）。Rust 字段绑定模式在断言强度上与 `assert_eq!` 精确值等价（任一字段不符即失败），仅损失失败时的 diff 输出，属装饰性差异。MoneyError 无 PartialEq 且 money.rs 冻结，把 derive 决定留给 owner（27/29）是正确的所有权纪律。其余断言（TechnicalError/PriceMemoryError）确为 `assert_eq!`。
4. **clippy 既有缺陷** — 按指令不复核不阻断（behavior/decision.rs:239 属任务 42，bank doc 警告属并发任务范围）。
5. **div_round_half_even 受控副本** — 接受，附两更正：(a) 与 amount.rs 副本逐行算法等价已核实，交叉引用注释属实；(b) b1f638d 提交树中**只有 2 份**该函数（amount.rs、strategy/technical.rs；`git grep fn div_round_half_even` 核实）——notepad 所称 industrial::loans/accounting::inventory 副本在提交树不存在（属并发任务未提交代码）。若并发任务落地使副本数继续增长，应开一张合并跟进票（单一 owner、pub(crate) 收编），本轮非阻断。

## 5. 结论

三项门禁问题全部通过：语义与计划 K5 精确一致且有官方/数学可核依据；改动为需求所必需且范围最小（0 删除、0 越界文件）；未发现遗漏边界测试，唯一跨层观察（TS 绑定）为波次先在债务、计划已派给任务 29，已登记。五项登记偏差全部裁决接受（含两处计数/表述更正）。

**残余风险（低）**：① TS 绑定积压延续到任务 29 前CI 该步保持红（含本笔 +2）；② `matches!` 断言失败时无 diff 输出，纯工效损失；③ 若后续任务落地使 `div_round_half_even` 副本 ≥3，需合并票据。均不阻断本笔合并。
