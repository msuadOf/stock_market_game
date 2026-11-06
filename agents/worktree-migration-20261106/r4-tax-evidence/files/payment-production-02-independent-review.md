# payment-production-02 独立源码审查

日期：2026-10-29。裁决：**APPROVED_SOURCE_ADMISSION**。

本审未实施本批产品改动。完整审查最初九文件候选并反馈一项静态编译阻断，随后复核最终十文件冻结及修复。仅写本报告，未修改产品、未运行 Cargo、未读取 target 或计算产物大 hash。本裁决允许最终冻结片进入实际构建与必要 green 验证，不代表构建通过、付款功能验收完成或接收树合成通过。

## 冻结绑定

审查树：`/data1/<USER>/workplace/stock_market_game/.worktree/r4-tax-production-2026-10-29`。

基线 HEAD：`779a7f913255b3e090c94d4f1a655a28c3913c14`。

最终原件：[payment-production-02.patch](payment-production-02.patch)、[payment-production-02-fingerprints.json](payment-production-02-fingerprints.json)。patch SHA-256 已实际核为 `e67b9b0c1deffd6ada68bcd650ddd360a1438c0bff0c6288264b89a627b51edd`。十份文件指纹全部匹配；九份 tracked diff 加一份新增源码构成的完整 live diff 与冻结 patch 逐字节一致。`git diff --check` 无输出。原付款测试文件完整旧内容仍是当前文件的逐字节前缀，只追加 case04，01..03 的断言未修改。

以下路径均相对审查树根目录：

| 文件 | SHA-256 |
|------|---------|
| `docs/trading-rules.md` | `6b7186af9125453b08d5dc995eea695b7746b7f7aab82889be6f1e26bf69e47e` |
| `packages/engine/src/runtime/drive/administrative_mirror.rs` | `7dfde33e585c50c1929cca67e23ec92e0bf25b7a3c44370c1ac0680f189942d9` |
| `packages/engine/src/runtime/world.rs` | `ef473be944c49a04e0d5381ea7de730e35a8cf54bd3c55c9e08e29d08a598d48` |
| `packages/engine/src/runtime/world/admin_transaction/stages.rs` | `de102db5f433c9dae550454fde849aaa5b0663e2213be6abcce1574ac05b23a9` |
| `packages/engine/src/session.rs` | `d66da63e895c1d6fb2ab809d41752ab9f602f909750997f467e9ec7d78d864fd` |
| `packages/engine/src/session/administrative_mirror.rs` | `2cf1d9699f58e5688296d5f9cec5f608e2b84bf470ee03a2df3f8d5cab07eb0d` |
| `packages/engine/src/session/civil_information.rs` | `0835febf9cfbc293b4660459c8b4af54dc6d582892baf1f008590f1b74a27c84` |
| `packages/engine/src/session/corporate_actions/distributions_day_end.rs` | `7d830fe6205867f0949141d391671b2650bd96c0462a96719dcf4444fcd409bc` |
| `packages/engine/src/session/ordinary_tax_day_end/payment_runtime_tests.rs` | `893099decc4694d56759295d415af454b590634ff6e94b73d38f7540c8af62e9` |
| `packages/engine/src/session/civil_cash_distributions.rs` | `164c9563398b1eb13f367b5318b69cf40785819ee4b4ff5f8abdfd3ec8499a39` |

初稿 `payment-production-01.patch`（SHA `7ba75eee75edc4c63786588efa15faeb368440200f84eb3cda5b5e365bd2a91d`）原件保留；本报告签最终 02，不把初稿追认为通过。

## 发现与修复关闭

**F1：缺少 World stage 类型导出，构成静态编译阻断；最终冻结已修复，源码复核关闭。**

初稿新 driver 签名与 Session import 从 `crate::runtime::world` 引用 `AdminOperationStage`、`PreparedAdministrativeStages`，但 `world.rs` 当时仅导出 `PreparedAdministrativeSource`。两个 stage 类型只在私有 `admin_transaction` 模块中再导出，无法经新调用方使用的 World 路径解析；产品编译将无法进入 case。这是源码发现，本审没有运行编译器，不能报告实际 E0432/E0412 原件。

修复只在 `world.rs` 原 crate-private reexport 处一并导出这两个类型，未改变公开宿主 API。最终十文件冻结包含该最小 hunk，路径、可见性及新调用签名已复核。共享 `prepare_stage_requests` 仍服务原 staged 入口及新 prepared staged 入口；新入口通过现有第二检查点取得真实 `PreparedAdministrativeSource`，不另建 pending/id 或 Commit。

**实施者补充的 typed source 链修正已复核。** `CashPreparationError` 三分支使用 `#[error("{0}")]` 配合 `#[from]`，保留原 `RuntimeError`、`DistributionsDayEndError`、`CompanySystemError` 为直接 `Error::source`；`CivilInformationError::new` 接受具名 typed error，不通过字符串解析恢复原因。未发现其余静态接口阻断，但实际编译仍是后续必要门。

## AGENTS 三问

### 1. 大 A 语义及依据

**通过本片限定。** 本审读取 ADR-0038、ADR-0040、正式 trading-rules 边界，并实际重读官方存档 `agents/branch-completion/distributions-dayend-r2-sources/tax-2015-101.txt` 与 `tax-2012-85.txt` 的相关条文。财税〔2015〕101号第二、三、五条给出派息时暂不扣缴、转让后按持期扣收、衔接85号及2015-09-08起的适用口径；85号第三条给出 FIFO、每日日终净增减口径，第二条保留资金不足补足及划扣要求。本片在2026-10-29复核沿项目已登记规则接线，未新增税率、计税算法、税日或交易所到账制度；没有以85号旧税率覆盖101号修订。本审使用仓库中的官方存档，未重新在线检索。

