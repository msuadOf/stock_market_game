# 批次 170 独立复核

结论：未发现可确认的“已批准承诺遗漏”或“错误历史核销”。`web-02`、`web-03`、`web-04` 均明确是未实施的候选设计/模块调查；其中 retain/support 判断不等于产品实现或验证承诺。现有 Web 模块职责总体符合 ADR-0007 的 React/Redux 边界及 ADR-0010 的 `EngineHost`/`HostUpdate` 边界。当前源码仍保留来源记录的若干边界候选：Worker `load` 缺少宿主侧 await 后 generation/disposed 检查、`postMessage` 同步抛错未立即清理请求资源，以及 Remote baseline 未比较新旧 generation。来源已明确把它们作为未证实可触发的独立候选；本轮确认仍可在 baseline 源码观察到，不将其写成已确认运行缺陷或错误核销。

复核方法：按计划核对三份来源的全文读取记录、SHA-256 与行数；当前 checkout HEAD 与计划 baseline `43b1aa5` 完全一致。检查 Worker/Remote host 与请求实现、`AutoOrderManager`、`MarketGrid` 的当前调用/状态所有者，并查阅 Q6、Q9、Q11、Q12 与 ADR-0005/0007/0010/0016/0024。未运行测试或构建，未改产品文件。

## 来源核验

- `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/web-02.md` — SHA-256 `9fdd1e9c59ad38945d17a1f267c450409ab52bee8a7ef6621012d6ba307ceb36`，450 行，读至 EOF。
- `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/web-03.md` — SHA-256 `ebcf8fe180e83f9cc8d43b4836e00bce0a56ba792723795ab98ff50094a12dce`，156 行，读至 EOF。
- `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/web-04.md` — SHA-256 `61742ca0d4ca1ad52ede483bd28718ed8dc66594aa1cafa5f7834c440154aa8a`，400 行，读至 EOF。
- 基线：当前 `HEAD` 为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`，与计划指定 commit 相同。

## 当前交叉核对

- `web-02`：`AutoOrderManager` 的 `nextId` 仍是模块级单调计数器；`App`/Redux 条件单投影以 ID 关联。因此多个 manager 的 ID 唯一性和 clear 后不复用仍是来源提出的测试建议，不是当前证明出的冲突。`CompanyQueryCoordinator` 与请求 gate 持有跨请求状态；组件、DTO 和映射函数仍留在 React/纯函数边界。`MarketGrid` 的移动模拟指数以所有当前行的价格与涨幅做算术平均，UI 明示“模拟指数”，不声称为真实 A 股指数；其价格/成交量图表展示换算不改变权威值。
- `web-03`：`ProtocolCoordinator` 负责会话级 protocol state 与副作用出口；解析、校验、归一化、归约、效果映射仍为函数。Redux 与协调器分别持有当前快照投影，来源已解释各自消费边界及 dispatch 后推进状态的顺序；没有确认重复状态已引发生产语义漂移。
- Worker 请求关联：`WorkerRequestScope.request` 将 `requestId` 与响应 `generation` 对本次请求字段匹配并在正常响应/超时清理 listener；它不检查 host 当前 generation/disposed。`worker-host.load` await restore 后安装 `nextGeneration`/snapshot，当前代码没有相应的 post-await stale/disposed 检查；`save` 有该检查。App 的 session replacement gate 限制 UI 替换流程，但不能证明公开 `EngineHost.load` 在并发调用或 dispose 后安全。来源将该项定为待补行为覆盖的候选，而非已证明可达故障。
- Worker 请求清理：listener 与 timeout 安装后同步调用 `postMessage`；同步异常使 Promise 拒绝，但当前路径没有立即调用 cleanup，资源留到 timeout 才回收。接口没有不抛错约定，worker-request 测试未覆盖该条件。属于来源已记录且仍存在的清理候选。
- Remote generation：`remote-wire` 校验已识别 baseline/frame 的 generation 形状；`remote-host` 对 protocol delta generation mismatch 请求 resync，而 baseline 分支直接替换缓存并增加 epoch。当前证据不证明旧 baseline 会到达，也未给出服务端乱序保证；须由协议维护者确认契约后判断行为，不能擅自确定接受/拒绝语义。无初始存档启动时先设置暂停偏好并发送 generation `"1"` 已由来源引用的 server 核查证明符合新会话 `timeline_generation=1` 契约，不应记为缺陷。
- 领域/ADR：Q6 仍仅承诺首发全中文且无 i18n 框架；本批没有语言策略改动。涉及价格/交易协议的审计信息沿用已定义 DTO 与游戏标注；未提出交易规则变化，不需要为纯对象归属候选虚构官方规则依据。Q9/ADR-0005 的撮合与 T+1、Q11/ADR-0016 的 NPC 知识语义、Q12/ADR-0024 的投资者资金边界均未被这三份候选材料改变。

## 审查判断

- 无需改动：三份来源本身明确“候选设计，未实施”；对象化保留/测试支持清单不是承诺已落地的证据。
- 保留并通知协调者的现状候选：Worker load stale/disposal、Worker request 同步抛错后的延迟清理、Remote baseline generation 顺序。三项均已有来源记录且需各自的契约/测试判定；本轮不把它们误归为解决或静默核销。
- 未发现本批 A 股单位或交易制度漂移，也没有新增 A 股语义承诺。未运行测试、构建。
