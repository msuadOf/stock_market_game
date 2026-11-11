//! 完整 prepared 初值的 INFO 接缝；只读捕获及首个 Start 前安装，不推进业务 source。
use super::*;
use crate::runtime::information_handover::InformationHandover;
use crate::runtime::personal_full_validation::{validate_personal_full, PersonalPresence};
use crate::AccountId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub(crate) struct CapturedInformationHandover {
    pub(crate) source: crate::runtime::source::WorldMirrorSource,
    pub(crate) information: InformationHandover,
    pub(crate) library: Arc<crate::information::PublicLibrary>,
    pub(crate) facts: Arc<crate::company::CompanyReadFacts>,
    /// 真实内部 RNG 流，未经过 EchoAttention 的布局编码。
    pub(crate) profiles: BTreeMap<AccountId, (u64, u64)>,
    calendar_record: Option<CommittedCalendarTick>,
    authenticated_source: crate::runtime::source::WorldMirrorSource,
    pub(crate) retained_index: Arc<crate::runtime::publication::RetainedIndex>,
}

/// 历史事实保持原 source；资格只证明目标 epoch 接管了此静点，不构造新的市场提交。
#[derive(Clone, Debug)]
pub(crate) struct InheritedCalendarCommit {
    record: CommittedCalendarTick,
    authenticated_at: crate::runtime::source::WorldMirrorSource,
    target_epoch: u64,
}

impl InheritedCalendarCommit {
    pub(crate) fn record(&self) -> &CommittedCalendarTick {
        &self.record
    }
    pub(crate) fn accepts(
        &self,
        record: &CommittedCalendarTick,
        current: crate::runtime::source::WorldMirrorSource,
    ) -> bool {
        current.authority_epoch == self.target_epoch
            && current.tick == self.authenticated_at.tick
            && record.source == self.record.source
            && Arc::ptr_eq(&record.calendar, &self.record.calendar)
            && record.calendar.after_input().tick == current.tick
    }

    pub(crate) fn validate_install(
        &self,
        owner: &Arc<crate::session::market_calendar_context::MarketCalendarSource>,
        bindings: &[(crate::StockId, crate::calendar::CalendarExchange)],
        layout: crate::session::exchange_calendar_model::SessionTickLayout,
        target: crate::runtime::source::WorldMirrorSource,
    ) -> Result<(), RuntimeError> {
        if !self.accepts(&self.record, target)
            || self.authenticated_at.day != target.day
            || self.authenticated_at.civil_date != target.civil_date
            || self.authenticated_at.day_end_stage != target.day_end_stage
            || self.record.source.tick != self.record.calendar.after_input().tick
            || self.record.source.civil_date
                != self.record.calendar.day().day_model().current_date()
        {
            return Err(invalid("Calendar 移交原事实、认证静点或目标 epoch 不一致"));
        }
        self.record
            .calendar
            .validate_for(
                owner,
                bindings,
                self.record.source.civil_date,
                layout,
                self.record.calendar.before_input(),
            )
            .map_err(|error| invalid(error.to_string()))
    }
}

fn invalid(detail: impl Into<String>) -> RuntimeError {
    RuntimeError::ProtocolInvariant {
        detail: detail.into(),
    }
}

impl World {
    pub(crate) fn authenticates_committed_calendar(&self, record: &CommittedCalendarTick) -> bool {
        self.last_committed_calendar.as_ref().is_some_and(|actual| {
            record.source == actual.source
                && Arc::ptr_eq(&record.calendar, &actual.calendar)
                && record.source.authority_epoch == self.authority_epoch
        }) || self
            .inherited_calendar
            .as_ref()
            .is_some_and(|inherited| inherited.accepts(record, self.mirror_source()))
    }

    pub(crate) fn has_committed_market_close_for_civil(
        &self,
        tick: u64,
        date: crate::calendar::CivilDate,
    ) -> Result<bool, RuntimeError> {
        let Some(record) = self.committed_calendar_tick()? else {
            return Ok(false);
        };
        let current = self.mirror_source();
        Ok(self.authenticates_committed_calendar(&record)
            && record.source.tick == tick
            && current.tick == tick
            && current.civil_date == date
            && record.calendar.day().day_model().current_date() == date
            && record.calendar.after_input().tick == tick
            && record.calendar.after_input().progress
                == crate::session::exchange_calendar_model::SharedDayProgress::TradingCompleted
            && current.day_end_stage != crate::runtime::source::MirrorDayEndStage::Tick)
    }
    fn information_handover_rosters(
        &self,
    ) -> Result<BTreeMap<GroupIdx, Vec<(AccountId, bool)>>, RuntimeError> {
        if let Some(reason) = &self.dead_reason {
            return Err(invalid(format!("完整移交拒绝已终止 World：{reason}")));
        }
        if self.worker_shutdown.load(Ordering::SeqCst)
            || self.group_txs.len() != self.n_groups
            || self.information_roster.len() != self.n_groups
            || self.pending_calendar_tick.is_some()
        {
            return Err(invalid(
                "完整移交 World 已停止、通道/名册不完整或市场拍尚未收束",
            ));
        }
        let expected = self
            .personal_presence
            .as_ref()
            .ok_or_else(|| invalid("完整移交缺实际 PersonalPresence 名册"))?;
        if expected.len() != self.n_groups {
            return Err(invalid("完整移交 PersonalPresence 缺组"));
        }
        let mut owners = BTreeSet::new();
        let mut rosters = BTreeMap::new();
        for (index, (group, rows)) in expected.iter().enumerate() {
            if usize::from(*group) != index
                || rows.iter().map(|(owner, _)| *owner).collect::<Vec<_>>()
                    != self.information_roster[index]
            {
                return Err(invalid(format!(
                    "完整移交能力名册与真实组名册不同：group={group}"
                )));
            }
            let mut roster = Vec::with_capacity(rows.len());
            for &(owner, presence) in rows {
                if !owners.insert(owner) {
                    return Err(invalid(format!("完整移交名册重复 AccountId={owner:?}")));
                }
                roster.push((owner, presence == PersonalPresence::Required));
            }
            rosters.insert(*group, roster);
        }
        Ok(rosters)
    }

