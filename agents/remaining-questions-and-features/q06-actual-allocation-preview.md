# Q06 本局实际初始化分配展示

## 权威来源与展示时机

新局只选择一次 seed，只创建一次 Engine。`GameSession::initial_allocation` 在该实例首次开跑前读取已经分配完成的账户持仓，逐证券汇总 `Retail`／`Inst`／`Hot` 的实际整数股数、人数与零持股人数。它不再次初始化，不消费 RNG，不发放现金或股份，不生成存档。Web `SessionHostLifecycle` 在新建宿主后、`start` 前等待该查询，再把本局结果交给桌面与移动新局设置区的 `InitialAllocationSummary`。

本局的开局结果只保留在 UI 内存中，后续成交不改变这份展示。分配草稿仍只影响下一次新局；界面明确标注实际分配、不是预计比例，不用配置权重或另一次随机试跑冒充真实结果。占比仅按实际股数除以该证券流通盘展示，两位小数是展示舍入，合计守恒使用整数股数。零流通盘的占比显示 `—`，而不是除零或虚构百分比。

## 输入边界与读档语义

Engine 的非持久 `fresh_initial_allocation` 表示该实例来自 `new`；`restore` 无论 tick 是否为零都设为 `false`。私有 tick shadow 保留来源资格，正常 tick／自然日推进另外受 tick 和开局日期守卫；通过 restore 重建的回滚实例不伪称新建。该资格不属于经济事实，不进入 `SaveSlot` 或业务 hash，没有增加格式字段或兼容路径。

查询只允许真正新建、tick 为零、仍处于开局自然日的实例。查询结果从三类账户真实持仓累加，账户人数与股数均 checked，玩家初始持股必须为零。存在 NPC 时必须分配完流通盘；零 NPC 的合法配置显式返回 `unallocated_shares`，UI 显示未分配，而不是补给玩家。每证券总流通盘、三类实际股数和未分配股数做整数对账。

WASM Worker 请求绑定 requestId／generation，返回后还检查 baseline epoch；Tauri 请求绑定 sessionId／generation 及读取游标；Remote 请求使用对应会话的 Bearer token、session_id／generation 与当前读取游标。Remote 首次查询可建立 Publisher 基线连接，但不调用启动撮合接口。服务器在 actor 内校验 generation 后只读，HTTP 查询字段严格校验。各宿主响应按当前 DTO 精确解析，拒绝错股、漏股、重复类别、越界值、人数／股数矛盾及不守恒。

读档不会调用该查询，也不会把恢复时的当前持仓重新标成初始分配。快速槽／文件读档成功后清除旧局初始结果；宿主创建或查询取消后的晚响应不能写 UI、不能启动旧实例。

## 领域范围

这是游戏开局 NPC 分配的权威只读展示，不模拟真实证券发行配售，不改变沪深撮合、申报单位、费用、T+1 或其他交易制度；不引入额外资金、日内持久化、历史行情校准或存档兼容。

## 验证记录

- parser 首次短测实测因 `initial-allocation.ts` 尚不存在而失败，随后两项契约 case 通过。新增 Engine API 的 Rust 失败编译尚未单独观察，不把其他模块编译失败记作本项 TDD 红证据。
- 外部 10000ms deadline 与 Node 10000ms case 上限下，lifecycle／parser 共 17 项通过，save commands 9 项通过；真实 React SSR 展示 1 项通过，验证 3／7 与 4／7 实际舍入占比、零持股人数、整数对账及读档不假称开局。
- 三宿主适配器定向 3 项通过：Worker／Tauri 同 generation 刷新后旧响应拒绝；Remote 在开跑前建立基线且不启动撮合，load 的基线重同步使旧请求显式中断，之后晚返回不复活结果。Remote 首跑断言预期 generation 错误，但生产通用请求 owner 已先中断；修正为精确断言真实中断原因，不放宽为任意拒绝。
- root 以 `--jobs 32` 统一刷新 Rust 编译，`.tmp/checklist-wave3/build-5.jsonl` 与 `host-build-2.jsonl` 均以 `build-finished: success=true` 结束。Engine 真实分配与 hash 不变、零 NPC 未分配／开跑后拒绝、同日 tick0 restore 拒绝三项 exact case 分别通过，用例时间 0.24／0.21／0.44 秒；Server owner／generation／未知字段与 Desktop 真实开局查询各一项通过，均为 0.19 秒。普通 case 经外部 10000ms deadline 单独运行，不把 no-run 编译称为测试执行。
- WASM 宿主绑定编译通过；本批不宣称真实浏览器 WASM runtime 验收。四个实际分配 DTO 由真实 ts-rs 导出，`types-2.log` 的 85 个绑定导出 case 全部通过，合计 0.33 秒；`tsc-3.log` 的 TypeScript 编译通过。未手写或迁移生成类型。
- root 的第三批编译发现 `hash.rs` 穷举 `GameSession` 字段未列入新建查询资格，报 `E0027`。已显式忽略该非业务实例资格，保留穷举检查，不用 `..` 掩盖以后新增业务字段漏 hash；这属于接线编译错误，不记为业务断言红测。
- 非作者已独立阅读完整实现及适配器／读档测试，发现 tick0 restore 查询资格遗漏，已修并补测；最终四个生成绑定、hash 穷举忽略与宿主执行日志的增量复核通过，并确认暂存范围没有混入 Q11／Q22，差异检查通过。未运行完整回归。
