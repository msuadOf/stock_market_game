//! 行政静点完整读面及同次候选入口；不消费 tick 输入或业务回执。

#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AdministrativeCacheSnapshot {
    pub source: crate::runtime::source::WorldMirrorSource,
    pub personals: Vec<Option<crate::runtime::world::BeliefParticipantSnapshot>>,
    pub pending_counts: (usize, usize, usize, usize, usize),
    pub sync_state: (usize, usize, usize, bool, u64),
}

#[cfg(test)]
impl super::RuntimeDriver {
    pub(crate) fn administrative_cache_snapshot_for_test(&self) -> AdministrativeCacheSnapshot {
        AdministrativeCacheSnapshot {
            source: self.mirror_source(),
            personals: self
                .belief_cache
                .iter()
                .map(|value| value.as_ref().map(|personal| personal.to_snapshot()))
                .collect(),
            pending_counts: (
                self.pending_retail.len(),
                self.pending_retail_rejections.len(),
                self.world.drained_retail.len(),
                self.pending_receipts.len(),
                self.pending_hold_inits.len(),
            ),
            sync_state: self.pending_sync_inputs_for_test(),
        }
    }
}

/// 首个 Prepare 前取得真实完整基线；其后只允许同次候选覆盖，不能回读 working。
pub(crate) struct AdministrativeBaseline {
    pub source: crate::runtime::source::WorldMirrorSource,
    pub roster: Vec<crate::AccountId>,
    pub per_group: usize,
    pub accounts: Vec<(
        crate::runtime::GroupIdx,
        Vec<crate::runtime::group::GroupAccount>,
    )>,
    pub personals: Vec<(
        usize,
        crate::AccountId,
        Option<crate::runtime::world::BeliefParticipantSnapshot>,
    )>,
    pub plans: crate::plans::PlanBook,
    /// 独立 profile 的真实调度读面；不以 raw personal/cache 的旧字段替代。
    pub attention: std::collections::BTreeMap<crate::AccountId, (u64, u64)>,
}

impl super::RuntimeDriver {
    pub(crate) fn prepare_administrative_baseline(
        &mut self,
        session: &crate::GameSession,
    ) -> Result<AdministrativeBaseline, crate::runtime::RuntimeError> {
        use crate::runtime::RuntimeError;
        let invalid = |detail: String| RuntimeError::ProtocolInvariant { detail };
        if !self.pending_mirror_domains().is_empty() {
            return Err(invalid(
                "行政镜像入口尚有未安装域；请先完成原同步，不消费旧 pending".into(),
            ));
        }
        self.ensure_plan_ownership().map_err(invalid)?;
        let source = self.mirror_source();
        let accounts = match self.world.snapshot_accounts() {
            Ok(value) => value,
            Err(error) => return Err(self.world.stop_failed_administrative_read(error)),
        };
        if accounts.len() != self.world.n_groups()
            || accounts.iter().enumerate().any(|(index, (group, rows))| {
                usize::from(*group) != index || rows.len() != self.accounts_per_group
            })
        {
            let error = invalid("行政完整基线账户组维度错误".into());
            return Err(self.world.stop_failed_administrative_read(error));
        }
        let full = self.world.collect_personal_full()?;
        if full.source != source {
            let error = invalid("行政完整基线的个人态来源已改变".into());
            return Err(self.world.stop_failed_administrative_read(error));
        }
        let mut personals = Vec::new();
        for (group, rows) in full.groups {
            for (local, id, mut snapshot) in rows {
                if let Some(value) = &mut snapshot {
                    if let Some(group) = &value.retail_experience {
                        value.retail_experience =
                            Some(match session.runtime_retail_experience_for(&id) {
                                Some(existing) => existing.merged_with_group_authority(group),
                                None => group.clone(),
                            });
                    }
                }
                let index = self.global_index((group, local));
                let cache_matches = match (self.belief_cache.get(index), &snapshot) {
                    (Some(Some(cached)), Some(snapshot)) => cached.matches_snapshot(snapshot),
                    (Some(None), None) => true,
                    _ => false,
                };
                if self.id_map.get(index) != Some(&id) || !cache_matches {
                    return Err(invalid(format!(
                        "行政基线与真实个人缓存不一致：group={group} local={local} account={id:?}"
                    )));
                }
                personals.push((index, id, snapshot));
            }
        }
        let plan_shards = match self.world.collect_plans() {
            Ok(value) => value,
            Err(error) => return Err(self.world.stop_failed_administrative_read(error)),
        };
        let plans = match self.plan_owners.merge(plan_shards, self.plan_policy) {
            Ok(value) => value,
            Err(error) => return Err(self.world.stop_failed_administrative_read(error)),
        };
        // 全量只读，不消费 ordinary tick 的 sparse/sync 输入；与个人快照共用静点来源。
        let scheduled = match self.world.collect_group_attention() {
            Ok(value) => value,
            Err(error) => return Err(self.world.stop_failed_administrative_read(error)),
        };
        let mut attention = std::collections::BTreeMap::new();
        let mut seen_groups = std::collections::BTreeSet::new();
        for (group, rows) in scheduled {
            if usize::from(group) >= self.world.n_groups() || !seen_groups.insert(group) {
                let error = invalid("行政完整调度组越界或重复".into());
                return Err(self.world.stop_failed_administrative_read(error));
            }
            for (local, due, seed) in rows {
                let id = self
                    .account_by_owner((group, local))
                    .filter(|_| (local as usize) < self.accounts_per_group);
                let Some(id) = id else {
                    let error = invalid(format!("行政完整调度属主不在真实名册：{group}/{local}"));
                    return Err(self.world.stop_failed_administrative_read(error));
                };
                if attention.insert(id, (due, seed)).is_some() {
                    let error = invalid(format!("行政完整调度属主重复：{id:?}"));
                    return Err(self.world.stop_failed_administrative_read(error));
                }
            }
        }
        if seen_groups.len() != self.world.n_groups() || attention.len() != self.id_map.len() {
            let error = invalid("行政完整调度缺少真实名册成员或组".into());
            return Err(self.world.stop_failed_administrative_read(error));
        }
        if self.mirror_source() != source {
            return Err(invalid("行政完整基线读取期间 World 来源改变".into()));
        }
        Ok(AdministrativeBaseline {
            source,
            roster: self.id_map.clone(),
            per_group: self.accounts_per_group,
            accounts,
            personals,
            plans,
            attention,
        })
    }

