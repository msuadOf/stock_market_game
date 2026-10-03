# 隐藏复核批次 130（owner=5）

## 基线与材料

复核基线为 `.worktree/implementation-reaudit`，`HEAD` 与产品 caller 均为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。依冻结 scan-plan 连续读取三份指定材料至 EOF；实测行数与 SHA-256 与计划相符，逐项凭证见配套 JSON。重点核对 `areas/web.md` 的候选对象边界，并从 `file-index.md` 定位 Web/协议契约相关归属；`constraint-index.md` 将配置、静态资源、协作/产品/架构文档与实现文件区分，不能单凭其“保留/支撑”分类推出产品功能完成。

本轮是静态复核，不改产品代码、Git 状态或正式文档，不运行测试/构建，也不重新认证完整源码或浏览器行为。

## 当前 owner 与调用链

- Web 生命周期边界已有 `createSessionHostLifecycle`，由 `useSessionHostLifecycle` 在 `App.tsx` 接入。生命周期对象拥有 effect 内取消标记、宿主注册/启动/停止/释放与 dispose；Shell 仍持有跨重挂存档来源、替换 gate、日终持久化及多项独立 UI/轮询 effect。这里没有证据要求另造覆盖所有职责的 `AppSession`，也不能因生命周期 helper 存在就宣称各宿主副作用都已确认完成。
- 存档命令已有 `createSaveCommands` / `useSaveCommands`，Shell 注入宿主、`InitialSaveSource`、`DayEndPersistence`、替换 gate、文件目标和 UI setters。读档/新局、授权文件与日终保存的写集不同；`ADR-0025` 约束只在成功自然日日结持久化，并保留启动/显式换档的一次读档语义。泛化为共享 authority 或把日内状态写入存档没有依据。
- 交易表单与命令已有 `useTradingCommands`，`App.tsx` 持有其返回对象，真实 host 接受订单，Redux/协议投影呈现委托事实；活动 `SessionSetup` 用于数量和市场单规则预检。该 hook 对象是 UI 命令门面，不替代引擎交易权威。`ADR-0022` 的 symbolic limit price 与现行委托/撤单确认语义须保持；本轮不复核交易规则本身。
- 图表缓存已有 `MarketChartProjection`，由 `useMarketChartRuntime` 持有，再由 `MarketRuntimeProvider` 提供给 App 与刷新视图。其价格/竞价/成交量/日线缓存是可重建的展示投影，Redux 和 protocol coordinator 的事实/游标 owner 仍分离。代码现有 `pricePointsFor` 等访问会新建数组；生产 `acceptReduction` 每次更新图表 state，旧 `G31` 的引用稳定/无关股票不刷新问题仍应按总账保留，不能把类已抽出当作 G31 修复。
- 真实 `TradeMarketControls` 在 `LocalRefreshViews.tsx` 仍以 `DEFAULT_SETUP` 查股票类别；读档路径由 `useSaveCommands` 把档案 setup 安装到 `activeSetup`，而 `useTradingCommands` 用该活动 setup。由此确认总账 `G68` 指出的规则来源分裂依然存在：快捷涨跌停提示和提交前规则可能不是同一 setup。此处是现有语义/配置消费缺口，不是 OOP 提案引入的问题；应按 G68 单独处理并保持提示与引擎权威校验一致，不据此扩大成交易撮合错误或断言所有股票类别限价不同。

## 总账与 ADR 判定

历史 Web 区域材料自称“候选设计，未实施”，但当前源码已逐步落地多个窄边界（生命周期、存档命令、交易命令、图表 projection）；这些事实以当前 owner/caller 为准，不回写历史状态，也不将 OOP 提取视为功能修复。代码文件索引的“有具体迁移项”是候选动作登记，“保留现有组织/测试支撑”是该次盘点的处置意见，不是现行生产承诺的验收证明。

相关现行约束包括 `ADR-0010` 的统一 HostUpdate 协议与宿主传输差异、`ADR-0022` 的符号限价、`ADR-0025` 的日终持久化、`ADR-0027` 的运行时宿主选择，以及最新 `ADR-0028` 的标签发布/静态 Pages 边界。它们未要求创建这些特定 UI 类，也不授权改变协议事实、持久化时点、活动 setup 规则来源或生产构建/发布契约。`docs/open-questions.md` 的产品级问题不因历史 OOP 候选关闭；审计总账内部 Q 编号与该文档编号不可混为一谈。

裁决：本批没有新增 G/Q，也没有发现材料声称 OOP 抽取已修复 G/Q 或错误关闭现行问题的证据。`G31` 与 `G68` 维持当前审计总账的未完成状态；其余 UI 错误反馈、金额/单位和辅助交互问题依总账分别处理，不与对象迁移合并。没有新增交易制度变化，不构成现行沪深 A 股规则认证；适用规则和简化仍以 `docs/trading-rules.md` 与相应 ADR 为准。

## 来源凭证

三份来源均连续读至 EOF，行数与 SHA-256 已和冻结 scan-plan 核对。来源描述的是完整代码文件索引、非代码约束分类和 Web 区域候选归属；它们不替代当前生产 caller、owner、consumer 追踪，也不构成测试或完整复核通过证明。
