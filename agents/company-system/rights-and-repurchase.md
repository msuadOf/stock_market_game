# 配股／增发与回购实施台账（2026-10-07 M 批）

本目录工作文件按 [AGENTS 约定](../../CLAUDE.md) 归档。本批实现 ADR-0038/0039
（含 2026-10-07 决策注记）授权的配股／增发与回购机制，及两个独立新局开关。

## 研究结论（两地口径与证据缺口）

取证基础为已归档的官方全文（`agents/company-system/remaining-corporate-rules-research.md`
与 `.tmp/company-system/corporate-rules-followup/`），本批另经 WebSearch 核验现行有效性：

- **配股（深市有据）**：深业〔2025〕68号 2.4.3（一）—（五）/2.4.4（一）（二）
  ——R-3 前申请、R 日日终派发配股权证、缴款期起始日至 L 日报盘认购、L+2 扣登记费
  后划款（含利息）至主承销商备付金账户、失败 L+2 退认购资金及利息、碎权证按数量
  降序＋同数系统随机各登记 1 份。206号令第 53 条：配股 ≤ 配售前股本 50%、代销、
  认购 < 70% 拟配售量即发行失败。缴款期天数指南未固定 → 参数化。
- **配股（沪市证据缺口，登记）**：沪业字〔2024〕6号第 2.4 节无权证/缴款期/L+2
  划款退款/碎权证操作条文，仅确认注册、募集资金到位说明、回购专户不享有配售权
  （后者为沪市明文、两市共用上位 63 号第 13 条）。按「深市有据口径实现＋沪市登记
  简化/待证」处理，不拿深市冒充沪市。
- **配股除权日 = L+1：实践口径待证**（实际认购比例 L 日才确定；交易所除权条文
  只给公式未给配股除权日）。
- **回购现行依据**：证监会《上市公司股份回购规则》〔2023〕63号已经〔2025〕5号
  修改（新《公司法》衔接）——本批据此修正任务书引用，以现行有效版本为依据；
  沪指引 7 号（2025 年 3 月修订，现行有效）第 22 条与 63 号第 13 条同文确认专户
  失权。集中竞价委托限制按 63 号第 30 条已核全文实现（不以涨停价申报、不集合竞价
  时段申报）；窗口禁止（第 31 条）与每日数量限制未取得完整现行可实施原文 →
  显式登记不支持。
- **注销不除权**：交易所除权公式只覆盖股份增加情形，注销无除权条文、市场实践
  不除权；无官方明文 → 登记为待证口径。
- **合成资金差额**：无官方规则（ADR-0038 游戏授权域）→ 口径：计划完成时未用
  差额显式回收、不落地为任何人余额。

## 红绿日志索引（`.tmp/company-system/rights-repurchase/`）

| 阶段 | 红 | 绿 |
| --- | --- | --- |
| 新局开关（严格持久化） | `switch-red.log`（JSON 层断言失败） | `switch-green.log`、`web-switch-green.log` |
| 配股 company 模块 | `rights-module-compile-red`（会话接线前编译红在 `session-wiring-red.log`） | `rights-module-green.log`（7/7） |
| 回购 company 模块 | 同上（新模块编译红） | `repurchase-module-green.log`（8/8） |
| 配股 Session 全链路 | `session-wiring-red.log`（缺 API/字段编译红） | `rights-session-green.log`（7/7） |
| 回购 Session 全链路 | 同上 | `repurchase-session.log`（4/4） |
| Web 契约 | `web-full.log`（finance 字段缺失红） | `web-final.log` |
| 基线对照 | `head-failures.txt`（main@78a35747 lib 257 失败清单） | `engine-lib-full.log`（同 257 失败、净增 33 通过） |

