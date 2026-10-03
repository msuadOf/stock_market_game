# 批次 195 隐藏复核

## 范围与来源校验

复核基线为产品 worktree `.worktree/implementation-reaudit`，HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。按 scan-plan batch 195 从头连续读取三份指定材料至 EOF，行数和 SHA-256 均与 plan 相符。已阅读 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、实现审计 G/Q 主账、session-06/07 模块记录及相关决策。仅静态核对；未运行测试/构建、未执行 Git 写操作、未修改产品文件。

| 来源 | 行数 | SHA-256 | 本次核对主题 |
|---|---:|---|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-session-06.md` | 13 | `ccd85de479b3cf0a518efdf1bc8b99d1775bc9a1e164c0e17010b5ac999b366b` | session-06 既有最终调查结论入口 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-session-07-binding-final.md` | 24 | `71bb600e6ef0e59d85d0a1c917c33f0071bb16230a972adb609e569c833a8857` | views/delta 的条目绑定及边界核对 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-session-07-manager-delta.md` | 27 | `909c22d606a63f79456210af9852383dc5f816beed8131e3600027024da7130e` | views/delta 的增量语义复核 |

## 基线 owner、caller 与 consumer

- **session-06：** `PlanChainOperationBatch` 仍拥有计划候选操作/延迟队列、按账户与股票键控的 pending route、候选代次、报告与重试/重新考虑信息；`GameSession` 仍解释 typed plan route/outcome、维护 parent 投影并同步权威计划事实。`ProtocolSession` 在 `protocol/civil/session.rs` 持有协议发布事务的 checkpoint/rollback，`step_frame` 和日终 `CivilUpdate` 仍从该边界进入。与来源记录的保留边界相符，没有发现应新增的 OOP 迁移。
- **session-07 行情观察：** `views.rs` 生成 owned `MarketView` 与价格路径观察；`decision_chain.rs`、`pipeline/decision_snapshot_capture.rs` 和计划候选路径消费这些值。`market_price_path_observations` 使用 session 权威分钟序列、已完成日 K 与活动日 K；不维护第二份权威状态。两个 helper 仍仅在 `cfg(test)` 下存在，当前内联测试未直接断言其输出，来源标记的覆盖缺口仍成立。
- **session-07 RuntimeDelta：** `GameSession::public_runtime_state` 校验快照并单独要求本地基线含 `AccountId(0)`；`RuntimeDelta::validate` 的账户 map 校验只约束已出现的账户，所以空 patch 合法。`ProtocolSession::tick_batch` 调用 `tick_batch_delta`；Web consumer 在 `protocol/reduce.ts` 经 `applyRuntimeDelta` 消费，parse/validate 层另行校验协议。空 patch 与本地 baseline 玩家账户存在性仍是两个契约。
- **绑定指纹不匹配：** manager delta 记录的 `views.rs`/`delta.rs` SHA-256 为 `6114abc8…947e0a`/`b461f02f…16f0d6`、行数为 318/263；指定 43b1aa5 基线现场计算为 `f4df3b74…34a58e0`/`22f00239…b316f`、行数为 333/278。故该记录的源码精确绑定不是本次指定基线。上述关键 owner、调用链和契约已在 43b1aa5 重新核对，两个 test-only helper 仍未获直接输出断言；旧指纹应视为来源/版本绑定缺口，不据此否定这些语义结论，也不能把旧 hash 当作本基线验证证据。

## G/Q 交叉核对

本批是保留现有观察投影、协议 DTO 和事务 owner 的历史 OOP 调查，不构成任何生产修复或缺口核销。按当前主账复核：

| 总账范围 | 与本批关系 | 裁定 |
|---|---|---|
| G01–G05、G18–G20、G40、G53、G66 | 宿主与远程发布/资源边界；session-06 保留 `ProtocolSession` 不证明这些宿主缺口已修复 | 不核销 |
| G06–G09、G20、G42–G43；Q02、Q11 | 策略、个人经历与估值；session-06 的候选续行边界不补齐散户/公司闭环 | 不核销、不重开 |
| G10–G15、G44、G46–G47 | UI 行情、K 线、档位与竞价绘制缺口；`MarketView`/价格路径观察由 engine 消费，不能代替 UI 展示核验 | 不核销 |
| G16 | 全历史 `PlanBook` 复制仍在 `RootReadContext::capture`；保留 session-06 的 owner 不解决复制成本 | 继续开放 |
| G17、G21–G26、G45、G48–G52、G54–G56、G64–G68 | 工程、交互、发布、账户与验收缺口；本批没有相应生产改动 | 不核销 |
| G28–G39、G41、G49–G52、G57–G63；Q01、Q03–Q10、Q12–Q23 | 公司闭环、工具与开放契约；观察/增量协议 owner 不能证明这些独立契约已满足 | 不核销、不重开 |
| Q01–Q23 总体 | 没有相关 ADR 或产品行为变更；Q10 已转 G39，沿用主账分类 | 不改变状态 |

## 决策与领域门禁

- ADR-0011 规定市场时间观察及逐账户风险值语义；本批只核对其观察投影 owner 和消费路径，没有改变窗口单位、持仓风险或交易规则。
- ADR-0017/0018 的 tick 提交与并发边界、ADR-0025 的日终持久化边界与 `ProtocolSession` 保留职责相容。ADR-0018 中未接受的完整时间线/COW 方案不因此成为本批要求。
- 最新 ADR-0028 规定发布、手动构建与静态 Pages 范围，与 session observation/RuntimeDelta owner 无直接关系；不影响本批结论。
- 未发现 A 股交易制度、现金分、股数、T+1 锁定或卖出预留语义变化。没有重新查询交易所/中国结算，本批不提出新的制度规则主张。

## 结论

三份来源的冻结指纹均匹配 scan-plan。session-06 的无迁移/保留结论与 43b1aa5 的现行 owner/caller 相符。session-07 关于 test-only helper 无直接输出断言、空账户 patch 与玩家账户 baseline 分属不同契约等结论在基线仍成立；但 binding review 给出的源码 hash/行数不属于 43b1aa5，必须作为版本绑定缺口记录。未发现本批证据足以核销 G/Q；G16 仍开放。未运行测试、构建或官方规则查询，不能据此报告测试或实现验收通过。
