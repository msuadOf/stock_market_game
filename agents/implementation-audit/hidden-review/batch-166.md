# 批次 166：Engine 测试与宿主候选复核

## 范围和方法

- 基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。已读该 worktree 的 `AGENTS.md`、`docs/principles.md`，并按 scan plan 连续读取三份来源至 EOF；行数和 SHA-256 与计划一致，记录见同名 JSON。
- 追踪基线实际 owner/caller/consumer，参照 `implementation-audit-2026-10-02.md` 的 G01–G68、Q01–Q23、`reaudit-host.md`、`reaudit-engine.md`，并核对 ADR-0005/0006/0010/0017/0018/0019/0021/0025/0027 与 `docs/open-questions.md`。ADR-0018 整体仍为 proposed；只将被后续明确接受的局部目标作为当前契约。
- 本批只复核历史 OOP 与测试支撑结论，不改产品或 Git，不运行测试/构建，不重新查询交易所规则。

## 候选结论

- `engine-tests-10.md` 所列 15 个文件均是测试入口、fixture 或协议样例；本批未找到应从测试层迁入生产对象的状态或生命周期。当前 `strategy.rs` 的 `retail_covers_all_stocks_not_just_first` 为多个代码构造相同 10% 上下限（`packages/engine/tests/strategy.rs:2353` 起）；它只证明选股覆盖，不能作为板块涨跌幅差异的证据。`urgency` 的 band/tick/lot 与 threshold 也只是输入/游戏策略参数（`packages/engine/tests/urgency/main.rs:42`、`gold.rs:65`），不据此宣称完整 A 股规则已覆盖。
- `hosts-01.md` 的单 writer 与宿主边界在基线仍成立：桌面 `SessionHandles` 的 `cmd_tx` 是私有字段（`apps/desktop/src-tauri/src/actor.rs:334-336`）；server 的 `cmd_tx`、`event_tx` 均私有，并由 `subscribe_events()` 只暴露 Receiver（`apps/server/src/actor.rs:607-625`）。WS 初始订阅和 Resync 均从 handles 重新订阅（`apps/server/src/routes.rs:1262-1272,1416`）。所以 H01 收窄 raw sender 及 server event publisher 的改动在 `43b1aa5` 已实现，不是待实施候选；`SessionCommand` 仍公开，与保留源码兼容的原结论一致。
- 桌面固定倍率约 16ms 聚合仍未实现。生产 `flush_fixed_events()` 仅清空 pending（`apps/desktop/src-tauri/src/actor.rs:894-896`），而 `publish_protocol_cycle()` 直接发 TickBatch/CivilUpdate（`:907-920`）。相关固定周期和最快压缩 helper 的测试不能证明生产聚合；G19 仍有效，符合 `reaudit-host.md` 的复核。
- `hosts-02.md` 的 `routes.rs` 按 HTTP session、query、WS 拆模块是可选代码组织建议，不是必要对象提取或产品缺口。`ClientFrameBuffer`、`DeploymentOptions`、`WebState` 与 Axum handler 的职责仍分明，未见需新增共享可变对象的依据。当前 `run_ws` 注释仍称客户端消息不处理业务，但实现解析 `Resync`、`GetFrame`、`SubmitIntent`（`apps/server/src/routes.rs:431`、`:1262` 起）；这是仍存在的注释不准确，属于独立文档清理线索，不升级为 G 项，也不扩大本批 OOP 动作。

## 影响与复核

- 本批没有发现 OOP 抽取必需项；可保留的模块拆分建议不得算作 OOP action。H01 已完成部分不应重复登记为缺口；固定倍率聚合 G19 不因测试 helper 或 OOP 文档而核销。
- 三份材料没有实现或证明任一 G/Q。主账 G01–G68（G27 维持既有核销状态）和 Q01–Q23 状态不变；尤其宿主实际断点仍以 caller 路径为准，不能从已有 actor/manager 类型推定 G01–G05、G18–G20、G40、G66 已完成。Q11 仍按 `reaudit-engine.md` 的待定 cause 分发边界记录；本批策略测试和 OOP 说明不改变它。
- 大 A 语义沿用 `docs/trading-rules.md`、ADR-0017/0021 和现行策略设计；没有新制度结论。ADR-0025 的日终持久化、ADR-0027 的运行时部署边界优先于候选文档中旧的泛化描述；ADR-0019 的容量范围优先于旧的任意请求配额设想。
- 候选状态：`engine-tests-10` = 支撑分类维持；`hosts-01` = 原 owner 结论维持、H01 已实现、G19 仍缺；`hosts-02` = 组织建议非 OOP 动作，另记录一处注释线索。未发现要新增或核销的 G/Q。

## 来源核对

- `engine-tests-10.md`：220 行，完整读至 EOF；涵盖 step skeleton、strategy/state、event mapping、technical memory、tick frame/protocol、urgency 与 verification evidence。旧结论再证：测试工具/fixture 不拥有生产生命周期。候选反证：strategy 多代码共用 10% 上下限不足以证明板块规则；无测试迁移候选。
- `hosts-01.md`：67 行，完整读至 EOF；涵盖桌面/server actor、命令入口、失败映射、管理器和对应测试。旧结论再证：各宿主 actor 独占协议会话且边界不同。候选反证：H01 修订建议在当前基线已通过私有 sender 与受控订阅实现；16ms 固定倍率发布仍缺。
- `hosts-02.md`：63 行，完整读至 EOF，SHA-256 为 `47d07ba22feb70062074cd6dbb35a5890f4bf2272ae9b8e1a929abbdc6097b27`（64 位）；涵盖 server 部署、路由、publisher、WebUI 与契约测试。旧结论再证：不需要 `Routes`/`ApiService` 大对象，server handler/纯函数边界合适。候选反证：唯一拆分只是可选模块整理；`run_ws` 注释和消费消息行为不符仍存在。
