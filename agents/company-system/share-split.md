# 拆股／缩股实施台账（2026-10-07 S1 批）

本目录工作文件按 [AGENTS 约定](../../CLAUDE.md) 归档。本批实现 ADR-0039
决策 3 授权的拆股（股份拆细）与缩股（股份合并）机制。任务书要求「方向、
比例及登记处理须先按官方现行语义另行核验，实施批次不得凭印象实现」——
官方研究结论先行并决定建模，登记于 docs/trading-rules.md「拆股／缩股
实际执行」节；本文件是实施与验证台账。

## 研究结论摘要（三级分级）

取证：沪市《交易规则（2026 年修订）》官方 docx 原文直接下载核验
（`.tmp/company-system/share-split/research/sse-rule/sse-rule-2026.txt`，
4.3.1—4.3.3）、WebSearch 核验现行口径（公司法 2023 修订、股改／重整缩股
先例、财税口径）；深市 4.4.1—4.4.3 沿用仓库既有归档核验。

- **明文**：除权公式与「权益登记日次一交易日」处理（沪 4.3.1—4.3.3／深
  4.4.1—4.4.3）；发行人可申请调整公式；新公司法面额股面值非必须 1 元。
- **解读**：公式代入负／正有理比例覆盖缩股／拆股（股改缩股市场实践先例）。
- **缺口（如实登记，不硬造）**：拆股无常规通道与任何 A 股先例（公司法无
  拆细程序、发行人指南无条文）；缩股仅见重整／股改／纯 B 股特殊场景；
  拆股／缩股的税务持股期限连续性无官方明文；缩股碎股处理无统一明文；
  拆股／缩股的 R/R+1 登记入账日程无官方条文（按送转先例）。

建模决定由研究结论驱动：拆股＝面值÷N、注册资本不变（标准拆细语义，
与送转「面值不变、注册资本增长」形成实质区分）；缩股＝面值×N、注册资本按
旧股口径消灭面值核减（形式减资）；回购专户参与换算（重新计值不是权益
分派，63号第13条失权不覆盖）；税账拆股增量按 R+1 新取得（送转同口径保守
解释）、缩股核减非应税且取得日延续（存活 lot）。

**二级 agent 纪律偏离登记（铁律三诚实汇报）**：任务书要求官方研究与测试
执行必须由二级 agent 并行执行。本 subagent 会话内无 agent 派生工具
（无 Task/Agent spawn 工具；ListAgents 仅见用户交互会话，不应转派工作），
Codex companion（唯一可用的外部二级执行器）setup 探针 120s 无输出判为
不可用。研究改由主线以 WebSearch＋官方原文下载完成、测试执行由主线
command 完成，全部证据如实落 `.tmp/company-system/share-split/`；此偏离
已在本文件与结构化返回中显式登记，不冒充已用二级 agent。

## 红绿日志索引（`.tmp/company-system/share-split/`）

| 阶段 | 红 | 绿 |
| --- | --- | --- |
| 机制模块 `company::share_split` | 新模块编译红（仓库先例口径；与测试同 commit 落地，未单独存日志） | 模块 6/6（本目录终端复跑） |
| registry `ShareReDenomination` scope | `registry-scope-red.log`（枚举变体不存在编译红） | `registry-scope-green.log`（share_registry 28/28） |
| 除权公式 `ShareSplitExRightsFormula` | `ex-formula-red.log`（未实现编译红） | `ex-formula-green.log`（ex_reference_price 24/24） |
| 税账核减 `record_redenomination_reduction` | 与实现同批（行为红：首跑「tax settled day does not match receipt tail」失败后修复，见下文修复记录） | `company::cash_dividend_tax` 19/19 |
| Simple 账面 `finance_share_split` | 行为红两处（拆股方向误走缩股下溢、核减公式误用新面值）修复后 | `finance-green.log`（company::simple 92/92） |
| Session 全链路 | fixture 面值 bind-once 与分红总额边界暴露后修正 | `share_split_session_tests` 7/7 |

## 实现清单

- `packages/engine/src/company/share_split.rs`：方向／整数比例／事件方案／
  状态机（Approved→Announced→Registered→Settled）／确定性换算（缩股碎股
  余数降序＋seed 洗牌；拆股恒整除）／限售继承（`holder_inherited_restriction`，
  混合限售显式拒绝）／Simple 声明与事实类型。
- `share_registry.rs`：`MovementScope::ShareReDenomination`（负向核减允许、
  限售 lot 可被重新计值消耗、发行股数按净额放大／缩小、同日追加、登记
  快照回放纳入）。
- `ex_reference_price.rs`：`ShareSplitExRightsFormula`（交易所分列、有理数
  比例、银行家舍入）。
- `cash_dividend_tax.rs`：`TaxRedenominationReceipt` + `record_redenomination_
  reduction`（FIFO 核减留痕、非应税、恢复重放勾稽；严格持久化新字段）。
- `simple/finance_share_split.rs` + `finance_validation.rs`：面值权威链
  （`current_par_value`）、拆股注册资本不变／缩股按旧股口径消灭面值核减、
  批准时点注册资本重构纳入缩股核减。