    pub(crate) fn with_prepared_administrative_transaction<P>(
        &mut self,
        grouped: Vec<(
            crate::runtime::GroupIdx,
            Vec<crate::runtime::world::AdminOperation>,
        )>,
        information: Option<crate::runtime::world::InformationPreparePayload>,
        repurchase: Option<crate::runtime::protocol::RepurchasePreparePayload>,
        inspect: impl FnOnce(
            &crate::runtime::world::AdminCandidate,
            &crate::runtime::world::PreparedAdministrativeSource,
        ) -> Result<P, crate::runtime::RuntimeError>,
    ) -> Result<(P, crate::runtime::source::PreparedMirrorAck), crate::runtime::RuntimeError> {
        self.world.with_prepared_administrative_transaction(
            grouped,
            information,
            repurchase,
            inspect,
        )
    }

    pub(crate) fn with_prepared_staged_administrative_transaction<P>(
        &mut self,
        stages: Vec<crate::runtime::world::AdminOperationStage>,
        information: Option<crate::runtime::world::InformationPreparePayload>,
        inspect: impl FnOnce(
            &crate::runtime::world::AdminCandidate,
            &crate::runtime::world::PreparedAdministrativeSource,
            &crate::runtime::world::PreparedAdministrativeStages<'_>,
        ) -> Result<P, crate::runtime::RuntimeError>,
    ) -> Result<(P, crate::runtime::source::PreparedMirrorAck), crate::runtime::RuntimeError> {
        self.world
            .with_prepared_staged_administrative_transaction(stages, information, inspect)
    }

    pub(crate) fn administrative_world_is_terminal(&self) -> bool {
        self.world.is_terminal()
    }

    /// civil 四域已在首 Commit 前验证；仅同步当前日期，不重写历史 Calendar 来源。
    pub(crate) fn install_administrative_civil_date(&mut self, date: crate::calendar::CivilDate) {
        self.world.set_civil_date(date);
    }
}

impl AdministrativeBaseline {
    /// 只搬同次真实完整 group book；身份分配与资金判断属于 P1 组候选。
    pub(crate) fn plans_after(
        &self,
        candidate: &crate::runtime::world::AdminCandidate,
    ) -> Result<Option<crate::plans::PlanBook>, crate::runtime::RuntimeError> {
        if !candidate
            .groups
            .iter()
            .any(|group| group.repurchase.is_some())
        {
            return Ok(None);
        }
        let mut shards = crate::runtime::plan_ownership::split(
            self.plans.clone(),
            &self.roster,
            self.accounts.len(),
            self.per_group,
        )?;
        for group in &candidate.groups {
            if let Some(prepared) = &group.repurchase {
                let slot = shards.get_mut(usize::from(group.group)).ok_or_else(|| {
                    crate::runtime::RuntimeError::ProtocolInvariant {
                        detail: "行政完整PlanBook候选group越界".into(),
                    }
                })?;
                *slot = prepared.book.clone();
            }
        }
        crate::runtime::plan_ownership::merge(
            shards
                .into_iter()
                .enumerate()
                .map(|(group, book)| (group as crate::runtime::GroupIdx, book))
                .collect(),
            &self.roster,
            self.accounts.len(),
            self.per_group,
            *self.plans.policy(),
        )
        .map(Some)
    }
}
