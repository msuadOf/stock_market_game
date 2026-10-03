# 隐藏扫描批次 034

## 范围与来源完整性

- 扫描计划 `scan-plan.json` 中 batch 34、owner 4 的三份来源；当前产品基线为 `.worktree/implementation-reaudit` 的 `43b1aa5`。来源从主工作区绝对路径读取，未将历史指令作为本轮授权。
- 三份来源均连续全文读取至 EOF，预期/实测行数与 SHA-256 一致：`frontend-review-notes.md` 3 行，`frontend.md` 33 行，`hosts_tooling.md` 19 行。逐篇章节清单、hash 和 EOF 证据见 `batch-034.json`。
- 仅作静态核对；没有修改产品代码、运行测试/构建或进行 Git 操作。本轮没有涉及交易规则变更。

## 逐篇核对

### `frontend-review-notes.md`

- 来源第 1–3 行只校正文档索引 `contracts-area-final.md` 的序号描述：手写条目为序号 1、Web 序号 2–131、根目录序号 132–256；总数仍为 256（1+130+125）。属于旁证文字校正，不是产品需求或实现承诺，不得由此推断覆盖源码或消费关系已经复核。
- 当前材料未给出需要变更的产品 caller/owner；没有发现旧产品缺口的关闭或新的遗漏。

### `frontend.md`

- 来源第 1–33 行明确这些是候选设计与历史整合交接，不是待执行批准。四项对象边界均能在当前基线找到真实实现和调用：`SessionHostLifecycle` 由 `useSessionHostLifecycle` effect 创建并接入启动/清理（`apps/web/src/app/useSessionHostLifecycle.ts:87-110,193-232`）；存档 facade 借用既有 gate、持久化 owner 与 refs（`apps/web/src/app/useSaveCommands.ts:17-60`）；`useTradingCommands` 持有界面表单与查询态、从协议/Redux消费订单，仍经 host 提交（`apps/web/src/app/useTradingCommands.ts:22-50,58-103`）；`MarketChartProjection` 经 `useMarketChartRuntime` 构造，由 `MarketRuntimeProvider` 提供 actions/data/selection，真实视图消费 Provider（`apps/web/src/app/useMarketChartRuntime.ts:4,15-17`、`MarketRuntimeProvider.tsx:17,49-50`、`LocalRefreshViews.tsx:17`）。当前独立代码核对亦覆盖这些 caller 与语义边界（`agents/implementation-audit/exhaustive-review/luna59.md:14-20`）。
- 因而不能将对象名称/结构本身等同于业务闭环：Trading 提交提示仍表示已提交；订单事实仍由协议/Redux 与 engine 持有；图表 projection 是视图派生；save facade 借用现有 authority。未发现本来源提出而现基线缺失的已批准新增封装承诺。来源中“候选未实施”的历史状态已被当前代码反证，应按当前 caller 认定相应提取已接入。
- 来源特别披露了 web-08 四个 HTML（含原型内联 JS）须纳入审查，以及 chart 多批次缓存和只读借用约束；这些是来源自身范围提醒，不足以单独证明最终闭合。当前总账的各主题边界仍然独立，不能由上述提取核销。

### `hosts_tooling.md`

- 来源第 1–19 行是收尾交接，明确候选未实施、只修改审计文档，并限制对旧 full_read/审查结果的继承范围。其 server pause-preferences 时序说明是对历史 Server 审查证据的总结；本批不把它扩张成新的 server 代码审查，也不把新局 generation=1 外推到 restore 后状态。
- 来源记录的 hosts/tooling/engine-tests 总数及 OOP 分类是工作材料统计，不是实现完成度。它明确列出的真实 caller（routes、actor/tests）和“sender 封装不迁移测试/无状态工具不算业务对象”边界，没有被本批发现与现行总账冲突。测试建议或既有测试存在不等于生产调用链已闭合。
- 该材料的金额/数量、T+1 和单位表述未提出新规则；本轮没有重新核验交易所规则，故不额外声称规则正确性。

## G01–G68 / Q 全章矩阵

按当前实现总账和覆盖索引逐章核对。本批三份历史材料均不构成 G/Q 的修复证据；G27 仍为总账已核销项，其余 G 缺口仍按原状态跟踪。Q10 是转入 G39 的编号历史，不作为开放 Q；其余 Q 状态依总账分别处理，不将它们笼统视作 bug。

| 总账章节 | 编号范围 | 本批交叉结论 |
|---|---|---|
| 宿主与远程链路 | G01–G05、G18–G20、G40、G53、G66 | 生命周期 owner 提取不改变 host 行为契约；无确认控制、pull/重连等缺口不能因 `SessionHostLifecycle` 存在而关闭。 |
| 策略、个人信息与估值 | G06–G09、G16、G28、G35–G38、G42–G43；Q02、Q11 | `hosts_tooling.md` 关于个人经历/策略简化不核销这些实际业务消费缺口；来源没有提供相应生产 caller 证据。Q11 的技术方向已解决，不能重开成新对象化需求。 |
| UI 与行情显示 | G10–G14、G22–G25、G30–G34；Q04、Q07、Q08 | Trading facade 和 chart projection 已接真实 UI caller，但不因此完成 UI 错误关联、图表周期/指标口径等独立语义；保持总账状态。 |
| 日历/配置/指标接缝 | G15、G17、G29；Q01、Q03、Q06、Q09 | 本批对象提取与配置/日历/跨端金额接缝无直接证据关系，不核销、不新增规则。 |
| 构建、发布与验证工具 | G21、G26、G27、G39；Q05 | hosts/tooling 材料只报告历史静态工作和测试盘点；未在本轮运行验证，不能作验收证据。G27 的既有核销状态不受影响。 |
| 交互、解析与验收边界 | G21–G26、G45、G48–G56、G64–G68；Q05 | Save/Trading facade 的存在不证明输入/API/用户错误提示/辅助交互等全部完成；逐项保留现行总账判断。 |
| 公司经营、合并、诊断与其余问题 | G28–G39、G41、G49–G52、G57–G63、G67–G68；Q01–Q03、Q06、Q09、Q12–Q23 | 来源无可映射的同一 owner/caller 证据；不把结构对象、历史测试覆盖或统计量当成这些缺口的核销。 |

矩阵章节存在交叉覆盖，重复编号沿总账主题保留，不代表重复缺口。这里是本批材料与全章分类的交叉检查，不是重新逐项复审全部 G/Q 的生产实现。

## 结论

- 未发现已批准封装承诺遗漏、旧 G/Q 核销被当前代码反证，或需新增候选。frontend 四个已实施对象有当前真实调用链；独立业务缺口仍由 G/Q 总账管理。
- 三份来源的数字索引校注不改变产品范围。A 股规则、存档/API 契约和 UI 文案本批均未修改；无交易语义主张变更。
