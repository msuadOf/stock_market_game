# 本轮发布前改动独立复核

复核日期：2026-10-03。复核者：`release_review`，未参与本轮产品或测试实现。

## 当前结论

本记录所列十四个产品、测试与配置文件的完整 diff 已独立复核，新增测试全文也已阅读；本轮有效发现均已修复并再次复核，最终完整回归、新 WebServer 构建 E2E 和 production 前端构建日志已核销。十四文件 SHA-256 与下表一致，无未审源码漂移，当前无未解决阻断发现；可以本地提交这十四文件与本主题工作记录。不得混入用户原有 `AGENTS.md`、旧 `agents/oop-refactor-audit/`、截图、日志或 JSON。本结论不是远端发布完成证明；后续新增产品 diff 仍须复核。

用户原有 `AGENTS.md` 改动不属于本轮实施；未修改。浏览器验收曾暂时写回 `.omo/evidence/company-information-npc-intentions/task-34-happy/` 的 PNG，实施者已归档新图并恢复原图，最终 `git status --short` 确认无这些 binary diff。`agents/oop-refactor-audit/` 为已有工作记录，不把其出现于 untracked 列表等同于本轮产品实现。

## 范围与读取依据

已阅读 `AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/open-questions.md`、`docs/trading-rules.md`、ADR-0025、ADR-0026，以及 ADR-0028 的发布约束。检查以下完整 diff，读取 Rust 源文件、相关生产入口及现有测试，并核对前端命令、生命周期、fixture 与 `InitialSaveSource` / `SessionReplacementGate` 调用关系：

- `.gitignore`
- `apps/web/src/app/save-commands.test.ts`
- `apps/web/src/app/session-host-lifecycle.test.ts`
- `packages/engine/examples/escrow_verification_harness/committed.rs`
- `packages/engine/src/orderbook/book_state.rs`
- `packages/engine/src/session/decision_chain.rs`
- `packages/engine/src/session/institutional_behavior.rs`
- `packages/engine/src/session/pipeline/decision_snapshot_capture.rs`
- `packages/engine/tests/session.rs`（第二轮增量）
- `scripts/publish-release.mjs`（第二轮增量）
- `scripts/publish-release.test.mjs`（第二轮增量）
- `apps/web/src/components/company/CompanyPanel.tsx`（第三轮增量）
- `apps/web/src/components/company/company-report-selection.test.ts`（第三轮增量，新增文件全文）
- `apps/web/e2e/company-information.spec.ts`（第三轮增量）

## 大 A 语义与依据可靠性

符合本轮大 A 语义保持要求。本轮没有调整交易制度、手续费、资金与股数单位、交易所归属、价格时间优先、T+1、整手/零股或竞价时段。

领域依据沿用 `docs/trading-rules.md` 的官方现行规则登记：沪深 2026 版交易规则于 2026-07-06 生效；该文档记录 2026-09-22 的条款核对，以及 2026-09-25 对沪市 3.5.1、深市 3.4.2 同价同向时间优先的再次核对。费用表访问失败的既有边界仍如实登记。本轮只验证既有规则没有漂移，未重新在线核验所有交易所条款，也没有作新的真实市场规则主张。

新增前端用例使用经过当前 schema 校验的日终档 fixture；没有用日内存档绕过 ADR-0025，也没有修改资金、持仓、冻结或计划事实。fixture 的 Rust 深度恢复正确性不是这些宿主 stub 测试所证明的范围。机构行为测试将 `Copy` 的 `InstitutionExperiencePolicy` 从 `.clone()` 改为解引用复制，参数值保持一致；相关行为仍是 ADR-0026 明确登记的游戏策略假设。

## 必要性、最小范围与等价性

两组前端测试补充发布验收中必要的异常与异步边界，没有改前端产品实现、扩大接口或引入依赖。Rust 改动属于 lint/format 修复：测试 module 从 `impl` 前移至文件末尾、去除一次多余借用、复制 `Copy` 参数、删除已经不对应后续函数的悬空 doc comment，以及对验证 harness 的调用换行。撮合函数正文与既有异常 fixture 断言未改变。

`CapturedExperienceObservation` 改为持有 `Arc<RetailExperienceState>` 等价：

