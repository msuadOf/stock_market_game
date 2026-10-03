# luna58：OOP 总复核三份材料 EOF 复核

日期：2026-10-03。审计目标产品提交 `08e4fc7`；审计工作树 HEAD 为 merge `a7c7ce3`，两者的 `packages/`、`apps/`、`scripts/` 树一致。依根 `AGENTS.md` 与 `docs/principles.md` 执行。本记录只读指定三篇历史材料及其签署/冻结索引，并记录可证实的当前源码接缝；未修改产品源码、未运行测试/构建/长验收、未作 Git 写操作。三份原文均从首行连续读取至 EOF，无截断：`final-review.md` 50 行、`initial-boundary.md` 45 行、`progress-01.md` 28 行。

## 章节覆盖矩阵

| 文档章节 / 行 | 历史签署、遗留和 caller 线索 | 当前复核判断 |
|---|---|---|
| final-review:1–6 标题、结论、reviewer、签署范围 | 原文写“通过”，署名 `/root/implementation_final_review`；签名对象为 `source-freeze.json` 的340个源码及相邻 ledger、integrity、validation，并明确“后续源码变化不能自动继承”。 | 这是绑定 `8cf34a1` 的历史签署，不是对 `08e4fc7` 再签。`git log` 显示 review 文件于 `8cf34a1` 新增；后续到目标提交间，冻结集合中有11个源码路径变化，故不能整体沿用 PASS。 |
| final-review:8–12，128目标核销 | 记 Domain 39、Pipeline 20、Session 19、Frontend 16、Hosts 34；提及真实 owner/method/caller、旧写口、恢复和 fixture 接线，以及 PositionExperienceTransition 六个 dated writer、microstructure accumulator 等增强。 | 这是对原冻结源码和具名组审查的汇总；本次只读三篇总复核记录，未重做128项或组级源码 diff。历史数量/逐项核销不能自动证明更新后的11条路径仍具备当时指纹。 |
| final-review:14–20，三门禁与兼容范围 | 语义结论明确是承接既有 `trading-rules`/ADR，不重认证全部官方现行制度；记录收窄 Rust 源码 API 但保留 wire/serde 语义，并限仓库内 caller。 | 范围限定诚实，没有把 OOP 审查说成现实制度完整合规证明。当前提交新增/改动内容需由其自身审查证明，旧结论不能用作其依据。A股相关变更候选见下文 `session.rs` 与 microstructure 说明。 |
| final-review:22–26，覆盖及本人亲读 | 明言340路径依赖具名未实施者完整 diff；总 reviewer 只亲读列出的接缝，其余承接组级 reviewer，不声称本人全仓重读。 | 属于有边界的历史覆盖描述。`initial-boundary:43` 也承认 assigned-actions 详细正文当时未全读。独立性和承接链不应简化成总 reviewer 本人读取全部340文件。 |
| final-review:28–36，发现闭环与指纹 | 记 FR/accounting、N07六 writer、Pipeline N01/N02 映射、lifecycle/地产元数据及组级问题闭环；宣称冻结340/340、源 SHA/bytes 无漂移。 | 这是签署时的完整性声明。它并未承诺未来提交不漂移；文件第6行明确排除这种推论。实际后续漂移在下文逐项列出。 |
| final-review:38–46，验证和未覆盖范围 | 记278个 Rust 精确 case、编译/lint/短测历史结果，同时明确未跑全回归/E2E/长期矩阵、部分测试无行为红绿证据；主门禁完成。 | 均作为当时报告的历史结果保留，不转写为本轮测试结果。未运行长验收也不是本次对 OOP 语义的反证。 |
| final-review:48–50，验证索引收尾 | 记273常规 case+5 Writer key 对账、schedule/pending 清零及两个 N07 case source 校正。 | 索引签名是冻结版本元数据闭环，不覆盖之后源码差异；不重新验证 case 运行记录。 |
| initial-boundary:1–9，身份/中间态 | 原文明确当时 implementation in progress、未获 final freeze；要求保护基线和完整范围核对。 | 此状态被同目录后续 `final-review.md` 历史签名取代，不能孤立引用为当前 unfinished；也不能反向覆盖目标提交的源码版本漂移。 |
| initial-boundary:11–23，核销规则 | 要求 owner、真实 caller、撤回旧写口、错误/serde/RNG 边界、短测及独立 reviewer；区分组审与本人亲读。 | 与本次追踪方法一致。Action ledger 的机械覆盖本身不是行为证明；本次未复读 ledger 各项正文。 |
| initial-boundary:25–39，跨层重点及测试/领域限制 | 覆盖账户、撮合优先级、P0/P1/P9、公共 SaveSlot、行业账户、Host/UI、测试输入/期限；明确复核不跑测试，不重新认证法源。 | 限定了当年审查的静态/行为保持范围。交易撮合受理顺序的现行调用边界可由后续 session 测试更新佐证，不能从文档签署独立推出。 |
| initial-boundary:41–45，已读/待读及冻结门槛 | 列出已亲读资料，明确五组详细正文未全量 EOF；要求冻结后核 tracked/untracked diff、指纹、128目标和组审。 | 与 final-review 的分层披露一致；总报告不应被读成早期的待读声明已由 reviewer 亲自读完全仓。 |
| progress-01:1–11，进度与生产接缝 | 当时尚不批准完成；记录 Account/COW、BeliefParticipantState 四 map 投影、N32 accumulator、AccountBudget/费用/结算接缝及读测局限。 | 中间态结论不是最终否决，也不是当前 caller 源码签名。最终报告记录 N07 等问题闭环；如要证明现行caller仍一致，应核更新后的 source diff 和对应具名组记录。 |
| progress-01:13–17，组级证据/可选项 | 指明部分报告输出截断、review inventory 仅属机械 identity 清单；optional 的具体有益子 owner 不能遗漏。 | final-review 后来声明补齐并闭环，但本次不把旧 inventory 当最终证据，也不重跑 optional 子目标审计。 |
| progress-01:19–22，FR-01/FR-02 | accounting reviewer canonical 身份待最终版本查验；N07 内部六 writer 当时待核。 | final-review:30–31 声明两项已补；历史待办由后续复核声明关闭。当前没有证据显示这些问题重新打开，但改变源码需要新 SHA review。 |
| progress-01:24–28，语言/保护/测试待办 | 当时 protected-baseline、untracked 源、128正文、组 reviewer、集中 build/test 均待收口。 | 属明确的中间态，不可单独当作最终结果；final-review:36、40–44 和50记载后续闭环与限制。之后11个冻结源码路径更新使原版本绑定局部失效。 |

