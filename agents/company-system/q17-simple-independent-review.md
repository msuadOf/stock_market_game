# Q17 Simple 更正独立复核

## 结论

**本限定增量复核通过。** 当前源码静审及 fresh host68 Q17 Session 短测符合所审契约。此结论仅覆盖本记录列出的 Simple 更正路径，不外推为完整回归通过或 CAS 28 合规声明。

## 依据与发现

- 当前 `end_civil_day_after_session_check` 在 `CompanySystem.advance_day` 前应用 pending correction。这是修复方向：更正 entry 按原始实际日期入当前仍开放期间，随后 Simple `advance_day` 对同一自然期间生成经营摘要、执行税务重估并关闭期间；不放宽 `ClosedPeriod` 守卫、不改写 entry 日期。日终外层 checkpoint 在后续 advance/disclosure 失败时恢复整个状态，成功才提交。
- host65 原失败日志 `.tmp/checklist-wave4/host65-q17.log` 显示 4 绿 3 红；host66 `.tmp/checklist-wave4/host66-baseline-q17-green.log` 显示 shared correction 核心 16/16 通过、Q17 Session 6/7 通过。作者修正输入错误映射，并保留 reason/detail 后，host68 `.tmp/checklist-wave4/host68-q17.log` fresh 执行 Q17 Session 7/7 通过，0.97s。动态通过覆盖：理由和拒绝详情映射、税务不变量 fatal 分类、completed provenance 拒绝、失败的整日回滚及 pending 保留、精确请求去重/旧 epoch 拒绝、成功日终发布/恢复幂等、过期目标拒绝后取消并重选、月末更正及恢复来源。
- 更正 prepare 仍先在候选 Books 上计算税务差额，再调用 ClosingEngine 事务性 `transact_post_batch` 实际入账凭证并生成更正版。此顺序使有效自然日实际日期保持真实，同时更正报告、税务 Position、重述映射由候选整体提交。失败应保留原系统、公开库、税务状态和 pending 请求；同凭证在外部输入修正后仍可重试。
- host67 初版 `InvalidInput => InvalidRequest(detail)` 曾丢弃更正理由；作者修正为在错误文案中同时包含 reason 和 detail，分类单测断言两者，stale case 保持原有 `InvalidRequest` 匹配。host68 7/7 通过，关闭此 finding。
- forged-company restore 测试现直接伪造 completed request 的 `company`，与原公开报告不一致，host66 通过预期错误断言；先前“夹具缺少第二家 Simple 公司”的 finding 已修复并撤回。
- 静态检查确认 `correct_report` 在本地候选 `CompanySystem` 与 `PublicLibrary` 上执行，成功后才同时替换调用方状态；`install_correction` 也先 clone、设置候选 owner 字段并 `validate`，成功才安装。因此这两个接缝本身支持失败无部分提交。共同 `prepare_correction` 的入账失败目前仍是上述阻断点。
- 所得税路径把更正后的候选账簿、原 `IncomeTaxPosition`、政策、来源序号及 prospective restatement map 交给 `preview_tax_cascade`；将产生的税务分录并入同一更正凭证，再验证候选税务与 ClosingEngine 重述映射相等。方向符合“损益重述目标期间、实际现金流依实际日期、税务差额随更正重估”的既有游戏契约；但因 `post_batch` 提前拒绝有效关账期间，该端到端语义尚未通过 Session 证据确认。依据文件将 CAS 28 标为取证受阻，故不作 CAS 28 合规主张。
- `validate_completed` 核对 operation key、原公开 Simple 单体报告与公司身份、实际 Journal 凭证完整相等、税务/Closing 重述映射及期间，以及新公开版本来源、发布时间、Correction origin、scope 和 supersedes 关系。原报告在 `PublicLibrary` 中保留，通过 append-only 新版本实现；恢复校验不是完整证明任意历史来源，但当前本次任务的 forged-company 用例仍因 fixture 错误而未动态验证。

## group_time fixture 核查

- 更正核对后，`packages/engine/src/session/company_groups/correction_time_tests.rs` 已正确设置 standalone 原件 offset 0（2030-03-20 18:00）、consolidated 原件 offset 7（2030-03-27 18:00）、失败尝试 2030-03-21、成功 retry 2030-03-28。root 提供的 host64 证据为 group 8/8 通过。此前将该 fixture 日期报告为错误是本复核失误，现撤回，不构成 finding。
- 此 case 保留单条同来源成功 retry 断言是必要的：它与首次失败候选不提交、旧版本不可变共同证明失败后可重试；不需要增加重复 retry 断言。

## 复核边界

- 本次为静态审查加 root 提供的短测日志审阅；没有自行运行 Cargo 测试。
- 未检查仓库内无关并行改动，也未修改生产代码。host68 是 7 个定向 Session case 的短验收，不是完整 engine/workspace 回归；A 股交易制度没有因本增量发生改变。CAS 28 更正/重述原文仍按正式台账标记取证受阻，本复核不判断准则合规。
