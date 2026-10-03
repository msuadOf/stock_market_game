# Luna49：三份 ignored 草稿全文与现行实现独立复核

## 范围和方法

本次按要求连续读取主工作区三份 ignored 草稿至 EOF；没有把 worktree 中找不到的 ignored 副本当作源文件。之后以指定基线 `08e4fc75b52a71a3262a8a938c57b44f8b5b4960` 对照审计 worktree 当前 `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`。两者在 ADR-0017/0018、`docs/work-status.md`、engine session pipeline 与 K7 runner 的相关内容无差异；worktree 当前额外未跟踪文件属于审计记录。依据优先次序为最新用户决定、正式 ADR、当前产品代码及其调用者、正式状态记录；draft 的旧假设、审批门禁、Todo 和 review session 不会自动授权或建立产品要求。

| 主工作区源文件 | 完整行数 | SHA-256 |
|---|---:|---|
| `.omo/drafts/resolve-blockers-wayland.md` | 58 | `40406fb24b014bd532de855c3801b7d6f02e702f8c785fc19dd57185dd1f8824` |
| `.omo/drafts/k7-deterministic-multicore-utilization.md` | 167 | `3c3c76946bcd95699fd9dfafb37acbecc3b48456604a64409ca35192bae52045` |
| `.omo/drafts/escrow-parallel-engine.md` | 173 | `04cf8bb40a8e90a12bc96a5fdf5bcfd3b63cb0136a778402fc3dd3b9bacdc4ac` |

只做静态追踪，没有运行测试、构建、K7、浏览器或桌面验证。没有编辑产品文件。以下行号属于主工作区 draft，其他行号属于审计 worktree，引用按符号复核。

## resolve-blockers-wayland（58 行）

| 草稿条款 | 按现行决定与调用者复核 |
|---|---|
| 1–9：`awaiting-approval`、下一步仅写 plan | **历史状态已取代。** 当前正式状态说明把此 draft 视为未填模板，具体专题计划另存于 `superpowers/plans/2026-09-13-resolve-blockers-wayland.md`（`docs/work-status.md:322`）。旧 front matter 不构成当前停工/批准门禁，也不证明专题验证通过。 |
| 11–19：C1–C5 拆分脏树/审查、Wayland、host parity、fresh K7、release | **专题仍可能需要证据，不能由当时诊断直接推成现行产品缺陷。** 现行 session 已是 `Result` step 与 shadow 提交（`packages/engine/src/session/failure.rs:29-51`、`pipeline/candidate_commit.rs:98`）；这不等同于 Wayland 截图或三宿主端到端证明。本次未执行这些验证。 |
| 21–28：Weston 1200×800、PNG、native driver、runner resume、8MiB body gate | **旧工具判断局部过时。** runner 当前有 source identity/checkpoint/resume 与并发 child 预算（`scripts/simulation/baseline-run.mjs:128,300-307,941-1005`）；server 限额见正式 ADR-0019 与 `docs/work-status.md`，旧 8MiB 不能恢复成未授权的限额变更要求。Wayland 图像/IPC 仍是待证事实，不能凭配置断言通过。 |
| 30–37：checkbox 旧状态、空截图、单 seed/不可 resume、依赖安装与 dirty tree | **均是原时点发现，不是当前源码审计结论。** 实际 runner 现支持 resume/并行；旧文件数、工具 PATH 和空截图需留在历史证据上下文，不作为今天的源码状态。 |
| 39–45：不得信 checkbox；Wayland 优先/Xvfb 后备；运行目录隔离；旧 WASM 禁复用 | **保留为专题验收原则。** 本轮未检查屏幕像素、host 行为或 release 产物；不能以有脚本/有 Weston 配置核销实测要求。 |
| 47–58：范围禁止伪证、减 seed、静默扩限、覆盖脏树；审批只建 plan | **范围底线仍有参考价值，审批边界已过时。** 具体执行依据取后续用户授权与专题计划，不能拿该 draft 的 `awaiting-approval` 要求重新索取已经授予的授权。 |

## k7-deterministic-multicore-utilization（167 行）

