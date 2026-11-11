//! 等待点账：运行时等待的业务契约定义阶段与期限，等待算法由 LibOS 实现。
//!
//! 本模块不选择等待算法，也不依赖平台；具体等待由 LibOS 执行。
use std::time::Duration;

/// 上层业务操作所处阶段，用于期限分档和错误上下文。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Worker 空闲等待。
    WorkerIdle,
    /// 协调者等待一轮 GroupReport 与 DeskReport 收束。
    TickConverge,
    /// 等待 SealAck 收尾确认。
    Seal,
    /// 等待日终 Drain 收束。
    Drain,
    /// 等待关停 Final 收集。
    FinalCollection,
}

/// 业务阶段对应的等待期限；等待机制由 LibOS 提供。
#[derive(Clone, Copy, Debug)]
pub struct DeadlineConfig {
    /// 一般业务阶段期限。
    pub normal: Duration,
    /// Drain 与 FinalCollection 阶段期限。
    pub long: Duration,
}

impl DeadlineConfig {
    /// 缺省一般阶段 5s、长阶段 60s。
    pub const fn default_config() -> Self {
        Self {
            normal: Duration::from_secs(5),
            long: Duration::from_secs(60),
        }
    }

    /// 返回业务阶段对应的等待预算。
    pub fn limit_for(&self, phase: Phase) -> Duration {
        match phase {
            Phase::Drain | Phase::FinalCollection => self.long,
            Phase::WorkerIdle | Phase::TickConverge | Phase::Seal => self.normal,
        }
    }
}

impl Default for DeadlineConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

#[cfg(test)]
mod tests {
    use super::{DeadlineConfig, Phase};
    use std::time::Duration;

    #[test]
    fn 业务阶段期限分档保留原有正常与长阶段边界() {
        let config = DeadlineConfig {
            normal: Duration::from_millis(4),
            long: Duration::from_millis(6),
        };

        for phase in [Phase::WorkerIdle, Phase::TickConverge, Phase::Seal] {
            assert_eq!(config.limit_for(phase), Duration::from_millis(4));
        }
        for phase in [Phase::Drain, Phase::FinalCollection] {
            assert_eq!(config.limit_for(phase), Duration::from_millis(6));
        }
    }

    #[test]
    fn 默认期限仍为五秒正常阶段与六十秒长阶段() {
        let config = DeadlineConfig::default();
        assert_eq!(config.limit_for(Phase::TickConverge), Duration::from_secs(5));
        assert_eq!(config.limit_for(Phase::Drain), Duration::from_secs(60));
    }
}
