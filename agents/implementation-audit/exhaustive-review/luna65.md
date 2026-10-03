# Luna65：build、deadline 与 fixtures1 复核记录全文扫描

- 日期：2026-10-03。
- 对象：产品 `08e4fc7`；本 worktree `HEAD` 为 merge commit `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`，沿用任务指定“merge 同一产品实现”的范围。
- 范围：`hosts/review-build.md` 71 行、`review-deadline.md` 131 行、`review-fixtures1.md` 134 行，合计 336 行，逐篇读至 EOF；追查相关实施状态、验证/复核记录、当前 Rust fixture 与 Writer caller。
- 方法限制：遵循 `AGENTS.md`、`docs/principles.md`。本轮未改产品、未运行测试/Cargo/长验收、未执行 Git 写操作，未重新在线核验交易所法规。下文区分原 review 执行记录、root 后续验证记录与本轮静态核对；不把它们混写为本轮运行结果。

## 全文章节矩阵

| 文档行/章节 | 全文所述与后续/current caller 对照 | 复核结论 |
|---|---|---|
| `review-build.md:1–19` 结论、完整 diff 范围、依据与运行限制 | 文档列出九文件 baseline diff、六动作/E01 及其读取依据；明确实施者的 46 短测、sandbox EPERM、rustfmt 不是 reviewer 重跑证据，reviewer 未跑 Cargo、matrix、真实构建。 | 范围和执行身份有区分；不把静态复核写成 runtime acceptance。 |
| `:21–33` B1 撤销过程 | 局部删除的 negative-control predicate 被完整调用链 `validate_reusable_pass → validateExecutorEvidence → negativeControlDetected` 反证；修正 receipt 的 characterization 用例重算 capture/receipt/summary receipt 后仍拒绝，旧 guard/error code 未重复添加。 | 撤销误报有具体反证与用例；不能继续把 B1 登记为产品问题。该 1/1 PASS 是 review 当时执行记录，不是本轮重跑。 |
| `:35–45` B2 及修复 | 最初 Writer 确实只有生产 caller、Node fakeHarness 不经过 Rust Writer；后续新增 `runtime.rs:927` 完整落盘/已有目录拒绝测试、`:964` 指定位置写失败/部分文件保留测试。当前 `write_bundle` 仍进 `CaptureArtifactWriter::create(...).write(...)`（`:636`），Writer 顺序仍 report、capture receipt、artifact iteration（`:606–620`）。 | 原测试缺口已补源码且原 reviewer 静态复核关闭；当前真实 Rust caller/测试直接命中 Writer。Writer 两个用例当时未由 reviewer 执行；不能沿用其静态关闭冒称当时已运行。 |
| `:47–55` 三门、A 股边界、必要性、后续限制 | 报告区分交易语义未变、动作范围、B1/B2 边界；明确领域官方材料日期是既有 trading-rules 记录，未在当时重查；要求 root 补 Rust 和 sandbox 运行证据。 | A 股依据只足以支持“本批工具/测试重构未改语义”的静态结论，不构成现行法规更新复核。 |
| `:57–71` 最终版本绑定 | 九文件 SHA 表绑定第二轮完整 diff。 | hash 是历史 review 快照绑定，不证明后续文件未变，也不证明构建/测试通过。 |
| `review-deadline.md:1–17` 范围和结论 | 四个改动脚本/测试及两个未改 caller 全文范围；R1 初轮为有效 acceptance 回归，修正后 reviewer 未自行运行四套最新短测。 | 文档明确初轮缺陷和最终执行边界。 |
| `:19–37` 语义、必要性及资源所有权 | `BoundedCommandRun` 拥有单次 child 生命周期，`ArtifactInventory` 封存 record；filesystem 与进程 I/O 留 adapter；真实调用方继续经过 public facade/runner。 | 抽取对象与实际 owner/caller 相符，没有把本地 deadline 误当交易规则。 |
| `:39–62` 生命周期、优先级及 Inventory 初轮边界 | spawn/abort/close 优先序与 cleanup 顺序维持；inventory descriptor 与 filesystem 交替验证；记录列出原有 race/局部失败行为。 | 结论限定于本次变更，没有承诺 Windows tree 已关闭、全面原子性或解决基线已有竞态。 |
| `:64–89` R1 证据与修复建议 | 2000 层 JSON 合法 extra 旧 JSON 算法可接受，新增 `structuredClone` RangeError 是有效接受面回归；修复需保留顺序、extra、duplicate 和 caller 隔离。 | 初轮发现有可复现输入与旧/新行为区分，成立。 |
| `:91–102` 历史验证记录 | 54/54 来自修复前 root sandbox 外执行；本 reviewer 只读证据并做 diff check。 | 历史绿明确是修复前快照，不能代表 R1 修复版本或最新 55 cases。 |
| `:104–131` R1 修复复核至 EOF | constructor/toJson 改 JSON round-trip，deep freeze 用显式 stack；depth2000 测试覆盖完整输出与深层修改隔离；红绿日志证明该 case 的修复闭环；reviewer 再做 diff check，未重跑测试。修后四套脚本 55 cases 的全量短测仍明确待 root。 | R1 已由代码/定向证据与静态复核关闭；4/4 是实施者定向日志，不能升级成 reviewer 运行证据。最新 55/55 全套未证明运行。 |
| `review-fixtures1.md:1–17` 范围、结论与语义来源 | 六动作、11 文件全文 diff 范围；没有跑 Cargo；官方依据日期沿用文档而非本次联网核验。 | 静态审查范围限定清晰。 |
| `:19–52` A 股语义/必要性动作矩阵 | 逐项区分沪深、证券类别、分/股单位、T+1、信息公开与个人获知、周五日结与周六无 step；各 fixture owner 与原 API 层次相符。 | 抽取的是集成测试输入组织；未把 synthetic 场景声称为真实行情或法定披露日历。 |
| `:54–73` 原断言/seed 保留 | 记录以逐文件计数和静态归一比对支持原测试函数与断言未删除/弱化。 | 属静态 diff 证据，不是执行结果；计数只对列出的文件/断言范围有效。 |
| `:75–95` restore 与 invalid 语义 | Auction fixture 的正常 SaveSlot 注入与坏档直接 restore 分流；save-contract 和 information-acquisition 的坏档仍进入原拒绝路径。 | 当前源码抽查与该边界相符；没有通过 fixture 修复篡改档案。 |
| `:97–111` 新 fixture 测试/覆盖限制 | clone 隔离、沪深身份/envelope keys、behavior breadth setter 的断言目标有界；明确 observed=0 未覆盖，未把它称作通用校验器。 | 边界限制披露充分；没有发现应扩大成生产行为变更的依据。 |
| `:113–134` 静态检查、hash 与 EOF | diff check/hash 绑定、Cargo 未运行等边界明确。 | 不能以 hash 或静态复核替代执行证据。 |

