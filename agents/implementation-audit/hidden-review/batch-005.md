# 批次 005 独立复核

基线：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`（worktree `HEAD` 一致）。只读复核；未运行 Git 写操作、测试、构建或回归。三个指定源文件均全文读至 EOF，覆盖 3/3，75 行；SHA256 与 scan-plan 完全相符，文件无漂移。覆盖矩阵与逐项分类见本记录及 `batch-005.json`。

## 现行依据

- 项目约束：`AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/open-questions.md`。`principles.md` 的纯核心/显式错误/最小状态方向适用于抽取审查；开放问题明确 ADR-0017 escrow 阶段已定，不能借验证器重构重新开放。
- 宿主语义：ADR-0010 规定 baseline 仅初始化、读档和显式重同步；命令成功需宿主确认，`CommandQueued` 不是受理/成交确认；push/pull 与 Worker 背压是宿主能力差异。
- 撮合与守恒：ADR-0017 现行修订优先于历史阶段顺序文字。保留 P0/P1/P9、卖单零现金 escrow、逐 envelope 资金/股份守恒；来源内序号与全局 receipt index 要分别验证，不将不同委托身份顺序解释为交易先后。仅本批 N22 涉及工具验证器，不代表任何交易规则改变。
- 范围/存档：ADR-0019 规定存档只留继续运行的最少事实；ADR-0025 只在成功自然日日结持久化；ADR-0027/0028 规定独立构建/发布边界，发布仅构建和制品核验，CI 为手动开发诊断。OOP 建议不得误转成日内持久化或发布测试门禁。

## 覆盖矩阵

| 源材料 | 完整读取 | 内容范围与复核结果 |
|---|---:|---|
| `frontend/review-revisions.md` 1–11 | 是 | 五条修订记录逐条核对。N02/N03 是纠正调查断言，不是产品修复；其余语言规范化与锚点修订是调查文档证据治理。 |
| `hosts/preliminary-review.md` 1–55 | 是 | 总体证据声明、22 个 N 候选及 E01、HR-01–07 全覆盖。重点反查 N14/N16/N17/N19/N22/E01 的当前 owner/caller/consumer，发现多项提案现已在产品/工具代码中实现。 |
| `hosts/reader-instructions.md` 1–9 | 是 | 作为历史调查方法与限制读取；其旧时指令禁止读前轮审计、禁止 Git/测试的命令不覆盖本轮用户新授权的基线及现行规则。 |

## 逐项判定

- `frontend-R2-N02`：修订合理。当前 `remote-host.ts:201-209` 切换 delivery 会使旧 connection 失效、关旧 socket、必要时重连；连接身份隔离是原有行为边界。凭空要求每次模式切换都再等基线会扩大为行为改动。是否同步取决于 generation/基线标记，不能笼统断言“新连接永不需要基线”。
- `frontend-R2-N03`：修订合理。调查需保留“先安装 timeline、再解析/安装 snapshot、后推进 epoch”的部分初始化顺序；这类失败清理问题是显式异常路径，不应把初始化包装成原子成功。此记录不把历史说明当成已新增缺陷。
- Hosts `N14`：源报告将连接状态抽取列为 accept，但当前 `apps/server/src/routes.rs:1077-1250` 已有 `WsPublisherConnection` owner，拥有 baseline cursor、timeline generation、buffer、resync/failure 状态；`ingest`、`require_resync`、`install_baseline_cursor`、`request_frame` 等方法封装迁移边界，`run_ws:1262+` 留 I/O 编排。该建议当前归为**已实现**。反证边界：不能因此注销实现无关的 G01–G05（浏览器鉴权、pull 客户端取帧、baseline 重放、重连）或把不同宿主合并成一条传输。
- Hosts `N16`：源报告称 WS 测试丢弃 JoinHandle；当前 `apps/server/tests/ws.rs:50-69,100-110` 的 `ServerFixture` 持有 `server_task`，`shutdown` 显式 abort 并 await，再移除会话。候选归为**已实现**，旧观察是时间点漂移；不是生产服务能力缺口。
- Hosts `N17`：源报告条件建议统一 route 检查来源；当前 `scripts/desktop/build-matrix.mjs:86-95,98+` 的 `planCell` 返回 `preflightChecks`，`describeChecks`/`printPlan` 消费同一列表，执行入口 `:247+` 迭代同一 `plan.preflightChecks`。该窄边界归为**已实现**；无证据要求再加 class/包装对象。`ADR-0027` 原生构建边界仍须由构建本身核验，plan 的结构化一致不等于构建通过。
- Hosts `N18`：条件性资源生命周期提案仍需核实实现；这次没有找到证据证明 `market-ui-report.mjs` 的 browser/server/CDP 全生命周期已迁入单一 owner。维持**现行候选/有条件**，仅看护已创建资源的释放，不能借此将图表分析、截图、报告构造并入该对象。此项是 OOP 结构建议，不对应新的 G 项。
- Hosts `N19`：条件提案所称库存值 owner 已在 `scripts/run-full-regression.mjs:179-239` 实现为 `ArtifactInventory`，其私有冻结 record、build/decode factory 与 descriptor/metadata/digest 校验闭合。归为**已实现**；文件 I/O 仍在外围 `writeJsonAtomically` (`:240+`)。它不核销 G61/G62/G63 的期限、树终止和逐 case 上限缺口。
- Hosts `N22`：当前 `scripts/simulation/escrow-verification-contracts.mjs:241-275` 有 `Resource` BigInt 值对象，`:285-380` 有 `EnvelopeConservation` 阶段状态和逐 receipt 校验，`:382+` 聚合账户及全局 receipt identity。归为**已实现**，其不变量与 ADR-0017 一致。该静态存在不证明对应负向语料本轮执行通过，不改 JSON 分/股字段，也不允许丢失错误上下文。
- Hosts `E01`：报告已将 `CaptureArtifact::write_to` 去重归入现存 writer。来源限定为提案修订；本轮不将它升级为产品缺口。Writer 对输出副作用的所有权仍要保持单一。
- Hosts 其余候选：N01/N02/N04–N06/N08/N10–N13/N15/N20/N21 仍为报告给出的 conditional/accept 结构建议，N03/N07/N09 已 reject；当前材料不支持把建议写成“尚未实现的产品功能”。特别 N12 只可视为测试恢复夹具，不得改写 ADR-0019/0025 的正式存档语义。N11 是已有 Base owner 上的方法归属建议。

## G/Q 交叉核对

采用当前总账 `agents/implementation-audit/implementation-audit-2026-10-02.md`，不沿用 OOP reader 指令中的旧状态。G01–G05、G18–G20、G40、G53、G66 在该总账仍登记为现行缺口（行 34–43、53）；其分类与 N14 等“协议状态 owner 已收拢”并不矛盾：状态类有 owner 不等于 UI adapter 已正确鉴权、pull 已持续取帧、baseline 生命周期、自动重连或命令确认闭环。G39、G60–G63 仍是工具/验收现行缺口（行 126、128–131）；N17/N19/N22 只涉及窄的结构边界，均不核销工具实际流程、外部 deadline 或验证覆盖。G56 是 Pages owner/root 路径差异（行 91），与 N17 的 Tauri 构建路由独立。未发现本批材料足以增加、关闭或改变 G01–G68 的结论，也未发现需新增正式 Q；历史报告写明“只提出调查动作”，不能将其旧测试/复核结果冒充当前验证。

## 语义与必要性结论

本批 OOP 材料没有改交易规则或存档契约，不需另查交易所规则；若实施 N22，单位固定为分/股，ADR-0017 阶段/守恒是现行依据。调查修订 N02/N03 及 HR-01–07 对调查准确性有必要；但在当前基线 N14/N16/N17/N19/N22 的窄对象已存在，不能重复实施或把“提案被接受”报告为现行缺口。N18 仍需按明确资源取得/释放边界验证。没有跑测试，不能声称历史候选的行为/测试证据已重现。