    /// 失败由 prepared 业务的拥有者统一 stop_without_drain；这里不取 Final 或消费 sparse。
    pub(crate) fn capture_information_handover(
        &mut self,
    ) -> Result<CapturedInformationHandover, RuntimeError> {
        let rosters = self.information_handover_rosters()?;
        let source = self.mirror_source();
        let calendar_record = self.committed_calendar_tick()?;
        if calendar_record
            .as_ref()
            .is_some_and(|record| !self.authenticates_committed_calendar(record))
        {
            return Err(invalid("Calendar 移交缺当前 World 认证资格"));
        }
        let published_retained = Arc::clone(&self.publication.load().retained);
        if published_retained.days.len() != self.retained_index.days.len()
            || published_retained
                .days
                .iter()
                .zip(&self.retained_index.days)
                .any(|(left, right)| !Arc::ptr_eq(left, right))
            || self.retained_days.len() != self.retained_index.days.len()
            || self
                .retained_days
                .iter()
                .zip(&self.retained_index.days)
                .any(|(left, right)| !Arc::ptr_eq(left, right))
        {
            return Err(invalid("移交 retained publication/index/days 不同源"));
        }
        for tx in &self.group_txs {
            tx.send(GroupIn::EchoInformationHandover { source })
                .map_err(|_| RuntimeError::SendFailed {
                    detail: "EchoInformationHandover 广播失败".to_owned(),
                })?;
        }
        let mut groups = Vec::with_capacity(self.n_groups);
        let mut current: Option<(
            Arc<crate::information::PublicLibrary>,
            Arc<crate::company::CompanyReadFacts>,
        )> = None;
        let mut seen = BTreeSet::new();
        let mut profiles = BTreeMap::new();
        for _ in 0..self.n_groups {
            match self.recv_world_up(Phase::Drain)? {
                UpMsg::GroupInformationHandover {
                    group,
                    source: actual,
                    handover,
                    library,
                    facts,
                    profiles: group_profiles,
                } => {
                    let roster = rosters
                        .get(&group)
                        .ok_or_else(|| invalid(format!("INFO 移交收到未知 group={group}")))?;
                    if actual != source || !seen.insert(group) {
                        return Err(invalid(format!(
                            "INFO 移交回执来源不同或重复：group={group}"
                        )));
                    }
                    if !group_profiles.is_empty()
                        && group_profiles
                            .iter()
                            .map(|(owner, _, _)| *owner)
                            .collect::<Vec<_>>()
                            != roster.iter().map(|(owner, _)| *owner).collect::<Vec<_>>()
                    {
                        return Err(invalid(format!(
                            "INFO 移交 profile 未完整覆盖真实名册：group={group}"
                        )));
                    }
                    for (owner, due, seed) in group_profiles {
                        if profiles.insert(owner, (due, seed)).is_some() {
                            return Err(invalid(format!("INFO 移交重复 profile owner={owner:?}")));
                        }
                    }
                    match &current {
                        Some((previous_library, previous_facts))
                            if previous_library.as_ref() != library.as_ref()
                                || previous_facts.as_ref() != facts.as_ref() =>
                        {
                            return Err(invalid(format!(
                                "INFO 移交当前 library/facts 不同：group={group}"
                            )));
                        }
                        None => current = Some((library, facts)),
                        _ => {}
                    }
                    groups.push((group, handover));
                }
                other => return Err(prepared_stop::read_failure("INFO 移交捕获", other)),
            }
        }
        let information = InformationHandover::merge(groups, &rosters).map_err(invalid)?;
        // 正常初值为双 None；Some 观察 commit 总成对安装签名及水位。
        if information.signature.is_none() && information.observed_through.is_some() {
            return Err(invalid("INFO 移交全部缺签名却有观察水位"));
        }
        let owners: BTreeSet<_> = rosters
            .values()
            .flatten()
            .map(|(owner, _)| *owner)
            .collect();
        if !profiles.is_empty() && profiles.keys().copied().collect::<BTreeSet<_>>() != owners {
            return Err(invalid("INFO 移交 profile 只覆盖部分真实账户"));
        }
        let (library, facts) =
            current.ok_or_else(|| invalid("INFO 移交没有当前 library/facts 回执"))?;
        if source != self.mirror_source() {
            return Err(invalid("INFO 移交捕获期间 World 静点发生变化"));
        }
        Ok(CapturedInformationHandover {
            source,
            information,
            library,
            facts,
            profiles,
            calendar_record,
            authenticated_source: source,
            retained_index: Arc::clone(&self.retained_index),
        })
    }

