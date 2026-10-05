# ADR-0032：按每会话实际接收事实排序 ingress

- 状态：accepted
- 日期：2026-10-05
- 决策者：用户（Q22 原答）；AI 记录
- 关联：ADR-0017、ADR-0018、ADR-0020、ADR-0030

## 上下文

Q22 原答要点为：第一目标是提高多线程并行度、减少锁；来源排列不决定受理顺序，按先后受理；NPC 在 T-1 做完决策下单后异步进入委托队列，在 T 统一受理和撮合，以账户独立处理／无锁设计提高并发。同账户资源约束与同股价格时间优先继续有效。该回答要求保留真实接收顺序、决策完成顺序和队列截止的可验证事实，并未要求证明整个系统绝对无锁。

当前 Engine queue 阶段已经给 NPC 决策结果分配接收 ordinal，并可按完成结果到达 consumer 的顺序逐账户投影及入队。但宿主的 auth/generation ingress 接收点尚未共用该 per-session receiver；若宿主先各自缓冲后才调用 Engine 同步入口，Engine 仍无法证明跨 Player/NPC 或跨宿主队列的真实到达先后。Browser 还需避免 WASM worker 忙于上一轮时阻塞真实 ingress。

## 决策

- 每个 `GameSession` 使用一个共享的 ingress receipt 接收机制。请求在其所属宿主 ingress 首次被权威会话实际接收时登记；同一 receive 事实分别产生账户资源域与股票入口域所需的局部 ordinal。该机制不建立覆盖无关账户/股票的全局交易总序，也不要求用全局递增锁串行化。receipt 表示接收事实，不由 `CandidateSource`、请求来源类别、固定生产阶段、key 或 `OrderId` 推导。
- Server、Desktop、Browser 的每会话入口都必须接入该共享 receiver；宿主身份鉴权和 generation 检查仍在各自入口执行，但已接收请求的同一排序事实由 receiver 记录。Player 与 NPC 不因来源获得固定优先级。
- NPC 在 T-1 决策完成后异步登记到同一会话队列；T 的统一 cutoff 决定哪些已登记请求进入当前 tick，cutoff 后登记者进入后续 tick。失败 tick 仍由现有 shadow/transaction 边界原子回滚。
- ordinal 只用于存在因果竞争的局部顺序；同账户现金/股份约束按真实 receive receipt 建立偏序，同股入口 FIFO 依据真实登记顺序并继续遵循现行价格时间优先。无资源依赖的账户和股票应继续并行，不要求以全局锁串行化撮合。
- Browser ingress 必须在 WASM worker 执行前轮时仍可非阻塞接收并登记请求，可采用 main/ingress worker 或共享 WASM-memory receiver；选择具体机制不改变以上语义。

## 备选方案

- 继续按 `NPC → Player` 等来源阶段排列候选 — 否决：来源顺序不等于实际接收事实，并可能改变同账户竞争结果。
- 将固定类别阶段、key 或 `OrderId` 作为受理顺序 — 否决：它们受实现布局影响，不是宿主接收事实；worker 完成只可用于定义 NPC 决策结果完成并登记的时间，不能代替 Player 已更早登记的请求。
- 每个宿主分别维护 ordinal，稍后由 Engine 合并 — 否决：无法定义跨宿主同会话请求的共同先后。
- 为全局顺序引入覆盖所有账户/股票的串行锁 — 否决：超出局部冲突所需并损害并行目标。

## 后果

- **正面：** 跨来源竞争有可审计、可测试的顺序事实；宿主传输差异不改变 Engine 的受理语义；无关账户和股票仍可并行。
- **负面：** 三宿主必须共同维护每会话 receiver 生命周期、ordinal 与 cutoff；Browser 需要在忙碌 worker 之外接收并登记消息。
- **后续需要做的：** 将 Server、Desktop、Browser 的真实 ingress 接收点接入共享 receiver；验证 cutoff 前后归属、同股 Player/NPC FIFO、同账户 Player/PlanChain 资源胜者及生产 `step` 顺序；提供正式 Engine producer 生成的 representative save，并完成适用的 WASM runtime 验证。上述接线未完成前，不得宣称 Q22 整体核销。

## 关联

- 用户回答：[`implementation-audit-2026-10-02.md`](../../agents/implementation-audit/implementation-audit-2026-10-02.md) Q22
- 实施状态：[`q21-q22-current-audit.md`](../../agents/remaining-questions-and-features/q21-q22-current-audit.md)
- 开放问题审计：[`open-questions.md`](../open-questions.md)
