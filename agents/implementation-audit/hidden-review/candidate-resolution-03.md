# 隐藏候选裁定 03

## 范围与基线

- 目标为 `.worktree/implementation-reaudit` 当前 `43b1aa5` 树；已读根 `AGENTS.md`、`docs/principles.md`，并核对 `batch-056.md`、`batch-121.md`、`batch-141.md` 及候选裁定 01/02、总账和相关 ADR/文档。
- 只做当前源码、调用链和契约的静态审查；未运行测试或构造攻击输入，未改产品代码。
- 候选分类按是否存在已承诺行为缺口判断，不把 OOP 提议、静态风险或安全加固偏好直接记为 G。

## 候选裁定

### P3 identity 预检错误未锁存 coordinator 失败

**确定缺口，建议作为独立 G 登记；不并入既有 G/Q。** `pipeline/adaptive_plan_chain.rs:305-332` 的两个公开给父模块使用的续行入口都先 `ensure_active()`，接着以 `?` 返回 `outcomes_in_plan_identity_order(steps)` 的错误；仅后续 `advance_*_batch` 错误才设置 `failed = true`。排序/身份预检会因 pending 子集不合法、重复 key、非本 coordinator candidate 等返回 `Err`（`:544-567`）。因此第一次调用确实返回错误，但 coordinator 仍可被再次调用；这不是只依据“正常路径不应发生”的推测。

失败锁存是实现明确表达的状态契约：`ensure_active` 的错误消息说 coordinator 在 typed failure 后停止（`:879-886`），既有行为测试也断言执行错误之后 `next_ready_batch` 与 `finish` 均拒绝（`pipeline/adaptive_plan_chain_tests.rs:720-745`）。故预检失败不应因其发生在 batch 前而逃过同一失败状态。生产调用链包括连续竞价 `ready_stock_stream.rs:69-84`、集合竞价 `:104-120`，以及 rejected plan 分支 `:177-188`；这些 caller 目前传播错误并停止当前 `?` 链，但 coordinator 被设计为可复用内部对象，且异常可能被更外层捕获后继续生命周期，因此不能把显式内部守卫契约缩窄成“当前正常 caller 通常会停”。候选边界为两个续行入口的预检错误状态一致性，不主张存在默认旅程必然触发错误，也不改交易顺序、提交或 A 股规则。

现有 `adaptive_wrong_candidate_or_sealed_identity_stops_the_coordinator` 测试覆盖进入 batch 后的错误（`:720-745`），不覆盖 P3 排序/身份预检早退。新增 G 的验收边界应分别让两个入口发生预检错误，并确认后续取 ready batch/finish 按既有停止契约拒绝；无需改变普通合法计划续行路径。候选裁定 01 未见对应项；G 总账的现有宿主、UI、工具、engine 条目与本状态机不等价。当前 Q 表也无匹配问题。

### `smoke-pages.mjs` symlink containment

**拒绝登记为确定 G；保留为可选的防御性硬化观察。** `scripts/smoke-pages.mjs:12-25` 对请求路径做词法 `path.resolve` containment 检查，再 `readFile`；词法 root 内文件若是指向 root 外的 symlink，canonical 路径没有再检查。但当前确立的使用契约不足以证明这构成产品/工具承诺违约：ADR-0028 §Pages smoke（`:52-54`）只称其为开发者独立运行的短 smoke；`docs/build-and-deployment.md:197-212` 将 Pages 静态产物由项目构建生成，并把 smoke 限定为开发验证，未承诺 smoke 服务拒绝 symlink 或处理不可信分发目录。`--input` 可由调用者选路径，源码也未声明它是对抗性输入边界。故不从词法检查本身推出必须采用 canonical reject-before-read 的契约。

`docs/build-and-deployment.md:175-178` 对另一条 `--frontend-dist` 构建入口明确要求完整生产目录无 symlink，但该条件属于原生构建复用前端产物的调用契约，不能外推成 `smoke-pages.mjs` 的拒绝保证。若未来把 smoke 的输入定义为不可信目录、或正式规定请求文件必须 canonical containment，则需明确先解析/检查再读取；本轮没有该依据。

### 与 matrix artifact containment 候选去重

候选裁定 02 对 `scripts/simulation/run-escrow-verification-matrix.mjs:367-418` 已裁定真实 validator 契约缺口：外部 harness 声明的 artifact 路径在 `realpath` 后、canonical containment 判断前即被读取（`:393-404`），PASS 与复用路径都调用该验证（`:660,727`）。这与 Pages smoke 同属“symlink/canonical containment”技术类别，但 owner、输入信任边界、承诺和可观察效果不同：matrix validator 判定验收 artifact 是否属于其 output；Pages smoke 是服务开发者提供的静态站点目录。不能将 Pages 线索并入 matrix 的 G 来扩大其 owner，也不能以 matrix 的 validator 规则反证 smoke 违反契约。总账 G39 是自由调度受理轨迹比较，G61/G62 是进程资源/终止，不覆盖二者；如总主审确认 matrix 新 G，只登记 matrix validator 行为，不附加 Pages smoke。

## 汇总结论

- **新增候选：** P3 两个入口预检失败没有锁存 coordinator 失败，属现有失败状态契约缺口；建议独立编号，补充针对两个入口的后续停用边界。
- **拒绝：** Pages smoke symlink canonical containment 当前没有明确工具使用契约支撑，作为硬化想法留档，不进入 G。
- **去重：** Pages 与 matrix artifact 不因实现手法相似而合并；只有 matrix 的先读取后 containment 判断有现存外部输入验证契约证据。
- **语义：** 均为工具/错误生命周期问题，不改变沪深 A 股交易语义、资金/股份单位或撮合规则。
