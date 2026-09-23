//! 类型化拒绝用例（按场景分组）：每条拒绝后断言**完整状态不变**
//! （`assert_eq!(re, before)`——账套 + 子账字节级不变，事件 id 不消耗）。

mod capitalization;
mod guards;
mod lifecycle;
mod loans;
