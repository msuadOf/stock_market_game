//! 只消费本 driver 已拥有的 World；正常关停与失败销毁保持分开。
use super::RuntimeDriver;
use crate::runtime::world::{ShutdownCompletion, ShutdownFailure, ShutdownIssue};

impl RuntimeDriver {
    pub(crate) fn shutdown(self) -> Result<ShutdownCompletion, ShutdownFailure> {
        self.world.shutdown_with_completion()
    }

    pub(crate) fn shutdown_after_failure(self) -> (ShutdownCompletion, Vec<ShutdownIssue>) {
        self.world.stop_failed_with_completion()
    }
}
