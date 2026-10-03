# 批次 049 独立复核

## 阅读完整性

| 文件 | 行数 | SHA-256 | EOF |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/modules/web-02.md` | 450 | `44400da019792d6392695c86110aaed6e8eb875457f6d923288a788472c848a9` | 是，逐段全文至末行 |
| `agents/oop-refactor-audit/exhaustive/modules/web-03.md` | 156 | `c241b88aad53aa71fb27245bcfc72c763e8dbe6738c9f67608a46e86d2b67daf` | 是，全文至末行 |
| `agents/oop-refactor-audit/exhaustive/modules/web-04.md` | 400 | `e465951ecf959428a37700a4f8e1864d5364b81ceb1e76e87e9634580a534f13` | 是，逐段全文至末行 |

`scan-plan.json` 的 batch 49 与三项文件指纹、行数一致。目标源码 `git rev-parse HEAD` 为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。已阅读 `docs/principles.md`、`docs/open-questions.md`、相关架构/错误处理规则、ADR-0007、ADR-0010 及 ADR-0023–0028；复核范围为三篇 Web OOP 来源、其当前 owner/caller/consumer 与总账适用性。未运行测试、构建、官方规则查询或 Git 写操作。

## 结论

三篇材料的主要 OOP 判断得到当前源码支持：已有状态对象、React/Redux 生命周期或宿主闭包由实际生产调用链消费；DTO、协议转换、展示映射和测试 fixture 不因函数较多而需要包装成 class。`AutoOrderManager`、`CompanyQueryCoordinator` 和 `ProtocolCoordinator` 都有真实生产 owner/caller；`tauri-event-coordinator` 则仅被测试使用，不可把 helper 测试当成 Tauri host 已调用它的证据。

“没有待实施必要 OOP 动作”不等于宿主、UI 或协议行为完整。历史来源自身已指出若干独立行为边界；当前总账的 G 项继续按其粒度跟踪。发现一处来源漏读当前测试的反证，以及一处可能未列入现行 G/Q 的 Tauri 初始化失败清理风险，详见下节。后一风险是静态控制流观察，尚无失败注入/运行复现，不升级为已确认产品缺陷。

## owner、caller 与 consumer

| 职责 | 当前 owner 与调用链 | 复核 |
|---|---|---|
| 条件单生命周期 | `auto-order-manager.ts` 的 `AutoOrderManager` 拥有单实例订单及 pending 集合；模块级 `nextId` 跨实例递增。`App.tsx` 保存 manager ref，`MarketRuntimeProvider.tsx` 向其提供规范化行情点，回调把同一 ID 投影到 Redux/UI。 | 保留判断成立。不得把 manager 实例放入 Redux，也不能据此推断连续量、展示或交易表单契约均已完成。 |
| 公司披露查询 | `App.tsx` 的 `connectProtocol` 创建 `CompanyQueryCoordinator(host, store.dispatch)`；host 查询数据，coordinator 管 generation、页请求、缓存、seq 覆盖及披露/民用日期事件，CompanyPanel 消费 store 状态。 | 有生产调用链；无需另造共享 mutable service。分页/披露显示的行为缺口按 G 项处理。 |
| 协议状态 | `App.tsx` 创建 `ProtocolCoordinator` 并接 baseline/applied/failure callbacks；coordinator 调用纯 `reduceEngineUpdate`，再 dispatch Redux 投影并推进自己的 cursor 状态。 | 已有运行时 owner。Redux 与 coordinator 中活动委托数据属于不同消费者的投影，不能以同名字段推断共享所有权或已消除复制。 |
| 宿主与局部 gate | Remote、Tauri、Worker host factory/闭包持各自 socket、timeline/generation、callback 与 dispose 状态；`IndicatorRequestGate`、`PlayerOrderRefreshGate`、`InspectorRequestGate` 持窄异步代次。 | 保留当前传输差异和生命周期边界。`createTimelineEventGate` 仅由 `tauri-event-coordinator.test.ts` 调用，生产 listener 在 `tauri-host.ts` 自己检查 session/timeline/disposed。 |

## 反证与遗漏

1. **web-04 的测试覆盖描述过时。** 来源称 `requestWorker` 同步 `postMessage` 抛错后没有测试覆盖。目标基线已有 `worker-request-scope.test.ts` 的专门用例：断言 Promise 以原错误拒绝、timeout 到期前 pending/listener 仍存在，timeout 到期后两者清零。实现也在 `worker-request.ts` 显式保留该时序。故该路径不是“无测试、静态未知”；但测试冻结的是延迟到 timeout 清理的行为，不能证明立即清理契约，也不能仅凭静态材料决定应否改变这一有意边界。该项在来源文档中应更正为“已覆盖既有延迟清理行为；若要求立即清理，需另立需求/验证”。
2. **Worker `load` 的 generation 守卫仍有独立缺口。** `restoreWorkerSlot` 只解析正安全整数；`createWorkerHost.load` await 后直接采纳 `nextGeneration` 并更新 baseline。对照 `save`/`refreshBaseline` 的 await 后 stale/disposed 检查，以及总账 G53“恢复 generation 必须推进”，此项继续由 G53 跟踪。不要把它与新提的并发/销毁后 stale-response 可能性合并为已复现事故；后者仍需说明并验证可达条件。
3. **Remote baseline 顺序语义仍未定。** `remote-wire` 对 baseline generation 做格式验证；Remote handler 没有比较新旧 baseline generation。ADR-0010 将 baseline 限定为初始化、读档、显式重同步，并要求 Tauri 的旧 timeline 消息被拒绝，但当前证据未证明 Remote 可能收到乱序旧 baseline，也未证明其排序保证。保持为协议契约问题，不凭格式守卫缺少比较就定为产品 bug；不以旧草稿恢复未接受的额外规则。
4. **Tauri 初始化清理存在有条件风险。** 第一个 `listen` 完成后第二个 `listen` 才被 await，二者均在初始化 `try` 外；若第二个监听注册拒绝，第一个 unlisten 没进入初始化 `catch`。若 `create_session` 已成功，而取 baseline/capability 失败，catch 会移除 listeners 但没有 `stop_session`。这指出两种初始化失败路径的可能资源遗留，但来源没有故障注入、活跃 production consumer 触发证据或确认资源生命周期约定。它与 G51 已记录的 dispose `stop_session` rejection 出口不同，不应重复并入 G51；可作为待独立复核的新清理候选。
5. **Worker restore 的调用者边界需保留。** 公共 `EngineHost.load` 由 App/session replacement 流程消费；App 的串行换宿主及 host identity 检查不构成 host 自身在 `await` 后的通用 `disposed/currentGeneration` 守卫。源码支持“尚未明确覆盖”的静态描述，不支持声称已发生迟到响应写回。

## G/Q 交叉矩阵

下表按现行实现审计 G01–G68 全部编号归组。这里是本批适用性核对，不是重新审查或核销每个生产缺口；G27 保留其已核销状态。某条存在于本批涉及的 Web 文件中，不表示该文件层 OOP 有缺陷。

| 编号 | 本批关系与处理 |
|---|---|
| G01–G05、G18–G20 | 宿主/RPC/刷新相关；Web host 有 owner 不证明凭据、pull、baseline 恢复、重连、背压、聚合或新局 seed 已完成。保持总账状态。 |
| G06–G09、G15–G17、G21、G26、G28–G29、G35–G39、G41–G43、G54–G63、G67 | 本批没有对应 engine、工具或对外库实现变更，也没有核销证据；保持总账状态。G53 在下一行单独说明。 |
| G10–G14、G22–G25、G30–G34、G44–G49、G64–G65、G68 | 涉及显示/交互的 Web 组件或纯映射，但已有组件/helper 不等于契约完成。保持连续量、跨日时间线、逐笔、零量、导航、单位、语言/无障碍及 setup 消费等主账状态。无交易语义变更。 |
| G27 | 总账已核销的发布标签 SHA 守卫；本批无相关实现变更，不撤销核销，也不把 ADR-0028 的 build-only 发布规则误作额外测试授权。 |
| G40、G50–G53、G66 | 宿主确认、渲染/协议/Worker/Remote 独立状态；无核销。G53 的 `nextGeneration` 递增校验由本批代码对照再次支持；G51 的 dispose IPC 错误出口仍与 Tauri 初始化资源清理风险不同。 |
| Q01–Q23 | 无本批决策或代码变更可解决任何 Q。交易单位仍为分/股，报告 DTO 与图表投影不得模糊精度；Q07/Q08 口径、Q13 日历差异等保持各自待决状态。Q09 等架构问题不由“owner 已存在”自动结案。 |

G 编号覆盖集合明确为 G01 至 G68（G27 仅保留核销状态）；Q 覆盖集合为 Q01 至 Q23。对 Q 的本批映射均为“无核销主张”，不根据模块名或共用 host 关键词强行绑定。

## ADR 与领域边界

- ADR-0007/0010 确认统一 `EngineHost` facade 与 React/Redux/协议层责任边界；适配器保留 Worker、WS、Tauri 传输差异。ADR-0010 对命令确认、baseline/delta、generation/timeline 的语义优先于早期 Web 草稿。
- ADR-0027/0028 接受启动宿主选择、HTTP(S) Remote 接入、七个手动入口、有效 tag Release/Pages 和发布仅构建/打包/核验/部署。早期材料中笼统的测试或发布步骤不能覆盖新决策；本批没有跑发布或产品验证。
- ADR-0023 将开局前史定为虚拟、游戏运行行情由实际撮合生成；ADR-0024 允许参与者现金池随费用减少；ADR-0025 只在成功日结生成持久存档；ADR-0026 定义机构个人经历参数假设。这些较新决定取代其历史材料中的冲突描述，本批没有对此提出重开。
- A 股语义未改变：只复核 UI/host ownership 和现行调用，不重新认证交易规则、撮合、资金、股份或 T+1；未查询交易所/中国结算原文。

## 结果

本批没有发现必要 OOP 抽取动作。候选文档应保留已有 owner/caller 结论，同时修正文档中 worker 同步抛错“未覆盖”的错误描述；对 Tauri 初始化失败清理风险作单独待核实记录，不将其伪装成已确认缺陷。G01–G68/Q 无核销主张，测试、构建和官方制度核查均未运行。
