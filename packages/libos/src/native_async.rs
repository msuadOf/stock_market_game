//! Native 执行器的邮箱和时钟接线；不决定业务节拍、到期策略或每轮工作量。

use crate::clock::Instant;
use tokio::sync::mpsc::UnboundedReceiver;
// 直接重导出不增加一次封装、分派或跨线程拷贝。
pub use tokio::sync::{broadcast, mpsc, oneshot, Mutex};

/// 独立提交通用执行任务；由调用方管理 JoinHandle，不附加业务调度策略。
pub use tokio::spawn;
// 通用 Future deadline；超时结果由调用方解释，不包含业务策略。
pub use tokio::task::{spawn_blocking, JoinHandle};
pub use tokio::time::timeout;

/// 提交包含同步 I/O 的长寿命任务，不占用普通 async executor worker。
/// 调用方必须通过关闭信号使 Future 结束，不能依赖 abort 取消已开始的阻塞任务。
pub fn spawn_io_lane<F>(future: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || runtime.block_on(future))
}

/// 调用方已选定的唤醒方式；`Yield` 让执行器处理其他就绪任务后再继续。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitPlan {
    Idle,
    At(Instant),
    Yield,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Wake<C> {
    Command(C),
    Deadline,
}

/// 与 `wait` 共用执行器单调时间线，也支持 Tokio 的冻结时钟。
#[inline]
pub fn now() -> Instant {
    tokio::time::Instant::now().into_std()
}

/// 邮箱与唤醒同时就绪时先交付邮件；邮箱关闭且排空时结束。
pub async fn wait<C>(receiver: &mut UnboundedReceiver<C>, plan: WaitPlan) -> Option<Wake<C>> {
    match plan {
        WaitPlan::Idle => receiver.recv().await.map(Wake::Command),
        WaitPlan::At(t) => tokio::select! {
            biased;
            command = receiver.recv() => command.map(Wake::Command),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(t)) => Some(Wake::Deadline),
        },
        WaitPlan::Yield => tokio::select! {
            biased;
            command = receiver.recv() => command.map(Wake::Command),
            _ = tokio::task::yield_now() => Some(Wake::Deadline),
        },
    }
}

#[cfg(test)]
mod tests;