- Session：`approve_share_split`（三层同除权日碰撞预检、面值整除校验）、
  `process_share_splits_on_day_end`（公告→R 换算→R+1 重新计值入账、账户
  持仓同步〔含 T+1 锁定等比缩小〕、税账续记／核减、Simple 回填）、
  恢复勾稽（账簿↔重新计值回执双向、除权组 split 分量）、发行人股数回放
  纳入重新计值净额、偏好送转提案的同除权日拆股存在性守卫、
  `AnnouncementContent::ShareSplit` 公告。
- Web：`save/schema/corporate-actions.ts` 新类型与严格 parser
  （ShareSplitBook／TaxRedenominationReceipt／MovementScope 变体／组
  split_event_ids）、`corporate-actions-schema.test.ts` 正负例。
- fixtures：三份存档 producer 守卫清单补 `share_splits` 空数组键并由
  release Engine 重生成（见验证节）。

## 验证范围

只做定向短单测、独立编译与生产构建，未运行完整回归（按批次纪律）。各组
命令外部 deadline 10000ms、编译/构建/长验收 300000ms，多核并行。证据日志
在 `.tmp/company-system/share-split/`。

| 验证 | 结果 |
| --- | --- |
| `company::share_split`（模块） | 6/6 |
| `company::share_registry`（含 ShareReDenomination 4 用例） | 28/28 |
| `company::cash_dividend_tax`（含核减 2 用例） | 19/19 |
| `company::ex_reference_price`（含拆股公式 4 用例） | 24/24 |
| `company::simple`（含 finance_share_split 4 用例） | 92/92 |
| `session::share_split_session_tests` | 7/7 |
| 受影响组并发复跑（14 组过滤，各组外部 10000ms deadline、直接调用已编译 test binary） | 全绿（合计 305 项：share_split 6、share_registry 28、cash_dividend_tax 19、ex_reference_price 24、cash_dividend 35、company::simple 92、session::corporate_actions 14、company_mechanism_switch 3、company_simple_session 36、dividend_tax_mode 8、issuer_repurchase_session 5、rights_offering_session 13、simple_preferences_session 15、share_split_session 7），`groups/*.log` |
| engine lib 全量 | 本批 1542 通过 / 257 失败，与 main@0a38e52e 基线（临时 worktree 独立构建复跑，1514/257）失败名单**逐项一致零新增、零修复**（净增 28 项通过 = 本批新用例），`engine-lib-full.log` |
| `cargo check --workspace --all-targets --exclude stock-market-game` | 0 error（desktop crate 的 `tauri::generate_context!` 要求 `apps/web/dist` 存在，属既有环境前置，P 批同例；本 worktree 未构建 web 产物）`workspace-check.log` |
| Web `corporate-actions-schema.test.ts` | 25/25（`web-schema-green.log`，含拆股正负例） |
| Web `simple-finance.test.ts` / `complete-save-schema.test.ts` | 7/7、7/7（fixture 键补齐后） |
| Web 全量 `scripts/run-web-tests.mjs` | 失败集与 main@0a38e52e 基线**逐文件一致零新增**：全量对照因 runner 早停不可直接比集合，对全量差出的 6 个文件在干净 main 临时 worktree 逐文件复跑均为 main 既有失败（company-config-commands、remote-login-behavior、public-financials-render、MarketKlinePanel、personal-trade-history-panel、trade-confirmation-table 各 1 fail） |
| ts-rs typegen（`cargo test -p engine export_bindings`） | 154/154；`ShareSplitExRightsFormula.ts` 新增、`AppliedExReferenceGroup.ts`/`SessionCorporateActions.ts`/`SimpleCompanyPreferences.ts` 更新（`typegen.log`） |
| Web `tsc -b --force` | 0 错误（`web-tsc.log`） |
| 三份存档 fixtures + company 切片（release Engine producer 重生成） | producer 自校验（含 `share_splits` 空数组守卫、restore/resave 深等）全过；closed-day/minimal 字节级、main 沿用既有已知非确定性（M 批 backlog 登记项） |

## 修复记录（红→绿）

- **拆股方向误走缩股下溢**：`record_share_split_credit` 与 validation 首版
  无条件 `before − after`，拆股（after > before）触发「消灭股数下溢」；按
  方向分支修复（拆股 destroyed = 0）。
- **缩股注册资本核减公式错误**：首版用 `destroyed × par_after`（新面值），
  正确口径为 `par_before × (S_before − ratio × S_after)`（旧股口径消灭
  面值；在 capital = par×shares 前提下核减后恰为 par_after × S_after）。
  由 Session 用例数值勾稽暴露后修复。
- **税账恢复尾日校验**：无日结回执时仅含核减回执的账簿被「tax settled day
  does not match receipt tail」误拒；尾日取「最后日结／最后核减」较大者。

## 未完成边界（显式登记）

- 偏好自动提案：仅留 `SimpleCompanyPreferences` 结构扩展位注释（现实中无
  常规提案通道；本批不自动提案拆股／缩股）。