| 草稿条款 | 按现行决定与调用者复核 |
|---|---|
| 1–18：entity FIFO、每实体写通道、serial coordinator、旧实现规划边界 | **整体架构已取代。** ADR-0017 采用单一并行路径、阶段 shadow/P9 提交（§1）；ADR-0018 §7 明确独立账户/股票不需跨实体总序或跨次全局结果等值。FIFO/local key 可保留为实体内部技术实现，不得恢复为全局交易优先。 |
| 20–36：Stock/Account/Plan/Session ownership 与旧源码行号 | **层次职责仍成立，单 writer/coordinator 具体方案退役。** 当前调用经 `DecisionResourceSnapshot::seal`、`AccountValidatorDriver`、股票 stream 与结算阶段连接；如重查仅以当前符号为准。不能因旧行号变化或不存在 `entity_fifo` 目录判缺失。 |
| 38–43：79%/20%、旧 stdout 哈希、10%/5%/10%阈值 | **历史测量与 draft acceptance 提案。** 没有在本次复测；阈值不属于现行 ADR 的提速承诺。架构结果与可见吞吐需依当前 `docs/work-status.md` 和实际性能 evidence，不能由旧 profile 代替。 |
| 45–53：全局 `LogicalRequestKey`、stage ranks、完整 roster seal、global queue-head、256 chunk | **过时算法。** ADR-0017 §1/§3–5 与 ADR-0018 §7 采用真实资源冲突/计划依赖形成的局部先后；本股 gate 登记局部受理。当前 `local_admission.rs:16-27,92-115` 是账户资源 lane + 股票 gate，不是全局最小 key drain。 |
| 55–67：按 root serial coordinator 提交、卖款/撤单当 tick 可用、失败保留前缀 | **执行与失败语义由 escrow contract 取代。** P1 budget 按 P0 后一次截点；密封释放不回补预算（ADR-0017 §1–2）；tick shadow 失败整轮丢弃。不得复活“部分前缀已提交”或把同 tick 资金返还当可用预算。 |
| 69–71：任意公共调用间保存、旧 schema/bytes 不变、临时队列 restore | **存档策略改变。** 当前 v2 明确拒绝旧 schema，外部持久化按 ADR-0025 自然日日结边界；内部静默点与恢复测试仍按现行协议验证。不能用历史 mid-auction save 约定覆盖新公共存档政策。 |
| 73–84：先 serial oracle、再 FIFO、再 parallel；不允许 channel | **被正式决定修订。** ADR-0017 选择单实现，且允许临时完成通知，不允许常驻 actor 权威或跨实体业务锁。串行 oracle 与旧排除列表不再是缺实现证据。 |
| 86–114：Todo 1–7 的 entity_fifo、旧 parity、性能与 host gate | **整体被获批 escrow 计划/当前代码替代。** 保留测试目标如资金竞争、同股 FIFO、失败原子性，但具体 `entity_fifo` 模块、三实现比较和旧命令均不再硬性要求。 |
| 116–128：七组 TDD 预期 | **风险场景可参考，部分旧断言已失效。** 例如同 tick 撤单、买卖释放、旧保存字节等预期须依 ADR-0017/0018 与当前保存政策重定；不能按旧矩阵全文机械要求。 |
| 130–136：old/serial/parallel 三 build 与每预算 full raw bytes 一致 | **跨 worker 整局等值旧门禁已被 ADR-0017:15–18、ADR-0018:778 取代。** 仍有现行工具契约风险，见“候选反证”G39。 |
| 138–154：resource-policy-v3、30/400 天、17+77=94 和末 seed rerun | **旧 policy/规模/命令被后来限时 K7 决策取代。** 保留 source identity 与结果链校验的原则；当前实际执行状态以 runner 与 `docs/work-status.md` 为准。本轮未启动 K7。 |
| 156–167：F1–F4、三道 exit、另批批准与数值阈值 | **旧交付流程，不是当前批准状态。** 独立审查和 provenance 仍需依项目守则执行，但不恢复串行参考、旧阈值或旧长任务矩阵。 |

## escrow-parallel-engine（173 行）

