//! 直接编译真实互斥容器源码，域对象使用 engine 的正式公开类型。

pub use engine::*;

#[path = "../src/session/company_state.rs"]
mod company_state;