> **失实更正（2026-10-07 修复轮，铁律三）**：本文件与 `current-handoff.md` 原记载
> 「Web 既有失败集与 main 一致、本批零新增失败」**不实**。批次终点实测
> `apps/web/src/save/corporate-actions-schema.test.ts` 为 **pass 2 / fail 18**
> （`corporate_actions.rights_offerings` 等三个新必填数组未同步进该测试 fixture，
> 除权组 `rights_event_ids` 与名册错误信息措辞亦未跟进），属本批引入的回归。
> 证据：`fix-round-major2-red.log`。复核门禁发现后已在修复轮补齐（见下节）。

## 验证范围

只做定向短单测、独立编译与生产构建，未运行完整回归（按批次纪律）。各组命令外部
deadline 10000ms、编译/构建/长验收 300000ms，多核并行（rustc 三 producer 并行、
Web 测试 shard 并行）。证据日志见上表目录。

| 验证 | 结果 |
| --- | --- |
| `company::rights_offering` 模块 | 7/7 |
| `company::issuer_repurchase` 模块 | 8/8 |
| `company::ex_reference_price`（含配股公式） | 19/19 |
| `session::rights_offering_session_tests` | 7/7 |
| `session::issuer_repurchase_session_tests` | 4/4 |
| 受影响组并发复跑（rights/repurchase/share_registry/ex_reference_price/corporate_actions/cash_dividend/company_mechanism/dividend_tax/company_simple 过滤） | 162/162 |
| engine lib 全量 | main@78a35747 基线 257 失败逐项一致；本批 1472 通过（净增 33） |
| `cargo check --workspace --all-targets` | 通过（含 desktop；web/dist 由本批构建产出） |
| Web `scripts/run-web-tests.mjs` | **失实，已更正**（见上方更正说明；修复轮后实测见下节） |
| Web 生产构建（tsc -b + vite + release WASM 校验） | 通过 |
| ts-rs typegen + `check-generated-types` | 通过 |
| 三份存档 fixtures（release Engine producer 重生成） | restore/resave 深等与场景守卫全过（**company 切片投影未随 main 同步重投影**，修复轮更正，见下节） |

## 修复轮（2026-10-07 门禁复核 findings）

复核门禁七项发现逐条修复（TDD 红→绿，红日志先落）：

| 发现 | 修复 | 红证据 | 绿证据 |
| --- | --- | --- | --- |
| major-1 Web exchange 枚举域错误（配股/回购/分红/送转 plan parser 用 `Shanghai/Shenzhen`，engine `CalendarExchange` serde 真值为 `sse/szse`） | 四类 plan parser 与公开公告分红 plan parser 全部对齐 `sse/szse`；新增 `CalendarExchange` 类型与 setup `StockExchange` 的显式映射勾稽；补 engine 真值正例测试（探针输出 `fix-round-exchange-probe.log`） | 同 major-2 红（旧 parser 拒绝 sse 值） | `corporate-actions-schema.test.ts` 24/24 |
| major-2 Web 契约测试回归＋台账失实 | fixture 更新到 10 数组键集（含新 `rejected_rights_subscriptions`）、除权组 `rights_event_ids`、名册错误措辞；**本文件与 handoff 的失实段更正**（见上方更正说明） | `fix-round-major2-red.log`（pass 2 / fail 18） | 同上 24/24；全量对照见下 |
| major-3 公开配售超额认购卡死日终 | 两层修复：受理侧按「剩余额度−已排队未结算量」显式拒绝（不截断不入队）；日终按队列序处理至额度耗尽，超出/无法入账条目出显式拒绝回执（新持久化 `rejected_rights_subscriptions`，恢复勾稽＋净认购唯一性互斥校验），陈旧排队（`submitted_on ≤ 当日`）一并重放 | `fix-round-major3-wiring-red.log`（编译红）、`fix-round-major3-behavior-red.log`（受理不拒绝）、`fix-round-major3-dayend-red.log`（日终卡死） | `fix-round-rights-session-green.log`（10/10） |
| minor-4 除权锚分量谓词不一致 | 两侧统一到权威谓词 `company::rights_offering::forms_ex_rights_component`（整数截位比例>0）；语义：截位为零的微量认购对参考价无分量影响，事实由认购记录与结算回执承载 | `fix-round-dividend-capital-recon-red.log` 同用例首红 | 同上（`tiny_paid_rights_with_same_day_cash_dividend_reconciles_at_restore`） |
| minor-5 同证券多回购方案成交双记 | 批准时拒绝同证券未完成方案（完成/取消后方可批准新方案） | （旧实现受理成功，见新负例用例） | `fix-round-repurchase-session-green.log`（5/5） |
| note-6 ADR-0039 注记措辞 | 更正为「机制层 `price_per_share` 纯显式必填；默认市场价属 P 批偏好域后续接线」 | — | 文档更正 |
| note-7 useSaveCommands 重复 setter | 去重 | — | tsc -b 通过 |

