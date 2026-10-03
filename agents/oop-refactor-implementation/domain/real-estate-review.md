# 地产 owner 迁移独立复核

- 日期：2026-10-03。
- reviewer：`/root/implement_domain/review_real_estate`；未实施本批源码，未调用其他 subagent。
- baseline：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 范围：`packages/engine/src/company/real_estate/**` 相对 baseline 的完整 tracked diff，以及未跟踪的 `ownership_tests.rs` 全文；同时读取地产全部源码、相关生产 caller 和既有地产行为测试。
- 动作契约：completeness `action-index.md` 的 `domain-N04` 地产部分，以及 challenge `action-index.md` 的 `domain-R2-N22/N25/N26/N27`。未将已拒绝的交付协调对象恢复为本批任务。
- 最终结论：未发现生产代码引入的行为变化；初审唯一发现 RE-R1 已修复并完成再次静态复核，当前无未关闭的有效发现。本记录不表示运行测试通过。

## 有效发现

### RE-R1（P3）：早于开发日的完工接受行为未被 fixture 实际保护

- 位置：`packages/engine/src/company/real_estate/ownership_tests.rs:110` 的注释与下一行 `complete_project(..., date(1))`；fixture 首次开发投入位于同文件 `:63`，同样为 `date(1)`。
- 行为：当前用例只验证完工日等于开发日，并未验证完工日小于开发日。因此未来若在 `validate_complete` 中加入 `date < dev_started_on` guard，该用例仍可通过，与注释宣称的保护范围不符。
- 原因与必要性：本批是纯 owner 重构，父任务明确要求保留既有 earlier-date 接受集；此测试正在承担该保护，fixture 应实测该边界。
- 修复建议：只调整这个用例的首次开发日为 `date(2)`，保留土地日 `date(1)` 与完工日 `date(1)`；或另构造最小 fixture 验证严格早于开发日的完成事实。不得为了测试添加生产 guard，也无需修改其他共享 fixture 的计息基线。
- 当前状态：已关闭；实现方修复后，独立 reviewer 再次读取最终完整 baseline diff 与未跟踪测试全文，详见文末复核记录。

## 三项门禁

### 1. 大 A 语义与依据

已读 `AGENTS.md`、`docs/principles.md`、ADR-0002/0016/0019、`docs/open-questions.md`、`docs/architecture.md`、`docs/company-accounting.md` 与 `docs/trading-rules.md`。

本批仅改变 engine 内部职责归属，没有修改证券买卖、沪深/板块差异、申报单位、T+1、交易时段或投资者资金。`AccountingAmount` 仍以分计量，利率仍为 bp，`FractionUnits` 仍为 `1/3_650_000` 分，项目 `units` 仍是房屋套数；没有把套数等同股票 shares。

地产政策沿用 `docs/company-accounting.md` 2026-09-10 依据登记：CAS 14（2017）第四/十三条对应交付时控制权转移确认收入，第三十九条对应预收款先记负债。公司经营与投资者现金隔离沿用 ADR-0016/0019。本次未重新访问官方网页，不能称为 2026-10-03 全部准则重新取证。CAS 17 原文取证受阻的事实保持明确，开发/中断/完工的资本化窗口仍是版本化游戏假设，不宣称真实准则全部覆盖。

源码仍维持预售签约不产生分录、收款进入合同负债、交付确认收入并结转开发存货、尾款只清应收、不重复确认收入；无项目借款 `None` 全部费用化，完工后永久费用化。本批未新增需要重定官方交易制度的行为。

### 2. 需求必要性与最小范围

