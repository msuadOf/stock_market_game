# Q14 公司债权来源身份独立复核

## 范围与证据

本批 reviewer 未实施代码，完整读取新增 `customer_finance/claim_identity.rs`、`customer_finance.rs`／`restore.rs` 的全部本批增量与 `q14-creditor-binding.md`。`company/mod.rs` 仅新增 `CompanyClaimIdentity`／`CompanyClaimSource` 导出属于本批；税务、更正和 `share_registry` 并行 hunk 不在本批审批范围。没有启动 Cargo、修改产品代码或提交，也不覆盖先前已完成的恢复和债权人绑定复核。

已逐项亲读 `.tmp/checklist-wave4/q14-claim-host39/` 的日志与结果：28 项实际运行，22 项通过、6 项真实失败。失败分别为 4 项明确未实施编码导致失败、登记和恢复的 2 项错误公司归属断言失败；每项运行 1 case，不把编译错误或零 case 当真红。当前已实现编码／解码和新增严格编码 guard，共 29 项；最终绿待主任务提供新产物实测证据。

已进一步亲读 `host40-binaries.json` 及 `.tmp/checklist-wave4/q14-claim-host40/` 的清单、结果和全部 29 份 case 日志：实际产物 `engine-48220fcc6e1a5073` 下各项实际运行 1 case、1 passed、0 failed、0.00 秒，结果均退出零。该证据支持已冻结来源编码实现与旧稳定测试 body 29/29 通过；后补的 8 个字段负例在此编译期间写入，不能以同名 case、二进制字符串或该批绿证明这些新断言已经执行，仍待下一 fresh 构建补验。工作文档准确登记这一证据边界，未扩张为最新断言全绿。

最终亲读 `host41-binaries.json` 和 `.tmp/checklist-wave4/q14-claim-host41/` 的清单、结果及全部 29 份日志：实际 Engine 产物下均为 1 case、1 passed、0 failed、0.00 秒、退出零。新增字段断言文件修改时间为 22:38:24，早于本次 fresh 构建，最新测试 body 验证已补齐；不再依赖 host40 猜测覆盖。主任务采用整批 10000ms、每 case 9000ms、8 进程并行和 Rayon 4，整批约 0.48 秒。本 reviewer 未额外启动 Cargo 或完整回归。

## 领域与必要性

强类型 `CompanyClaimSource` 区分工商应收、贷款本金、每笔利息计提、地产预售、地产交付尾款及保险应收保费；本金与利息不共享一个合同号债务，每笔利息另带公司域实际 `BusinessEventId`。该类型表示客户欠公司的来源，不包含保险赔案等公司应付或银行存款。公司身份必须保留，不将局部账套事件或经营序号冒充全市场唯一身份，符合已确认 Q14 的真实来源／去向要求。

这不是新增 A 股证券交易制度，沿用 Q14 的 CAS 14／CAS 22 收入、回款与核销分离边界。本批仅构造和绑定债权身份，不确认收入、生成应收、补现金、指定经济默认参数或声称完整经营闭环。地产预售未收款转交付尾款不得增加第二份相同法律债务，生产接线仍需明确重分类关系；文档保留该未完成依赖，没有靠六类身份自动制造两份债权。

## 完整代码检查

- `CompanyClaimIdentity` 字段私有，具名构造和严格 `ClaimFacts` 恢复均验证非空公司及对应来源。没有 schema、迁移或默认补齐。
- 保留 `company-claim:` 职责前缀和固定来源类别，以 UTF-8 字节长度编码原文。解码显式拒绝未知类别、非 canonical 长度、溢出、截断、UTF-8 半字符及尾随内容，并重编码比对；不同来源段可唯一分解，无冒号／数字／Unicode 碰撞假设。
- 利息来源采用局部 canonical u64 十进制字符串 serde，MAX 无损；不接受 JSON number、加号、前导零或溢出，也不顺带改动其他 `BusinessEventId` 契约。
- 具名普通 `DebtId` 返回 `None` 表示另一类显式债务，不是错误时兼容 fallback；带保留前缀但不合法的编码明确失败。
- `register_company_claim` 仅派生确定性债务身份和同公司债权人，复用原注册校验／原子性。generic 注册与严格账簿恢复均交叉验证编码公司和实际 `DebtCreditor::Company`，不能把甲公司债权付给乙公司。
- 原 21 项金额、付款、核销、回收和恢复断言未变。本批 8 项覆盖跨公司相同开项、分隔符与 Unicode、来源类别及各次利息、共享客户实际 100／50 付款、重复拒绝与恢复幂等、非法数字来源、错误 owner 注册／恢复及严格编码负例。

## 发现与门禁

**本批限定 PASS。** 未发现本批代码阻断问题或不必要的功能扩张。严格字段覆盖建议已由作者落实，并经 reviewer 亲读增量：现有 `typed_claim_restore_requires_valid_identity_and_decimal_accrual_source` 加入 8 个顶层／source 变体缺字段、未知字段和重复字段负例，先确认每段都是合法 JSON，再要求强类型拒绝，未弱化原身份和精度断言，没有增加 case 数。最新 host41 29 项实测已经补验此 body。完整审阅 `q14-creditor-binding.md`、`q14-customer-cash.md` 和 `q14-finite-ledger-followup.md` 全部最新工作 diff，实际红绿与生产依赖口径准确，没有把编码身份的完成等同于真实 Owner 收付闭环。

依赖亦独立核验：本批三个来源文件不存在 `canonical_u64_decimal`、orderbook 或 session 引用；`CompanyId`、`OpenItemId`、`ContractId` 以及 `BusinessEventId::new/value` 在 HEAD 原有实现中已存在。本地 `business_event_decimal` 为新来源字段实现 canonical 字符串，JSON 和编码反解共用 `parse_accrual`；不是 AccountWire 未提交 helper 的兼容包装。整体统一构建使用其他并行 AccountWire 改动，不能据此认定本叶子直接依赖这些改动。此结论不审批并行 `share_registry` 或其他文件。

公司真实 Owner source 关联、银行利息返回来源、地产原签约／到期事实及预售转尾款不重复债务、共享客户簿与公司 `Books` 原子收付均为后续依赖，不因本批身份构造局部通过而核销；源类别本身也不证明已经存在真实账务来源。
