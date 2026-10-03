# Hosts/account callers 三章全文 EOF 独立复核

复核基线：产品提交 `08e4fc75b52a71a3262a8a938c57b44f8b5b4960` 是当前 `HEAD` `a7c7ce357bdc9f88c03633744b2d5815db49e9b2` 的祖先；工作树未跟踪文件中已有其他审计记录，本记录只新增本文件。复核不运行测试、不写产品代码、不进行 Git 写操作。

## 全文行数与章节矩阵

| 文件 | EOF 行数 | 全文覆盖章节 | 主要复核对象 |
|---|---:|---|---|
| `agents/oop-refactor-implementation/hosts/account-callers.md` | 44 | 范围与结果；验证及交接；Market 价格状态边界测试后续迁移 | integration caller、账户/持仓 API、fixture 构造、断言与迁移边界 |
| `agents/oop-refactor-implementation/hosts/actors/status.md` | 66 | hosts-01-A01；hosts-01-A02；hosts-N01；hosts-R2-N13；有限检查；短测 filters；Root 最终代表性验证 | actor owner、队列/确认、Setter、订阅 Receiver、真实 caller、状态结论 |
| `agents/oop-refactor-implementation/hosts/coordination.md` | 36 | 基线与范围；分工；协调者改动；复核；2026-10-03 收口及最终验证 | 批次完成主张、正式 ADR 义务与历史证据界限 |

按原文顺序逐段读至 EOF；下列代码片段来自实现源文件，作为对旧结论的独立核实。行数由 `wc -l` 得到。

## account-callers.md

原文主张 caller 迁移限于测试访问封装：`Account::cash/strategy/position` getter 和 `Position::from_restored_parts`；溢出 fixture 改用 `grant_position`，四个 integration test 文件未改断言。Account API 源码吻合：`packages/engine/src/account.rs:124-136` 提供 `cash`、`strategy`、`positions`、`position` 只读访问；`packages/engine/src/account.rs:258` 是 grant 入口，`packages/engine/src/account.rs:622` 是恢复构造入口。文内具体 caller 亦能在 `account.rs`、`strategy_state.rs`、`company_opening/isolation.rs`、`controlled_experience.rs` 看到。

原文关于从不合法内部状态迁出 Market 的三个测试，指出合法公开撮合流程先溢出而不能重建指定 fixture；其迁移收口在 `hosts/coordination.md` 与 `domain/orderbook-review.md` 中有引用。该历史迁移/静态等价检查不等价于测试执行；文中也明确没有运行 Cargo，故不能核销运行验证。

旧结论“未改变费用、T+1 等交易语义、无需另查规则”与本次有限测试 caller 修改范围一致；但这只说明所审文件访问方式/fixture 迁移不触碰交易算法，不能由此证明全批 A 股实现或 ADR 已履约。ADR-0024 明确改变投资者资金池长期不循环的模型，但这里没有资金流逻辑的改写证据；不能用本章核销该决策的全局实现覆盖。

## actors/status.md

### hosts-01-A01/A02：句柄字段与队列确认

桌面实现 `apps/desktop/src-tauri/src/actor.rs:335` 将 `cmd_tx` 留在 `SessionHandles` 私有字段，公开方法再发命令；`set_speed` (`:558-562`) 只 `mpsc::send`，无 reply receiver。文档对 fire-and-forget、队列顺序、ActorGone 边界的描述与代码一致。真实生产 caller 通过公开句柄方法，命令最终由 actor 的 `cmd_rx` (`:800+`) 收取；不能把“已 send 成功”说成“actor 已应用”或业务确认。

Server 实现 `apps/server/src/actor.rs:608-625` 私有化 `cmd_tx`/`event_tx`，新增 `subscribe_events` 返回 `broadcast::Receiver<EngineUpdate>`；WebSocket 首订阅与 Resync 在 `apps/server/src/routes.rs:1271,1416` 使用该方法，测试 caller 在 `apps/server/tests/actor.rs`、`protocol_updates.rs`、`ws.rs` 使用 `subscribe_events`。`set_speed` 在 server 有 oneshot：`:815-825` 建 reply channel 并送 `SetSpeed`，`handle_command` `:1456+` 应用并回执，因此与桌面的行为有意不同。Status 将这些差异列明，未把两个宿主误作一致确认语义。

### hosts-N01：Pacing owner/Setter caller

两宿主 `ServerPacing`/`DesktopPacing` 独占 running、fastest、requested speed、tick interval 与采样器；代码 server `apps/server/src/actor.rs:301-394`、desktop `apps/desktop/src-tauri/src/actor.rs:135-232` 与文档字段清单相符。Setter/更新入口为 `apply_speed`、`set_running`、`pause_at_civil_boundary`、`reset_after_restore`、`refresh_metrics`；调用点包含 manager 初始化、主循环 interval/select、`SetSpeed`/`SetRunning`/metrics、restore、civil boundary 和 fatal stop。失败停止只写 running，未重置采样；civil pause 会重置采样。这正对应文档刻意说明的边界，未发现 caller 遗留直接更新 Pacing 字段于生产路径。

实现保持两个宿主速度边界差异：server 对非 Fastest 用 `debug_assert!` 校验内部速度；desktop 对非法内部输入返回 false，由 actor 报告/忽略；固定 tick 下限 server 1ms、desktop 1µs。调用层的倍速校验/回执差异不应在后续抽象时抹平。没有交易制度或股/分单位逻辑进入 Pacing。

