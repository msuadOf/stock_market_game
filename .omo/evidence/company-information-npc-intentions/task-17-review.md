# Task 17 Independent Review — 账户身份、主导风格与分析权重解耦

VERDICT: APPROVE

- Commit under review: `02e484d` (`feat(engine): 分离NPC身份与混合分析能力`), branch `codex/feat/web-ui-polish`
- Reviewer: independent subagent（未参与实现），按 AGENTS.md「大A语义与独立复核门禁」执行
- Review date: 2026-09-11；review 时 HEAD 为 `91a8d55`（含后续 task 8 提交，见验证日志说明）

## 0. Diff 范围清点（最小性前置事实）

`git show 02e484d --stat`：7 文件、1225 insertions、0 deletions，纯新增：

| 文件 | 行数 | 性质 |
|---|---|---|
| `packages/engine/src/strategy/analysis_profile.rs` | 276（新增） | K5 类型 + 恢复边界 |
| `packages/engine/src/strategy/factory_profiles.rs` | 161（新增） | K5 派生/归一工厂 |
| `packages/engine/src/strategy/mod.rs` | +9 | 2 个 `mod` 声明 + `pub use` 再导出，契约区零改动 |
| `packages/engine/tests/analysis_profiles/{main,gold,invariants,failures}.rs` | 68/220/281/210（新增） | 24 个测试 |

`git show 02e484d --name-only` 确认无 strategy/ + tests/analysis_profiles/ 之外的任何触碰。
无删改行 ⇒ 既有策略语义零改动（必要性与 Q2 的基础）。

## 1. AGENTS.md 问题一：大A语义符合性与依据可靠（此处= K5 数学契约与既有策略语义未被破坏）

逐项复核，全部符合：

**1.1 权重表逐字核对（factory_profiles.rs:19-43 vs 计划 K5 节文本）**
13 风格 × 5 权重（基本面/趋势/量价/技术/成本经历，bp）逐一对照计划原文，完全一致：
LongTerm 5000/1000/500/1000/2500；DipBuyer 1500/1000/1000/1500/5000；Momentum 500/4500/2000/2000/1000；
Noise 0/1000/1000/1000/7000；Panic 0/2000/1000/1000/6000；Dormant 1000/1000/500/500/7000；
机构 DeepValue 7000/500/500/500/1500、Growth 7000/1000/500/500/1000、Balanced 6000/1000/1000/1000/1000、
Defensive 6500/500/500/500/2000、ActiveTrader 0/4500/2500/2000/1000；
游资 Momentum 0/5000/2500/2000/500、Reversal 0/1500/2500/3000/3000。
13 行各自总和均 = 10000（手工验算 + `gold::default_table_matches_plan_verbatim` 双重锁定）。

**1.2 派生契约**
- 主导风格是输入：`derive_analysis_profile(&StrategyProfile, AccountId, &mut dyn Rng)` 只读 profile，绝不重抽；`invariants::dominant_style_is_not_resampled_per_derivation` 锁定方法链接与零/非零模式与 RNG 序列无关。
- 每个非零权重恰好采样一次：`sample_nonzero_weight`（factory_profiles.rs:134-139）零基线早退不碰 RNG；`main.rs` 的 `StrictSeqRng` 超抽即 panic。`gold::institution_active_trader_zero_fundamental_worked_example` 只供给 4 个 f64（ActiveTrader 有 4 个非零权重）且断言 `draws_used()==4` —— 严格抽次契约测试存在且有效。
- 倍率 [0.6,1.4]：`sample_multiplier_bp`（128-132）= 6000 + `min(next_f64()*8001, 8000)` → 闭区间 6000..=14000 整数 bp；映射式与 sampling.rs `sample_u64_inclusive` 逐字同构（house 约定 `[lo, lo+width)` + 浮点边界 clamp），非新发明语义。

**1.3 最大余数归一（factory_profiles.rs:87-124）**
floor + 按余数降序 +1；`order.sort_by(remainders[b].cmp(&remainders[a]).then(a.cmp(&b)))` —— 同余按数组下标即字段声明顺序（基本面→趋势→量价→技术→成本经历）破同分。
零保持零的数学保证成立：Σremainder = total×deficit 且每个正余数 < total ⇒ 正余数字段数 > deficit ⇒ +1 到不了余 0 字段（我独立复算了该论证）。
tie 金样 `largest_remainder_normalize([1,1,1,0,0], 10000)` → `[3334,3333,3333,0,0]` 被断言（invariants.rs:252-263），整除路径 deficit=0 亦被断言。
i128 中间量：raw_i ≤ 7000×14000 = 98M，×target(10000) = 9.8e11，远在 i128 内，无溢出路径。

