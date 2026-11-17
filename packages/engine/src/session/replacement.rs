//! 会话替换的共同提交围栏。prepare 不改变旧会话；commit 后只允许报告新代错误，
//! 不允许因清理、存储或传输失败伪装回滚。宿主仍负责各自租约和绑定的原子安装。
use super::SessionError;
use crate::{error::ValidationFault, SharedSessionIngress};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplacementPlan {
    previous: u64,
    next: u64,
}

impl ReplacementPlan {
    pub fn prepare(generation: u64) -> Result<Self, SessionError> {
        let next = generation
            .checked_add(1)
            .ok_or_else(|| SessionError::ResourceLimit(ValidationFault::bare(1292)))?;
        Ok(Self {
            previous: generation,
            next,
        })
    }

    pub const fn previous_generation(self) -> u64 {
        self.previous
    }
    pub const fn generation(self) -> u64 {
        self.next
    }

    pub fn require_current(self, generation: u64) -> Result<(), SessionError> {
        if generation != self.previous {
            return Err(SessionError::InvalidSave(ValidationFault::new(
                1290,
                vec![self.previous, generation],
            )));
        }
        Ok(())
    }

    /// 外部旧 sink 撤销前的单向封口，保留 poison 诊断且不提前推进 generation。
    pub fn fence_retiring_ingress(
        self,
        generation: u64,
        ingress: &SharedSessionIngress,
    ) -> Result<(), SessionError> {
        self.require_current(generation)?;
        ingress.close_for_replacement();
        Ok(())
    }

    /// 必须在宿主已经拿齐部署绑定锁、存储 writer lease 的事务中调用。
    /// 先拒绝过期计划，旧入口关闭成功后才安装新 generation 和 receiver。
    pub fn install_ingress(
        self,
        current: &mut (u64, SharedSessionIngress),
        replacement: SharedSessionIngress,
    ) -> Result<(), SessionError> {
        self.fence_retiring_ingress(current.0, &current.1)?;
        *current = (self.next, replacement);
        Ok(())
    }

    /// 单一 authority 串行执行此所有权转移；返回旧 owner 供适配器异步清理。
    /// 后续失败不得把返回的旧 owner 装回去，也不得减少 generation。
    pub fn commit<T>(
        self,
        current: &mut T,
        generation: &mut u64,
        replacement: T,
    ) -> Result<T, SessionError> {
        self.require_current(*generation)?;
        let retired = std::mem::replace(current, replacement);
        *generation = self.next;
        Ok(retired)
    }
}

/// commit 回执不依赖 baseline 编码是否成功；客户端据此撤销旧代缓存。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CommitReceipt {
    pub previous_generation: String,
    pub generation: String,
}
impl ReplacementPlan {
    pub fn receipt(self) -> CommitReceipt {
        CommitReceipt {
            previous_generation: self.previous.to_string(),
            generation: self.next.to_string(),
        }
    }
}

/// 恢复保留用户的速度和日界暂停设置；运行意图在新绑定准备完成后才放行。
#[derive(Clone, Copy, Debug)]
pub struct RestoreSettings {
    pub speed: super::runner::RequestedSpeed,
    pub running: bool,
    pub pause: super::protocol::PausePreferences,
}

impl RestoreSettings {
    pub fn capture(
        pacing: &super::runner::Pacing,
        pause: super::protocol::PausePreferences,
    ) -> Self {
        Self {
            speed: pacing.requested_speed(),
            running: pacing.is_running(),
            pause,
        }
    }

    pub fn install(
        self,
        authority: &mut super::authority::Authority,
        tick: u64,
        now: libos::clock::Instant,
    ) {
        authority.reset_after_restore(tick, now);
        let multiplier = match self.speed {
            super::runner::RequestedSpeed::Fixed { multiplier } => multiplier,
            super::runner::RequestedSpeed::Fastest => f64::INFINITY,
        };
        authority
            .control_speed(multiplier, tick, now, None)
            .expect("已由共享 pacing 验证的速度必须仍可表示");
        // 壳在全部新绑定就绪后启动唤醒任务；这里仅安装共同的运行意图。
        authority
            .control_running(self.running, tick, now, None)
            .expect("新 authority 尚无故障或归档阻塞");
    }
}

#[cfg(test)]
mod tests;
