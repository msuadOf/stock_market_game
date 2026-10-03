# 批次 220 独立复核

## 核验范围

基线为 `43b1aa5`。按唯一计划连续读取三篇来源至 EOF，SHA-256 与行数均匹配。对照基线与当前 Web 宿主代码、`useSessionHostLifecycle` 的工厂调用及 UI 启动路径、Server 新会话及暂停偏好处理，并查阅 ADR-0010、`docs/open-questions.md` 与现行 web-04 items/module。

## 结论

Remote 与 Tauri 复核的事实和限定与代码相符。WASM Worker 复核对重复 `create` 风险的判断也成立，但它覆盖 `worker-host.ts` 却未提及同文件中已明确记录的 `load` 响应边界；应将该复核理解为对其明示候选的局部结论，不能作为完整 Worker 生命周期复核。

## 交叉核对

- **Remote：** `useSessionHostLifecycle` 先创建宿主并在有初始存档时调用 `load`，随后接入协议；新局由应用后续设置暂停偏好。`remote-host.ts` 在没有缓存 baseline 时发 generation `"1"`，`RemotePublisherState.installBaseline` 对任何 baseline 都替换缓存，只在代次变化时失效报告缓存，没有比较新旧代次。Server `new_session` 初始化 `running=false`、`timeline_generation=1`；`SetPausePreferences` 只要求代次相等、不要求运行态，因此新局前置配置合法。旧 baseline 乱序策略仍未由 ADR-0010 或开放问题定案，材料保留为协议边界是适当的，不应由本复核代替协议维护者作决定。
- **Tauri：** `createTauriHost` 的两个 `listen` await 均在初始化 `try` 之前；第二个注册失败时首个 listener 不受后续 catch 清理。两个 listener 就绪后若 `create_session` 成功、随后 baseline 或 capability 读取失败，catch 会 unlisten 但未调用 `stop_session`。可确认的是前端清理路径缺失；不能据此断言 native actor 一定持续运行或 Rust 侧没有其他回收。Tauri 状态仍由单工厂闭包持有，独立记录资源清理风险没有推出必须引入 class。
- **WASM Worker：** `initialize` 没有 in-flight promise，`ready` 收到后宿主即发送 `create`；`WasmSessionSlot.create` 会覆盖旧 handle 而不先 drop。重复创建风险有代码依据，但正常宿主只发一条 init/create，实际触发条件仍需按报告所述协议窗口单独验证。另一个不同边界是 `worker-host.ts::load` 在 await restore 后直接写入 `currentGeneration`、baseline 与 callback，没有检查 `disposed` 或当时代次；同文件 `save` 则有相应的 post-await 检查。`WorkerRequestScope` 只按请求 ID 和该请求回显的 generation 关联响应，Worker dispose 只 terminate，scope 无显式取消/拒绝。因此 load 后写回在销毁/竞争边界尚无 host 侧防护，但其可触发性和产品调用路径需另行验证。当前 App 有 replacement gate，不能把其 UI 串行化扩大解释为公共 `EngineHost.load` 的保证。

## 决策与语义边界

ADR-0010 要求保留 WASM、Remote、Tauri 的真实传输差异，并明确 Tauri event 与 invoke response 不天然全序；它没有决定 Remote baseline 乱序策略，也没有定义 Worker 重复 create 或 load/dispose 契约。`docs/open-questions.md` 未将上述宿主生命周期候选列为待决产品问题。三项审查涉及宿主通信和资源生命周期，没有改变沪深 A 股撮合、价格、账户或单位语义；无需交易所规则依据。不得因这些风险扩大到无关 OOP 聚合。

## 限制

本次是静态交叉复核，未运行测试或构建。Remote baseline 乱序、Tauri native 清理以及 Worker 重复 init/create、并发 load/dispose 的运行期可达性仍未验证。WASM 来源 review 未覆盖 load 边界这一遗漏已明确记录，不据其通过结论宣称 Worker 生命周期完整通过。