### hosts-R2-N13：测试 ActorHarness 与 receivers

`apps/desktop/src-tauri/src/actor.rs:691+` 的 `ActorHarness` 持有 actor、命令发送端与需要时建立的 event/failure receivers；文档说无生产 caller，场景 helper 为 fatal/protocol 测试，符合 `cfg(test)` 定位。Sender close 与 receiver 生命周期属于测试 fixture 语义，不可将 harness helper 计作生产 API。

### 最终状态与未结事项

文档结尾记载 root 的 build/check 与挑选的精确 case 成绩，同时明确 actors API doctest、完整 suite、matrix/E2E/性能未覆盖。故“无未关闭实施或独立复核发现”只指已列实施动作的审查状态，不能扩写为 ADR 对宿主及客户端契约的完整履约。

可核实的正式 ADR 候选残余：ADR-0010 `docs/decisions/0010-unified-host-protocol-and-local-refresh.md:60` 规定有副作用的命令必须等宿主确认，且 `CommandQueued` 不等于订单接受/成交；桌面 `set_speed` 文档与代码明确为 fire-and-forget、仅确认命令入 mpsc 队列。倍速设置是否属于此处“有副作用的命令”需按 ADR 语境确认，不能仅凭 status 将其判为违规，也不能静态检查后宣称 ADR 确认义务闭合。Server `set_speed` 有 actor reply，WebSocket 路径见 `apps/server/src/routes.rs:880`。

ADR-0010 同文 `:33`、`:90` 明确前端局部刷新尚未完成/不得声称完全消除全量替换；这三章的 Pacing、sender 与 receiver 迁移不覆盖前端订阅/渲染，不能核销该正式义务。ADR-0017 有宿主 fatal 映射要求（末尾 Consequences；`docs/decisions/0017-escrow-parallel-tick.md:148`）；本章列出了 fatal 路径及已选 actor 用例，但未给出每种 fatal 对用户可见错误的完整端到端映射证据，不能代替专门复核。ADR-0017 §1 的交易执行链与验收矩阵不是本轮 host 状态记录可核销的功能范围。

## coordination.md

协调记录列出 34 个 assigned actions、account callers 和后来 production-entry TradingPlan getter caller 补迁；source-review-manifest/status.json 作为结构化索引，文字也记载指定审阅修复。这里是历史批次收口记录，不是按 ADR 的要求逐义务映射的完整证明。历史“仅运行 Node 短测/未运行 Cargo”与后面的 root 最终验证分段并不矛盾；不可合并成所有目标、所有过滤器都已跑过。

文中最强结论为“34 个动作…静态审查三门通过”“最终验证已引用…build/check、选定 lib case、Writer、桌面 CLI”。即使接受这些记录，也只核销其明确选择的动作/短测。文件自身限定未覆盖完整 suite、actors doctest、真实 matrix/E2E/性能。尤其 ADR-0010/0017 中的协议确认、恢复/Resync 屏障、fatal 错误显示及前端刷新语义需要按正式文档单独核对；coordination 的章节范围和静态检查不能替代这些功能证据。

## 新候选及反证摘要

| 候选 | 支持证据 | 反证/限定 | 复核处理 |
|---|---|---|---|
| 桌面 `set_speed` 的排队回执可能不足 ADR-0010 的宿主确认 | 句柄方法只 send，无 oneshot；ADR-0010 要求“有副作用”的命令等待宿主确认 | 该设置是否属于 ADR 所称副作用未由章节解释；桌面旧语义显式 fire-and-forget，且无订单业务副作用 | 保留待 ADR 语义确认，不记有效缺陷，也不记 ADR 已核销 |
| ADR-0010 局部刷新尚未实现 | ADR-0010 原文仍标组件局部刷新未完成/禁止声称全量替换消除 | 属 Web/UI 范围；本次 host 章节未触碰也未声称完成 | 正式残余，不能由 host 工作核销 |
| ADR-0017 宿主 fatal 映射/更大交易验收可能未闭合 | ADR-0017 明确 fatal 显式映射、交易链验收及跨预算证据要求 | 本次章节 scope 是 hosts/Account caller；status 已明示只选短测且无真实矩阵 | 记为范围外正式待核义务，需独立证据，不把历史 scope 当功能核销 |
| Account caller fixture 改动可能改变交易行为 | 溢出 fixture 改用 `grant_position`，多个测试用恢复构造 | API 与断言迁移针对测试 setup/读取；正文称原 assert 序列一致，未改生产交易实现 | 未形成语义缺陷候选；静态等价不替代测试运行 |

## 结论

三章中的 caller、queue、Setter 和 Receiver 描述大体与产品源码吻合，未发现足以推翻其局部实现记录的真实 caller 遗漏。能够核实的差异均是需要保留的宿主协议差别。仍有正式 ADR 义务不能凭这三章或历史复核结论核销，尤其 ADR-0010 的桌面倍速“确认”适用性待解释、前端局部刷新明文未闭合、ADR-0017 的宿主 fatal 端到端交付与其交易语义验收需各自证据。无 A 股交易规则变化发现；本复核不构成该批次或项目整体完成声明。