    /// 一次完整移交初值；部分安装失败也不激活，由拥有者丢弃并统一回收 prepared World。
    pub(crate) fn install_initial_information_handover_for_prepared(
        &mut self,
        captured: &CapturedInformationHandover,
        accounts: &BTreeMap<AccountId, GroupAccount>,
        personals: &BTreeMap<AccountId, Option<BeliefParticipantSnapshot>>,
        attention: &BTreeMap<AccountId, (u64, u64)>,
    ) -> Result<(), RuntimeError> {
        let rosters = self.information_handover_rosters()?;
        if captured.source != captured.authenticated_source {
            return Err(invalid(
                "完整移交捕获来源被改写，不能认证 Calendar 继承资格",
            ));
        }
        if !self.report_log.is_empty()
            || self.admin_next_id != 0
            || self.last_committed_calendar.is_some()
            || self.non_tick_revision != 0
            || self.publish_generation != 0
        {
            return Err(invalid("完整移交初值只能在首个 Start 或 Admin 前安装"));
        }
        if captured.information.signature.is_none()
            && captured.information.observed_through.is_some()
        {
            return Err(invalid("INFO 初值全部缺签名却有观察水位"));
        }
        let owners: BTreeSet<_> = rosters
            .values()
            .flatten()
            .map(|(owner, _)| *owner)
            .collect();
        if accounts.keys().copied().collect::<BTreeSet<_>>() != owners
            || personals.keys().copied().collect::<BTreeSet<_>>() != owners
            || (!attention.is_empty()
                && attention.keys().copied().collect::<BTreeSet<_>>() != owners)
        {
            return Err(invalid(
                "完整移交 accounts/personals/attention 未精确覆盖真实新名册",
            ));
        }
        let personal_groups: Vec<_> = rosters
            .iter()
            .map(|(&group, rows)| {
                (
                    group,
                    rows.iter()
                        .enumerate()
                        .map(|(local, (owner, _))| {
                            (
                                local as crate::runtime::LocalIdx,
                                *owner,
                                personals[owner].clone(),
                            )
                        })
                        .collect(),
                )
            })
            .collect();
        validate_personal_full(
            self.personal_presence
                .as_ref()
                .ok_or_else(|| invalid("完整移交缺能力名册"))?,
            &personal_groups,
        )
        .map_err(|error| invalid(format!("完整移交个人态非法：{error:?}")))?;
        let groups = captured.information.split(&rosters).map_err(invalid)?;
        let mut source = self.mirror_source();
        if captured.source.tick != source.tick || captured.source.civil_date != source.civil_date {
            return Err(invalid("完整移交捕获与 prepared 当前 tick/day/date 不同"));
        }
        source.day_end_stage = captured.source.day_end_stage;
        source.day = captured.source.day;
        let inherited_calendar =
            captured
                .calendar_record
                .as_ref()
                .map(|record| InheritedCalendarCommit {
                    record: record.clone(),
                    authenticated_at: captured.source,
                    target_epoch: source.authority_epoch,
                });
        if let Some(inherited) = &inherited_calendar {
            inherited.validate_install(
                &self.calendar_source,
                &self.calendar_bindings,
                self.tick_layout,
                source,
            )?;
        }
        let mut initial_publication = (*self.publication.load()).clone();
        initial_publication.retained = Arc::clone(&captured.retained_index);
        initial_publication.global.day = source.day;
        for (group, handover) in groups {
            let roster = &rosters[&group];
            let accounts = roster
                .iter()
                .map(|(owner, _)| (*owner, accounts[owner].clone()))
                .collect();
            let personals = roster
                .iter()
                .map(|(owner, _)| (*owner, personals[owner].clone()))
                .collect();
            let attention = roster
                .iter()
                .filter_map(|(owner, _)| {
                    attention.get(owner).map(|&(due, seed)| (*owner, due, seed))
                })
                .collect();
            self.group_txs[usize::from(group)]
                .send(GroupIn::InstallInitialInformationHandover {
                    source,
                    roster: roster.clone(),
                    handover,
                    library: Arc::clone(&captured.library),
                    facts: Arc::clone(&captured.facts),
                    accounts,
                    personals,
                    attention,
                    inherited_calendar: inherited_calendar.clone(),
                })
                .map_err(|_| RuntimeError::SendFailed {
                    detail: format!("完整移交初值广播失败：group={group}"),
                })?;
        }
        let mut seen = BTreeSet::new();
        for _ in 0..self.n_groups {
            #[cfg(test)]
            if self.handover_host_abort_after == Some(seen.len()) {
                self.handover_host_abort_after = None;
                self.drain_host_abort_for_test = true;
            }
            match self.recv_world_up(Phase::Drain)? {
                UpMsg::GroupInformationHandoverInstalled {
                    group,
                    source: actual,
                } => {
                    if actual != source || !rosters.contains_key(&group) || !seen.insert(group) {
                        return Err(invalid(format!(
                            "完整移交初值回执来源、组号或唯一性错误：group={group}"
                        )));
                    }
                }
                other => return Err(prepared_stop::read_failure("完整移交初值安装", other)),
            }
        }
        self.inherited_calendar = inherited_calendar;
        self.mirror_day_end_stage = source.day_end_stage;
        self.global.day = source.day;
        self.retained_days = captured.retained_index.days.clone();
        self.retained_index = Arc::clone(&captured.retained_index);
        self.publication = Arc::new(crate::runtime::publication::ViewPublication::new(Arc::new(
            initial_publication,
        )));
        if source != self.mirror_source() {
            return Err(invalid("完整移交初值安装期间 World 静点发生变化"));
        }
        Ok(())
    }
}