修复轮额外发现并修复（边界用例暴露）：

- **分红声明注册资本重构漏配股增量**：`company::simple::finance_validation` 重构
  分红批准时点注册资本时只扣送转入账、漏扣配股结算增量，「先批准分红、后结算
  配股」的局在结算后财务校验误判不一致。已按同一口径补扣（红
  `fix-round-dividend-capital-recon-red.log`，绿见 tiny_paid 用例）。
- **company 切片投影失同步**：M 批重生成 main 存档 fixture 时未同步重投影
  `current-company-slice.json`（HEAD 实测两者不等，违反「切片 = main 的
  company_system 精确投影」约定）。修复轮已重投影。

修复轮验证（全部命令外部 deadline 10000ms、producer 长验收 300000ms）：

| 验证 | 结果 |
| --- | --- |
| 受影响组复跑（13 组过滤：rights_offering/issuer_repurchase/share_registry/ex_reference_price/session::corporate_actions/cash_dividend/company_mechanism/dividend_tax/company::simple/mechanism_switch/company_simple/rights_session/repurchase_session） | 全绿（合计 257 项），`fix-round-affected-groups.log` |
| `cargo check --workspace --all-targets` | 0 error |
| Web `scripts/run-web-tests.mjs` 全量 | 失败 9 项全部在 main@78a35747 可复现（6 项 main 全量即失败；3 项〔利润表渲染、Tauri 更正命令、decimal account map〕在 main 单文件运行亦失败，属分片布局敏感的既有脆弱用例；另 Worker×3、WASM parser×3 归入 main 全量即失败类——按用例名去重共 9 个不同用例），本修复轮零新增失败；`fix-round-web-full.log`（main 基线对照见 `/tmp` 临时 worktree 运行记录，结论已并入本表） |
| Web `tsc -b --force` | 通过 |
| 三份存档 fixtures + company 切片（release Engine producer 重生成） | producer 自校验（restore/resave 深等、场景守卫）全过；main producer 为多核长任务（46 线程、约 96s），按长验收 300000ms deadline 执行 |
| ts-rs typegen | `RejectedRightsSubscription.ts` 新增、`SessionCorporateActions.ts` 更新 |

### 集成合并更正（2026-10-07）

上表「Web 全量失败 9 项、零新增」的结论**失实**：`apps/web/src/save/schema/company/simple-finance.test.ts`
中两个送转注册资本用例（「事实按批准时点口径核对」「既有分红声明按其批准时点注册资本核对」）的内联
fixture 漏补 M 批新增的 `rights_offerings: {}` / `issuer_repurchases: {}` 必填键（同文件其余三处
fixture 已在 major-2 修复轮补齐，这两处遗漏），在 M 批终点 bac1146f 单文件运行即失败（5 pass / 2 fail），
全量布局因首个失败分片中止而未暴露。P×M 集成合并时经 main@abdb0bf0 基线逐用例对照发现，已按同一口径
补齐两键并复验（单文件 7/7、污染配对 12/12、同布局全量枚举零新增失败），证据见
`.tmp/company-system/m-integration/`。

## 独立复核门禁（大 A 语义）

本 diff 须由未实施本批的 subagent 按 CLAUDE.md 复核：语义依据、最小范围、
边界测试与跨层语义漂移。本文件留待复核结论回填（复核记录建议另存
`rights-and-repurchase-review.md`）。