## 后续状态与真实 Rust caller

- **build / Writer：**当前 `runtime.rs:636` 的 public `write_bundle` 进入唯一 `CaptureArtifactWriter`；`:600–620` 保留顺序写及显式部分输出语义。`:927–962` 真实测试以临时目录调用 `write_bundle`，核对报告 bytes、artifacts bytes/name/length/hash、receipt 和既有目录拒绝；`:964–986` 直接调用 writer，阻断 `event-stream.json` 后核验此前文件存在、后续文件不存在。`committed.rs::execute` 是真实市场执行与 `assemble` 上游，但 Writer 两个短 DTO 用例本身不执行市场；不能混淆二者。
- **deadline / ArtifactInventory：**`hosts/deadline/status.md` 与 `status.json` 记录最终修正后 R1 定向 4/4；具体日志 `deep-json-red.log`、`deep-json-green.log` 保留红绿证据。status 明确初版 54/54 是修正前，修后最新四脚本共 55 case 未整套重跑。root 在其它任务中记录的 Rust 编译及短筛选不能代替 Node 55-case 执行；本轮没有找到可据以声称 55/55 的执行结果。
- **fixtures1 / root 验证：**`hosts/fixtures1/status.json` 最终记录指向 `targeted-tests-build-07-result.json`（编译/类型检查）和 `rust-all-targets-check-08-result.json`，并将所选短 case 按 source/filter 映射；范围字段明确未选中旧场景、整个 suite、性能矩阵/E2E。`review-fixtures1.md` 中逐文件断言比对仍只是静态证据。`AuctionFixture` 当前位于 `packages/engine/tests/auction.rs:8` 并有真实多处测试消费者；`SeasonedSaveFixture` 当前位于 `save_contract/main.rs:116`，`build_session` 仍每次鲜建两日 Session，baseline JSON clone 独立；`AnnouncementExposureFixture` 当前位于 `attention_discovery/exposure.rs:92`，调用者仍从 fixture 派生公开曝光输入。它们均留在 integration test 内，没有进入生产 API。
- **manifest 与 SHA：**`final-review/review-evidence-inventory.json` 对三篇 review 记录仅声明机械记录路径/hash/大小，“不等于亲读全文或审查通过”；本轮实际逐行读到了 EOF。`final-integrity.json` / coverage binding 引用 source-review manifest 和 review 文档，是快照完整性/覆盖绑定材料，不取代代码 caller 核对或运行日志。review-build 的九文件 SHA 和 review-fixtures1 的十一文件 SHA 都是旧审查快照；不能把其静态结论自动扩展到不同 SHA 的后续代码。

