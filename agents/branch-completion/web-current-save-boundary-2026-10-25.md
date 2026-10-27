# Web 当前存档边界适配（2026-10-25）

## 真实来源与范围

- root 授权在 PlanId 机械适配之后，独立修复真实 Rust SaveSlot 触发的既有严格 parser 漂移；不改变交易规则或引擎 wire。
- 原档：`/data1/baiyifan/workplace/stock_market_game/.tmp/plan-identity-target/producer-save.json`，1819353 bytes；SHA-256：`03a7e1776a5b81e09cf6bd91b365f4eeea1229b5b4c9cf470c7653739561ca24`。
- 核心 producer 使用真实 GameSession 两日 `step → end_civil_day → save`，含 owner=3/4、sequence=0 的两项真实计划；核心执行 `decode_save_slot → GameSession::restore` 后才发布原档。Web 未改原档字节。producer 源码位于核心树 `agents/branch-completion/plan-identity-producer.rs`。
- `current-producer-fragments.json` 仅从上述原档提取完整子对象，不修改子对象字段/值；它是逐字段 TDD 的短 fixture，不替代完整原档往返见证。

## 第一批：根标记与个人身份

| 字段 | 当前 Rust 依据 | 严格 Web 约束 |
|---|---|---|
| `desk_mirrored_resting` | `session.rs::SaveSlot`、生成 `SaveSlot.ts` | 必填 boolean，原样保留 |
| `InformationState.owner`、`BeliefBook.npc` | 字段直接使用 `AccountId`，`orderbook.rs::canonical_u64_decimal` | canonical u64 string，与对应账户 key 一致 |
| 个人状态账户 map key | `AccountId` map | canonical u64 十进制键，不转 number |
| `InformationState.companies` key | 数值 `CompanyId(u32)` map | canonical u32 十进制键 |
| `BeliefEntry.company`、`CompanyMismatch.expected/report` | `CompanyId(u32)` | 数值 u32，拒绝旧代码串 |

没有默认补值、旧格式 alias 或宽松 unknown。历史大 JSON 保留原字节，仅其 test-only projection 显式投影新格式。

原始新阶段日志位于本树 `.tmp/plan-identity-web-receipts/`：

- `current-wire-red.log`：3 case 全红，exit=1；分别因未知根标记、旧数值 owner 与错误值未拒绝而失败。
- `current-wire-green.log`：实现后 17/18 绿，旧测试 fixture `company:"company"` 先于 confidence 校验失败，exit=1。
- `current-wire-green-2.log`：更新该旧测试输入为真实数值 CompanyId 后，18/18 绿，exit=0。
- `root-personal-regression.log`：59 case 绿，exit=0，wall 3.03s。

上述命令均经 `scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000`，聚焦并发4、关联回归并发8。执行 cwd 为仓库根或 `apps/web`，日志中的测试路径与命令一致。

## 类型检查与已知限制

- 已精确 cherry-pick 核心实际导出的生成类型 `21cbee78` 为本树 `0cf0abb3`，没有手改生成文件。
- 首次共享300000ms进程外编译批次并行启动 app/node/临时DEV三个 TypeScript project：app 8673ms exit=0、node 1599ms exit=0；DEV临时 config 放在 `.tmp` 造成 types 路径与 glob 配置错误，740ms exit=2，不能记为DEV类型通过。原日志在 `.tmp/plan-identity-web-typecheck/{app,node,dev}.log`。
- NPC inspector 与 EngineHost 的查询请求仍采用已有安全整数 API；本轮保证返回 PlanId 的完整 u64 与显示，不宣称请求端也支持完整 u64。
- 最初 PlanId 阶段红绿输出仅保留于会话工具 receipt，未落盘，不能补造历史原始日志：红 `090898`/`6d0d97`；绿 `a1e815`/`cf7c77`/`5e54a3`。该限制保留供审查。

## 后续批次与门禁

- CompanySpec/catalog/CompanyId、report source/scope 与公告 tagged content 仍待适配；公告六变体由 root 指派的独立 writer 负责两个新增文件。
- setup 当前模式字段需按现 Rust 定义严格适配，不冒称新局 UI 模式旅程已覆盖。
- 所有批次后须在同一未改动原档上执行完整 Web parse，并闭合 Rust restore 见证；目前未宣称此门禁通过。
- 可信独立 reviewer `sol61_pending122_contract` 逐批审查增量完整 diff；未完成终审前不宣称交付完成。

## 第二批：公司身份、披露结构和税务仓位

此批由同一原档逐域失败直接驱动，没有更改 Rust 生产语义：

| 当前字段 | Rust/生成类型依据 | Web 适配 |
|---|---|---|
| `CompanySpec.code/issued_shares` | `company/spec.rs` | code 取代旧 id；股份正 canonical u64 string |
| `CompanyOperations.catalog.metas[].code/name` | `company/spec.rs::CompanyCatalog` | exact 两项元数据；拒绝重复代码及派生 by_code |
| `companies` map key、`PaymentFailure.company`、`ScheduledAction.company` | `CompanyId(u32)` | key 为规范 u32 十进制串，值身份为 u32 number |
| `PublishedReport.company/source` | `information/publication.rs`、`PublicationSource.ts` | 数值 CompanyId；严格两种 source |
| 报表和 ClosingRegistry 的 `Standalone/Consolidated` | `MemberId(u32)` | 数值 u32，拒旧公司代码串 |
| `ReportSet.roe` 六项结构 | `accounting/reports/mod.rs`、`ReportRoe.ts` | 复用已有公共 DTO 的完整 ROE validator；新增断言入口无类型绕过 |
| `income.report_period/report_period_net_income_to_parent` | `accounting/reports/income.rs` | 必填列及 nullable 金额，原样保存 |
| `BsLine` 新增列 | `accounting/reports/balance_sheet.rs` | 补 `SimpleDividendSettlementAdjustment`、`SimpleIssuerFundingAdjustment`、`CapitalReserve`、`StatutoryReserve`；只接受 Rust 实际枚举 |
| `Announcement.content` | `information/publication.rs` | 接独立 writer 六变体 parser；外层 exact content，保 PaymentFailure 单日幅度校验 |
| `IncomeTaxPosition` | `company/income_tax.rs` | 严格 loss_pool、assessments 年度 map、initial_deferred_tax_asset、restatements；年度项 strict pretax/current_tax/opening_loss_pool/deferred_tax_asset |

