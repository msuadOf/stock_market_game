use crate::calendar::CivilDate;

/// 私有镜像读取的真实 World 静点；不复用公共事件 seq 或 Admin attempt id。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorldMirrorSource {
    pub authority_epoch: u64,
    pub tick: u64,
    pub day: u32,
    pub generation: u64,
    pub day_end_stage: MirrorDayEndStage,
    pub non_tick_revision: u64,
    pub civil_date: CivilDate,
}

pub(crate) struct PreparedMirrorAck {
    pub(super) domains: MirrorDomains,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MirrorDayEndStage {
    Tick,
    DayEndCommitted,
    RetainedPublished,
}

/// 非 tick 成功写集；getter 与失败 prepare 不改变这些待汇合原因。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MirrorDomains {
    pub accounts_full: bool,
    pub personal_full: bool,
    pub attention_full: bool,
    pub plans_pending: bool,
}

impl MirrorDomains {
    pub(crate) fn is_empty(self) -> bool {
        !self.accounts_full && !self.personal_full && !self.attention_full && !self.plans_pending
    }

    pub(crate) fn union(self, other: Self) -> Self {
        Self {
            accounts_full: self.accounts_full || other.accounts_full,
            personal_full: self.personal_full || other.personal_full,
            attention_full: self.attention_full || other.attention_full,
            plans_pending: self.plans_pending || other.plans_pending,
        }
    }
}