| 动作 | 核对结论 |
|---|---|
| `domain-N04` 地产部分 | `RealEstateConfig::check_opening_lines` 直接读取自己的 `opening_lines`；11 个拒绝科目、首个命中错误、政策/上限先于种子 guard 的顺序均不变。 |
| `domain-R2-N22` | 私有 `BorrowingCostAccrualPlan` 只持有单次调用的 split、分录、through/base 与项目汇总；未进入 serde、未复制持久子账，符合计划动作。 |
| `domain-R2-N25` | lifecycle 局部 guard 移入已有 `ProjectState`，preview resume 返回原 `Interruption`；无新生命周期 enum 或日期规则。成本算法和资本化窗口保持原 owner。 |
| `domain-R2-N26` | 全额付息预览、还本金额 guard 收回 `ProjectLoanState`；贷款目录、总账、授信和 Counterparty flow 仍归 Books。删除 map-level `apply_split` 后内联原 no-op/调用行为，无第二套贷款 aggregate。 |
| `domain-R2-N27` | collection/delivery 局部 guard 与 checked 未收余额收回 `PresaleContract`；可售套数和完工性仍由 Books 跨合同/项目协调，未创建额外预售 ledger 或交付 plan。 |

public API、持久化字段、serde derive/属性均未变化，无新增依赖。借款/项目/预售的稳定 id 仍由 Books 索引和向错误提供上下文，不复制进已有实体。新增计划对象的复杂度属于已授权 N22，未借机修复或扩大前置校验、原子性和长期经营范围。

### 3. 边界测试、跨层语义与复杂度

- `plan_accrual` 仍按 BTreeMap 合同顺序迭代，先拒绝回拨，再跳过零天数，再校验指定项目/分类天数；保持 None 的 `cap_days=0` 与原 ACT/365F 两条独立余数链。
- `build_accrual_entries` 对每份合同仍先资本化、后费用化；仅正金额分录消耗连续的 `base + entries.len()` source 槽，金额为零仍保留 split 并推进 carry/date。
- `accrue_interest` 仍按 `post → 所有 loan apply → checked project 汇总 → project apply` 执行。planning/ledger 失败不写子账；post 后 loan add 失败仍留下已提交 ledger/source；project 汇总溢出或 project apply panic 仍保留对应原有部分写入面。未以 plan 类型宣称全流程原子。
- `validate_development` 保留 completed → interrupted → 正金额；suspend 保留未开工 → 已完工 → 已暂停；resume 保留未暂停 → 日期非前进；complete 保留未开工 → 暂停 → 已完工。Books 的未知 id 与 Counterparty 首错仍在调用这些 guard 之前。
- collection 保留 delivered → 正金额 → checked remaining → 合同金额上限；delivery 保留合同存在 → 已交付 → 项目存在 → 完工性 → remaining/cost preview；偿付保留未知贷款先于 owner guard，以及还本正金额先于本金上限。
- 所有持久类型的字段/serde 接受集保持；包括既有可编辑子账事实和异常值，并未在抽取中新增 Deserialize 校验。`remaining_payment` 的调用点仍沿用原 `expect` 与原错误上下文。
- 新短测试静态覆盖同一项目多笔贷款 + None 混合、项目资本化累计、事件槽计数、planning 回拨与重复 source 提交失败完整不变、零金额双 carry/date、分段双链累计守恒、最后一套成本/数量归零、owner 只读 guard、post 后 loan overflow 的原部分失败。既有 gold/lifecycle/loans/capitalization 源码继续覆盖主要跨层行为与 serde 往返。
- RE-R1 是本轮唯一有效发现。source 精确逐条顺序以及 project 汇总/应用极端失败面主要依赖完整源码逐句对照，当前新增短用例没有对每一种异常分别运行证明；这属于本次验证覆盖说明，不宣称已证明任意异常原子性。

## 验证范围与限制

本复核只进行 read-only 源码、完整 diff 与文档分析。依任务限制，未执行 Cargo、编译、测试、Git 写操作或源码修改；仅新增本工作记录。测试是否编译、运行通过以及 TDD 红绿证据由实现方验证记录另行证明。没有对全仓其他并发任务改动、宿主运行和性能进行验收。

## RE-R1 修复后的最终复核