**1.4 三个手算金样独立复算（reviewer 手工重演，全部吻合）**
- LongTerm + AccountId(9)，倍率 [6000,14000,10000,8000,12000] → raw [30M,14M,5M,8M,30M]，S=87M，floor Σ=9998，deficit=2 归给量价(余62M)/技术(余47M) → **[3448,1609,575,920,3448]**，方法=现金流(9 奇)。与 gold.rs:87-93 一致。
- ActiveTrader + AccountId(2)，4 抽 → raw [27M,20M,20M,12M]，S=79M，floor Σ=9997，deficit=3 归给成本经历(78M)/趋势(57M)/量价(51M，同余按序胜技术) → **[0,3418,2532,2531,1519]**，方法=None。与 gold.rs:111-113 一致。
- Reversal + AccountId(4)，raw [9M,25M,42M,24M]，S=100M 整除，deficit=0 → **[0,900,2500,4200,2400]**。与 gold.rs:128 一致。

**1.5 方法链接（K5a）**
- Bank/Insurance → EquityRoe 恒定：`method_for_company_kind`（analysis_profile.rs:179-187）按 kind 规则计算，非逐实例抽样；零基本权重时对一切行业返回 None（身份不授予能力），由 `zero_fundamental_weight_survives_sampling_and_disables_the_analysis` 在 13 风格 × 32 seed × 4 行业网格锁定。
- 工商/地产：机构 DeepValue/Defensive→EarningsMultiple、Growth→CashFlow（factory_profiles.rs:147-150）；其余（Balanced/ActiveTrader/散户/游资）按 `account_id.0.is_multiple_of(2)`：偶→EarningsMultiple、奇→CashFlow（151-159）。
- 方法槽 Some ⇔ fundamental_bp>0：`AnalysisProfile::new`（155-174）双向 match 强制。
- EquityRoe 进槽位被拒：构造与存档恢复两路均触发 `IllegalMethodKindMapping`（failures.rs:53-65、124-137）。

**1.6 持久化边界**
`PersistedAnalysisProfile` 带 `deny_unknown_fields`（analysis_profile.rs:212）；`AnalysisProfile` 经 serde `into`/`try_from` DTO，恢复路径全部走 `AnalysisWeights::new` + `AnalysisProfile::new` 校验。往返测试（invariants.rs:267-281）含未知字段拒绝；负/全零/总和不符在 failures.rs 全部类型化触发。

**1.7 无 common-V 复活**
两份新源文件通读：`AnalysisProfile` 仅含 5 权重 + 方法槽，无任何公允价/目标价/市场均值字段。既有 `StockView.fundamental_value`（隐藏 V，编排层授权可见）未被本 diff 触碰。

**1.8 RNG 流回归（关键探针）**
- diff 不含任何 session/ 文件；grep 全仓确认 `derive_analysis_profile`/`default_analysis_weights`/`largest_remainder_normalize`/`PersistedAnalysisProfile` 的引用仅存在于 strategy 模块内部与 analysis_profiles 测试二进制 —— 会话层零调用点，`populate_npcs` 未触碰。
- `derive_analysis_profile` 接受注入 `&mut dyn Rng`，未内联任何会话 RNG。
- 全量套件中 `extraction_replay` 3/3 绿（`identical_construction_replays_bit_identical` 等），session 112 过含 `step_is_deterministic_same_seed`、`save_restore_preserves_rng_and_strategy_price_history` —— 字节锚点未移位。

**结论**：K5 数学契约逐项成立，既有策略语义零漂移。依据可靠（计划文本 + 交易所无关的纯数学域，不涉及真实 A 股制度字段，无 trading-rules.md 登记义务）。

## 2. AGENTS.md 问题二：必要性与最小范围

- 改动 = 计划任务 17 的全部内容（类型 + 工厂派生 + 测试 + mod 接线），1225 行纯新增、0 删除。
- 唯一的既有文件触碰是 mod.rs 的 +9 行（2 个模块声明 + pub use），属于任务 3 既定架构（mod.rs=contract+re-export）的标准接线。
- 未夹带无关重构、未改 sampling.rs（倍率映射式复用而非抽取共享 —— 反而保持了最小足迹）、未提前做任务 18-20/22-23/26 的接线（ts_rs derive 与 populate_npcs 接线均按计划留给后续任务，learnings 已登记）。
- 无超范围内容。**符合必要性与最小范围。**

## 3. AGENTS.md 问题三：遗漏边界测试、跨层语义漂移、不必要复杂度

