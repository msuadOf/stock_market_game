# Tax-only 投影首片实际证据

日期：2026-10-27。唯一 writer 为 `.worktree/tax-admin-projection`，起始 HEAD `1f4c8fe6`。
初始仅未跟踪合同；原件与状态保留于 `initial-state.json` / `contract-before.md`。

`red-01` 为 harness 缺少 crate re-export 的编译失败，不算业务红。
`red-02` 编译成功，真实 World fixture 因 stub 返回“税投影尚未实现”失败，算业务红；stub/test 原件为 `red-source.rs` / `red-tests.rs`。
`green-02` 为补充边界测试的 borrow 编译错误，`green-03` 为 fixture 误认为 World 拒绝空事务的断言失败；均完整保留，不算实现业务红。
`green-05` 为最终源码绑定的 5/5 通过，源码运行前后 SHA 一致；包含真实 tax-only commit、拒绝后真实账户不变及成功返回后发布、全部非现金字段 Eq 防篡改、坐标集合和 typed 正额/顺序现金边界。
每次构建与执行均由真实外部 `run-with-deadline.mjs 10000` 监督，rustc `codegen-units=8`，harness `test-threads=8`。
构建约 1.35 秒，测试约 0.12 秒；没有 Cargo、target clone 或新 normal rlib。

来源是新叶源码链接既有密封 normal engine rlib（基线 `6ac167e1`，见 receipt 所链 artifact-binding），不是当前完整 crate 构建。
叶暂未注册，生产 caller 尚无；实际混合 tax-stage producer 由 R4 负责，不能以本片冒其交付。

共享 core `inspect_tax_stage` 直接借真实 id、stage_index、group 与原 operation_index，tax-only wrapper 从真实 candidate 取 id；不伪造 AdminCandidate。
准备值不生成 TaxIncomeKind、税事件、序列化、实收或 committed 标记。生产 publish 仍仅唯一外层 commit 成功返回后允许。
本片不计算法定税额、税日或归属；个人 Bonus 生产门继续保留。