1. `capture` 仍从 `AccountPagedMap::get` 的 `&RetailExperienceState` 深复制账户私有值，然后按原顺序计算 equity、观察持仓、构造风险和 SelfView 的持仓输入、prune watchlist。`Arc::new` 放在这些 mutation 全部完成之后。
2. `consume` 消耗私有 enum；其间只读取 `reference_equity` / `peak_equity`，不存在后续 experience mutation。`SelfView` 的 Money 错误仍先于 risk observation 错误，risk 错误仍先于策略导出错误；`NoExperience` 路径未改。
3. 成功路径原先在 `consume` 结尾分配一次 Arc，现在在 `capture` 结尾分配一次 Arc。`DecisionAccountInput::new_shared` 和 `replace_existing_shared_parallel` 仍接收同一对象的共享引用，账户观察不会被提前安装到 shadow；snapshot 校验失败仍不会安装 experience updates。
4. 失败路径可能在 `SelfView` / risk / strategy 报错之前先完成 Arc 的内存分配，随后释放；这仅改变内存分配时机，不改变可返回的业务错误、领域状态或序列化形状。此结论不声称机器内存耗尽时仍有可恢复行为。

`.gitignore` 的新增 worktree 规则锚定仓库根目录；`/agents/**/*.json` 只忽略 Agent JSON 工作记录。已用 `git check-ignore --no-index` 验证：`.worktree/`、`.worktrees/` 与 Agent JSON 命中，Agent Markdown、engine JSON 和 `.worktree-other/` 不命中。已有忽略条目仅翻译注释，没有扩大密钥或源码忽略范围。

## 边界、跨层漂移与复杂度

新增测试有效约束了以下边界：两类 load 失败后 metrics 和替换屏障释放并能重试；baseline 同步期间换宿主后的晚到错误不能触发新会话提示或 fatal，也不能结束新 generation；初始化 load/preferences await 期间 cleanup 后不能重新 register/start/ready，资源只 dispose 一次；初始 load 失败保留启动源并在重试时恢复同一档；stop 抛错仍释放后续资源并保留原错误身份。

既有 `decision_snapshot_capture_tests` 已覆盖持仓 equity/peak 与 T+1、一/四 worker 一致性、非 Retail experience 无 Retail risk、缺失行情、equity overflow 不安装观察值、SelfView/risk/strategy 错误优先级和不同可撤窗口的工作单预算。本轮 Arc 调整没有引入新的领域分支，不要求增加镜像实现的测试。未发现跨层语义漂移或新增不必要 owner / 状态机。

### R1：重试恢复测试的顺序断言可能假阳性——已修复并复核

原新增用例仅检查 `indexOf("load-retried") < indexOf("register")`。若失败路径错误地消费 `InitialSaveSource`，重试直接按默认 setup 启动，不调用 load；此时 `-1 < registerIndex` 仍成立，无法证明“保留启动源并重试同一日终档”。

实施者已增加 `load-retried` 和 `register` 各出现一次的断言。复核确认缺失任一调用都会失败，顺序与 slot 身份断言随之有约束力。实施者报告修复后的八个 lifecycle 用例重测 exit 0；本复核者没有重复执行该测试，不把转述结果写成独立运行结果。

## 第二轮增量复核：符号价格 fixture 与发布前 tag SHA

结论：三文件增量没有发现需修复问题。已阅读完整发布脚本及其测试、Rust 两个相关用例全文、邻接符号价格测试、`sample_setup` / `player_session_with_position` / `TestOrderSaveFixture`，并检查 `Market` 的 reference/bound/resolution 以及发布 workflow 调用关系；对 `session.rs` 全部改动逐行比对，没有改变其他测试。

### 符号价格用例不是弱化原断言

原用例同时 enqueue Sell 和 Highest Buy，随后假定 enqueue 顺序就固定同股实际受理顺序；此假定与当前并发入口契约不符。fixture 现在先执行一轮，明确断言 990 分、100 股 Sell 已 `OrderAccepted`，再 enqueue Highest。该 fixture 无 NPC，连续竞价、价格笼子开启，证券为沪市主板、最小价位 1 分、T+1 开启；初始 100 股是未锁定的既有持仓。

原有“不拒单”、990 分成交 100 股、剩余 100 股 Buy 固定在 1010 分、存档恢复后仍为 1010 分的断言均保留；未受理请求保存 `Highest` 的断言只因队列仅余一项将索引 1 改成 0。新增 Sell 受理断言使先决事实更强，没有改成宽松价格或容许原本应成功的拒单。引擎内部 save/restore 用于检查点与请求/挂单类型验证，没有增加日内持久化入口。