## 旧结论、发现与新候选反证

| 候选/历史结论 | 当前反证或边界 | 本轮处置 |
|---|---|---|
| build B1：删重复判定让伪造 negative-control witness 被复用为 PASS | validator 在 artifact 比较前执行 typed witness 校验；重算全部相关 receipt 的独立 characterization 仍应在 validator 失败。 | 已撤销误报；不恢复重复 predicate。 |
| build B2：Writer 无真实落盘/失败测试 | 两项 Rust 单元测试现在直调 `write_bundle`/Writer 并检查字节、顺序、目录拒绝及部分失败状态。 | 测试源码缺口关闭；review 时未运行，root 后续记录 Writer 5/5，但该结果须按 root evidence、case 范围报告。 |
| deadline R1：structuredClone 拒绝合法深 extra | JSON round-trip 与 iterative freeze 维持原 wire JSON 接受范围；depth2000 fixture 红绿日志覆盖。 | 修复及独立静态复核关闭。最新 55-case 完整套件仍无绿灯证据。 |
| fixtures1：fixture 抽取改变交易语义或弱化测试 | 当前 fixture 仍只生成/承载测试输入；沪深/类别/单位和恢复拒绝由原生产类型与测试断言承担。selected cases 有 source 映射记录。 | 未发现新语义漂移；不将有限代表性短 case 说成全套测试。 |
| 新候选：Writer 新增测试改变了失败原子性 | Writer 仍逐文件顺序写入，失败允许留下前序文件；测试恰好断言此契约，不引入 staging/rollback。 | 不是回归；保留“不保证原子发布”边界。 |
| 新候选：R1 修复改变 inventory canonicalization 或 duplicate 语义 | JSON stringify/parse 保持输入 key order、extra 与 duplicate；旧 identity digest 仍先对原 decoded record 验证。 | 未见新问题；运行范围仍受 4/4 定向而非 55-case 限制。 |
| 新候选：Auction/Save fixture 的常量可能混淆真实 A 股规则 | `AuctionFixture` 的证券类别、exchange、tick、limit 与 seed 是测试 setup；review 记录说明引用的是已有交易规则，未声称本轮联网核验。 | 不作为新领域缺陷；不将 synthetic fixtures 作为现行规则依据。 |

## 结论

三篇指定文件均已完整扫描到 EOF。build B1 撤销有端到端 validator 反证，B2 测试源码补齐并与当前真实 Writer 调用链一致；deadline R1 修复与独立静态复核闭环，但最新 55-case 完整 Node 短测仍未在记录中核实通过；fixtures1 的静态复核与后续 root 编译/代表性短测记录并存，不能外推为全套 Cargo 验收。A 股语义判断仅是“本次工具及测试 fixture 重组未改动领域规则”的静态确认，交易规则仍沿用正式文档既有日期，未在本轮重新在线核验。未发现需新增登记的产品问题。
