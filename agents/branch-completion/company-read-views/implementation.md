# B4e 最小公司只读查询切片

## 范围与基线

独占 worktree：`/data1/baiyifan/workplace/stock_market_game-company-read-views`；基线 `f5820291`；分支 `feat/engine-company-read-views`。

按 `reintegration-campaign-plan.md` §B4e，纯查询只依赖 B4b；动账命令依赖 B4d。本批只恢复：

- `company_period_explanation`：委托现有 `CompanySystem::period_change_explanation`。
- `account_dividend_tax_status`：按实际账户种类与名册/税账投影。
- `dividend_tax_outstanding_views`：当前 `outstanding` 和真实 `latest_collection`。
- `owner_flat_withholding_receipts`：按 owner 过滤冻结回执，非 Flat 显式拒绝。

`corporate_actions() -> Option<&SessionCorporateActions>` 在基线已存在，未冒报新增，也未改变其契约。无 approve/认购/提案/日终/host/ts-rs 接线，不宣称实际现金结算能力。未加依赖，未改存档字段。

## 当前契约与历史差异

旧 `18725d07` 的 Session 公司状态必在场，且将查询错误转为 `SessionError::InvalidSetup(String)`；当前公司系统/公司行为为 `Option`，`InvalidSetup` 已要求数值 `ValidationFault`。本批经主 agent 同意使用现有领域错误保真：期间查询直接返回 `CompanySystemError`，无公司线返回带公司/期间上下文的 `Unsupported`；税务查询直接返回既有 `SessionCorporateActionsError`，未知账户、无公司线、缺模式及非 Flat 均具体报错。不引入未登记错误码或新 `SessionError`，未来 host 映射另批统一。

独立复核指出旧查询的 `outstanding > 0 => InsufficientAvailableCash` 不能覆盖合法“已评税未划收”及“部分划收后后来新增应税额”状态。已保留该误报的真实红例，再移除推导 `needs_funds/cause`。当前视图直接复用严格 `ExactDividendTaxAmount`（不另造宽松 Fraction）及末张真实 `TaxCollectionReceipt`；无回执为 `None`，历史回执不冒充当前余额全部已尝试划收。domain/save 均不变。

## A 股语义与依据边界

本批读取既有事实，不新增税率、计税、舍入、登记日或现金结算算法。账户本人税务数据与全账诊断查询分开：全账查询不等同 host 已具备授权，宿主权限过滤仍待接线。

已读 ADR-0035/0037/0040/0041、正式 trading-rules、既有官方研究 `agents/remaining-questions-and-features/dividend-research.md` 与 `agents/company-system/remaining-corporate-rules-research.md`。既有研究列明财税〔2015〕101号（2015-09-08施行）及其继续援引的85号操作规则，个人公开市场与机构/游资不混同；Flat 属用户明确的游戏简化。此处只继承已存在 domain，不将本次称为重查官方现行全文或两市税款舍入全面验收。

## 红绿与资源约束

构建与执行分开。冷构建：独占根 `.tmp/company-read-views-target`，`CARGO_PROFILE_TEST_DEBUG=0`，`cargo ... --no-run -j16`，进程外 supervisor 300000ms 上限。机器128 CPU，预算16 jobs；采样确认同时有多个 rustc/原生编译进程，engine 单crate前端阶段受rustc自身串行限制，未把jobs数当实际满核。

普通执行：预构建 binary，`scripts/run-with-deadline.mjs 10000` 进程树监督，`--test-threads=8`。query/foundation 两个独立 binary 并行，每命令受10000ms约束；不运行 all/完整回归。

- `red-build.log`：最早缺 API 编译失败，仅作盘点，不当业务红。
- `red-stub-build.log` / `red-fixture-tests.log`：可编译拒绝骨架，6条查询因“尚未接线”真实失败；1条无公司查询拒绝已通过。
- `red-tests.log`：初版 Flat fixture 随机稀疏分配令账户1无股；修为显式80/20类间与类内均分，未弱化 owner 隔离断言。
- `pending-assessment-red.log`：合法已评税未划收仍被旧推导误报资金不足，真实断言红。
- `foundation-tests.log`：原有公司装配16条通过（2.73s）。
- `green-tests.log`：当时7条中6通过，Exempt构造因既有stamp税模式前置矛盾红，不当完整绿证据。

独立 Exempt 修复由 `sol61_exempt_setup_fix` 负责：本基线无条件 `stamp_tax_rate == 0.0005` 与 Exempt 要求0矛盾，fixture 报 fault1013。保留 Exempt 真查询用例，待审过的修复叠加后必须重跑。

- `domain-amount-tests.log`：当前10条中9通过（1.43s）；新增纯domain未划收/新评税与非空Session wrapper已通过，唯一Exempt构造前置仍红。

## 后续独立复核修正

C5：合法 `CashDividendTaxBook` 的同日 `collections` 物理数组可被交换后受检恢复，真实接受顺序由唯一 `operation_seq` 决定。新增短测已实际反序列化并 `validate()`，`.last()` 真红（得到 seq1，期望 seq2）；仅将投影改为 `max_by_key(operation_seq)` 后 exact 绿（0.00s），不改 domain/save。

原始红绿：`sequence-red-tests.log`、`sequence-green-tests.log`；修复不削弱既有断言。最终独立复跑仍等待 Exempt 前置。

## 最终依赖与实际验证

收到可信独立 `sol61_review_exempt` 批准后，按主 agent 授权顺序叠加 `669b2948` 与 `7cdb36d3`，本树分别为 `349e9a7b` 与 `558a870d`。这两笔属于独立已审 Exempt 修复，不混称本批只读实现。

当前受验源码提交 `558a870d`；完整 query 增量为 `b2d8b0ac` + `69e1b27e`。最终构建 `final-build.log`：Cargo `--no-run -j16`，外部300000ms deadline，`CARGO_PROFILE_TEST_DEBUG=0`，实报1m54s。原有486条警告仍存在，未声称警告清零。

两个预构建 binary 用8个 harness threads并行执行，各有10000ms进程树门禁，真实exit均为0：

- `final-query-tests.log`：11/11通过，2.19s，包含真实 Exempt 构造与账户税务状态/税账查询，不再被前置拒绝。
- `final-foundation-tests.log`：23/23通过，2.79s；并非此前16条，也不是24条（Exempt作者24合计含另一旧A股校验case，未混报为本套件）。

`final-source-fingerprint.txt`、`final-binary-fingerprint.txt` 和 `final-binding.json` 绑定当前5份源码、两个实际binary及构建/运行原件SHA。查询前后save bytes不变、owner隔离、未划收/部分划收后新评税、合法同日回执顺序交换均在11条中验证；不扩大为B4c全量跨账勾稽或实际公司付款完成。

最终 source/binary 已保持冻结，交未实施 `sol61_review_company_views` 完整diff与独立并行复跑；最终裁决待追加。未运行all、未推送、未合并main。

## 独立终裁

未实施者 `sol61_review_company_views` 已对完整diff与最终冻结源码/产物复核，给出 **APPROVED**，C1–C5有效发现全部关闭。其独立并行运行：query11/11，harness2.19s / wall2.306s；foundation23/23，harness2.78s / wall2.890s；各8threads、各10000ms进程树deadline。五份源指纹与两个binary SHA前后均一致。

独立原件位于主工作区 `agents/branch-completion/company-views-review-evidence/final-query-independent.log` 与 `final-foundation-independent.log`。此终裁限本批只读切片与已审Exempt依赖，不代替完整回归；后续 host owner授权过滤、数值错误映射、ts-rs 与 UI 接线仍属后续批次。
