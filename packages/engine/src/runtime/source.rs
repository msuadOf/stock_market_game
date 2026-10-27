use crate::calendar::CivilDate;

/// 私有镜像读取的真实 World 静点；不复用公共事件 seq 或 Admin attempt id。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorldMirrorSource {
    pub tick: u64,
    pub generation: u64,
    pub day_end_stage: MirrorDayEndStage,
    pub non_tick_revision: u64,
    pub civil_date: CivilDate,
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
