//! wasm 永久停止的独立发送端所有权；native 仅在测试中编译本模块。
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

pub(super) struct WakeSenders<G, D> {
    groups: Vec<Sender<G>>,
    desks: Vec<Sender<D>>,
    group_shutdown: fn() -> G,
    desk_shutdown: fn() -> D,
}

impl<G, D> WakeSenders<G, D> {
    pub(super) fn new(
        groups: Vec<Sender<G>>,
        desks: Vec<Sender<D>>,
        group_shutdown: fn() -> G,
        desk_shutdown: fn() -> D,
    ) -> Self {
        Self {
            groups,
            desks,
            group_shutdown,
            desk_shutdown,
        }
    }
}

pub(super) fn permanent_stop<G, D>(shutdown: &AtomicBool, wake: &mut Option<WakeSenders<G, D>>) {
    shutdown.store(true, Ordering::SeqCst);
    if let Some(wake) = wake.take() {
        // 永久停止时接收者可能已退出；某一send失败不能短路其余worker的唤醒。
        for sender in wake.groups {
            let _ = sender.send((wake.group_shutdown)());
        }
        for sender in wake.desks {
            let _ = sender.send((wake.desk_shutdown)());
        }
    }
}

pub(super) type ConstructionGuardTransfer<G, D, I, H> = (Option<WakeSenders<G, D>>, Vec<(I, H)>);

pub(super) struct ConstructionGuard<G, D, I: std::fmt::Display, H> {
    shutdown: Arc<AtomicBool>,
    wake: Option<WakeSenders<G, D>>,
    handles: Vec<(I, H)>,
    join: fn(H) -> Result<(), String>,
}

impl<G, D, I: std::fmt::Display, H> ConstructionGuard<G, D, I, H> {
    pub(super) fn new(
        shutdown: Arc<AtomicBool>,
        wake: WakeSenders<G, D>,
        capacity: usize,
        join: fn(H) -> Result<(), String>,
    ) -> Self {
        Self {
            shutdown,
            wake: Some(wake),
            handles: Vec::with_capacity(capacity),
            join,
        }
    }

    pub(super) fn push(&mut self, who: I, handle: H) {
        self.handles.push((who, handle));
    }

    #[cfg(all(target_family = "wasm", feature = "verification-harness"))]
    pub(super) fn handles_for_verification(&self) -> &[(I, H)] {
        &self.handles
    }

    pub(super) fn stop_and_join(&mut self) -> Vec<String> {
        permanent_stop(&self.shutdown, &mut self.wake);
        let mut errors = Vec::new();
        // 全员Shutdown已入队后才开始join，避免组和交易部互等。
        for (who, handle) in std::mem::take(&mut self.handles) {
            if let Err(error) = (self.join)(handle) {
                errors.push(format!("worker {who}：{error}"));
            }
        }
        errors
    }

    pub(super) fn spawn_failure(&mut self, primary: String) -> String {
        let secondary = self.stop_and_join();
        if secondary.is_empty() {
            primary
        } else {
            format!("{primary}；构造清理附加错误={secondary:?}")
        }
    }

    pub(super) fn transfer(mut self) -> ConstructionGuardTransfer<G, D, I, H> {
        (self.wake.take(), std::mem::take(&mut self.handles))
    }
}

impl<G, D, I: std::fmt::Display, H> Drop for ConstructionGuard<G, D, I, H> {
    fn drop(&mut self) {
        if self.wake.is_some() {
            // wasm构造的普通Err和成功出口均已显式移交，panic=abort也不运行Drop。
            // 若未来新增遗漏清理的出口，次生错误必须显式失败，不能依赖wasm无输出的stderr。
            let errors = self.stop_and_join();
            #[cfg(target_family = "wasm")]
            if !errors.is_empty() {
                panic!("构造guard遗漏显式清理，Drop附加错误={errors:?}");
            }
            #[cfg(not(target_family = "wasm"))]
            {
                use std::io::Write;
                for error in errors {
                    // native析构诊断的写失败不能再panic，避免覆盖正在运输的主错误。
                    let _ = writeln!(std::io::stderr(), "构造guard Drop清理错误：{error}");
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "wasm_shutdown/tests.rs"]
mod tests;
