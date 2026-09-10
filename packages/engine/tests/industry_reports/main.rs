//! 任务 13（company-information-npc-intentions）：行业财务报表与版本化结账集成测试。
//!
//! K3 报表语义：四类行业账套 + 合并 Scope 均产出五产物（资产负债表/利润表/
//! 现金流量表/所有者权益变动表/附注）。报表是**分录的纯函数**（窗口化推导）：
//! 同一日记账 ⇒ 逐字节相同的 ReportSet；比较项缺历史以类型化 Unavailable(reason)
//! 呈现，绝不填零。结账（closing.rs）产出不可变期间版本；更正以当期开放期间
//! 调整分录 + 新版本处理，原版本永不改写。
//!
//! 金样单位约定：注释写「元」便于人读，执行值一律「分」，全部手工可复核。
//!
//! QA 入口：`cargo test -p engine --test industry_reports`（happy 与 failure
//! 同命令覆盖）。

mod consolidated_gold;
mod failures;
mod fixture;
mod industrial_gold;
mod industry_spot_gold;
mod lifecycle_gold;