## 版本、指纹与后续 caller

- 三份材料的当前字节指纹：`final-review.md` SHA-256 `48de81763ee32557b70260c2c3f4e09b0d17ae4fab9e974faadbee7734e63c58`；`initial-boundary.md` `f335f589e4cf46539669c24d7dae6df987775ea52c6f77d09298c52e4d506b43`；`progress-01.md` `1c79c7237a082cdca71a388f3b3c2d387ca233186b9219eb56f9861f10ad3498`。三篇均由 `8cf34a1` 首次新增；当前内容指纹用于标识所读材料，不是源码签名。
- `final-integrity.json` 自报冻结源 `source-freeze.json`、冻结路径340、签署时 drift/missing/unrelated 均为空；`source-freeze.json` 基线为 `b89afb3346743a4b4fccf26c9ac9ff108595f696`。签名提交 `8cf34a1` 的父提交正是该基线。目标 `08e4fc7` 之后的 HEAD merge `a7c7ce3` 在产品/工具目录相对 `08e4fc7` 无差异。
- `8cf34a1..08e4fc7` 有24个 `packages/apps/scripts` 源码/测试路径变化，其中11个属于340冻结范围：`apps/server/tests/actor.rs`、`apps/web/src/app/save-commands.test.ts`、`apps/web/src/app/session-host-lifecycle.test.ts`、`packages/engine/examples/escrow_verification_harness/committed.rs`、`packages/engine/src/diagnostics.rs`、`packages/engine/src/diagnostics/causal/microstructure.rs`、`packages/engine/src/orderbook/book_state.rs`、`packages/engine/src/session/decision_chain.rs`、`packages/engine/src/session/institutional_behavior.rs`、`packages/engine/src/session/pipeline/decision_snapshot_capture.rs`、`packages/engine/tests/session.rs`。因此 final-review 的“后续源码变化不能自动继承”条款已触发；这是复核范围/指纹失效，不等同于已证明11处存在产品缺陷。此处未重新计算全部340条冻结 SHA，也未声称完整更新后 OOP 总复核。
- caller/行为候选逐项对照：`session.rs` 的规模 fixture 从“恢复实例与不中断实例逐 tick、逐事件完全相同”改为每个独立调度实例分别核算现金/股份/收费、成交事件/收据与日 K，并在同一 Rayon 预算下并行推进；注释解释 seed 不记录自由并发实际受理轨迹。文件还新增“先确认 Ask 已受理再提交 Highest”与相反受理次序导致价格笼拒单的测试。这些变化具体化实际受理语义，未见把请求顺序冒充受理顺序或混淆分/股单位的迹象；这是静态读 diff 的候选反证，不替代正式完整 review/测试。
- `microstructure.rs` 把 `side` 的可选分支改成模式匹配 `Some(direction)` 后再累计；从当前 diff 看等价于原先 `if let Some(direction)` 内的累计，保留 None 不入样本。`decision_snapshot_capture.rs` 将已捕获 experience 放入 `Arc`，caller 从再次分配 Arc 改为转交；`decision_chain.rs` 的测试索引访问适配 private getter。其余冻结变更分别是测试新增/重排、说明注释、fixture/格式或策略值传递调整。这里的摘要不替代逐文件边界审查。
- 当前caller风险不是历史 review 的“通过”字样，而是上列变更后各组 reviewer 是否按新 SHA 复审。指定三篇总审记录没有列出后续24路径的复审身份/指纹；本复核也没有追读其他组报告，故不能认定这些变化无人复审或已经复审。对这11条冻结范围变化，建议总签署方按当前 source SHA 将其与现存 review manifest 对账，再决定旧结论可保留的未变范围及需要增量复核的范围。

## 结论

三篇文档均已 EOF 读取。历史 `128/128`、`340/340` 与 `278 case` 是以 `8cf34a1` 冻结范围为依据的原签署；中间态 backlog 已在其后 final-review 中被声明关闭。目标 `08e4fc7` 相对签署提交改变了24条产品/测试/工具路径，其中11条在历史340路径签署集合内，故历史 PASS 不能充当更新后源码的全量当前 PASS。对这11条这里只确认变更和当前接缝，未确认 A股语义错误；完整的现行独立复核仍需按当前 SHA 完成。未运行测试或长验收，也没有将历史验证伪称为本轮通过。