- 时间：2026-10-03；仍由未实施源码的同一 reviewer 独立复核。
- 复核范围：重新全文读取地产最终 9 个 tracked 文件相对 baseline 的全部 diff，及未跟踪 `ownership_tests.rs` 全文与实现记录；与初审内容对照，生产 diff 保持原内容。
- 修正位置：`ownership_tests.rs:90` 的测试使用局部 Fixture；`:96` 首次开发日为 `date(2)`，`:117` 完工日为 `date(1)`；`:119`–`:121` 分别断言两项日期和严格早于关系。共享 `developing()` 仍为原开发日期，因此计息、分段双链与成本 fixture 不受影响。
- 结论：日期关系现在实际进入 API 调用和结果状态断言，RE-R1 已完整关闭。既有 guard、错误上下文、serde 接受集合、计划分录 source 与 post 后失败顺序保持；未发现新的边界、跨层或不必要复杂度问题。
- 冻结测试内容 SHA-256：`6fede901143cbf4f97b1a13d56d58680677be8bd5dcc8088dce8aff5cc65fc03`。
- 运行限制：本 reviewer 未运行 Cargo、产品测试或编译；实现方记录的 rustfmt / diff 检查成功不替代测试运行。独立静态审查门禁已满足，编译/测试门禁仍须主协调者集中执行并据实回填。

## 最终版本绑定补录

2026-10-03，由 canonical reviewer `/root/implement_domain/review_real_estate` 本人执行
read-only 核对，未由 manager 代写审核 hash。将当前完整 tracked diff 原文与本人修复后
最终复核时保存的两段完整 diff 原文合并结果逐字节比较，结果一致；这次取证前后再次
读取完整 diff，结果也一致。9 个 tracked 生产文件仍对应已审版本，未跟踪测试的最终
SHA-256 也与上次复核相同。本次仅补工作记录，未改源码、未运行 Cargo、编译或测试。

- baseline：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 完整 tracked diff 生成命令：`git diff b89afb3346743a4b4fccf26c9ac9ff108595f696 -- packages/engine/src/company/real_estate`。
- 完整 tracked diff 字节数：`33689`。
- 完整 tracked diff SHA-256：`41101415c8c0f93673d6bf00ec689037d887a27e1953408803f6b6a03c0e56c4`。
- 文件路径均相对于仓库根目录；`ownership_tests.rs` 为未跟踪测试文件，使用单独文件 SHA-256 绑定，未假定 `git diff` 包含其内容。

| 文件 | 最终 SHA-256 |
|---|---|
| `packages/engine/src/company/real_estate/borrowing_costs.rs` | `ed36caa86d3cb38a59f015adf579a84402716290b30200d52bd2eb973aa9424a` |
| `packages/engine/src/company/real_estate/config.rs` | `c2a411919cd14f78b3006df9c9a688f159a6182b2e2d184a0bf45d6e8263a7e3` |
| `packages/engine/src/company/real_estate/debt_service.rs` | `26c44853040e93ba0496139eee77e3fc6e90b3599426c4e73b7bfea2ae1a6a4a` |
| `packages/engine/src/company/real_estate/delivery.rs` | `6b4480b06d54823d27e51eff1240e65b1b0faafa1be07f9e6c9684b07d2c4d2c` |
| `packages/engine/src/company/real_estate/development.rs` | `481b507f5657b030fb9e581609ae0c77bcdc0ca0e923b0cbe6e1cb01c9793903` |
| `packages/engine/src/company/real_estate/loans.rs` | `a110422296ccbe76594f0848721c608c10887513190c5439aa990e826a4cde9a` |
| `packages/engine/src/company/real_estate/mod.rs` | `c2759084795f74036f6c1b9bb688e999dbea0c69e1996956d02630f47eeaf6a7` |
| `packages/engine/src/company/real_estate/presales.rs` | `f095b6b3ee1f9f3f0119d29114e6dc33c596e3e9687c6d1fd01b2657bde46e14` |
| `packages/engine/src/company/real_estate/projects.rs` | `982f7a4d9a7ad203368237fe77301e28ab4ccb147e0225abef22286ac6006fed` |
| `packages/engine/src/company/real_estate/ownership_tests.rs` | `6fede901143cbf4f97b1a13d56d58680677be8bd5dcc8088dce8aff5cc65fc03` |

绑定后的最终独立静态结论保持：RE-R1 已关闭，无未关闭有效发现。后续文件或 diff 变化
不自动继承本结论，须针对变化另行复核；本 SHA 取证不构成测试运行结果。
