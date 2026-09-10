//! 任务 15（company-information-npc-intentions）：定期报告、临时公告与
//! 不可变公开信息库集成测试。
//!
//! K4 披露语义：定期报告按游戏排期（年报次年 3-20 / Q1 4-20 / 半年 8-15 /
//! Q3 10-20，各公司稳定偏移 0–7 自然日，18:00 公布）；公布是**纯 civil 域
//! 事件**（非交易日 18:00 照常、零撮合零市场事件）；公开更正是新版本关联
//! 旧 ID，历史不可覆写；只有结账引擎登记簿中勾稽通过的版本可公开；开局
//! 已公开集合只纳入公布时点早于开局的报告（SeededPrehistory 标记），开局
//! 后才公布的报告走 live 排期，绝不提前纳入。
//!
//! QA 入口：`cargo test -p engine --test publications`（happy 与 failure
//! 同命令覆盖）。

mod books_fixture;
mod correction_gold;
mod failures;
mod fixture;
mod prehistory_gold;
mod schedule_gold;
mod session_fixture;
mod weekend_publish;
