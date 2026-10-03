# Sweep 32：银行历史复核、开工基线与 F3 真实浏览器证据

审查日期：2026-10-03；产品基线 `b76ece3`，工作树 HEAD `4ad5a2e` 为审计 merge。遵循已读根 AGENTS/principles。未修改产品，未执行 Git 写操作或长测试。

连续全文读完：

| 文档 | 全文行数 |
|---|---:|
| `.omo/evidence/company-information-npc-intentions/task-9-review.md` | 203 |
| `.omo/evidence/company-information-npc-intentions/worktree-baseline.md` | 9 |
| `.omo/evidence/escrow-parallel-engine/F3/manual-qa/README.md` | 78 |
| 合计 | 290 |

## 全文章节与条款状态

| 文档/章节 | 原文位置 | 当前代码与结论 |
|---|---|---|
| task-9 标题与 §0 隔离执行 | 1、8 | 绑定旧提交 3bb5096 的 660/0/4，不是 b76ece3 全绿证明。旧命令与临时 worktree 属历史执行记录，无产品待实现要求。 |
| task-9 §1 政策依据/拒绝高级产品 | 30、34 | 当前 BankProductKind 与 UnsupportedContract 仍显式限制业务支持面；不要求恢复 FVTPL/FVOCI/衍生品。依据可用性不能由历史 APPROVE 自动替代当前官方取证，现行法源债沿用主台账。 |
| task-9 §1 存款负债、放贷资产 | 42、44 | `company/bank/deposits.rs:206` 吸收存款及 `bank/lending.rs:20` issue_loan 分录仍在；贷款借1301贷1003、流类 Operating，非费用。 |
| task-9 §1 利息、逾期、Stage3 | 46、48、50 | `bank/loans.rs:136` ACT/365F、`bank/deposits.rs:98` 存款到期截断、`bank/loans.rs:268` 净额法基数存在；贷款不截 maturity 是明确登记简化，不登记“忘了停息”。 |
| task-9 §1 ECL、核销回收 | 52、57 | `bank/ecl.rs:180` 显式情景校验/目标差额，`bank/writeoff.rs:17,66` 核销与回收存在；无用股价跌幅推导信用损失代码。 |
| task-9 §1 现金约束、分类层 | 60、64 | 支付失败显式类型；`accounting/reports/bank.rs:18` 码表、分类函数已有。经营到期存款支付未接线已由 sweep04 S04-N1 提出，不重复。 |
| task-9 §1 手算金样 | 70 | `tests/bank_accounting/gold.rs:31` 全链金样仍在；历史特定金额不要求把现行模拟参数冻结为这些值。 |
| task-9 §1 serde 与标签扩展 | 84 | 当前 BusinessKind 银行语义及 serde 存在；旧存档反序列化兼容主张只限当时提交，不能要求现行严格新 schema 接受旧档。 |
| task-9 §2 必要性/最小范围 | 93 | 模块依赖 company→accounting、独立账套与共享码表仍在；旧22文件/+2893不是当前目录大小约束。原文112拒绝零状态变化须限有效输入，见下方边界观察。 |
| task-9 §3 K3 覆盖矩阵 | 116、121 | 银行纯处理器、四行业报告与经营内核存在；不代表四行业 session 装配/封账/公开查询完成，沿用 G36。 |
| task-9 §3 F1 成功收本测试 | 132 | `bank/lending.rs:78` 成功分录/子账/对手方流实现已存在，经营 `operations/dispatch.rs:105` 有生产 caller；不是未实现收本。见下方 F1–F4 状态。 |
| task-9 §3 F2 零差额、F3 逾期、F4 365天 | 137、140、143 | 分支实现分别为 `bank/ecl.rs:197,221`、`bank/loans.rs:136`、`bank/deposits.rs:148`；原文是明确测试债，不把缺专用测试改写为缺处理器。 |
| task-9 §3 复杂度/方向 | 146 | 当前重构文件与历史布局不同，不能要求恢复四份私有函数副本或旧文件长度。 |
| task-9 §4 偏离裁定 | 154 | ECL不折现/EAD=账面/单利/合同利率等是已登记简化；不把简化逐一算新功能遗漏。银行税与手续费支持面按当前会计范围，不据旧 issues 单方面扩范围。 |
| task-9 §5 Clippy | 170 | 当时共享未提交状态与提交状态区别是历史事实；未在当前运行Clippy，不报告旧0警告为当前结论。 |
| task-9 §6 证据链 | 185 | 红/绿/全套/交叉验证均历史证据，不能替代当前代码与验收。 |
| task-9 §7 残余风险 | 194 | “BankBooks未接session”当时不属本任务；当前四行业 session 缺口已列G36，经营/前史纯内核已实现，不能说所有银行仍未接任何生产入口。 |
| worktree-baseline 全文 | 1–9 | revision=6ad461e、2026-09-10、当时dirty none；纯安全记录，无可执行功能承诺，更不能说明当前工作树干净。 |
| F3 标题/Source binding | 1、6 | PASS 只绑定 bf3d444 与四个旧源码哈希；当前 App/Worker 已重构，未运行当前E2E，不能搬用PASS。 |
| F3 Invocation/bounds | 18 | two workers、临时config、300000ms监督及28.736s是历史真实执行记录；临时配置删除属预期，不登记为缺失工具。 |
| F3 Scenario mapping | 47 | 当前 `e2e/trading-workflows.spec.ts:58,71,87,108,127` 五case仍在；第五case已按day-end新契约改写，详见下文。 |
| F3 console/page-error trace审计 | 60 | 五份历史trace的零错误不是当前全应用异常兜底实现证明。新React异常接线候选已在sweep14，不重复。 |
| F3 Artifact hashes/preflight | 64、76 | 历史制品/失败留档不要求重造；preflight不计测试次数的诚实边界保留。 |

