//! 独立 Native 性能诊断：直接引用仓库中的真实 mailbox 和准入候选源码。

extern crate self as libos;
extern crate self as runtime;

#[path = "../../packages/engine/src/libos/mailbox.rs"]
mod mailbox;
#[path = "../../packages/engine/src/libos/fault_domain.rs"]
mod fault_domain;
#[path = "../../packages/engine/src/libos/admission.rs"]
mod admission;
mod clock { pub use std::time::Instant; }

mod worker_inbox {
    use std::sync::mpsc::SendError;
    pub(super) trait WorkerSender<T> {
        fn send(&self, value: T) -> Result<(), SendError<T>>;
    }
    impl<T> WorkerSender<T> for crate::mailbox::Sender<T> {
        fn send(&self, value: T) -> Result<(), SendError<T>> { self.send(value) }
    }
}

#[path = "../../packages/engine/src/runtime/worker_admission.rs"]
mod worker_admission;

fn main() { worker_admission::benchmark_send_path(); }
