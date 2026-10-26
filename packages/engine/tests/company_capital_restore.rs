//! 直接编译真实 validator 源文件；领域输入仍使用 engine 的正式公开类型。

pub use engine::*;

#[path = "../src/session/company_capital_restore.rs"]
mod company_capital_restore;