`AShareIndividual` 复用原候选与个人税 core，未卖出者付款激活既有登记所得；真实转让产生的税由原候选决定，收缴只使用同一次真实 `GroupAccount.cash`，不把 Session 含 purse 的总现金当作可扣余额。`FlatWithholding` 仍按 ADR-0040 的显式游戏简税计算，原 gross credit 与 withholding 分列，机构与外部持有人边界保持原候选语义。`Simple` 发行人付款来源沿 ADR-0038 合成账面清偿，不冒充 `Simulation` 的真实资金链。

cash-only wrapper 复用原纯候选中的现金分红与个人 collection，只排除 stock distribution 推进；不以 `CashDividend` 或现金税原语冒股份 movement。税额为零不生成零额 `WithholdingTax`，不创设补钱机制。正式文档已明确 native 候选接线及尚待实际 green，未宣称 WASM 或完整送转链完成。

### 2. 必要性与最小范围

**通过。** 新实现从一份 `AdministrativeBaseline` 同时取得真实 Owner 名册、完整 raw 账户及原 mirror 来源；付款候选形成后、首 Prepare 前再次检查来源未变，未另取一份现金权威。候选只修改 civil shadow 中的 Companies，成功前不安装 `Paid` 或 collection。

`candidate.intents` 按原顺序划分连续 gross/tax 段，不按证券或 kind 重新排序。同一账户的 credit、tax 及 checked overflow 顺序保留；跨组重分桶不改变账户内顺序。stage 的 `group_index` 来自实际 rows 首次出现的位置，`operation_index` 为组内跨 stage 的 flat 位置，与 World 原 operand join 的枚举对应。没有在 stage 内重置 repurchase 的既有 flat 语义。

实际来源核对覆盖 plan 与 stock、公司 gross source、账户 gross receipt、Flat receipt 或个人 collection 身份；匹配后消耗集合并拒绝遗漏或额外事实。最终 inspect 核原 intent 与借用的 typed operand、税投影的 id/stage/group/flat index/local/stock/amount，以及完整最终 `GroupAccount`。简单 cash 加减仅用于候选与完整账户的勾稽，法定税率、FIFO、舍入和部分收缴仍由原纯候选及唯一税 core 计算。

World、driver、Session 的新 prepared staged 通路是薄转发，复用现有 prepare/inspect/commit/abort 与单一 mirror ack。原 flat、repurchase 入口继续使用原路径；未引入新宿主命令、持久 token、第二本资金账或付款后补偿事务。少量与本片有关的格式整理不改变语义。

### 3. 边界、失败原子性与跨层一致性

**限定源码准入通过，运行及扩展边界待后续。** 所有可失败工作均留在首 Commit 前：纯候选、原来源核对、组真实 checked apply、税投影、完整行政镜像与最终公开投影。任一阶段、后组或最终投影拒绝都沿原 Admin Abort；Companies 候选仍留在 shadow。全部内部 Commit 成功后，固定成功尾安装已准备的 mirror 与 `PreparedCivilFields`，再确认同一 ack；没有 Commit 后重新计算付款或收缴。

新增 case04 先要求最终 shadow 已有真实 +200 cash 和 `Paid`，再主动拒绝公开投影，比较原 business hash、Companies、账户 cash、runtime cache/source 并要求 World 健康。它是实现后的回归保护，未伪装成历史先红。该 case 的既有收盘已安装，因此 Session 仍沿原 civil poison 策略；World 普通 Abort 的健康状态不被表述为 Session 可重试。

本片源码未发现剩余阻断问题，但付款01..04尚未实际证明：raw cash 与 Session 总现金不同的真实 purse 场景、资金不足部分 collection、多证券多方案同日顺序、付款过程中后组拒绝、transport/partial Commit/panic、完整 INFO 私域逐字段回滚与下一真实 Decision 消费。原 World/税 core 既有测试不能冒充这些新生产组合的动态证据。实际构建、01..03 green、case04 运行及适当旧路径回归仍须取得。

## 实际首红证据与未运行范围

本审实际读取本主题 `payment-red-runner-01` 下三份 `feature-engine-r4_cash_0{1,2,3}_*.stderr`，分别在 `payment_runtime_tests.rs:207`、`:284`、`:429` 命中指定现金 marker，实际差额为 `0 vs 200`、`0 vs 180`、`0 vs 180`。这是付款尚未接线的目标首红，没有把更早 fixture 或编译错误当首红。

同时读取实际 front：session `57906`、WAIT exit `0`、chunk `60f4e3`；selected/executed/rawBusinessRed/expectedRed/expectedMatched 均为3，green为0。front SHA-256 已实际核为 `29c5308a9ca6c3c89072947fd39477ff118bb858a8bce8ea37b641ccb46c580b`。本审未重跑上述批次；首红停在现金断言，不证明其后 `Paid`、receipt、save/restore 断言已通过。最终生产02尚无实际 Cargo/green，case04也尚未实际执行。

## 接收树适配约束

本审实际比较到 integration 的 `session/administrative_mirror.rs` 已有较新的 causal 字段、shadow clone、acquisition 采集参数及成功安装逻辑，本审树没有这些既有接收方内容。**只允许按净 hunk 适配新入口与调用，必须保留接收树 causal 采集及安装；不得用本树旧整文件覆盖。** `world.rs` 的新增导出也只搬最小 hunk，保留接收方较新的 WASM 等内容。接收树完整合成 diff 及其实际编译必须另行复核，本报告不预签该结果。

**最终结论：payment-production-02 十文件冻结可进入实际验证；F1 已修复并独立复核，typed source 链修正已核，无剩余阻断源码发现。没有实际 green，不宣称完整付款、WASM 或公司行为链验收完成。**
