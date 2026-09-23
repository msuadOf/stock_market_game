//! 任务 16（company-information-npc-intentions）：公共曝光与 NPC 本人
//! 获取信息分离的集成测试。
//!
//! K4 语义（计划行 121–123）：`NpcInformationState` 逐公司记录**本人已获取**
//! 的报告/公告 id 及获知时点；公共曝光只改变发现机会（`discovery_candidates`
//! 候选面，任务 25 注意力接线消费），未观察 NPC 不能直接读最新全部公告；
//! 一次获知只记一次；dev 查看不写入 NPC 信息状态；被个人状态引用的版本
//! 永不删除，新报告/更正不追溯重写 NPC 当时看过的材料（历史版本按获知
//! 时点钉死）。
//!
//! QA 入口：`cargo test -p engine --test information_acquisition`（happy 与
//! failure 同命令覆盖）。

mod acquisition_gold;
mod failures;
mod fixture;
mod view_gold;
