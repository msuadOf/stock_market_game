# 隐藏候选独立裁定 04

## 范围与依据

核对输入为隐藏复核 `batch-049.md`、`batch-139.md`、`batch-151.md` 全文，基线 `43b1aa5`。已读仓库 `AGENTS.md` 与 `docs/principles.md`，并对照目标 worktree 中的生产调用、相关测试、`implementation-audit-2026-10-02.md`、`reaudit-host.md`、`ADR-0010` 及既有隐藏复核 `batch-082.md`、`batch-083.md`。这是静态审查；没有运行测试、利用复现或 Git 操作，也没有更改产品源码。

## 逐项裁定

| 候选 | 代码和契约核实 | 裁定及去重 |
|---|---|---|
| `CivilInstant` 反序列化越过秒域 | `packages/engine/src/calendar/date.rs` 对私有 `second_of_day` 字段 derive `serde::Deserialize`；`new` 才检查 `< 86400`。因此经 serde 的外部数据可绕过构造器，形成非法 `CivilInstant`。这是当前真实输入不变量缺口，而非只凭历史注释推断。 | 新候选，现有 G01–G68 及 Q01–Q23 中未检出相同登记；建议列为核心 engine 序列化边界缺口。修复设计需保留 `Serialize`/TS/API 契约并在恢复输入处报显式错误。未验证每个生产反序列化调用都可达，故范围限于公开 serde 输入边界。 |
| `CalendarPolicy` digest 未绑定 `source_digest` | `calendar/policy/validation.rs::compute_content_digest` 编码 exchange、year、`source_citation_id`、closed ranges 和 fallback digest，但不编码 `OfficialCoverageEntry.source_digest`；校验仅要求 source digest 非空。改动来源摘要不会改变 policy content digest。字段注释称摘要绑定通知文本，`calendar/data/mod.rs` 又称数据 digest 进入 `CalendarPolicy.content_digest`，为内容身份完整性提供现行依据。 | 新候选，未发现 G/Q 重复项。作为 policy 内容身份/来源完整性缺口登记，不延伸声称交易日历规则或 A 股制度数据错误。需确认数据装配链对 source_digest 的设置后决定是否将摘要纳入规范 digest；当前源码已足以确认字段变化不影响该 digest。 |
| Worker `load` stale/generation 校验 | `worker-host.ts` 的 load await `restoreWorkerSlot` 后直接采纳返回 `nextGeneration` 并更新 baseline；总账 G53 已明确记载恢复响应必须推进 generation，并指出正常 WASM 绑定自增、问题在异常响应缺少大于原值守卫。 | 归 G53，重复候选不新增。无证据把并发/销毁后响应扩展成另一已确认故障。 |
| `postMessage` 同步异常后请求资源延迟清理 | `WorkerRequestScope.request` 设置 timeout、pending 和 listener 后同步调用 `postMessage`；throw 会使 Promise reject，而 timeout 后才执行 cleanup。测试明确断言这一时序。WorkerHost 确实消费此 scope；但源码注释和测试均把该行为固定为既有行为，历史 OOP 审查也把改变请求清理时序列为范围外。 | 不登记新 G。可见资源会短暂保留至已有有限 10 秒 timeout，且没有立即清理契约/认可的资源生命周期要求。除非后续提出并批准同步失败应立即释放的行为需求，否则不能仅凭有 cleanup 延迟就定为缺陷。 |
| Tauri listener 部分注册、初始化失败回滚 | `tauri-host.ts` 中两个 listen await 位于初始化 try 外；第二次注册失败时前一个 listener 可能未退订。create_session 成功后 baseline/capability 初始化失败的 catch 会退订 listener，但没有 stop_session。它们是具体失败路径。batch-049 与更早来源均仅称条件风险；batch-083 已明确区分于 G51 的 dispose `stop_session` rejection。 | 本轮不升格 G：没有已批准的初始化回滚/部分注册清理契约，也没有证据证明这些临时资源在失败后违反现行生命周期保证。保留为明确待核实的资源清理候选，不与 G51 合并。若产品要求失败初始化原子回滚，应按独立契约与故障注入验证立项。 |
| Remote pause preference 使用 generation `"1"` | 当前 `remote-host.ts` 在 publisher baseline 未建立时用 `"1"`；lifecycle 确在 `host.start()` 前 await 设置。batch-082 记载 server 跨域核实认为新局初始 generation 1 合法。此项只针对无 baseline 的新局，不证明恢复后的状态。 | 已由现有协议证据解释，不是缺口；不是 G04。G04 明确是暂停后再次 start 重发旧缓存 baseline，独立保持未修。 |
| Remote 旧 baseline/乱序语义 | Remote 收到 baseline 时安装/替换缓存；ADR-0010 规定 baseline 用于初始化、读档和显式重同步，但没有提供网络中旧 baseline 到达顺序的保证。此前审查也明确未取得端到端顺序证据。 | 未决协议边界，不新增 G；不能凭缺少 generation 比较或 generation 1 单独判错。G04 的缓存重放已有具体调用链，勿以此未决项取代。 |
| Account 手续费注释 | `batch-139` 报告的买卖注释与实际过户费扣减不一致，属于文档/注释与代码漂移；不是实现漏扣或交易规则结论。 | 单独作为准确性修正文案，不形成产品行为 G；本任务不改源码。大 A 费率依据未在本次重新核验。 |

## 汇总结论

新增真实输入边界候选为 `CivilInstant` serde 秒域校验，以及 `CalendarPolicy` content digest 未绑定 `source_digest`；均未发现与现行 G/Q 重复。`Worker.load` stale/generation 属 G53。postMessage 延迟清理虽代码路径真实且有测试，但目前是既有、有限时序，没有可据以升级为缺陷的立即清理契约。Tauri 初始化 cleanup 保留条件性待核实项，与 G51 不同但当前证据不足以定为新 G。Remote generation 1 是新局已确认语义，G04 仍是 pause/resume 旧 baseline 重放。Account 手续费材料是注释漂移，不应误写成交易执行缺陷。

候选涉及 serde 输入完整性与日历政策来源内容身份，不改变沪深撮合、交易时段或证券规则；未查询交易所/中国结算规则，不对费率或交易制度作新结论。本文仅为受限静态裁定，不宣称任何动态测试或运行验收通过。
