# Batch 106：Web OOP 中文化复核记录的当前边界

## 输入与基线

- 按 `scan-plan.json` batch `106`、owner `1`；产品 caller 基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。工作区为 `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`，来源根为 `/data1/baiyifan/workplace/stock_market_game/`。
- 三份来源逐篇连续读至 EOF，行数和 SHA-256 与 scan-plan 完全相符，aliases 均为 1。精确字段见 `batch-106.json`。
- 已阅读 caller `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`，并核对相关 ADR-0005、ADR-0007、ADR-0010。ADR-0007 已定首发中文界面及 AG Grid；Q06 仅保留未来增加第二语言的开放问题。ADR-0010 已定 baseline 仅用于初始化、读档、显式重同步，以及 Remote 的独立传输能力。没有发现后续决定把来源中的 retain 或“未核实”改成产品实现承诺。

## 章节族状态

- **web-02 / 行情表格与组件状态：** 来源引用的是对 `MarketGrid.tsx` 特定 delta 的独立复核，并明确沿用旧报告其余 59 个文件的结论；delta 核对 `appliedRowsRef` 的提交目标语义、模拟指数展示及 retain。当前 caller `apps/web/src/components/MarketGrid.tsx:36-58,66-72` 已使用 `MarketGridRowSynchronizer`，通过 React hooks 持有交互/组件资源，并将“模拟指数”按行数据的算术平均展示。和 ADR-0010 的 AG Grid 稳定行 ID/事务边界一致。没有证据要求把该 retain 改为额外对象迁移，也没有把旧 delta 通过扩大为其余文件的新复核。
- **web-03 / wire parser 常量：** 来源只复核 `parse.ts` 的 `EVENT_NAMES`、`EVENT_SOURCES`、`REJECTION_REASONS`、`CIVIL_KINDS` 清单及 retain。当前 caller `apps/web/src/host/protocol/parse.ts:31-41,122-123,198-206` 仍以 `as const` allowlist 配合 switch/enum 校验；`SettlementError.reason` 与 `IntentRejected.reason` 的不同契约不应合并。它们是无状态 wire 标签集合，不构成新的对象所有权遗漏。没有由此增补交易所制度或交易规则语义。
- **web-04 / Remote generation：** 来源确认新局无初始存档时，pause preference 使用 generation `1` 合法；旧 generation baseline 的乱序接收仍明确是未核实协议边界。ADR-0010 对一般 baseline 使用时机及 Tauri 时间线给出契约，但没有把 Remote 旧代 baseline 的到达顺序证明为已解决。当前 caller `apps/web/src/host/remote-host.ts:75-90,117-135,192-195` 仍在 baseline 到达时安装缓存，按 delta generation 不匹配触发 resync；新局 generation 默认值仍为 `1`。因此保留来源的未决边界，不误报为已修复。

## 现行候选交叉核对

- 当前审计的 `G01–G68` 与 `Q` 台账没有因这三份中文化记录而可核销的条目，也没有从“对象抽取”直接推出产品修复。相关但独立的现存候选包括：行情表格第三方英文辅助文本与桌面键盘选股（`G48`、`G64`，见 `exhaustive-review/luna02.md:79-80`）；Remote 凭据/取帧/重连和 pause-resume 重送旧 baseline（`G01–G05`，见 `implementation-audit-2026-10-02.md:34-38`、`reaudit-host.md:13-31`）；协议错误缺少已知 generation/cursor 上下文（`G52`，见 `implementation-audit-2026-10-02.md:88`）。这些是后续当前 caller 发现，不能倒写为三份来源已经提出或已经关闭的事项。
- `G04` 的当前问题是 pause-resume 再次 `start` 时重送缓存 baseline；它与 web-04 所保留的网络中旧代 baseline 乱序/接收语义边界相关但不等同。不得把其中一项的证据当作另一项已经解决。
- 领域语义与必要性：本批材料仅是中文化版本映射及有限 delta 复核；未改变 A 股规则、交易金额/股数单位、撮合或价格语义。retain 限于静态常量和明确记录的组件状态边界，符合 OOP 审计的最小范围。未发现本批范围内新的已批准承诺遗漏或错误核销。

## 结论与限制

- 三篇旧记录的“通过”仅对应其写明的中文化/指定 delta 范围；web-02/web-03 其余内容沿用旧 review，web-04 明示未核实 baseline 乱序语义。它们不等于完整源码审计、当前产品验证或运行时测试通过。
- 本批未改产品代码、交易规则或 Git 状态；未运行测试、构建、产品回归或官方规则查询。完整读取仅针对 scan-plan 指定的三份来源；旧语义 review 仅按其引用逐篇核对了相关范围，没有把其范围外结论伪装成本批复核。
- EOF 已确认：web-02.md 22 行；web-03.md 22 行；web-04.md 22 行。
