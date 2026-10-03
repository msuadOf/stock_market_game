# 续核发现独立审查

## 范围

- 基线：`.worktree/implementation-reaudit`，`HEAD c0ab4299d104f07589008fee1af886198a2f783b`。
- 连续读取 `bank-restore.md`、`clock-bounds.md`、`belief-identity.md` 全文；只做静态核验，不运行测试/构建，不做利用复现，不修改产品源码、三份原报告或总账。
- 复核了恢复入口、对应 validator/caller、正式契约段落及相关 G/Q；未对大 A 交易制度提出新规则主张。

## 发现裁定

### Q19 / Bank ECL 恢复

- 新局装配确实固定为 `Industrial`，所以默认开局没有银行业务闭环；但这不足以证明完整 `SaveSlot` 恢复不可达银行状态。`SaveSlot.company_operations` 保存并 serde 解码完整 `CompanyOperations`，`GameSession::restore` 通过校验后将保存值整体安装；`validate_company_domain` 未按 `CompanySpec.kind` 与 books 类型配对，也未调用 `BankBooks`/`EclPolicy` validator。因此携带 `BankBooks` 的恢复输入在静态类型与调用链上可达，结论限于恢复边界，不代表默认游戏旅程能生成银行会话。
- `EclPolicy::validate` 校验情景表非空、单项范围及各表权重合计 10000bp；`BankBooks::new` 调用它。`docs/superpowers/specs/2026-09-13-company-information-learnings.md` 任务 9 要求构造期与逐次重估校验，任务 27 的恢复契约要求恢复边界做业务状态校验；`docs/open-questions.md` Q7 明确外部权威 `SaveSlot` 经 Rust engine 深度验证。故完整 SaveSlot 恢复缺少 ECL policy validator 可登记为恢复校验缺口。`initial_allowance_target` 上方“serde 恢复不新增政策校验”的注释仅描述库级反序列化兼容和既有错误次序；它不豁免外部 SaveSlot 的深度验证契约。
- 结论不要求改变 `BankBooks` 独立 serde 接受集，不要求每次 `issue_loan` 重复整表校验，不把合法编辑资产判为非法，也不声称已动态复现。Q19 应更正“完整会话恢复尚未开放银行”所导致的不可达推论；G36 默认行业装配边界仍独立保留。与 G73/G79 的恢复深验关注可去重，但它们是不同 owner/状态不变量。

### CivilClock 日期边界

- `CivilClock::from_parts` 对每条 pending 只验 ID 小于游标及日期不早于 current；`register_due` 还调用 `calendar.day_status(exchange, due_date)`，后者按冻结政策范围拒绝超界日期。`validate_company_domain` 将时钟队列用于与 scheduler 的包含匹配，但没有补日期范围校验。
- 依据 K1 `docs/simulation-calendar.md` 的 2099-12-31 推进上界和恢复冻结政策，恢复可能接受注册入口不能创建、且运行期不能合法触发的越界 pending due。报告只主张 pending 日期缺范围校验，没有扩大为任意 current_date 越界；该边界有据。建议并入 G78，不新开 G；G15/G71/G75/Q13 不等价。

### BeliefBook 身份关系

- 报告确认的账户 key 与 `book.npc()` 校验，以及 `StrategyState.profile` 与重建策略 profile 校验，均由代码支持。创建时 `BeliefBook` 接收同账户策略 profile；其内部 profile 参与 belief 修订/horizon，故存在 profile 错配的潜在语义风险。
- 但当前正式契约未证明恢复后 `BeliefBook.profile` 必须等于同账户策略 profile，也未证明 belief profile 是不可编辑的身份字段。ADR-0016 允许个体分析能力和策略风格解耦；因此不能把 `AnalysisProfile` 差异、经验 policy 阈值编辑或合法分析差异判成身份错误，也不能仅由创建时初值一致推导外部存档必须强制相等。此候选不升新 G；建议作为未决身份契约保留，待正式恢复契约定清后再裁定。
- 去重上，G07 讨论分析能力与账户身份解耦，不等价于解决 profile 是否必须相同；G42/G43/G69/Q11 也无直接重合。本轮不据此改写旧项。

## 来源凭证

- `belief-identity.md` 写出的三个来源行数与 SHA-256 已用 `wc -l` / `sha256sum` 程序复核，全部匹配：ADR-0016 为 119 行、`1b3c7deef0a29a0f3f628b2b4455713760f9a73619d477f72ac4863070a52ae5`；`engine-strategy-02.md` 为 343 行、`17e17cf8b54910d01cb9b0cf9c58224a4f60f831cb450bcc89e7a67c53ef700b`；`batch-042.md` 为 35 行、`35c953978121f3fc19e6669c5a884d81ce50b2f7a0a23fd8326c9a3c3c9f9cbe`。各文件 `tail -n 4` 的末段与报告 EOF 描述一致。
- 银行报告指定来源当前凭证：`docs/company-accounting.md` 251 行，SHA-256 `c93d5d25ad67725ad3028198ad483ff0eb2231e2428f51a3b1e777da78f4b0d7`；`luna52.md` 52 行，SHA-256 `9411bbabaf0be4484713eaa19079c4b8b031715af323d1c4b6484c4084e488c1`；`batch-155.md` 19 行，SHA-256 `c22fd0c56404a98814494e23a4fa8787786522aba3477def3231bddfa69dfa2a`。时钟正式来源 `docs/simulation-calendar.md` 为 125 行，SHA-256 `40e677df7f5ae1ce9b37a588ce6c9957e277033eddcd50c98df9e429e4e8132f`。行数、哈希及 tail 末段由程序取得，未手工录入推算；哈希仅标识本 worktree 当前文件。
- 续核报告未为银行/时钟指定来源声称 SHA 或行数；以上是本次独立采集，不回填成报告作者曾核验的凭证。`luna52.md` 与 `batch-155.md` 已连续读取至 EOF；正式来源按相关章节复核。未声称重新核验交易所现行规则。

## 交接

- 三报告中 Belief profile 等值判断应维持未定，不据此登记新 G；银行和时钟报告可供总账维护者分别裁定 Q19 与 G78 更新。
- 当前目标 worktree 中 `packages/engine/src/session/persistence/v2.rs` 存在，策略身份比较实际位于约 291–313 行；报告写出的 `persistence/v2.rs` 路径在该基线有效。`saved_runtime.rs` 在该 worktree 不存在，故不应把此处报成路径错误。
- 本记录未审查主控随后要求的 7–10 文件完整文档 diff，也不代表该 diff 已通过独立门禁。