- **边界测试覆盖**：8/8 公共错误变体均有触发测试（Task-21 教训达标）——NegativeWeight、AllZeroWeights、WeightSumMismatch、IllegalMethodKindMapping、MethodWithoutFundamentalWeight、FundamentalWeightWithoutMethod、UnknownFundamentalMethod、InvalidNormalizationInput；成功路径（new/derive/normalize/serde 往返/to_persisted）全部可达且被测。未发现遗漏的边界：零权重跨行业、32-seed 网格、同余破同分、整除路径、恢复冲突四象限（负/未知 id/equity_roe/方法-权重冲突）均已覆盖。
- **跨层漂移**：无。档案是 engine 内部层概念，web/server/wasm 未接触（cargo check --workspace 过）；mod.rs 只加再导出不改契约；`Rng` trait 本就在 strategy/mod.rs 公有（`super::Rng` 合法）。
- **复杂度**：无多余抽象。i128 归一防溢出、StrictSeqRng 抽次证明、DTO 边界集中校验均为必要防御，不是过度设计。

## 4. 注册偏离裁决（issues.md §W3-Task 17，逐条）

1. **奇偶方向（偶→盈利倍数、奇→现金流）**：**接受**。计划 K5a 只说「按稳定 AccountId 奇偶」未定方向；本任务在 doc 注释（factory_profiles.rs:52）、learnings 与测试三处文档化且方向可测试（invariants.rs:72-135 用 8/9 双账号锁定）。二选一必须有人定，无语义优劣依据反对该选择。
2. **行业-方法链接放 `method_for_company_kind(CompanyKind)` 纯函数而非档案字段**：**接受**。银行/保险是规则不是逐实例抽样；若存字段则存档可被篡改注入 EquityRoe，而当前设计使恢复边界能以 `IllegalMethodKindMapping` 拒绝任何槽内 EquityRoe（有测试）。更小的攻击面，与「能力由实体表达、规则由代码表达」一致。
3. **文档化 expect**：**接受**。实测 3 个调用点分布在 2 处（登记口径「两处」按函数计；`default_analysis_weights` 1 处 + 归一函数 floor/deficit 2 处）——登记措辞与调用点计数有 1 处粒度差，非诚实性问题。每处均有锁定依据：K5 表 13 行总和被 `default_table_matches_plan_verbatim` 断言 =10000；floor ≤ 已校验 target ≤ u32::MAX；deficit < 正余数字段数 ≤ 5（我复算过证明）。符合 plans/ 先例。
4. **warning band**：**接受并沿用约束**。review 实测纯行（剔除注释/空行，含属性行）：analysis_profile.rs 217、invariants.rs 249（登记值 201/241 因属性行计法差异，两口径均 <250）。约束有效：任务 18-20 不得扩展这两个文件，task 18 须按计划拆 fundamental/ 子模块。

## 5. 验证日志（reviewer 重跑，含命令、退出码、计数）

| 命令 | 结果 | 退出码 |
|---|---|---|
| `git show 02e484d --stat` / `--name-only` / 全量 diff | 7 文件 / 1225+/0-，范围=策略+测试 | 0 |
| `cargo test -p engine --test analysis_profiles` | **24 passed / 0 failed / 0 ignored**（期望 24/24 ✓） | 0 |
| `cargo test -p engine` | **614 passed / 0 failed / 4 ignored**（含 extraction_replay 3/3 绿） | 0 |
| `cargo check --workspace` | 5 crate 全部 Finished，无错误 | 0 |

计数说明：任务时点证据（task-17-full-suite.txt）为 585 过/0 败/4 忽略（基线 561+24）；本次 review 在 HEAD `91a8d55`（task 8 的 `industrial_accounting` 29 测试已随后续提交入库）重跑得 614 = 585+29，仍满足 ≥585/0/4 门禁，且 4 ignored 即 4 个 release 模式压测门（50k/100k/20k accounts + 20k 经历成本探针），与既知一致。三份任务证据（happy 24/24、failure-path 10/10、full-suite 585/0/4）与本次重跑自洽。

## 6. 非阻塞观察（不构成 REJECT，无需后续任务认领）

1. `InvalidNormalizationInput { total }` 在「负输入」分支携带的是数组总和（如 `[1,-1,0,0,0]` 与全零都报 total 0），诊断精度略低于按字段报告；但两分支均为显式类型化拒绝（铁律二达标），该函数仅工厂内用、输入恒合法，实际不可达。记录备查，不要求修改。
2. `sample_multiplier_bp` 与 sampling.rs `sample_u64_inclusive` 的 clamp 惯用法是 4 行式重复而非共享助手 —— 保持 sampling.rs 未触碰是最小足迹的正确取舍，若任务 18-20 再需要同式采样可考虑统一。
3. issues.md 登记「两处 expect」实为 2 函数 3 调用点（floor/deficit 分列两行）——登记粒度问题，内容属实。

## 7. 结论

三项 AGENTS.md 问题全部通过；4 项注册偏离全部裁决接受；K5 表、派生契约、归一数学、方法链接、持久化边界、无 common-V、RNG 流不变量逐项验证成立；三条验证命令全绿。**APPROVE**。