## 四行业、混合报告与银行测试债反证

四行业 report 内核真实存在：`accounting/reports/{industrial,bank,insurance,real_estate}.rs`；`tests/industry_reports/industry_spot_gold.rs:62,131,175` 实际生成并校验银行、保险、地产报告，工商在 `industrial_gold.rs:59`。混合集团并非只有类型：`tests/consolidation/gold.rs:34` 的 Industrial+Bank 80/20 测试调用 consolidate（`:116`）；`accounting/consolidation/aggregate.rs:73` 聚合、`:114` 显式拒绝科目语义冲突，`reports/consolidated_window.rs:52` 消费consolidate并生成报告窗口。因此不能登记“没有混合行业报告内核”。但 `session/company_assembly.rs:341,430,431` 仍固定Industrial，四行业公共报告会话闭环继续由G36覆盖。

F1 当前直接 bank_accounting 成功收本专用断言仍不足，但 `operations/dispatch.rs:105` 真实收本 caller 存在；`tests/company_operations/fixtures.rs:271` loan_term_days=5，`determinism.rs:30,36` 四行业配置跑30自然日，能经过贷款到期 dispatch。原文“成功路径无任何测试触达”不应原样搬到当前；该间接执行也不冒充本金/总账/对手方流三面独立金样已补齐。F2 assess_credit 零差额明确返回None但更新阶段/留痕并消耗event_id（`ecl.rs:221–231`），不能误称对象完全no-op。F3贷款逾期与F4恰365天仍为值得定向验证的测试边界，本次不新建产品G。

## F3 当前五场景映射与新决定

开盘市价拒绝、早段竞价撤单、09:20之后拒撤并转连续、连续竞价不足资金，在当前E2E `:58,71,87,108` 分别保留。旧README`:58`“保存活动连续挂单、刷新恢复同tick”已被 `docs/decisions/0025-day-end-only-persistence.md` 替代：当前case`:127`要求日内不写档，`:139`断言旧档字节不变，`:143–167`推进自然日日结、挂单失效、日终存档刷新恢复并继续。不能据旧截图/任务名恢复日内保存挂单。

早撤case `:71–85`以活动委托消失验证撤单，没有定量断言现金完全回到撤前值；这是既有浏览器证据力度边界，engine冻结/释放有其正式规则测试，不据此宣称资金释放未实现。当前不足资金case存在真实申报/反馈；表单字段级即时错误仍沿用G22。

## 观察但未提升的新候选

历史task-9`:112`有“拒绝零状态变化”广泛表述；当前 `bank/behavior_tests.rs:335` 明确测试手改serde出的 `written_off=true, recoverable=10, allowance=i128::MAX`，`recover_written_off(1)`返回AmountOverflow却next_event_id、Books和recoverable已改变（`:349–353`）。对应 `writeoff.rs:77`先post、`:92`后apply；`loans.rs:339`先减recoverable、`:340`再加allowance。

反证与限界：该fixture是人为不一致账/子账的低层BankBooks serde状态；正常`apply_write_off`在`loans.rs:329`清allowance，合法回收总额受recoverable限额约束；当前session恢复并未开放银行账套（G36）。因此本范围证据未证明正常生产输入可触发，且历史任务复核不是所有低层方法必须事务化的新ADR。仅记录边界观察供总审查判断，不登记为已确认新产品遗漏、也不建议扩大为全方法复制回滚。

本轮没有新增确定产品缺口；已确认内容映射到G22/G36、sweep04 S04-N1及sweep14错误出口候选，避免重复。没有新执行测试，不声称当前四行业浏览器或真实长验收通过。
