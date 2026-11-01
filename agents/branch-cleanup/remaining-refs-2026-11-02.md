# 剩余未绑定 worktree 的分支核销

基线：`codex/branch-convergence-2026-11-01`，`fe7cc863`。核销只处理未绑定 worktree 的本地 branch；所有 worktree、stash 和 WIP 保持原状。检查时共有六个候选。

| 原 branch 与 HEAD | 分类与处理依据 |
| --- | --- |
| `backup/pre-rewrite-main` `ef5be1c5` | 旧 main 重写前的历史锚点，存在多个 merge base；创建 `archive/pre-rewrite-main-ef5be1c5` tag 后删除 branch。 |
| `codex/archive-root-evidence-miscommit-20261030` `17605579` | 线程原型绑定证据误提交，创建 `archive/root-evidence-miscommit-17605579` tag 后删除 branch。 |
| `test/company-b4e-actor-recovery-2026-10-28` `94d1f0ad` | 双宿主更正拒绝恢复测试；基线有同名测试并扩展，但尚未逐断言确认覆盖。`4f87bb86` 与其 stable patch-id 同为 `7c7253998f460af66d0c8d1ce71d707e933f5a54`，但不是基线祖先。保留 branch。 |
| `feat/b4c-rights-offering-r3b` `429e9182` | 公司回购批准与 Web 严格存档合同；含独有证据，且有对应保全 stash。保留，后续逐项核对；不能仅凭非祖先关系认定未吸收。 |
| `feat/company-b4e-commands` `9c671946` | 公司现金分红、送转与更正提案候选；有对应保全 stash。基线存在后继实现，但候选字段已经演进，保留并按合同逐项核对。 |
| `integrate/product-union-2026-10-28` `87576b7a` | 自然日日终、回购 Calendar、Closed retained authority 与 WASM 收尾组合；基线已有后继但七个独有提交跨多个领域，保留并按行为和测试逐项映射。 |

独立只读复核确认历史锚点、公司候选和 product-union 的保留风险；另建议对 actor 测试核实吸收。随后发现 stable patch-id 相同的 `4f87bb86` 并非基线祖先，已恢复原 branch；不能据此断言吸收。此项是测试历史核销，没有变更 A 股交易规则或产品树。共删除两个 branch，均转换为 tag；四个候选保留。未运行产品测试。
