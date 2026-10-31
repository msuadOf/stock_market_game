//! 接管已提交 World 输入；三表以真实 owner 覆盖最后整态，不缓存业务事件。
use super::*;
use crate::runtime::driver_sync::{FullReason, SyncOwner};

impl RuntimeDriver {
    pub(super) fn next_sync_source_sequence(&self) -> Result<u64, String> {
        self.sync_source_seq
            .checked_add(1)
            .ok_or_else(|| "driver sync source_seq 已耗尽".to_owned())
    }

    pub(super) fn capture_sync_inputs(
        &mut self,
        input: &mut PreparedDriverStep,
        source_seq: u64,
    ) -> Result<(), String> {
        if source_seq != self.next_sync_source_sequence()? {
            return Err("driver sync source_seq 与下一真实输入不一致".to_owned());
        }
        let source = self.mirror_source();
        if self.sync_last_source == Some(source) {
            return Err("driver sync 同一已提交来源不得重复登记新输入序号".to_owned());
        }
        let mut accounts = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for (group, rows) in input.accounts.iter().enumerate() {
            if group >= self.world.n_groups() {
                return Err(format!("sync 账户未知组：{group}"));
            }
            let group = u16::try_from(group).map_err(|_| "sync group 超 u16")?;
            for (local, value) in rows.iter() {
                if *local as usize >= self.accounts_per_group || !seen.insert((group, *local)) {
                    return Err(format!("sync 账户 owner 越界或重复：{group}/{local}"));
                }
                if self.account_by_owner((group, *local)).is_none() {
                    if value.as_ref()
                        != &crate::runtime::group::GroupAccount::new(
                            0,
                            self.world_public_views().len(),
                        )
                    {
                        return Err(format!("sync 非空 padding：{group}/{local}"));
                    }
                    continue;
                }
                accounts.push((
                    SyncOwner {
                        group,
                        local: *local,
                    },
                    std::sync::Arc::clone(value),
                ));
            }
        }
        let mut personal_owners = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for (index, id, value) in &input.personals {
            if self.id_map.get(*index) != Some(id) || !seen.insert(*id) || !value.matches_owner(*id)
            {
                return Err(format!("sync 个人属主或名册错误：{id:?}"));
            }
            let group = u16::try_from(*index / self.accounts_per_group)
                .map_err(|_| "sync 个人 group 超 u16")?;
            let local = u32::try_from(*index % self.accounts_per_group)
                .map_err(|_| "sync 个人 local 超 u32")?;
            personal_owners.push(SyncOwner { group, local });
        }
        // 先核全部账户/个人身份，再接管调度输入；首次及非 tick Full 取完整只读面。
        let full = !self.sync_inputs.has_successful_baseline()
            || !self.pending_mirror_domains().is_empty()
            || source.day_end_stage != crate::runtime::source::MirrorDayEndStage::Tick;
        let attention = if full {
            self.world.collect_group_attention()
        } else {
            self.world.collect_group_attention_sparse()
        }
        .map_err(|error| format!("sync 调度输入收集失败：{error}"))?;
        let mut attentions = std::collections::BTreeMap::new();
        let mut seen_groups = std::collections::BTreeSet::new();
        for (group, rows) in attention {
            if usize::from(group) >= self.world.n_groups() || !seen_groups.insert(group) {
                return Err(format!("sync 调度未知或重复组：{group}"));
            }
            if full
                && rows.len()
                    != self
                        .id_map
                        .len()
                        .saturating_sub(usize::from(group) * self.accounts_per_group)
                        .min(self.accounts_per_group)
            {
                return Err(format!("sync 完整调度缺少真实名册成员：{group}"));
            }
            for (local, due, seed) in rows {
                let owner = SyncOwner { group, local };
                if local as usize >= self.accounts_per_group
                    || self.account_by_owner((group, local)).is_none()
                {
                    return Err(format!("sync 调度属主未知：{owner:?}"));
                }
                // sparse 的演进标记可重复，回执读取的是同一最终 profile；同值幂等。
                if let Some(previous) = attentions.insert(owner, (due, seed)) {
                    if full || previous != (due, seed) {
                        return Err(format!("sync 调度重复异值或完整回执重复：{owner:?}"));
                    }
                }
            }
        }
        // owner 已全部验证，source_seq 大于所有本窗口行；三个 record 不再引入新业务拒绝。
        for (owner, value) in accounts {
            self.sync_inputs
                .record_account(owner, source_seq, value)
                .map_err(|error| error.to_string())?;
        }
        // 全部身份、布局与调度验证成功后才消费本拍个人态；失败不会留下半取走的输入。
        for (owner, (_, _, value)) in personal_owners
            .into_iter()
            .zip(std::mem::take(&mut input.personals))
        {
            self.sync_inputs
                .record_belief(owner, source_seq, value)
                .map_err(|error| error.to_string())?;
        }
        for (owner, value) in attentions {
            self.sync_inputs
                .record_attention(owner, source_seq, value)
                .map_err(|error| error.to_string())?;
        }
        self.sync_inputs.mark_plans_pending();
        if !self.pending_mirror_domains().is_empty() {
            self.sync_inputs
                .require_full(FullReason::DomainMutationWithoutFacts);
        }
        self.sync_source_seq = source_seq;
        self.sync_last_source = Some(source);
        input.sync_source = Some(source);
        Ok(())
    }

    /// 只有真正消费镜像时才复制累计行的只读句柄；失败 prepare 保留三表用于同来源重试。
    pub(crate) fn materialize_sync_mirror_inputs(
        &self,
        input: &mut PreparedDriverStep,
    ) -> Result<(), String> {
        let Some(source) = input.sync_source else {
            // 自带 raw Full 的静点及独立完整 getter 保留原始输入，不重贴当前累计值。
            return Ok(());
        };
        if input.static_read.is_some()
            || Some(source) != self.sync_last_source
            || source != self.mirror_source()
        {
            return Err("累计镜像输入的捕获来源已过期或与 raw Full 模式混用".to_owned());
        }
        let mut groups = vec![Vec::new(); self.world.n_groups()];
        for (owner, row) in self.sync_inputs.accounts() {
            #[cfg(all(test, not(target_arch = "wasm32")))]
            super::sync_input_tests::record_sync_copy(
                super::sync_input_tests::SyncCopy::MirrorAccount,
            );
            groups[usize::from(owner.group)].push((owner.local, std::sync::Arc::clone(&row.value)));
        }
        input.accounts = groups.into_iter().map(std::sync::Arc::new).collect();
        input.personals = self
            .sync_inputs
            .beliefs()
            .iter()
            .map(|(owner, row)| {
                let index = self.global_index((owner.group, owner.local));
                #[cfg(all(test, not(target_arch = "wasm32")))]
                super::sync_input_tests::record_sync_copy(
                    super::sync_input_tests::SyncCopy::MirrorPersonal,
                );
                (index, self.id_map[index], row.value.clone())
            })
            .collect();
        Ok(())
    }

    pub(crate) fn sync_attention_for_projection(
        &self,
    ) -> Result<Vec<(AccountId, u64, u64)>, String> {
        if self.sync_last_source != Some(self.mirror_source()) {
            return Err("driver sync 覆盖输入与当前 World 静点来源不同".to_owned());
        }
        self.sync_inputs
            .attentions()
            .iter()
            .map(|(owner, row)| {
                let id = self
                    .account_by_owner((owner.group, owner.local))
                    .ok_or_else(|| format!("sync 调度安装属主不在名册：{owner:?}"))?;
                Ok((id, row.value.0, row.value.1))
            })
            .collect()
    }
}