新增负控制先确认 Highest Buy 在空簿参考价 1000 分时以 1020 分受理，随后提交 990 分 Sell，断言该股收到 `PriceCageExceeded`、没有 Trade、原 1020 分 200 股 Bid 保留、Ask 为空。它固定了另一种实际受理事实，证明不能把原并发失败解释为“990 分 Sell 必然仍合法”。

价格依据：ADR-0022 规定符号价在同股实际处理时解析一次；已有挂单不自动重价。ADR-0021 第 5 节记录 2026-09-27 官方条款核对，沪市 3.3.14 / 深市 3.3.16 的笼子以及 3.5.3 / 3.4.4 的成交价规则。990 分 Ask 对买价使用 102% 与加十价位的较高者，当前最小价位下得到 1010 分；空簿用 1000 分 reference 得到 1020 分。1020 分 Bid 成为 Sell reference 后，990 分低于其 98% 与减十价位中的较低者，必须拒单。本轮没有新制定规则或重新在线查询条款。

这两个短 fixture 验证“已明确先发生的受理事实”，不证明不同自由调度下整局字节相同，也不单独覆盖同 tick 所有并发轨迹；现有价格解析、执行及重放测试承担该层边界。没有通过限制 worker、改变生产排序或关闭笼子来绕过失败。

### 发布前 SHA 二次核对保持 draft

发布脚本原有编译后首次 tag SHA 核对、十类制品校验、draft 身份和所有上传 assets 的 name/size/digest 校验完整保留。新增第二次 `gh api repos/.../commits/<tag> --jq .sha` 紧接 assets 校验之后、`gh release edit --draft=false` 之前；SHA 不符显式抛错并提供期望/实际值与下一步。API 查询抛错同样自然传播，没有 fallback 或随后 edit，也没有删除 draft 的分支。

新增测试让第一次查询匹配、第二次查询返回另一个完整 SHA，核对两次查询、已经 create draft、没有任何 edit；成功用例同时明确核对第二查询在 edit 之前。既有上传身份或 digest 错误保持 draft 的测试未弱化。`release.yml` 仍通过五分钟进程外 supervisor 执行同一脚本，没有实际对外发布。

二次查询缩小长上传期间 tag 移动的检查间隔，不构成 GitHub 对 tag 与 Release edit 的原子比较交换：若 tag 在第二次查询成功之后再次移动，该极短窗口仍存在。此改动满足本次“公开前再查 SHA”的必要范围，不应将其描述为绝对排除所有并发移动。

实施者报告两个增量均完成红绿验证；本复核者没有重新执行测试，仅执行源码、完整 diff 和 `git diff HEAD --check`，后者实际通过。E2E 排查尚未结束，本记录不宣称本轮最终完成。

## 第三轮增量复核：公司报告选择与本轮工作记录

结论：CompanyPanel 修复与 E2E 改动范围必要且最小；新增测试 fixture 的身份不一致已修复。已全文阅读三份产品/测试文件，以及 `company-view-model`、`company-presentation`、`company-slice`、`CompanyQueryCoordinator`、`ReportQueryContext`、`DisclosureList`、`memoryHook` 等调用与身份边界。

CompanyPanel 现在只有在 `ready` 或真正 `empty` 时，根据权威可见报告列表协调选择；`loading`、`error`、`idle` 和 `unavailable` 时保留的是用户选中的 ID。`reports` 仍来自当前 `companyId` 的 cache，经 `visibleReports` 从 root 的 ready 页面链取得；实际报告仍从当前列表查找，且内容渲染要求 `state.kind === "ready"`。因此暂时保留 ID 不显示旧公司报告、不查询未公开事实，也不跳过错误提示。ready 后 ID 不再可见则使用既有默认选择；empty 后清空。公司切换没有新增跨公司共享 cache，公共 ID/公司归属与 generation 校验仍由现有 host/coordinator 负责。

大 A 信息边界沿用 ADR-0016：经营事实与公开信息分离，未披露信息不得进入公开查询或 NPC 决策。生产公共库的 `PublicationId` 在当前库中单调不复用，`ensure_scope_mirrors_company` 强制 scope 身份与公司一致。本轮没有更改披露时刻、scope、报告期间、版本、财务金额或查询/API 契约；只修复公开内容刷新期间的 UI 选择丢失。