| 草稿条款 | 按现行决定与调用者复核 |
|---|---|
| 1–16：r24 状态、#9 用户裁定、R† 废除、旧零现金断言限定 | **#9 领域裁定与范围一致。** ADR-0017:133 将每腿实收限定为 `min(累计应计-已收, 本腿成交额)`、卖单现金预留 0，并把旧/新差异限定于旧预留非零；当前 `transition.rs:191-211` 实际生成实收与非负净交付。本工作区未找到 front matter 引用的 `.omo/reviews/escrow-parallel-engine-r24.md`，因此无法独立核验该收据内容；可核验的决定以 ADR 和 plan 状态为准。草稿“实现尚未开始”已与现状冲突：正式 work plan 显示 1–8/11/12 已完成及 9/10 阻塞（`docs/work-status.md:320-321` 及 work plan D 收尾表），不能继续使用该句。 |
| 17–20：r21 retail experience 聚合、ReceiptLocalKey 排序修订 | **实现已承接，但键排序不等于交易优先。** `retail_projection.rs:392-409,690-705` 按委托身份聚合；`receipt_key.rs` 有显式来源/journal rank。ADR-0017:12、113 与 ADR-0018 §7 限定收据身份用于唯一性/局部链，不规定跨委托成交优先。 |
| 21–105：r18–r22 多条 `in_flight/null` review | **明确历史记录，非当前阻塞。** 顶部 r24 round 已完成声明覆盖旧 session；不得由旧 session ID 重新启动审查或推断产品缺项。 |
| 106–114：r15/R† 旧批准、r14 inventory blocker、单一 parallel | **旧审查已被新裁定覆盖。** 用户 #9 使 R† approval/stop-resume 机制废除；`docs/test-cleanup-checklist.md:81-88` 记录 sealed corpus 适配/复验能力经用户确认全清。保留的 helper 不能据此假定有 production caller。单一 parallel 仍与 ADR-0017 一致。 |
| 115–128：C1–C6 拓扑，包括 serial-reference、poison/v2/hosts/TS、Rayon 与 K7 | **实际状态部分承接、部分已过时。** ADR 已存在，step Result、poison、v2 与 host adapter 有代码；但是 serial-reference 子项被单一实现决定否决；host/证据尚有未闭环项，不能因组件表而宣称完成。 |
| 130–140：五个可否决默认 | **逐项套用正式规则。** worker 拒单是否消耗 ID 必须区分 P3/P4（ADR-0017:127）；P3 拒绝/Cancel 不耗，P4 拒绝耗。仅允许撤前 tick 已被 ADR-0017/0018 的当前订单簿撤单边界替代。无 STP、Rayon WASM posture 仍有效。普通结算一致性失败应为类型化 step fatal，不可把“托管构造上不应发生”扩成静默业务拒绝。 |
| 142–149：旧源码锚点、费用链与交易所材料结论 | **只能作历史 grounding。** `step()->Vec<Event>`、schema v1、旧行号已变；现行依据须按符号读当前代码与 rules。此处关于 SSE 通知/规则的在线结论不是本次重新查官方来源，不构成新的规则依据。 |
| 151–157：proceeds next tick、break save、escrow 替代 coordinator、Prometheus 建议 | **用户与 ADR 已采纳的部分优先。** P1 后同 tick 释放不回补，旧 save 拒绝，escrow 取代旧协调器。Prometheus 的 serial-first 与草稿内 C2 serial-reference 均被单一并行实现决定取代。 |
| 159–173：IN/OUT 与 approval gate | **边界需按现行 ADR 修订。** engine 不用常驻 actor/业务锁、无旧档迁移、无 STP/GPU 等仍适用；“完全无 channel”被允许短时完成通知修订。草稿的批准只覆盖创建 plan 已由现有计划状态取代；末尾“尚需批准”不代表当前执行要重新征求批准。 |

## 候选反证与保留问题

1. **G39 的已登记跨 worker 门禁仍存在：** `scripts/simulation/run-escrow-verification-matrix.mjs:583-594` 比较多预算完整 artifact vector；`scripts/simulation/escrow-verification-contracts.mjs:131-160` 比较各 budget/repeat 与 `1/0` 的 artifacts 和 execution coverage。向 ADR-0017:15、ADR-0018:756-778 对照，这会把独立账户/股票的受理差异视为 determinism failure。它是验证工具契约问题，不是要求改生产代码来强制全局同序；保留现有 G39 编号，不另重复登记。
2. **另需主审确认 K7 canonical rerun 是否同属 G39：** `scripts/simulation/baseline-run.mjs:1278-1287` 规定末矩阵 seed 重跑且 `rerun.sha256 === canonical.sha256`。虽然相同 `resource_policy` 和 seed，ADR-0018:778 仍允许同 tick 并发受理随调度变化；需判断该验收是否超出其最新公平性契约、或 fixture 是否足以排除相关输入。这一点 sweep49 对 94 项矩阵的过时规模判断未消解，不能默默以“同预算”认定安全。
3. **正式受理顺序决定仍未全部迁移：** ADR-0018:778 明文说 `npc → player → plan_chain` 与账户/计划遍历隐含优先级尚待拆除，且后文称候选准备/受理前路径仍需核对。当前 `ready_ingress.rs:37-44` 先 `take_ready_npc_batch` 后 `capture_player_candidate_batch` 并组合；`local_admission.rs:19-22,32-47` 将 `PreviousCommit < BetweenTicks < ReadyThisTick`，`build_resource_edges` 按该序加账户资金/股份依赖（:92-115）。这是正式承认的迁移未闭环，不可因旧 K7 草稿 superseded 而核销。属于已知范围/语义候选，主审应确认它是否已有后续新决定覆盖及现行优先级是否有用户认可的例外；本次未运行相应竞争用例。
4. **R24 收据缺失属审计溯源限制，不是业务实现缺陷：** draft front matter 指向 `.omo/reviews/escrow-parallel-engine-r24.md`，主工作区与指定 worktree 当前均不存在该文件。草稿保留的 r21/r14 旧 review 已明确失效；如果验收需证明 review round 的具体细节，应恢复/链接可信 receipt，不能把 plan `approved` 状态或本报告代替该 receipt。
5. **否决的候选：** 缺 `entity_fifo`/serial oracle、恢复旧 v3 单 child policy、恢复旧 30/400 天 full K7、8MiB body gate、R†、旧 preserved corpus 清单，均被后续 ADR、resource policy 或 `docs/test-cleanup-checklist.md:81-88` 明确取代/退役；不作为当前实现遗漏。

未做交易制度规则改动；本报告的交易语义依据为项目接受的 ADR-0017/0018 与现行代码，不代替官方规则复核。没有运行测试、编译、host 或桌面验证，也没有把静态审阅写成运行通过。
