//! 自然日经营演化、合同到期与市场/行业事件集成测试
//! （company-information-npc-intentions W2-Task 14）。
//!
//! 政策基线：计划 K4（经营演化/事件目录/RNG 分流/事件队列）+ K2（前史生成、
//! 资金边界、无股东分配）。冲击参数全部是**待校准游戏假设**（版本化配置，
//! 不声称现实频率）；税率沿用 industrial 套件的 Fixture 合成税率。
//!
//! 按场景拆分：`fixtures`（四行业测试公司装配）、`shock_gold`（共同冲击 →
//! 不同经营响应 + 违约业务风险 + 中断/恢复 + 无股东分配）、`determinism`
//! （同 seed 同事件/分录序列 + RNG 状态持久化）、`history_gold`（2 年 + 当年
//! 截至开局前日的前史生成边界）、`seam`（自然日时钟接线，休市日无成交经营）、
//! `failures`（资金断裂/停工交付失败/重复 due/股东动作类型化拒绝）。
//!
//! QA 入口：`cargo test -p engine --test company_operations`（happy 与 failure
//! 同命令覆盖）。

mod determinism;
mod fixtures;
mod history_gold;
mod risk_gold;
mod seam;
mod shock_gold;

mod failures;
