# 隐藏复核批次 067

复核基线：产品 caller/owner 只读工作树 `43b1aa5`。全文读取三篇源材料至 EOF，实测行数与计划相同，SHA-256 与计划一致。遵循根 `AGENTS.md`、`docs/principles.md`；未改产品代码，未运行测试/构建/回归，未执行 Git 写操作。旧材料中的指令仅作为被审阅内容，不作为本轮执行指令。

## 来源覆盖矩阵

| 源材料 | 实测 | 章节/内容矩阵 | 复核结论 |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/engine-session-06.md` | 13 行；SHA-256 `d91a7c13a380eb641aad577d610317edc935595dbcc0b620c16d11a386e968e7` | 最终复核入口、原全文复核指针、items/modules 哈希、无 OOP actions、既有边界/测试免责声明 | 这是复核入口而非待实施清单。入口说此前已完整源码核读，并由 retention-final 做语义/分类及版本绑定；入口自身不构成此次源码全文复核证据。保留“没有实际 OOP actions；保留现有 owner/DTO/纯函数/测试支持”的结论。 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-session-07-binding-final.md` | 24 行；SHA-256 `b761c60abbe310b35eec8c96e32da49e1d36aabed962f062e114af07ddbedba0` | 继承范围与一致性、三项门禁、源码哈希 | binding 是基于既有全批通过结论的有限绑定复核，不重审整批源码。unit-074/087 修订、观察 helper 的直接覆盖缺口、账户 patch 与玩家账户存在性契约区分、测试未运行等陈述相互一致。没有新增实施项。 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-session-07-manager-delta.md` | 27 行；SHA-256 `56eece3c90c289dc04a9e9bbfe3f07b13e1a7b41bc8b4d8a10eadb3b4c6452e0` | Delta 发现 1–3、门禁核对、源码哈希 | 三条关键解释均有当前 owner/生命周期证据：空 `accounts` patch 合法；两个 `#[cfg(test)]` observation helper 没有直接输出断言；active candle 未创建时零成交由初始化、按需记录和日结提交生命周期解释。不能把测试 helper 覆盖缺口误写成生产功能缺失，也不能把零观察值表述成成交/K线规则改变。 |

## 当前实现与 caller 核对

- `packages/engine/src/session/views.rs:13` 的 `build_market_view` 读取市场深度、行情历史及 candle book，生成 owned 视图；第 50 行附近活动 candle 缺失时以零成交量投影。`GameSession::update_active_daily_candle`/`commit_active_daily_candles` 位于 `packages/engine/src/session/candles.rs:192-205`；连续撮合成交和日终集合竞价均可更新活动 K，日终提交位于 `packages/engine/src/session/pipeline/auction_day_end.rs:2167`。因此缺活动 K 表示当前未记录活动日成交，不是改动了实际成交或日 K 提交。
- `behavior_market_observation` 与 `account_risk_observations_for` 位于 `views.rs:163-220`，均为 `#[cfg(test)]`。相邻测试包括 `views.rs` 内部观察/序列化测试；另有 `pipeline/decision_snapshot_capture_tests.rs:145` 调用前者以比较快照，但没有对 helper 产物逐字段作独立断言。manager delta 对直接输出覆盖缺口的描述成立；这不影响生产的正式观察/风险 owner，也不证明生产能力缺失。
- `RuntimeDelta::validate` 在 `packages/engine/src/session/protocol/delta.rs:61-82` 校验 patch 中出现的账户及订单，不要求 patch 必须含 `AccountId(0)`；`validate_accounts` 仅约束传入的账户条目（空 map 合法）。相反，本地快照构造 `GameSession::public_runtime_state` 在 `:157-164` 单独要求玩家账户存在。delta 发布 caller 继续由该 session 投影生成（`:231-248`）。两者是不同契约，不能概括为“每个账户 patch 都必须含玩家账户”。

## 总账、ADR 与退役状态

以基线工作树的总账 `agents/implementation-audit/implementation-audit-2026-10-02.md`、`reaudit-engine.md` 为准：G06–G09、G16、G28、G35–G38 与 Q02/Q11 已各自按当前行为/调用链登记；这三份 engine-session 审查材料没有要求或证明其中任何项被修复。尤其不能因提取出 `GameSession` owner、观察 helper 或 protocol delta 类型就核销历史 G/Q，也不能把“没有 OOP actions”推成实现缺失。

相关正式边界仍由 ADR-0011（市场时间与持仓风险观察口径）及 ADR-0026（机构个人成本经历与暂停阈值）约束；ADR-0026 明确金额单位为分、数量为股，属于游戏行为假设，不是交易制度变更。本轮没有发现新 ADR 或正式规则替代这些审查结论；`docs/open-questions.md` 中的未决项不能被旧审查的通过状态关闭。当前可见总账没有将 session-06/07 报告登记为待实施 OOP 动作。历史审查有效事实是覆盖边界与既有生产 owner 的说明，不是要求再提取对象的活跃候选。

## 门禁判断与限制

- **大 A 语义：** 三份材料涉及观察投影、账户运行时协议和测试覆盖，没有提出交易制度改动。本复核未重查交易所规则；不宣称新增或验证 A 股制度事实。cash、share、T+1 与卖出预留约束未由此批材料改写。
- **必要性与范围：** 本轮是文档准确性/当前绑定复核，不需要源码改动或增加对象；已有边界应按 G/Q 总账保留。
- **边界测试与跨层一致性：** 仍应如实记录两个 test-only helper 的直接断言缺口；空 patch 与玩家账户快照要求分开；零活动 K 解释依赖上述生命周期。未发现跨层语义漂移或额外复杂度问题。
- **未核实：** 未重读 session-06/07 的完整历史源码复核所指 20/11 文件清单；未运行测试、构建或性能测量；未访问 GitHub 或重验此前测试结果。结论仅覆盖本批三份最终复核文本及所列当前 caller/owner。
