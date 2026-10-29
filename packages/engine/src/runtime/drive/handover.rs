//! 完整静点移交按 AccountId 捕获真实权威；新布局不解释旧 Owner 坐标。
use super::*;
use std::collections::BTreeMap;

pub(crate) struct RuntimeHandoverCapture {
    pub(crate) information: super::super::world::CapturedInformationHandover,
    pub(crate) accounts: BTreeMap<AccountId, super::super::group::GroupAccount>,
    pub(crate) personals:
        BTreeMap<AccountId, Option<super::super::world::BeliefParticipantSnapshot>>,
}

impl RuntimeDriver {
    #[cfg(test)]
    pub(crate) fn inject_handover_stop_failure_for_test(&self) {
        self.world
            .inject_stop_failure_for_test()
            .expect("移交尾真实 worker 失败握手");
    }

    pub(crate) fn capture_runtime_handover(&mut self) -> Result<RuntimeHandoverCapture, String> {
        let information = self
            .world
            .capture_information_handover()
            .map_err(|error| error.to_string())?;
        let accounts = self.current_accounts_by_id()?;
        let personals = self
            .world
            .collect_personal_full()
            .map_err(|error| error.to_string())?
            .groups
            .into_iter()
            .flat_map(|(_, rows)| rows.into_iter().map(|(_, owner, value)| (owner, value)))
            .collect();
        if information.source != self.mirror_source() {
            return Err("完整 handover 捕获期间 World 来源改变".to_owned());
        }
        Ok(RuntimeHandoverCapture {
            information,
            accounts,
            personals,
        })
    }

    fn current_accounts_by_id(
        &mut self,
    ) -> Result<BTreeMap<AccountId, super::super::group::GroupAccount>, String> {
        let mut result = BTreeMap::new();
        let groups = self
            .world
            .snapshot_accounts()
            .map_err(|error| error.to_string())?;
        for (group, rows) in groups {
            for (local, value) in rows.into_iter().enumerate() {
                if let Some(owner) = self.account_by_owner((group, local as _)) {
                    if result.insert(owner, value).is_some() {
                        return Err("完整账户移交重复 AccountId".to_owned());
                    }
                } else if value
                    != super::super::group::GroupAccount::new(0, self.world_public_views().len())
                {
                    return Err("完整账户移交遇到非空 padding".to_owned());
                }
            }
        }
        if result.len() != self.id_map.len() {
            return Err("完整账户移交缺少真实名册成员".to_owned());
        }
        Ok(result)
    }

    pub(crate) fn seed_prepared_runtime_handover(
        &mut self,
        captured: &RuntimeHandoverCapture,
    ) -> Result<(), String> {
        let mut accounts = self.current_accounts_by_id()?;
        let mut personals: BTreeMap<_, _> = self
            .world
            .collect_personal_full_for_prepared()
            .map_err(|error| error.to_string())?
            .groups
            .into_iter()
            .flat_map(|(_, rows)| rows.into_iter().map(|(_, owner, value)| (owner, value)))
            .collect();
        let mut attention = BTreeMap::new();
        for (group, rows) in self
            .world
            .collect_group_attention()
            .map_err(|error| error.to_string())?
        {
            for (local, due, encoded_seed) in rows {
                let owner = self
                    .account_by_owner((group, local))
                    .ok_or("prepared profile 缺少真实 owner")?;
                // EchoAttention 的编码仅供 restore；初值移交要保存实际已推进的内部 RNG。
                let mix = u64::from(group)
                    .wrapping_mul(0x9E3779B97F4A7C15)
                    .wrapping_add(u64::from(local));
                attention.insert(owner, (due, encoded_seed ^ mix));
            }
        }
        for (&owner, value) in &captured.accounts {
            let slot = accounts.get_mut(&owner).ok_or("新名册遗漏原账户")?;
            *slot = value.clone();
        }
        for (&owner, value) in &captured.personals {
            let slot = personals.get_mut(&owner).ok_or("新名册遗漏原个人态")?;
            *slot = value.clone();
        }
        for (&owner, &value) in &captured.information.profiles {
            let slot = attention.get_mut(&owner).ok_or("新名册遗漏原 profile")?;
            *slot = value;
        }
        self.world
            .install_initial_information_handover_for_prepared(
                &captured.information,
                &accounts,
                &personals,
                &attention,
            )
            .map_err(|error| error.to_string())
    }
}
