# 批次 150 复核

## 来源完整性

按唯一计划文件 `scan-plan.json` 的 batch 150 核对三份来源。均从首行连续读取至 EOF；行数与 SHA-256 完全匹配计划：

| 来源 | 行数 | SHA-256 | 完整性 |
|---|---:|---|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/managers/frontend-before/reviews-web-01.md` | 48 | `886a2197b8b1d24c0beb34c664209b2ece262f6f48588bf676866da1f1cb789a` | 一致 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/managers/frontend-before/reviews-web-02.md` | 92 | `c6e8c51c52daba9e2895f286b3e91813a55b76b112a3f926cf117f063824f5ec` | 一致 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/managers/frontend-before/reviews-web-03.md` | 53 | `3a4be758be7521d65e183cb0c78e395570ec3e826ca8225e299cb592972bae9d` | 一致 |

三份审计记录均不在 Git 基线 `43b1aa5`，无法从该提交独立验证这些记录的历史原始字节；上述完整性结论仅表示当前来源符合计划登记值。

## 独立复核

- `web-01` 的 A06 描述与基线 `App.tsx` 的 owner/caller 链相符：App 持有 `startDateError`、speed metrics 及 polling generation state，并将 setter、request gate 和 load-in-progress ref 交给保存命令与 polling hook。`web-01` 指定的回写仍落入 App 所持 state，未见在提取命令时复制状态权威的说法。A27 也准确区分纯投影函数与 `upsertFrames` 对传入 accumulator 的原地更新；后者由 `MarketChartProjection` 持有并在 `useMarketChartRuntime` 中调用。
- `web-02` 对条件单身份的结论有代码依据：基线 `auto-order-manager.ts` 的 `nextId` 位于模块作用域；同一模块实例内多个 manager 共用序列，`clear()` 清单及 pending 而不重置 ID。App 的 manager 回调与 Redux ID 投影、行情 runtime 消费点与记录一致。多 manager 唯一性仍只是可补充测试建议，材料没有声称该测试已经存在或执行。
- `web-03` 记录的协议 reducer/coordinator 关系、Redux snapshot 投影与 manager/gate owner 属调查性描述。本批来源本身未提供对该轮 29 个源码文件逐一复读的原始证据，故只确认其陈述与本次可见的 `useMarketChartRuntime` 接线及 ADR 状态边界没有冲突，不把引用既有逐文件表格误记为本轮重审全部 29 项。
- A 股边界方面，`web-01` 如实登记了一个仍存在的语义缺口：基线 `LocalRefreshViews.tsx` 用 `DEFAULT_SETUP.stocks` 取证券类别显示快捷涨跌停价，而 `useTradingCommands.ts` 的下单预检用 `activeSetup.stocks`。因此自定义局的类别与默认设置不同时，快捷价格提示可能与下单校验采用不同类别。材料将其标成既存限制且未声称已修复，状态表述准确；这是应保留并后续处理的产品问题，不能由本次文档审查视为通过或关闭。
- 工程约束与决策核对：现行 `docs/principles.md` 要求状态权威清晰、中文首发文案与领域边界一致并由独立 reviewer 核查；ADR-0004 规定 engine 为权威、Redux 为 UI 状态和结果快照；ADR-0007 §2 与 `open-questions.md` Q6 将首发界面定为全中文且不引入 i18n 框架，未来第二语言仍须另行决定。审计记录没有借“中文本地化”扩大为引入国际化框架或改变交易规则。ADR/项目文档只用于确认项目语境，本轮没有重新核验交易所、中国结算官方规则，也不对规则数值作新确认。

## 结论与范围

未发现三份记录对其明确复核事项作出与可查基线实现相矛盾的结论。快捷涨跌停类别来源差异是准确披露的既存 A 股语义风险，仍未解决；web-02 的跨 manager ID 测试也是建议而非已有覆盖。审查范围是三份指定记录及其关键 owner/caller/consumer 依据，并非重跑三个 web 审计或复核完整产品 diff；未运行测试或构建，未改产品文件或 Git 状态。
