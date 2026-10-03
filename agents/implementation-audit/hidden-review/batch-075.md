# Batch 075：全文核验记录

## 基线、约束与来源

- 产品基线为 `43b1aa5`（caller worktree HEAD 精确为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`）；源树 `/data1/baiyifan/workplace/stock_market_game`，caller `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 按 scan-plan 连续全文读取三份来源至 EOF，并核对 SHA-256、行数、章节边界及 aliases；三项均匹配，无截断补读。未运行测试、构建或回归，未执行 Git 写操作。
- 已查阅 caller `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/architecture.md`、`docs/trading-rules.md` 及 ADR-0010、0017、0025、0027。来源审查的是宿主 actor / server adapter 组织及接口可见性，没有提议变更交易规则、证券类别或单位；交易语义仍由 engine 与正式规则文档拥有。

## 来源章节矩阵

| 来源（行数；SHA-256） | EOF 内容范围 | 复核意见 |
|---|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/hosts_tooling-closure-final.md`（40；`4f5d890a36092401e537000b4f93938c83359025844222ce79a36201962dc56b`） | L1–L8 范围/绑定；L9–L29 批次、unit 和区域 closure 核对；L30–L35 三门结论；L36–L40 限制与结论 | 记录明确限定为 closure 和审计材料静态复核；计数、SHA 绑定及 unit resolution 核验是文档层结论，不声称重审全部产品源码或已修复缺陷。host 相关归纳与 hosts-01/02 最终复核一致。 |
| `agents/oop-refactor-audit/exhaustive/reviews/hosts-01.md`（57；`7ebb896bc3265f2a4c61030276ef4ebf4546cf1484e037b8d9cc46ce4d52ee89`） | L1–L40 首轮完整复核；L41–L52 短 delta；L53–L57 metadata delta | 结论将 sender 私有化、保留公开 `SessionCommand` 限定为 API 边界建议；正确区分桌面/server 速度语义、生产路径与测试 helper，并将此前只读 Git 误用如实披露。A02 caller metadata 已更正：实际订阅迁移调用者为 `tests/actor.rs`、`tests/protocol_updates.rs`，`tests/ws.rs` 仅用 `public_baseline`。 |
| `agents/oop-refactor-audit/exhaustive/reviews/hosts-02.md`（40；`b53eb04c7948bd469be9ea6581e2c5091ce85f4c565443aa33f8dce89af75bf2`） | L1–L28 首轮逐文件复核；L30–L34 最终 delta；L36–L40 metadata delta | 候选建议保留部署组合根、publisher、web adapter 与测试职责，并把 routes 子模块化列为组织建议而非 OOP action；复核明确候选未实施。指出的 `run_ws` 注释与实际 Resync/GetFrame/SubmitIntent 处理不一致，保留为源码注释缺陷，没有改写协议行为。 |

## 当前 caller / owner / consumer 核对

- caller `apps/server/src/routes.rs:862-882` 通过 `SessionHandles::set_speed` 处理 API；`routes.rs:1262-1272` 在 `run_ws` 经 `handles.subscribe_events()` 建立 Receiver，初次连接先订阅再取 baseline；`:1416` 的 Resync 同样先重新订阅，再取 baseline。source 对调用顺序、覆盖迁移者和竞态边界的描述相符。
- owner 为 server `SessionActor`：`apps/server/src/actor.rs:1005-1012` 持有命令接收端和广播发布端；`SessionHandles` 将 `cmd_tx`、`event_tx` 保持私有，并通过受控命令方法及 `subscribe_events()` 暴露能力。actor 内部持有 event sender，外部订阅者拿到 Receiver，不获得发布权。desktop actor 的 `SessionHandles::cmd_tx` 同样私有；它不提供 server 式事件广播接口。
- 当前 server caller 包括 routes/API/WS 和测试消费端；source metadata 已精确区分 `tests/actor.rs`、`tests/protocol_updates.rs` 的广播订阅与 `tests/ws.rs` 的 baseline 使用。保留 `SessionCommand` 可见性与私有 sender 并不冲突：下游能命名 enum 不等于获得向 actor 投递命令的 sender。
- hosts-02 指出的 `run_ws` 注释仍称“仅存活/pong 探测”，但实际消费 `Resync`、`GetFrame`、`SubmitIntent`；这是真实文档注释不符，来源已登记为 defect lead。它不证明 WebSocket 实际协议错误，也不应以抽取模块为由改变消息行为。

## G/Q、ADR 与候选

- **G：** 对照总账 `implementation-audit-2026-10-02.md` 与宿主复核 `reaudit-host.md`，G01–G05、G18–G20、G40、G53、G66 等均没有被本批来源核销。候选封装和 sender 可见性建议不解决 Remote 鉴权/拉帧/恢复、baseline 生命周期或命令确认等既有生产缺口；本批也没有足够 caller 证据新增或升级 G 项。hosts-02 的 `run_ws` 误导注释作为已有独立 defect lead 保留，不另造重复 G。
- **Q：** 来源不触及总账中的产品决策开放项。Q01–Q23 状态维持原样；特别是不能把 ADR-0010 的统一协议语义解释为三宿主传输实现完全相同，也不能借本批审查关闭尚待验证的宿主验收问题。
- **ADR：** ADR-0010 的 baseline/delta、序号与重同步能力边界，ADR-0017 的 actor/tick 数据流，ADR-0025 的日终存档与 actor-mediated restore，ADR-0027 的可选 `web-ui` 服务面均与审查建议相容。此批没有交易所规则主张，无需新增官方规则依据；未以 server adapter 或测试 fixture 替代 engine 的 A 股规则权威。
- **新候选：** 无。hosts-01 提案是 sender 可见性边界及受控订阅接口；hosts-02 routes 抽取是组织建议。两者都不能被说成已实施或修复既有宿主 G 项。

## 结论与限制

- 三份源材料的 EOF、SHA-256 与行数均符合 scan-plan；复核范围、调用方/所有者关系和历史 delta 结论基本一致。发现项是 hosts-02 已明示的 `run_ws` 注释缺陷，不是新缺口或交易语义漂移。
- 本批仅静态核对指定三份审计材料及当前 caller / ADR / 总账索引；没有重新扫描全部宿主源码，没有运行测试或构建，不声称行为通过或候选已落地。没有修改产品源码、G/Q 台账或正式文档。
