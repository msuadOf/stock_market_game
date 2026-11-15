//! 向宿主提供进程分配器的选型。LibOS 不注册全局分配器：由使用者显式安装。
//! 直接重导出底层类型，分配与释放不会经过额外的包装或动态分派。

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub use std::alloc::System as ProcessAllocator;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use tikv_jemallocator::Jemalloc as ProcessAllocator;

/// 可用于 `#[global_allocator]` 声明，无需运行时初始化。
pub const PROCESS_ALLOCATOR: ProcessAllocator = ProcessAllocator;