新增单测真实加载 CompanyPanel，使用真实 reducer 生成 ready/loading/error/empty；`memoryHook` 模拟 state/memo，新增 effect collector 用 `Object.is` 比较依赖并在一次 render 后运行 effect，dispatcher 由既有 `finally` 恢复。它没有替换选择算法或预先写入期望结果。红灯日志实际显示 `8` 被错误改为 `7`，修复后 reviewed 绿灯记录三个 suite 共 13 case 通过。其真实性限于组件 state/effect，不是 DOM/React 完整提交周期或并发渲染证明；注释已明确该边界，真实交互由 E2E 承担。

E2E 将报告选择目标从 listitem 改成具有真实 accessible name 的 button；原公司、期间、公开编号、键盘焦点和布局断言全部保留，并增加点击后与键盘操作后的 `aria-pressed=true` 和报告期间保持断言。没有删掉失败断言或放宽超时来让用例通过。

### R2：公司切换 fixture 的 scope 身份不一致——已修复并复核

原新增 fixture 只改 `company_id` 为 `C-002156`，而 `financials.scope.Standalone.entity_id` 仍为 `C-600101`；真实 PublicLibrary 不可能产生该报告。实施者已同步 scope 的 entity_id，两层身份均为 `C-002156`，其余公开字段保留。已全文再次复核新增文件；显式 `memoryHook<Props, ReturnType<typeof views.CompanyPanel>>` 只修复初始 props 的过窄类型推断，不修改测试行为。reviewed 绿灯日志为 13 passed、0 failed/cancelled/skipped；此结果是实施运行日志，不是复核者独立重跑。

### R3：跨文件脚本聚合缺少共享 deadline 的工作流程边界——已修复并复核

原 `scripts-final-results.md` 如实记录聚合耗时 14234ms，但工作入口没有共享进程外 deadline，且将跨文件聚合称为普通测试。实施者已把聚合明确分类为长验收，并在 `run-scripts-tests.mjs` 增加外部 `runBoundedCommand` 300000ms supervisor；32 个文件各自的 10000ms child deadline 与 Node case timeout 保留，四个文件 worker 仍并行。监督同时覆盖 discovery、短命令与结果文件写入，没有放宽普通 child/case 上限。已全文复核工作脚本；supervised 批次报告与日志实际为 32/32 文件通过、13675ms、最长文件 7961ms。

### 工作记录中文与证据范围

已阅读本目录的 `release-process-checklist.md`、`publish-tag-sha-tdd.md`、三轮 scripts 结果、supervised 结果、`summary.md`、本复核记录，以及一次性 `run-scripts-tests.mjs` / `e2e-tablet-diagnosis.mjs`。人工说明与新增注释使用中文；代码符号、命令、路径与原始工具输出保持英文原貌。一次性脚本不属于产品实现；诊断脚本的 700ms 观察等待及消息监听仅用于排查，没有被写成正式测试证据。

summary 已说明 `.log`、JSON inventory 和 trace 是被忽略的本地证据，Markdown 日志链接不代表克隆仓库后可取得附件。旧沙箱失败、Node/Corepack 环境限制、既有 lint warning、非默认 ignored 用例及 Rust case watchdog 缺口均被如实保留，不能据此声称这些额外门禁通过。最终又全文核对 `company-report-selection-fix.md` 与更新后的 summary：失败 trace、复用 preview、fresh 编译错误、最终 fresh 成功按真实顺序分别记录，没有覆盖或改写失败证据。

特别说明：`e2e-fixed.log` 的 12 passed 使用了既有排查 preview，不能作为最终重新构建的结果。第三轮复核时 `e2e-fixed-fresh.log` 仍记录 WebServer 启动 exit 2，fresh tsc 类型问题虽已修改源码，最终新构建 E2E 结果仍待 root 核销。本复核不把旧 preview 结果转写为最终通过，也不宣称已发布。

## 最终证据核销与提交签认

上节等待状态属于第三轮复核时的历史，现由以下最终实际日志核销：