- 公告独立实现 `d6aa0d05` 精确 cherry-pick 为本树 `283afef0`，未在本批重写。
- 公司字段红灯原日志 `company-wire-red.log`；逐域暴露后续 `roe`、`income.report_period`、`BsLine` 的原失败在 `company-wire-green.log`、`company-wire-green-2.log`、`company-wire-green-3.log`，不是通过收据。
- 所得税仓位单独红灯 `tax-position-red.log`、绿灯 `tax-position-green.log`；原档税务完整子对象已原样加入短 fixture。
- 旧 fixture 关联回归首轮28/30绿，两个旧输入分别仍用旧 assessment 和把合法数值 CompanyId 当非法；更新输入形状后 `company-related-regression-2.log` 50/50绿，exit=0，wall3.92s。全程保留原断言意图、未知字段/坏类型/缺字段拒绝，不弱化断言。
- 历史大 JSON 及历史公司切片原字节保留；仅测试 helper 投影 schema，并明确税务/ROE 新增事实不作为 Rust 恢复证据。

## 真实原档闭环（不是历史 projection）

- 本树 `.tmp/plan-identity-web-receipts/full-save-roundtrip-green.log`：`run-with-deadline.mjs 10000 -- node .tmp/plan-identity-web-receipts/roundtrip.mjs`，exit=0，wall0.46s。脚本先核原03a7哈希，执行完整 `parseSaveSlot → deepEqual原对象 → JSON.stringify → 再parse → deepEqual原对象`，最后再核原档未变。
- Web 输出 `.tmp/plan-identity-web-receipts/roundtrip-save.json`，SHA-256 `e5541bd1e4fd6a3f4aab201095e98176a5b6721a0226731354c9425e9ef534fe`。
- 核心随后对该确切回写文件执行真实 Rust `decode_save_slot → GameSession::restore`，2计划，外10000ms，exit0。证据 commit `90e9b624` 的 `plan-identity-web-roundtrip-evidence.json.log` 绑定完整命令、probe源码/二进制/engine rlib及两份原档SHA；说明 `5237591a`。原03a7在最终核心重新构建后逐字节一致。
- 本原档没有 `company_system` 等8项 setup 可选键；此闭环不证明 Simple 公司模式或税务新局 UI 的完整旅程。8项新parser由另独立 writer 提供并以类型/单测验证；真实Simple含选项存档证据由后续 B4f 任务补。

## 第三批：setup wrapper 与最终短门禁

- 独立 setup helper `0644fedd`、覆盖限制 `fef582b8`、有效复核修复 `2df421f8` 已分别精确 cherry-pick 为 `c06bf728`、`f940b81d`、`ed589dc5`。复用 journal/tax 两个既有 parser，只导出供 helper 使用，没有复制另一套解析。
- `market.ts::parseSetup` 只将实际出现的8键加入 exact 并展开严格 helper 返回值；不补税务模式或公司默认值。
- wrapper 红灯验证临时使用本树 HEAD 原 market.ts，新增测试因 `setup.company_system` 未知而失败；随后重装接入实现。原日志 `setup-wrapper-red.log` exit1、`setup-wrapper-green.log` 12/12 exit0。helper 本身11项测试由独立 writer 保留原日志。
- 最终35文件存档/诊断关联短回归216/216通过，8并发，case与整个进程树上限均10000ms，wall3.52s；命令与文件清单保留 `.tmp/plan-identity-web-receipts/final-short-regression.json`，原stdout `final-short-regression-2.log`。
- 首轮该回归211/212，独立worktree缺少Tauri生成的ACL manifest而ENOENT；没有改测试。随后核 Cargo.lock、Tauri Cargo.toml、capability、tauri.conf 与主树逐字节一致，复用相同依赖已有生成物，记录 `acl-cache-precondition.json`。这是本地缓存前置，不是新Tauri构建证据。
- 最终 app/node/DEV 三 project 并行 TypeScript 编译，统一进程外300000ms上限，实际 app8144ms、node1784ms、DEV1635ms，全部exit0；原 `.tmp/plan-identity-web-typecheck/final-supervisor.log` 和三个 compiler log，DEV临时配置放在 apps/web 使类型解析与正常项目一致，未放宽类型选项。
- 最终受影响路径 oxlint exit0；原 `.tmp/plan-identity-web-receipts/final-lint.log`，10000ms外监督；`git diff --check`通过。
- 本批是保存边界修复；仅证明现有真实非公司模式原档和精确helper契约，不宣称Simple公司新局UI/所有公司动作真实旅程已验收。最终独立完整复核仍由root门禁，尚未在本记录自签交付完成。