- 非整数比例缩股（如股改 1:0.6074）：不支持，受理显式拒绝（比例须整数
  ≥ 2）。
- 拆股面值非整除缩小：受理显式拒绝（面值最小单位为分）。
- 拆股／缩股×送转／×配股／第二起重新计值同除权日：受理时显式拒绝（合并
  口径未核实，对齐既有先例）。
- 主档 fixture 运行间非确定性（OHLC 切片）为 M 批已登记的独立排查主题
  （`m-integration/main-fixture-nondeterminism-backlog.md`），本批重生成
  沿用同一 producer 行为，不新增处置。

## 独立复核门禁（大 A 语义）

本 diff 须由未实施本批的 subagent 按 CLAUDE.md 复核：语义依据、最小范围、
边界测试与跨层语义漂移。本文件留待复核结论回填（复核记录建议另存
`share-split-review.md`）。

## 修复轮（2026-10-07 S1 拆股批 findings）

门禁 findings 逐条修复（基线 HEAD `f112c9c1`，未提交）：

1. **四处核减公式注释措辞统一**：`finance_share_split.rs` 模块头与
   `record_share_split_credit` 文档、`share_split.rs` `ShareSplitDeclaration`
   文档、`session.rs` `approve_share_split` 文档、`finance_validation.rs`
   校验注释——原「消灭股数 × 新面值」是错误公式，统一改为
   「旧股口径消灭面值：par_before×(S_before−ratio×S_after)」
   （与 trading-rules.md 拆股／缩股节及实际实现一致）。
2. **碰撞与偏好边界用例补齐**：
   - `split_and_rights_offering_same_ex_date_are_rejected_at_approval_both_ways`：
     镜像既有 split×送转 both_ways 用例，补拆股×配股同除权日双向受理拒绝
     （两个方向均校验账簿不变）。
   - `auto_stock_proposal_rejected_when_same_ex_date_split_event_exists`
     （Session 集成）＋`stock_evaluation_rejects_same_ex_date_split_event`
     （偏好评估单元）：`same_ex_date_split_event=true` 时自动送转提案拒绝并
     指明拆股／缩股，拒绝如实记入偏好台账、日结不 poison、显式缩股按自身
     日程正常入账。
3. **share_split_session_tests.rs 注释算术**：`3,000,000 % 7 = 3`（原误写 1），
   两处更正（consolidation_plan 文档与基准注释）。
4. **本台账 14 组合计**：304 → 305（明细求和即 305，原合计手误）。
5. **`corporate-actions.ts` `parseTaxClass`**：首两行被挤成一行，拆回独立行
   （纯格式，无语义变化）。
6. **`share_registry.rs` `validate_request` 同号校验（红→绿）**：`is_redenomination`
   分支原只拒净额为零；净额为负的混正负变动（如 +10/−30）账面勾稽自洽，
   会被当成缩股静默接受——新增「所有 change 同号」校验（混正负显式拒绝，
   错误指明 one direction），负例 `mixed-1` 先红（`fix-round-registry-red.log`
   净额 −20 组合无此校验时通过）后绿（`fix-round-registry-green.log`）。

### 修复轮验证

| 验证 | 结果 |
| --- | --- |
| 混号负例红→绿（`share_redenomination_validates_scope_specific_rules`） | 红 `fix-round-registry-red.log`（净额 −20 混号在同号校验缺失时被接受）→ 绿 `fix-round-registry-green.log` |
| 新增 split×配股双向碰撞用例 | 绿 `fix-round-split-rights-green.log` |
| 新增同日拆股偏好拒绝用例（Session 集成 + 偏好评估单元） | 绿 `fix-round-pref-split-reject.log`、`fix-round-group-session__simple_preferences_session_tests.log`、`fix-round-group-company__simple.log` |
| 14 组并发复跑（`availableParallelism=128`；并发 14 组、每 binary `harness_threads=4`、`RAYON_NUM_THREADS=5`；各组外部 10000ms 进程树 deadline） | 全绿：share_split 6、share_registry 28、cash_dividend_tax 19、ex_reference_price 24、cash_dividend 35（filter 含 tax 重叠 19，与首轮口径一致）、company::simple 93（+1 单元）、session::corporate_actions 14、company_mechanism_switch 3、company_simple_session 36、dividend_tax_mode 8、issuer_repurchase_session 5、rights_offering_session 13、simple_preferences_session 16（+1）、share_split_session 8（+1）；含重叠合计 308（=305+3 新用例）。日志 `fix-round-group-*.log` |
| Web `corporate-actions-schema.test.ts` | 25/25（`fix-round-web-schema.log`） |
| `cargo test -p engine --lib --no-run` 与 `cargo check -p engine` | PASS（共享 300000ms 期限、`Cargo jobs=availableParallelism=128`，`fix-round-build.log`／`fix-round-cargo-check.log`） |

本轮不提交（编排层纠正：用户未授权 git commit）；改动保留在 worktree
`wf_90e7d2a3-5ba-1`（HEAD `f112c9c1`），由编排层 reviewer 对 `git diff f112c9c1`
审查，新文件按 `git status` 单独读取。
