# 批次 171：web-05–web-07 独立复核

## 读取凭证与范围

以 `.worktree/implementation-reaudit` 的 `HEAD=43b1aa5f25226c72976ca172d32f4a8eeb2272ad` 为产品基线；唯一计划为 `hidden-review/scan-plan.json` 的 `id=171, owner=1`。三份来源从主工作区指定路径逐篇连续读取至 EOF，实测行数与 SHA-256 均匹配计划，完整元数据见配套 `batch-171.json`。web-07 首次整体输出超过工具显示上限，随后按 1–150、151–300、301–436 行连续补读，覆盖全篇。已读目标 worktree 的 `AGENTS.md` 与 `docs/principles.md`。来源中历史 agent 操作要求只作为材料，不作为本轮授权；OOP 提取建议不自动构成缺陷修复。

## 来源章节族及候选再核

- `web-05.md`：移动端行情模型/组件、K 线与分时、Mobile UI state、存档候选校验、`DayEndPersistence`、日终 targets、页面隔离和 React 启动。旧材料的核心判断仍成立：`MinutePointCollector`、`AuctionPointCollector` 保有跨事件状态，但当前仓库中它们的生产引用只见定义及 `market-model.test.ts`；实际行情路径由 `useMarketChartRuntime.ts` 投影并派发 Redux 更新。故“有对象”不证明生产链使用这些 Collector，也不能拿 Collector 测试消除最新台账 G10/G11/G44 等显示边界。相反，`DayEndPersistence` 持世代与写入队列，`App.tsx` 以 ref 创建并由 `useSaveCommands.ts` 在换档/恢复/日结协调，属于真实生命周期 owner。来源提出的 retain/不机械 class 化是结构建议，不是修复报告。
- `web-06.md`：文件存档目标、LocalStorage repository、严格 schema 及个人/公司账簿 DTO/parser。当前 `CompressedLocalStorageSaveRepository` 仍持有 storage/key/codec；`save-file.ts` 目标闭包保有授权句柄与异步写入责任；公共 `parseSaveSlot` 进入 `parseStrictSaveEnvelope`，Worker 与 WASM host 的 restore/load 路径继续调用严格 parser。保留这些状态 owner 与纯 parser 分层再证。来源同时指出 upload picker 的 focus listener 清理边界；这属于具体资源行为候选，OOP 重组本身不会修复。未将其升级为本轮产品变更，也未声称 listener 现状已满足所有取消/失败清理契约。
- `web-07.md`：个人存档 schema、Redux slices、协议快照、会话替换、格式化、A 股表单输入、WASM 声明与 Vite 配置。`InitialSaveSource` 与 `SessionReplacementGate` 由 App refs 分别持有不同 epoch/异步状态；`store.ts` 继续用 plain serializable RTK state，`ProtocolCoordinator` 负责 baseline/delta 投影。保持这些边界与 ADR-0004/0010 一致。`RecursiveChineseNumberFormatter` 的实例持有并复用格式配置，纯格式映射仍是函数。`trade-input.ts` 是 UI guard 与显示帮助，不替代 engine 下单校验或交易规则。

## 当前调用链与账目边界

- 行情：`useMarketChartRuntime.ts` 将本地投影 `setSnapshot`；协议链由 `protocol-coordinator.ts` 安装 baseline、应用 runtime delta。`market-model.ts` 的两个 Collector 在源码存在，但不在这条生产调用链中。独立 `applyPriceTickMarket` 由对应 host 测试直接调用；不能按测试 import 推定 Redux 或生产 caller。
- 存档：`App.tsx` 创建 `InitialSaveSource`、`DayEndPersistence`；`useSaveCommands.ts` 协调载入/恢复/新局与 invalidation/idle 屏障；文件目标从 `save-file.ts` 选择；保存 parser 由 `save-schema.ts` 暴露并由 host restore 调用。没有理由将授权句柄、排队世代或解析 DTO 合并成单一领域对象。
- 行情展示缺口按最新 `implementation-audit-2026-10-02.md` 保持原分类：G44 零成交量画正高度、G46 桌面卖档 rank 标签、G47 竞价 null 槽跨线仍属现行候选；web-07 不能核销它们。G68 还记录合法非默认 setup 的快捷价格带读取 `DEFAULT_SETUP`，来源提到的 UI 辅助价带/下单输入契约不能证明非默认局已对齐。没有引入或修改 A 股制度；金额“分/元”、股数/手及 engine 受理权威边界保持现状。
- 其余 G01–G68/Q 仍以最新审计总账为准；本批 OOP 文档不构成相应行为的生产证据，也不关闭任何总账条目。没有从审计材料中发现可支持新增/关闭 G/Q 的生产证据。

## ADR 与裁定

ADR-0004（RTK plain state）、ADR-0010（host/protocol baseline 与局部刷新）、ADR-0019（不设任意订单数配额）、ADR-0022（Highest/Lowest 意图在 engine 受理时解析）、ADR-0025（日终持久化）、ADR-0026（机构个人经历与暂停状态）、ADR-0027/0028（运行时宿主及部署/发布边界）均与来源中应保留的边界相容。ADR-0018 仍为 `proposed`，不能用其整体方案推导已批准实现要求；具体已接受目标只按总账登记。较新 ADR 没有把 OOP 提取转化为修复授权，也没有改变该批所涉 A 股撮合语义。

## 结论

三份历史材料的主要对象归属建议在基线调用链下得到再证：有跨调用状态/资源职责者由现有 owner 持有，DTO/parser/reducer/投影维持函数或可序列化数据形态。候选没有形成新的生产修复项；需留意的具体缺口仍按其独立契约进入 G/Q 或后续工作，不以对象抽取代替行为修复。本轮仅静态复核并记录，不修改产品代码、正式规则、G/Q 总账或 Git，不运行测试/构建，也未做官方规则联网核验。
