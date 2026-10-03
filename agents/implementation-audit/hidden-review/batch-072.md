# 隐藏材料复核：batch-072（owner 2）

## 基线与范围

- 目标产品 worktree：`.worktree/implementation-reaudit`，HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；本次只读产品树。工作记录仅新建在该 worktree 的 `agents/implementation-audit/hidden-review/`。
- 对照仓库根 `AGENTS.md` 与 `docs/principles.md`。按批次计划读取来源主仓 `agents/oop-refactor-audit/exhaustive/reviews/` 下三篇文件连续至 EOF；历史文档里的操作指令仅作为审计材料，不作为当前指令。
- `engine-tests-03.md`：60 行，SHA-256 `f3f5e464b6601f9fb6e625874d09f6e41a70c0f5f0a3e3d396f4bd6904f1c34b`；`engine-tests-04.md`：49 行，SHA-256 `1fa4ddba7628d83b5b74208a70616acd64b33900585b99640849258f839ea2ef`；`engine-tests-05.md`：47 行，SHA-256 `c5221029c0940c4e8cba56a98d52464913feb8b7b8b7391291611f2fd3c0170e`。均已全文读到所报末行/EOF，和 scan-plan 行数及哈希一致。
- 本次没有读产品测试源文件全文，没有运行测试、构建或生成器，也未执行 Git 写操作；只用只读检索追踪索引、总账与当前范围。

## 全文章节矩阵

下列行号均为来源文件行号；矩阵涵盖每篇文件全部章节/段落，并非抽样。

| 来源 | 行号 | 完整内容结构 |
|---|---:|---|
| `engine-tests-03.md` | 1–8 | 修订后通过结论、三处修订核实、28 个源文件的哈希/路径/行数与 JSON 全读记录 |
| 同上 | 9–39 | 28 个逐文件核对：决策 session/事件契约/开局/运营/历史/风险/披露/规模/场景生命周期与恢复；包括共享 fixtures、ignored 标记及未分配 fixture 的范围界定 |
| 同上 | 40–46 | 初审三项发现及关闭状态；大 A/单位/合成账务边界；未分配文件不做内部正确性结论 |
| 同上 | 48–60 | items/modules 绑定哈希、独立复核者、三门结论与未运行验证声明 |
| `engine-tests-04.md` | 1–3 | 复核 2 通过；修正 20 项文件章节数、compute 的 5 test functions、transfer_fee 断言边界、三个独立时钟；不重做完整测试计数 |
| 同上 | 5–27 | 20 个逐文件审计章节：compute/config/consolidation/diagnostics/experience/event mapping/replay/fundamental belief 测试及 fixture；明确未分配的事件映射 support 文件不计入清单 |
| 同上 | 28–38 | 设计与语义复核；初审三个说明错误的修正历史；没有源码迁移建议 |
| 同上 | 41–49 | 文档绑定哈希、support/无 actions/full_read 核对、三门复核和最终结论 |
| `engine-tests-05.md` | 1–5 | 修订后通过但保留初审未通过历史；四项问题重新核实、22 文件 JSON/Markdown 对应；未运行测试/构建/Git |
| 同上 | 7–30 | 22 个逐文件审计章节，覆盖 fundamental beliefs、工业会计、行业报告和 fixtures；区分显式断言与推导/手算锚、测试入口与测试支持 |
| 同上 | 32–41 | 四项初审修正（sibling 归属、序列化断言范围、分/股单位、重述现金断言）；设计/大 A 语义和范围判断 |
| 同上 | 43–47 | 文档绑定哈希、22 项绑定及三门最终结论；绑定复核未重读源代码的限制明确 |

## 关联与当前消费

