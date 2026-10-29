//! RuntimeDriver 将 World 权威状态同步到 Session 镜像的时机策略。

/// RuntimeDriver 将 World 权威状态同步到 Session 镜像的时机策略。
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS,
)]
#[serde(rename_all = "snake_case")]
#[ts(rename_all = "snake_case")]
#[ts(export)]
pub enum RuntimeSyncPolicy {
    /// 每个已提交 tick 后同步会话镜像。
    #[default]
    EveryTick,
    /// 仅在日终同步；此策略不接受日内玩家输入。
    DayEnd,
    /// 在显式同步或计算消费点同步。
    OnDemand,
}

impl RuntimeSyncPolicy {
    /// 是否在每个已提交 tick 后同步会话镜像。
    pub const fn needs_tick_flush(self) -> bool {
        matches!(self, Self::EveryTick)
    }

    /// 是否允许日内玩家输入进入运行时。
    pub const fn allows_player_ingress(self) -> bool {
        !matches!(self, Self::DayEnd)
    }

    /// 是否在读取计算所需会话数据前执行同步。
    pub const fn needs_compute_flush(self) -> bool {
        matches!(self, Self::OnDemand)
    }
}

/// 同步调用来源，仅供运行时调度与日志标记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FlushTrigger {
    EveryTick,
    DayEnd,
    OnDemand,
    ComputeRead,
}

#[cfg(test)]
mod tests {
    use super::RuntimeSyncPolicy;

    #[test]
    fn default_policy_flushes_every_tick_and_allows_player_ingress() {
        let policy = RuntimeSyncPolicy::default();

        assert_eq!(policy, RuntimeSyncPolicy::EveryTick);
        assert!(policy.needs_tick_flush());
        assert!(policy.allows_player_ingress());
        assert!(!policy.needs_compute_flush());
    }

    #[test]
    fn day_end_policy_defers_tick_flush_and_rejects_player_ingress() {
        let policy = RuntimeSyncPolicy::DayEnd;

        assert!(!policy.needs_tick_flush());
        assert!(!policy.allows_player_ingress());
        assert!(!policy.needs_compute_flush());
    }

    #[test]
    fn on_demand_policy_flushes_before_compute_consumption() {
        let policy = RuntimeSyncPolicy::OnDemand;

        assert!(!policy.needs_tick_flush());
        assert!(policy.allows_player_ingress());
        assert!(policy.needs_compute_flush());
    }

    #[test]
    fn policy_uses_snake_case_serde_names() {
        assert_eq!(
            serde_json::to_string(&RuntimeSyncPolicy::EveryTick).unwrap(),
            "\"every_tick\""
        );
        assert_eq!(
            serde_json::to_string(&RuntimeSyncPolicy::DayEnd).unwrap(),
            "\"day_end\""
        );
        assert_eq!(
            serde_json::to_string(&RuntimeSyncPolicy::OnDemand).unwrap(),
            "\"on_demand\""
        );
        assert_eq!(
            serde_json::from_str::<RuntimeSyncPolicy>("\"on_demand\"").unwrap(),
            RuntimeSyncPolicy::OnDemand
        );
    }
}