- `full-verified.log`：密封 build 6874ms，execute supervisor 40876ms。74 个 Rust binary、一个必跑跨年长例及 workspace doctests 均完成，八个 Web shard 覆盖 123 文件。复核者独立汇总日志的 80 个 Rust result section，得到 2193 passed、0 failed、6 ignored；八个 Web result section 汇总为 608 passed。ignored 不计为通过。
- `e2e-fixed-fresh-final.log`：实际出现新 WebServer / Vite preview 4189 启动记录，两个 workers、retries=0，12 passed（40.8s）。实施者同时记录 CI=1 与共享 300000ms supervisor；新日志核销了前一次 WebServer exit 2，不使用复用 preview 的 12 passed 替代。
- `frontend-production-final.log`：去除 E2E mode 后完成 production Vite 构建与 `check-web-release-wasm.mjs`，日志以 `release WASM verified` 结束；root 报告命令 exit 0。此构建只更新被忽略的产物，没有引入产品源码漂移。
- `scripts-supervised-results.md` / 对应批次日志：32 文件全部通过，保留单文件/单 case 10 秒与聚合五分钟共享监督，13675ms；不得改写为普通整命令十秒内。

本复核者没有运行上述完整测试或构建，只核对实际日志、统计、更新后的工作记录、十四份最终源码哈希与 `git diff HEAD --check`；后者实际通过。最终 `git status` 只剩本轮范围、已说明的用户 `AGENTS.md` 与旧 audit 目录，原截图恢复。

本地提交门禁通过：大 A 语义和信息披露边界保持，改动必要且最小，R1/R2/R3 均已闭环，全部已审源码版本冻结。拟提交的本主题中文 Markdown 和两个一次性工作脚本已阅读；日志、JSON、临时 `.tmp`、worktree 与旧工作主题均不纳入本次提交。当前没有实际 push/tag/Release 证据，不能把本地签认写成发布完成。

## 验证与保留边界

本复核只执行只读差异、源码与忽略规则检查；`git diff HEAD --check` 实际通过。未执行完整测试、Rust 编译或发布，没有提交、push 或创建子 agent。完整验收结果由实施者单独记录；当时 `scripts-results.md` 尚记有五个失败，Rust regression 也有待定位用例，因此此记录不证明整仓测试全绿、性能达标、三宿主验收或发布可用。

## 本次复核的文件 SHA-256

| 文件 | SHA-256 |
|---|---|
| `.gitignore` | `f7261e465ccaf75962ed4ad1f85fdb2f7eb37b999a80c73d3e8e1edf16237def` |
| `save-commands.test.ts` | `9e37c754e5c4231d33997be07cc8f9fa361f455a3fd0a75b478aff4c53bccc28` |
| `session-host-lifecycle.test.ts` | `4d08c1a1dd0da893bebadb44bc91fe39f4a35c57ccf5c3cb934ac906dd66918f` |
| `committed.rs` | `626650a971bdd492f632082135c828e3c168a337491ccc3e70e7fd00ac103d9a` |
| `book_state.rs` | `7ee0de82608409b22aaff90a513a60c250d48e125b0e8a9d9674d2866dddfec4` |
| `decision_chain.rs` | `e09bd1d81208b3f91fd1016eb5a91befeadc1399281d5d1ad1838032e293db6e` |
| `institutional_behavior.rs` | `175908bf136825aa13d6e1e9cd55e42a94210967045f7dc5c5799c29b7d4b6ee` |
| `decision_snapshot_capture.rs` | `fe6f839ad06c33aae371e344bfb33b70f0da610f7009a0769f890758cc0b5efe` |
| `packages/engine/tests/session.rs` | `65040107173b81c4c08697acf7a9d3cc01c0eae93b746076050e74d8be957393` |
| `scripts/publish-release.mjs` | `02bb3440f70e7dd7e25be232fb33b0688c381537c27f49c975387a227b07194c` |
| `scripts/publish-release.test.mjs` | `81a9c0aedf35bc5560c33545a4c2ad317f8d77b079a1f4b7a174205131630032` |
| `CompanyPanel.tsx` | `e42d076293479da25dff1a7637ea0fa977626c4a4c073b53d87575cbfe7158b2` |
| `company-report-selection.test.ts` | `194140b8db055e80c2f9705dfe0746741e915d9bb18d88a0ec72b35ef835d534` |
| `company-information.spec.ts` | `9ad0079f52e1c1a8a0fd50edce40e2355ec812d778c89fd18cdb7a253fd30ad0` |
| `run-scripts-tests.mjs`（工作脚本） | `b9efc6636e0b53a7771dba0aac929367ebbf3dd85641041b9ccee0a5837d5bdf` |
| `e2e-tablet-diagnosis.mjs`（工作脚本） | `afe380f16fa8131122dbf3183b153ff9f6350dcb17f7a2d2bc3df0a45d940b17` |