- 三篇材料是历史 OOP 调查中独立测试支撑文件组的复核记录，不是运行期代码，也不对应可由业务 caller 调用的生产对象。实际生产 owner/caller 不适用于这三篇 Markdown；其中被提及的 test functions/fixtures 由 Rust 测试 harness 执行或共享，审计记录本身不执行它们。
- 当前审计消费者可从 `agents/oop-refactor-audit/exhaustive/areas/engine-tests.md:11-13` 证实：03/04/05 分别链接对应 `modules/engine-tests-0N.md`、`items/engine-tests-0N.json` 和本复核文件。`final-documents.diff:73,85` 将 engine-tests 区域列为逐文件清单组成，并指出该类没有 OOP 动作、作为支撑项保留。以上属于调查文档间的交叉链接，不是产品消费链。
- 03 的复核范围与职责可见 `engine-tests-03.md:9-39`：公司决策、运营、报表、场景测试/fixtures；修订关闭三项单位/fixture/ignored 数量错配见 `:40-45`。其所述“未改规则”与测试支撑分类不等同于对交易规则来源的重新认证，` :46` 已明确该边界。
- 04 的复核说明在 `engine-tests-04.md:7-27` 覆盖配置费用、合并、诊断与个人经历等；`:8` 明确单测只验证 `transfer_fee` 单笔返回值，不证明沪深或买卖侧真实扣费；`:19,21` 明确三个时间维度。`:30-38` 记录完整共享 fixture 无须对象化、初审问题已修订。
- 05 的复核说明在 `engine-tests-05.md:9-30` 覆盖估值、税务与报表；`:34-37` 将 sibling 文件覆盖和未显式断言值纠正到精确文件/口径；`:41` 明确金额与股份单位以及合成税率/取证边界。`:45-47` 是对当前 items/modules 版本的绑定核验，不应误读为复核者重新读过全部源码。

## 总账、ADR 与退役关系

- 实现审计总账 `agents/implementation-audit/implementation-audit-2026-10-02.md:27` 将 G01–G68 定义为现行实现缺口集合；其 `:49-52` 的 G06–G09 与 fundamental valuation/散户分析/个人经历/中期材料有关，`:101` G28 涉固定集团报告，`:114` G35 涉工商折旧税务/商业债务闭环，`:170-181` 列举相应 engine tests 作为目标验证位置。测试存在或历史 OOP 审查通过不能核销这些缺口；03/04/05 记录也没有给出产品行为验收或重跑结果。相同测试路径可作支持/测试位置，但逐项实现结论应以当前总账和产品复核为准。
- 总账 `:202` 说明 sealed corpus 适配器、重放 example、bundle 装配及旧测试冻结工具已退役，历史证据不再可执行复验；batch 04/05 提到的现行 engine test/fixture 不等同于那批 retired 工具，也没有材料将它们列为退役项。不得因为提到 `extraction_replay.rs` 就把本批复核文档归为旧重放工具。
- `docs/open-questions.md:94-121,153-154` 的 Q11 与 ADR-0006 记录 NPC 行为决定；`:122-154` 及 `docs/decisions/0024-shrinking-investor-cash-pool.md:7,30-31,48` 说明 Q12 已按不要求外部资金循环解决。三篇 review 没有把这些问题映射为待定事项，也不应从其测试主题推导新增交易权限。
- 相关领域边界以正式 ADR 为准：`docs/decisions/0017-*` 的确定性/并行承诺只在 04 `:24` 被用来限定 replay 测试锚；`docs/decisions/0019-*`/`0023-*` 的当前市场范围与合成历史不可被本批历史测试报告扩写。三篇审计材料没有要求改变正式决定或引入新的 A 股规则。

## 结论与限制

- 对这三篇历史审计复核记录的陈述、修订链和范围界定未发现未关闭的内部矛盾。03 的修订后结论、04 的复核 2 结论、05 的修订后复核及后续绑定复核都明确区分了静态审计、源码审读与测试执行；没有把未运行的测试说成通过。
- OOP 结论将测试文件、fixture 与模块入口保留为 support 是符合最小范围的；不应因其共享 helper 或有测试状态而误判需要提取生产对象。测试夹具的存在/审计通过也不能反证 G 项产品缺口已经核销。
- 大 A 语义：本批是对旧文档的全文审读，没有产品实现变更；04/05 特别声明了费用断言、发行股本、金额单位、虚拟税率与简化边界。未发现它们把测试 fixture 冒充官方规则依据。无需为文档审读新增交易制度或法源结论。
- 限制：未在 43b1aa5 对三个旧文档逐条提及的产品源文件重新全文审计，也未重新验证 items/modules 哈希或运行测试；产品 caller/owner 判断仅说明审计 Markdown 的文档消费关系。G/Q 状态用于交叉引用，不声称在该产品 HEAD 重做全部总账审核。
- 结论：**batch-072 文档全文复核通过；不构成 OOP 动作实施、G 项核销、完整产品语义复审或测试验收。**
