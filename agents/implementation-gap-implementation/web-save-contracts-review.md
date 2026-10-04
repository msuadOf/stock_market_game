# Web save contracts 独立复核

## 范围与依据

- 复核者未实施产品改动；审查工作树为 `.worktree/implementation-audit-final`，对照现有 HEAD 的 `apps/web/src/save/` 全部变更（含新增文件）、`SessionSetup.ts` 与 `SaveSlot.ts`。
- 已阅读 AGENTS、principles、architecture、open-questions、ADR-0025/0026，以及既有 company-assembly-review；对照 Rust `company/events.rs`、`company/operations/core.rs`、`company/real_estate/loans.rs`、`session.rs`、`session/persistence.rs` 和公告校验。
- 此批为权威事实的 Web 类型/严格 parser 对齐，不新增交易制度。依据沿用 `docs/trading-rules.md` 的既有 A 股规则与公司经营简化登记，本复核未独立联网复取官方规则。不得将 PaymentFailure 公告解释成法定重大违约、停牌、退市或股东清算。

## 语义、必要性与边界

- G35/G41：PaymentFailure 保存事项和两位小数会计金额，按 i128 分检查正数及上界；它只能是单日、幅度 0 的公告事实，不能注入 active shocks。与 Rust `ShockKind::applies_to`、`AnnouncedEvent::validate_payment_failure` 一致，不引入投资者补钱或经济冲击系数。
- CompanyOperations.payment_failures：必填，日期严格早于 next_expected；记录非空、公司存在、事项非空、金额为正，与 Rust `validate_payment_history` 一致。缺字段不迁移，不补默认值。
- RealEstate.maturity_date：必填真实 CivilDate；允许早于 last_accrual_date，因为逾期贷款须继续保存，不能因付款失败伪造新到期日。范围及测试必要。
- G28/G36：setup 的 company_operations/groups 保持 Rust serde 允许的省略，显式配置完整校验；root groups 必填。Industrial trade_counterparty_events 仅存往来归属身份、不另存派生金额。groups 拒绝重复成员、自持、非安全正整数股份；成员存在性、持股比例及 setup/save 一致性仍由 Rust authority 校验。本复核不覆盖公司装配 reviewer 未闭合的会计/真实入口结论。
- Generated SessionSetup/SaveSlot 新字段与 session.rs 的 ts-rs 属性对齐；没有无关产品扩展。SaveSlot 其余 ts(skip) 字段是既有分工，由 StrictSaveEnvelope 维护，不能把生成类型当成完整存档校验器。

## 发现与门禁

1. **G07 暂不放行：Retail 账户暂停状态跨层漂移。** Web 新增拒绝 Retail.institution_account_risk_paused=true；Rust BeliefBook serde 仍接受该 bool，`session/persistence.rs` 只拒绝非机构 institution_policy，没有对应 paused=true 语义校验。已通知作者交 Rust persistence 唯一 owner 修复，并要求真实 restore 边界短测与再次复核。这是静态调用链发现，未声称已运行新增 Rust 复现用例。
2. 最终 root binary 的 ts-rs 再生成门禁未完成：作者报告两个导出并发运行均触发 10 秒 timeout，生成产物存在并复制到正式路径，tsc 通过。这里仅记录作者报告，未把 timeout 或产物存在写成生成门禁通过。
3. 尚缺真实四行业/集团日终存档的 Rust 输出→Web strict parser 往返证据；仅静态形状和人工 fixture green 不等于完整跨宿主验收。本轮不运行完整回归。

## 独立定向验证

命令：`node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=4 apps/web/src/save/company-contracts.test.ts apps/web/src/save/company-books-schema.test.ts apps/web/src/save/schema/personal/beliefs.test.ts apps/web/src/save/complete-save-schema.test.ts apps/web/src/save/save-schema-v2.test.ts apps/web/src/save/company-policies-schema.test.ts apps/web/src/save/company-schema.test.ts`。

- Node v25.8.2；4 个并发测试文件预算，case timeout 与进程树外 deadline 均为 10000ms。
- 输出为 7 个测试文件通过、0 失败，Node duration 554.67ms，命令 wall 0.71 秒。文件级汇总不是内部 case 数量，也不是 CPU 利用率或完整回归证据。
- 覆盖 Retail null/false 与非法状态、PaymentFailure 金额/事项/active 注入、记录日期与公司关联、单日公告、到期日缺失/非法/逾期、groups 结构及往来身份；未修改断言。

## 当前结论

G35/G41 的 Web contracts 与 maturity_date 变更静态复核及定向短测通过；改动必要且范围受控。整体暂不宣称完成，G07 最新静态修复结论见下节，最终生成门禁仍待完成；G28/G36 真实入口仍以对应独立 reviewer 结论为准。

## G07 修复增量复核

- 已独立查看最新 `session/persistence.rs`：非 Institution 且 `institution_account_risk_paused()` 为 true 时返回带原因的 InvalidSave；正常 Retail false 不受影响，Institution true 仍保留。该约束只修复机构个人记忆错误归属，不新增散户策略暂停制度。
- 新增 `retail_restore_rejects_institution_account_risk_pause` 使用真实 Retail 新局、先断言保存值 false，再编辑为 true、经 SaveSlot 反序列化后断言真实 restore 明确拒绝；实现与失败边界测试针对根因且范围最小。
- 原发现 1 的跨层静态漂移已消除；Rust owner/root 统一编译与该用例动态短测未完成。本复核按要求不重复 build，不宣称 Rust 新用例 green。
- 最新 Industrial inventory_source_events 已由 company-assembly reviewer 静态审查；本报告不把早先 Web fixture green 当成该追加字段或最新三份真实 JSON 桥接验证。等待作者最新桥接结果及独立复核。

## 最终 binding 与真实 JSON 增量核收

- 作者提供最终 root 共享 binary 的两项精确 export 测试证据：并发 2、各 4 test threads、共享 10000ms deadline，显式隔离 TS_RS_EXPORT_DIR，两个 case 全绿，约 0.63 秒。复核者独立核对 binary mtime 为 2026-10-04 16:25:17，并实际执行两次 cmp：正式 SessionSetup.ts / SaveSlot.ts 与 `.tmp/web-save-binding-final/{setup,save}` 新产物逐字节一致。未重复 build 或导出运行。
- 原发现 2 的最终生成门禁已由最新成功证据替代；早先 timeout 仍是失败历史，不能改写为通过。上述 export green 为作者执行报告，cmp 为复核者独立执行结果。
- 复核者独立读取 `.tmp/company-assembly-review-v2/{four-industries,mixed-group,internal-sale-group}.json`，逐份执行 parseSaveSlot 与 assert.deepEqual 全量比较，并删除真实 Industrial.inventory_source_events 作负控。三份 bridge 和三份缺字段拒绝均通过，使用 10000ms 进程树外 deadline，无构建、无完整回归。这消除了本报告发现 3 的代表性真实输出 bridge 证据缺口，不替代长期会计验收。
- 最新真实 JSON 的生产测试 3/3 Rust green 由公司 reviewer 提供；对应会计语义及 G28/G36 整体放行仍以其完整复核为准。本复核只核收 Web contracts。
- 当前 Web contracts 无未修复静态发现；G07 新 Rust restore 用例的动态核收仍交 root，不把静态复核或 bridge 当成该负控用例 green。
