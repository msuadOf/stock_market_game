# 批次 211：hosts 与关系复核

## 范围与方法

复核基线为 `.worktree/implementation-reaudit` 的 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。已按任务要求在主工作区逐篇连续读取三份来源至 EOF，并核实各自行数和 SHA-256；另读复核工作区 `AGENTS.md`、`docs/principles.md`。来源中旧 reviewer/reader 对后续工作的指令仅是历史记录，不作为本轮执行要求。当前 OOP 代码、调用点、实施状态与既有复核另行对照；未运行测试、构建或 Git 写操作。

## 来源及章节族

| 来源 | 行数 / SHA-256 | 全文结构 |
|---|---:|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/hosts-03.md` | 38 / `f02b1d133849585219bb312305cc071c7331446e7eb526c4d154f4e90838b39f` | 逐文件复核与旧总评（1–18）；首次 delta 范围门禁（20–28）；修订后二次复核（30–38）。 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/hosts-area-final.md` | 31 / `175cced1d6e2efdd05dc6954a0900bcde5c018d01bddb9621361f91d01500c68` | 绑定材料（5–15）；hosts 区域汇总（17–23）；三门结论（25–31）。 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/relationships-before-final-cache-correction.md` | 80 / `2d8ffd1bd7083b038428f23ecb446c9ebcccc934d15c2a9d5b1bb9a887f07f96` | 账户/候选事务（5–20）；单股与整轮提交（22–42）；Web 生命周期（44–60）；计划写入（62–66）；传输与宿主（68–74）；复核范围（76–80）。 |

三个来源均已从首行连续读取至 EOF；具体 `read_to_eof` 绑定见配套 JSON。来源哈希、行数与委托元数据一致。

## 现行代码与后续状态

- **hosts-03 / WASM `SessionRegistry`：候选已实现，NEXT 缺陷仍开放。** 当前 `apps/web-wasm/src/lib.rs:43-55` 建立 thread-local registry；`:57-117` 将注册、构造/恢复、句柄访问/移除和 `step_update` 聚合到 `SessionRegistry`。外部 `create_session`、`step`、`restore`、`restore_json` 与有句柄操作分别在 `:328-350`、`:475-530` 经此 owner。构造成功后才注册；`step_update` 保留 `CivilUpdate` 屏障及 `step_frame → tick_batch` 顺序。实施记录 `agents/oop-refactor-implementation/hosts/server/status.md:9,18-19` 明确标为“已实施待验证”，保留 wasm_bindgen 导出、错误形状及原始 `NEXT` 行为；`review-server.md` 的三门结论确认静态复核通过，但统一 Cargo/测试仍待父任务验证。旧二次复核所要求的缺陷拆分（来源 `hosts-03.md:30-38`）正确：`NEXT.fetch_add` 在 `lib.rs:49,57-61` 仍可能回绕后覆盖存活 handle，现行总账 `implementation-audit-2026-10-02.md:168` 的 Q20 仍要求决定耗尽策略。对象聚合没有修复或核销 Q20。
- **WASM 边界与协议语义：旧结论再证。** `protocol_tests.rs:199-235,237-275` 覆盖独立 handle、构造/恢复失败不登记、未知句柄、移除及恢复后的 CivilUpdate 屏障。此处是源码已有测试，不能表述为本轮运行通过。对象没有把 `step_frame` 与 `tick_batch` 变成原子操作，也不改变 `ProtocolSession` 的交易权威；ADR-0010 对统一应用协议与宿主差异的约定（`docs/decisions/0010-unified-host-protocol-and-local-refresh.md:56-73`）与实施边界相符。
- **hosts-area-final 三项动作：候选转为已实施记录。** 旧区域复核（`hosts-area-final.md:19-23`）将 desktop/server sender 边界和 WASM registry 列为三项 OOP 候选，并将 routes 组织整理排除在 OOP action 外。当前实施账 `agents/oop-refactor-implementation/hosts/server/status.md:9,12` 与 actor 审查记录显示 Registry、server `WsPublisherConnection` 及 `subscribe_events` 已接线；desktop/server pacing 与 raw sender 收口分别在同账其它行及 `review-actors.md` 记录。`review-server.md`、`review-actors.md` 提供独立静态复核，均将运行门禁留待统一执行。因此历史“候选未实施”不能继续当作当前代码状态；另一方面也不能升级成编译、测试或发布验收已通过。该对象边界本身不改变交易协议。
- **G01–G68/Q 对照：无核销，也无新增重复项。** 相关宿主条目仍按当前总账：G01–G05、G18–G20、G40、G53、G66 记载的是认证、帧消费/重连、背压、固定高倍率发布、命令确认、恢复响应校验及请求终态等调用契约（总账 `:36-45`）；sender 或 owner 抽取不能替代这些契约。Q20 是上述 WASM handle wrap-around 缺陷，不应与 SessionRegistry 等价聚合混为一事。G27 已核销但无关；其它 G/Q 未发现本三篇材料能直接改变的状态。没有把 G39 等无关工具门禁牵强映射到 hosts 对象。
- **relationships 跨域章节：关系边界再证，候选实施状态不等于缺陷关闭。** 账户与候选事务（来源 `:5-20`）强调 AccountBook 缓存失效、收据补丁准备/安装和可编辑存档校验职责分开；单股/整轮（`:22-42`）保持单股受理、P9 候选提交、outbox 和并行调度边界；Web 生命周期（`:44-60`）不引入包揽所有状态的 `AppSessionController`；计划（`:62-66`）保持 PlanBook 索引与 TradingPlan 局部写入；宿主（`:68-74`）保持 Server/Desktop actor 会话权威、remote adapter 闭包对象及传输/DTO 边界。工作区的实现与后续状态记录没有推翻这些所有权关系。此 80 行关系页不是现行缺陷清单，不能凭其提取建议给 G/Q 重新定状态。
- **明确后续决策与取代：** ADR-0010 保持宿主协议、baseline、fatal 与能力差异契约；ADR-0017 保持 engine tick/CivilUpdate 事务边界。未找到后续 ADR 将本批对象边界整体取代。后续 OOP 实施与 review 状态取代的是历史“候选未实施”状态，而非上述 ADR、Q20 或总账中尚未完成的生产调用缺口。

## 结论

历史材料对当时 hosts 候选和最小边界的归纳总体仍可信。当前必须按后续代码与记录修正其时态：WASM `SessionRegistry` 及其余 hosts 封装已实施并有独立静态 review；它们尚非父任务统一运行验收完成。Q20/NEXT 回绕仍是独立未修边界，宿主调用链 G 项也未被 OOP 结构提取核销。A 股交易规则仍由 engine/`ProtocolSession` 持有；本轮未发现交易单位、费用、T+1、撮合或市场语义漂移，未重新查验官方规则。没有新的 G/Q 候选。