#[cfg(test)]
use crate::runtime::information_fixture as fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::group::TickPlan;
    use crate::runtime::personal_full_validation::{PersonalAbsenceReason, PersonalPresence};
    use crate::runtime::skeleton_tests::basic_spec;
    use crate::AccountId;

    fn world(rows: Vec<(AccountId, Option<BeliefParticipantSnapshot>)>, groups: usize) -> World {
        let per_group = rows.len().div_ceil(groups);
        let mut spec = basic_spec(
            groups,
            2,
            per_group,
            Box::new(|_| Box::new(|_, _| TickPlan::default())),
        );
        spec.ticks_per_day = 100;
        spec.auction_ticks = 0;
        spec.closing_auction_ticks = 0;
        spec.decision = Some(crate::runtime::decision::DecisionSetup {
            profiles: (0..rows.len())
                .map(|_| crate::runtime::decision::NpcProfile {
                    strategy: crate::strategy::StrategyData::retail(0.0, 100, 0.0),
                    seed: 17,
                    next_due: 100,
                    attention_base: 0.3,
                    retail_style: None,
                })
                .collect(),
            attention_gap_ticks: 4,
        });
        spec.calendar_start_date = fixture::at(0, 0).date();
        spec.civil_start_date = fixture::at(0, 0).date();
        let mut root = fixture::projection(rows.iter().map(|(owner, _)| *owner).collect());
        for (params, (_, snapshot)) in Arc::get_mut(&mut root)
            .expect("fixture 独占 root")
            .chain_params
            .iter_mut()
            .zip(&rows)
        {
            if snapshot.is_none() {
                *params = None;
            }
        }
        spec.root_projection = Some(root);
        spec.belief_seeds = rows.iter().map(|(_, snapshot)| snapshot.clone()).collect();
        let mut world = World::spawn_with_calendar_source(
            spec,
            fixture::calendar_source(),
            fixture::calendar_bindings(),
            #[cfg(all(target_family = "wasm", feature = "verification-harness"))]
            None,
        )
        .unwrap();
        let roster = rows
            .chunks(per_group)
            .enumerate()
            .map(|(group, rows)| {
                (
                    group as GroupIdx,
                    rows.iter()
                        .map(|(owner, snapshot)| {
                            (
                                *owner,
                                if snapshot.is_some() {
                                    PersonalPresence::Required
                                } else {
                                    PersonalPresence::Absent(PersonalAbsenceReason::NoStrategy)
                                },
                            )
                        })
                        .collect(),
                )
            })
            .collect();
        world.install_personal_presence(7, roster);
        world
    }

    fn initial_world() -> World {
        world(
            [11, 37]
                .into_iter()
                .map(|id| {
                    let owner = AccountId(id);
                    let mut snapshot = fixture::personal(owner);
                    snapshot.attention.information_cadence =
                        crate::session::notices::NpcInformationCadence::Immediate;
                    (owner, Some(snapshot))
                })
                .collect(),
            2,
        )
    }

    #[test]
    fn 完整个人态和信息移交遇宿主故障必须终止且不发布() {
        for read_personal in [true, false] {
            let mut world = initial_world();
            let before = world.publication.load();
            world.drain_host_abort_for_test = true;
            let result = if read_personal {
                world.collect_personal_full_for_prepared().map(|_| ())
            } else {
                world.capture_information_handover().map(|_| ())
            };
            assert!(matches!(result, Err(RuntimeError::HostAborted { phase: Phase::Drain, .. })));
            assert!(world.is_terminal());
            assert!(Arc::ptr_eq(&world.publication.load(), &before));
            assert!(world.run_ticks(1).is_err());
        }
    }

    #[test]
    fn 生产完整个人态宿主故障与清理诊断同时保留() {
        let mut world = initial_world();
        let before = world.publication.load();
        world.drain_host_abort_for_test = true;
        world.start_cleanup_message_for_test = Some(UpMsg::DayEndAborted {
            who: WorkerId::Desk(0),
            tick: 0,
        });
        let error = match world.collect_personal_full() {
            Ok(_) => panic!("宿主故障不得返回个人态"),
            Err(error) => error,
        };
        match error {
            RuntimeError::HostAbortedCleanup { primary, detail } => {
                assert!(matches!(*primary, RuntimeError::HostAborted { phase: Phase::Drain, .. }));
                assert!(detail.contains("非期望消息"));
            }
            other => panic!("必须保留宿主故障和清理诊断：{other:?}"),
        }
        assert!(world.is_terminal());
        assert!(world.handles.is_empty(), "生产清理应收获 owned handle");
        assert!(Arc::ptr_eq(&world.publication.load(), &before));
    }

    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn 局部合成故障在部分组安装后保留两端_publication() {
        let mut old = initial_world();
        let captured = old.capture_information_handover().unwrap();
        let rows: Vec<_> = old.collect_personal_full_for_prepared().unwrap().groups
            .into_iter().flat_map(|(_, slots)| slots.into_iter().map(|(_, owner, personal)| (owner, personal))).collect();
        let mut next = world(rows, 2);
        assert_eq!(next.n_groups, 2);
        let (accounts, mut attention) = seeds(&mut next);
        for (&owner, &profile) in &captured.profiles { attention.insert(owner, profile); }
        let personal = personals(&mut next);
        let old_publication = old.publication.load();
        let next_publication = next.publication.load();
        next.handover_host_abort_after = Some(1);
        let error = next.install_initial_information_handover_for_prepared(
            &captured, &accounts, &personal, &attention,
        ).expect_err("一个组确认后宿主故障不得完成安装");
        assert!(matches!(error, RuntimeError::HostAborted { phase: Phase::Drain, .. }));
        assert!(next.is_terminal());
        assert!(Arc::ptr_eq(&next.publication.load(), &next_publication));
        assert!(Arc::ptr_eq(&old.publication.load(), &old_publication));
        // 此 Native seam 没有广播实例级全局故障，旧 World 可读只限此测试。
        assert!(old.capture_information_handover().is_ok(), "局部故障不得改变旧 World");
        let (completion, issues) = next.stop_failed_with_completion();
        assert!(completion.owned_workers > 0);
        assert_eq!(completion.joined_workers, completion.owned_workers, "prepared World 必须收获全部 owned handle");
        assert_eq!(completion.worker_results.len(), completion.joined_workers);
        assert_eq!(completion.worker_results.iter().map(|entry| &entry.worker).collect::<std::collections::BTreeSet<_>>().len(), completion.joined_workers);
        assert!(issues.is_empty(), "本次局部故障回收不应出现次生问题：{issues:?}");
        old.stop_without_drain().unwrap();
    }

    fn seeds(
        world: &mut World,
    ) -> (
        BTreeMap<AccountId, GroupAccount>,
        BTreeMap<AccountId, (u64, u64)>,
    ) {
        let mut accounts = BTreeMap::new();
        for (group, rows) in world.snapshot_accounts().unwrap() {
            for (&owner, account) in world.information_roster[group as usize].iter().zip(rows) {
                accounts.insert(owner, account);
            }
        }
        let mut attention = BTreeMap::new();
        for (group, rows) in world.collect_group_attention().unwrap() {
            for (local, due, seed) in rows {
                if let Some(&owner) = world.information_roster[group as usize].get(local as usize) {
                    let mixed = (group as u64)
                        .wrapping_mul(0x9E3779B97F4A7C15)
                        .wrapping_add(local as u64);
                    attention.insert(owner, (due, seed ^ mixed));
                }
            }
        }
        (accounts, attention)
    }

    fn install(
        world: &mut World,
        captured: &CapturedInformationHandover,
    ) -> Result<(), RuntimeError> {
        let (accounts, mut attention) = seeds(world);
        for (&owner, &profile) in &captured.profiles {
            attention.insert(owner, profile);
        }
        let personals = personals(world);
        world.install_initial_information_handover_for_prepared(
            captured, &accounts, &personals, &attention,
        )
    }

    fn personals(world: &mut World) -> BTreeMap<AccountId, Option<BeliefParticipantSnapshot>> {
        world
            .collect_personal_full_for_prepared()
            .unwrap()
            .groups
            .into_iter()
            .flat_map(|(_, rows)| {
                rows.into_iter()
                    .map(|(_, owner, personal)| (owner, personal))
            })
            .collect()
    }

    #[test]
    fn capture_keeps_current_none_refresh_separate_from_last_observation_and_source() {
        let mut world = initial_world();
        let observation = fixture::payload(true);
        world
            .with_information_transaction(Vec::new(), observation.clone(), |_| Ok(()))
            .unwrap();
        let mut refresh = fixture::payload(false);
        fixture::announcement(&mut refresh, 0);
        refresh.current_facts = fixture::facts(20_000);
        world
            .with_information_transaction(Vec::new(), refresh.clone(), |_| Ok(()))
            .unwrap();
        let source = world.mirror_source();
        let pending = world.pending_mirror_domains();
        let before = world.collect_personal_full().unwrap();
        let captured = world.capture_information_handover().unwrap();
        let after = world.collect_personal_full().unwrap();
        assert_eq!(captured.source, source);
        assert_eq!(world.mirror_source(), source);
        assert_eq!(world.pending_mirror_domains(), pending);
        assert_eq!(before.groups, after.groups);
        assert_eq!(captured.library, refresh.library);
        assert_eq!(captured.facts, refresh.current_facts);
        let material = &captured.information.signature.as_ref().unwrap().material;
        assert_eq!(material.library, observation.library);
        assert_eq!(material.facts, observation.current_facts);
        assert_ne!(captured.library, material.library);
        assert_ne!(captured.facts, material.facts);
        world.stop_without_drain().unwrap();
        capture_rejects_different_current_shared_inputs_across_groups();
        inherited_close_keeps_original_calendar_and_retained_through_repeated_join();
    }

    #[test]
    fn initial_install_regroups_checkpoint_and_preserves_immediate_same_through_dedup() {
        let mut old = initial_world();
        let payload = fixture::payload(true);
        old.with_information_transaction(Vec::new(), payload.clone(), |_| Ok(()))
            .unwrap();
        let captured = old.capture_information_handover().unwrap();
        let personal = old.collect_personal_full().unwrap();
        let mut rows: Vec<_> = personal
            .groups
            .into_iter()
            .flat_map(|(_, slots)| {
                slots
                    .into_iter()
                    .map(|(_, owner, snapshot)| (owner, snapshot))
            })
            .collect();
        rows.reverse();
        rows.insert(1, (AccountId(99), None));
        let mut next = world(rows, 1);
        let next_source = next.mirror_source();
        install(&mut next, &captured).unwrap();
        assert_eq!(next.mirror_source(), next_source);
        let installed = next.capture_information_handover().unwrap();
        assert_eq!(installed.information, captured.information);
        assert_eq!(installed.library, captured.library);
        assert_eq!(installed.facts, captured.facts);
        for (owner, profile) in &captured.profiles {
            assert_eq!(installed.profiles.get(owner), Some(profile));
        }
        next.with_information_transaction(Vec::new(), payload, |candidate| {
            assert!(candidate.groups.iter().all(|group| group
                .information
                .as_ref()
                .unwrap()
                .personals
                .is_empty()));
            Ok(())
        })
        .unwrap();
        old.stop_without_drain().unwrap();
        next.stop_without_drain().unwrap();
    }

    #[test]
    fn initial_install_all_none_retains_some_observation_eligibility() {
        let mut old = initial_world();
        let mut refresh = fixture::payload(false);
        fixture::announcement(&mut refresh, 0);
        old.with_information_transaction(Vec::new(), refresh, |_| Ok(()))
            .unwrap();
        let captured = old.capture_information_handover().unwrap();
        assert!(captured.information.signature.is_none());
        assert!(captured.information.observed_through.is_none());
        let mut next = initial_world();
        install(&mut next, &captured).unwrap();
        next.with_information_transaction(Vec::new(), fixture::payload(true), |candidate| {
            assert_eq!(
                candidate
                    .groups
                    .iter()
                    .map(|group| group.information.as_ref().unwrap().personals.len())
                    .sum::<usize>(),
                2
            );
            Ok(())
        })
        .unwrap();
        old.stop_without_drain().unwrap();
        next.stop_without_drain().unwrap();
        initial_install_rejects_second_all_none_seed_and_reports_worker_cleanup();
    }

    #[test]
    fn initial_install_rejects_watermark_without_signature_before_sending() {
        let mut old = initial_world();
        let mut captured = old.capture_information_handover().unwrap();
        captured.information.observed_through = Some(fixture::at(19, 0));
        let mut next = initial_world();
        let before = next.mirror_source();
        assert!(install(&mut next, &captured).is_err());
        assert_eq!(next.mirror_source(), before);
        assert!(next
            .capture_information_handover()
            .unwrap()
            .information
            .signature
            .is_none());
        old.stop_without_drain().unwrap();
        next.stop_without_drain().unwrap();
    }

    #[test]
    fn initial_install_rejects_after_real_none_information_commit() {
        let mut old = initial_world();
        let captured = old.capture_information_handover().unwrap();
        let mut next = initial_world();
        next.with_information_transaction(Vec::new(), fixture::payload(false), |_| Ok(()))
            .unwrap();
        let before = next.mirror_source();
        assert!(install(&mut next, &captured).is_err());
        assert_eq!(next.mirror_source(), before);
        old.stop_without_drain().unwrap();
        next.stop_without_drain().unwrap();
    }

    #[test]
    fn initial_install_preserves_raw_account_flags_and_rejects_incomplete_maps_without_writes() {
        let mut old = initial_world();
        let captured = old.capture_information_handover().unwrap();
        let mut next = initial_world();
        let (mut accounts, mut attention) = seeds(&mut next);
        let mut personals = personals(&mut next);
        personals
            .get_mut(&AccountId(11))
            .unwrap()
            .as_mut()
            .unwrap()
            .attention
            .base_probability = 0.77;
        attention.insert(AccountId(11), (873, 291));
        let account = accounts.get_mut(&AccountId(11)).unwrap();
        account.shares[0] = 0;
        account.ever_held[0] = true;
        account.foreign_active[0] = 1;
        account.buy_purse_held_cents = 73;
        account.frozen_today[1] = 100;
        account.invested_cents[1] = 223;
        let mut missing = attention.clone();
        missing.remove(&AccountId(37));
        let before = next.snapshot_accounts().unwrap();
        assert!(next
            .install_initial_information_handover_for_prepared(
                &captured, &accounts, &personals, &missing
            )
            .is_err());
        assert_eq!(next.snapshot_accounts().unwrap(), before);
        let mut invalid_personal = personals.clone();
        invalid_personal.insert(AccountId(11), None);
        assert!(next
            .install_initial_information_handover_for_prepared(
                &captured,
                &accounts,
                &invalid_personal,
                &attention
            )
            .is_err());
        assert_eq!(next.snapshot_accounts().unwrap(), before);
        next.install_initial_information_handover_for_prepared(
            &captured, &accounts, &personals, &attention,
        )
        .unwrap();
        let after = next.snapshot_accounts().unwrap();
        assert_eq!(after[0].1[0], accounts[&AccountId(11)]);
        assert_eq!(
            next.capture_information_handover().unwrap().profiles[&AccountId(11)],
            (873, 291)
        );
        assert_eq!(self::personals(&mut next), personals);
        old.stop_without_drain().unwrap();
        next.stop_without_drain().unwrap();
    }

    #[test]
    fn initial_install_rejects_after_real_start_without_changing_source() {
        let mut old = initial_world();
        let captured = old.capture_information_handover().unwrap();
        let mut next = initial_world();
        next.run_ticks(1).unwrap();
        let before = next.mirror_source();
        assert!(install(&mut next, &captured).is_err());
        assert_eq!(next.mirror_source(), before);
        old.stop_without_drain().unwrap();
        next.stop_without_drain().unwrap();
    }

    fn capture_rejects_different_current_shared_inputs_across_groups() {
        let mut world = initial_world();
        let (accounts, attention) = seeds(&mut world);
        let personals = personals(&mut world);
        let captured = world.capture_information_handover().unwrap();
        let source = world.mirror_source();
        let owner = AccountId(11);
        let &(due, seed) = &attention[&owner];
        world.group_txs[0]
            .send(GroupIn::InstallInitialInformationHandover {
                source,
                roster: vec![(owner, true)],
                handover: crate::runtime::information_handover::GroupInformationHandover {
                    observed_through: None,
                    signature: None,
                },
                library: captured.library,
                facts: fixture::facts(20_000),
                accounts: vec![(owner, accounts[&owner].clone())],
                personals: vec![(owner, personals[&owner].clone())],
                attention: vec![(owner, due, seed)],
                inherited_calendar: None,
            })
            .unwrap();
        assert!(matches!(
            recv_deadline(&world.up_rx, Phase::Drain, &world.deadline).unwrap(),
            UpMsg::GroupInformationHandoverInstalled { group: 0, .. }
        ));
        let error = world.capture_information_handover().unwrap_err();
        assert!(error.to_string().contains("library/facts"));
        assert_eq!(world.mirror_source(), source);
        world.stop_without_drain().unwrap();
    }

    fn initial_install_rejects_second_all_none_seed_and_reports_worker_cleanup() {
        let mut old = initial_world();
        let captured = old.capture_information_handover().unwrap();
        let mut next = initial_world();
        let (accounts, attention) = seeds(&mut next);
        let personals = personals(&mut next);
        next.install_initial_information_handover_for_prepared(
            &captured, &accounts, &personals, &attention,
        )
        .unwrap();
        let source = next.mirror_source();
        let error = next
            .install_initial_information_handover_for_prepared(
                &captured, &accounts, &personals, &attention,
            )
            .unwrap_err();
        assert!(error.to_string().contains("安装一次"));
        assert_eq!(next.mirror_source(), source);
        let cleanup = next.stop_without_drain().unwrap_err();
        assert!(cleanup.to_string().contains("安装一次"));
        old.stop_without_drain().unwrap();
    }

    fn inherited_close_keeps_original_calendar_and_retained_through_repeated_join() {
        fn prepared(
            rows: &[(AccountId, Option<BeliefParticipantSnapshot>)],
            epoch: u64,
            tick: u64,
        ) -> World {
            let mut spec = basic_spec(2, 2, 1, Box::new(|_| Box::new(|_, _| TickPlan::default())));
            spec.ticks_per_day = 2;
            spec.auction_ticks = 0;
            spec.closing_auction_ticks = 0;
            spec.calendar_start_date = fixture::at(0, 0).date();
            spec.civil_start_date = fixture::at(0, 0).date();
            spec.initial_committed_tick = tick;
            spec.initial_last_day_end_tick = (tick > 0).then_some(tick);
            spec.root_projection = Some(fixture::projection(
                rows.iter().map(|(owner, _)| *owner).collect(),
            ));
            spec.belief_seeds = rows.iter().map(|(_, personal)| personal.clone()).collect();
            spec.decision = Some(crate::runtime::decision::DecisionSetup {
                profiles: (0..rows.len())
                    .map(|_| crate::runtime::decision::NpcProfile {
                        strategy: crate::strategy::StrategyData::retail(0.0, 100, 0.0),
                        seed: 17,
                        next_due: 1000,
                        attention_base: 0.3,
                        retail_style: None,
                    })
                    .collect(),
                attention_gap_ticks: 4,
            });
            let mut world = World::spawn_with_calendar_source(
                spec,
                fixture::calendar_source(),
                fixture::calendar_bindings(),
                #[cfg(all(target_family = "wasm", feature = "verification-harness"))]
                None,
            )
            .unwrap();
            world.install_personal_presence(
                epoch,
                rows.iter()
                    .enumerate()
                    .map(|(group, (owner, _))| {
                        (group as u16, vec![(*owner, PersonalPresence::Required)])
                    })
                    .collect(),
            );
            world
        }
        let rows: Vec<_> = [11, 37]
            .into_iter()
            .map(|id| (AccountId(id), Some(fixture::personal(AccountId(id)))))
            .collect();
        let mut old = prepared(&rows, 7, 0);
        old.run_ticks(2).unwrap();
        old.run_day_end_public().unwrap();
        let original = old.committed_calendar_tick().unwrap().unwrap();
        old.freeze_retained_day(Arc::new(crate::runtime::publication::RetainedDaySnapshot {
            date: fixture::at(0, 0).date(),
            securities: BTreeMap::new(),
        }));
        let mut captured = old.capture_information_handover().unwrap();
        let raw = old.collect_personal_full().unwrap();
        let rows: Vec<_> = raw
            .groups
            .into_iter()
            .flat_map(|(_, slots)| {
                slots
                    .into_iter()
                    .map(|(_, owner, personal)| (owner, personal))
            })
            .collect();
        for epoch in [8, 9] {
            let mut next = prepared(&rows, epoch, 2);
            assert!(
                next.committed_calendar_tick().unwrap().is_none(),
                "restore 标记不能补造原生 commit"
            );
            install(&mut next, &captured).unwrap();
            let inherited = next.committed_calendar_tick().unwrap().unwrap();
            assert_eq!(inherited.source, original.source);
            assert!(Arc::ptr_eq(&inherited.calendar, &original.calendar));
            assert!(next
                .has_committed_market_close_for_civil(2, fixture::at(0, 0).date())
                .unwrap());
            assert!(Arc::ptr_eq(&next.retained_index, &captured.retained_index));
            assert!(Arc::ptr_eq(
                &next.publication.load().retained,
                &captured.retained_index
            ));
            assert_eq!(next.mirror_source().generation, 0);
            let clock = crate::runtime::InformationClockContext::after_committed(inherited);
            next.validate_information_clock(&clock, clock.cutoff(), &clock.anchors().unwrap())
                .unwrap();
            let mut payload = fixture::payload(false);
            payload.observation = Some(crate::runtime::protocol::InformationObservation {
                clock: clock.clone(),
                through: clock.cutoff(),
                market: fixture::payload(true).observation.unwrap().market,
                company_facts: fixture::facts(10_000),
                as_of_anchors: clock.anchors().unwrap(),
                memory_retention: crate::runtime::protocol::InformationMemoryRetention::Preserve,
            });
            Arc::make_mut(&mut payload.observation.as_mut().unwrap().market).tick = 2;
            next.with_information_transaction(Vec::new(), payload, |_| Ok(()))
                .unwrap();
            let mut foreign = original.clone();
            foreign.calendar = original
                .calendar
                .day()
                .tick(original.calendar.layout(), original.calendar.before_input())
                .unwrap();
            let foreign_clock = crate::runtime::InformationClockContext::after_committed(foreign);
            assert!(next
                .validate_information_clock(
                    &foreign_clock,
                    foreign_clock.cutoff(),
                    &foreign_clock.anchors().unwrap()
                )
                .is_err());
            captured = next.capture_information_handover().unwrap();
            if epoch == 9 {
                let date = fixture::at(0, 0).date().next().unwrap();
                next.set_civil_date(date);
                assert!(!next.has_committed_market_close_for_civil(2, date).unwrap());
                next.run_ticks(1).unwrap();
                assert!(next.inherited_calendar.is_none());
                assert_eq!(
                    next.committed_calendar_tick()
                        .unwrap()
                        .unwrap()
                        .source
                        .authority_epoch,
                    epoch
                );
                assert!(Arc::ptr_eq(
                    &next.publication.load().retained,
                    &captured.retained_index
                ));
                assert!(next
                    .validate_information_clock(&clock, clock.cutoff(), &clock.anchors().unwrap())
                    .is_err());
            }
            next.stop_without_drain().unwrap();
        }
        old.stop_without_drain().unwrap();
    }
}
