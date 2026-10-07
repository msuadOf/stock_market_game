use super::*;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, ts_rs::TS)]
#[serde(transparent)]
#[ts(export)]
pub struct OpaqueSubjectId(String);

impl<'de> serde::Deserialize<'de> for OpaqueSubjectId {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl OpaqueSubjectId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn new(value: String) -> Result<Self, MembershipError> {
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(MembershipError::InvalidSubject);
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct AdmissionFunding {
    pub external_cash: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct MarketMembership {
    #[serde(with = "account_id_decimal")]
    #[ts(type = "string")]
    pub account_id: AccountId,
    pub admission_funding: AdmissionFunding,
}

pub(in crate::session) mod account_id_decimal {
    use crate::AccountId;

    pub fn serialize<Serializer>(
        value: &AccountId,
        serializer: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error>
    where
        Serializer: serde::Serializer,
    {
        super::super::canonical_u64_decimal::serialize(&value.0, serializer)
    }

    pub fn deserialize<'de, Deserializer>(
        deserializer: Deserializer,
    ) -> Result<AccountId, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        super::super::canonical_u64_decimal::deserialize(deserializer).map(AccountId)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct MarketMembershipState {
    pub members: BTreeMap<OpaqueSubjectId, MarketMembership>,
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum MembershipError {
    #[error("认证主体无效")]
    InvalidSubject,
    #[error("当前市场没有该主体的成员关系，请确认重新加入；确认后按当前设置发放一次入场资金")]
    RejoinConfirmationRequired,
    #[error("市场创建者只能在新局初始状态绑定一次")]
    CreatorAlreadyBound,
    #[error("入场资金不能为负数")]
    NegativeAdmissionCash,
    #[error("市场账户身份空间已耗尽")]
    AccountIdExhausted,
    #[error("市场已经停止，无法加入或修改入场资金：{0}")]
    MarketUnhealthy(String),
    #[error("加入市场的共享收件入口登记失败：{0}")]
    IngressRegistration(String),
}

impl MarketMembershipState {
    pub(super) fn local_owner(cash: Money) -> Self {
        Self {
            members: BTreeMap::from([(
                OpaqueSubjectId("local-owner".into()),
                MarketMembership {
                    account_id: AccountId(0),
                    admission_funding: AdmissionFunding {
                        external_cash: cash,
                    },
                },
            )]),
        }
    }

    pub(super) fn validate(&self, save: &SaveSlot) -> Result<(), SessionError> {
        let npc_count = u64::from(save.setup.npcs.retail_count)
            + u64::from(save.setup.npcs.inst_count)
            + u64::from(save.setup.npcs.hot_count);
        let mut accounts = BTreeSet::new();
        for (subject, member) in &self.members {
            OpaqueSubjectId::new(subject.0.clone())
                .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
            if member.admission_funding.external_cash.cents() < 0
                || !accounts.insert(member.account_id)
                || !save.snapshot.accounts.contains_key(&member.account_id)
                || (member.account_id.0 > 0 && member.account_id.0 <= npc_count)
            {
                return Err(SessionError::InvalidSave(
                    "市场成员账户或AdmissionFunding事实无效".into(),
                ));
            }
        }
        if !accounts.contains(&AccountId(0)) {
            return Err(SessionError::InvalidSave(
                "初始账户0缺少经济成员关系".into(),
            ));
        }
        let mut expected: BTreeSet<_> = accounts
            .into_iter()
            .chain((1..=npc_count).map(AccountId))
            .collect();
        if save.setup.issuer_repurchase_enabled {
            // 发行人回购专用账户：开关开启时确定性创建于 NPC 序列之后。
            expected.insert(AccountId(npc_count.saturating_add(1)));
        }
        if expected != save.snapshot.accounts.keys().copied().collect() {
            return Err(SessionError::InvalidSave(
                "账户集合必须恰好为NPC与市场成员账户".into(),
            ));
        }
        Ok(())
    }
}

impl GameSession {
    pub fn trading_subject_accounts(&self) -> BTreeMap<OpaqueSubjectId, AccountId> {
        self.state
            .memberships
            .members
            .iter()
            .map(|(subject, member)| (subject.clone(), member.account_id))
            .collect()
    }
    pub fn current_setup(&self) -> &SessionSetup {
        &self.state.setup
    }

    pub fn initial_seed(&self) -> u64 {
        self.state.seed
    }
    pub fn bind_market_creator(
        &mut self,
        subject: OpaqueSubjectId,
    ) -> Result<MarketMembership, MembershipError> {
        if !self.fresh_initial_allocation
            || self.state.memberships.members.len() != 1
            || self.tick() != 0
        {
            return Err(MembershipError::CreatorAlreadyBound);
        }
        let local = OpaqueSubjectId("local-owner".into());
        let member = self
            .state
            .memberships
            .members
            .remove(&local)
            .ok_or(MembershipError::CreatorAlreadyBound)?;
        self.state
            .memberships
            .members
            .insert(subject, member.clone());
        Ok(member)
    }

    pub fn market_membership(
        &self,
        subject: &OpaqueSubjectId,
    ) -> Result<&MarketMembership, MembershipError> {
        self.state
            .memberships
            .members
            .get(subject)
            .ok_or(MembershipError::RejoinConfirmationRequired)
    }

    pub fn resolve_trading_account(
        &self,
        subject: &OpaqueSubjectId,
    ) -> Result<AccountId, MembershipError> {
        Ok(self.market_membership(subject)?.account_id)
    }

    pub fn set_admission_cash(&mut self, cash: Money) -> Result<(), MembershipError> {
        self.require_healthy()
            .map_err(|error| MembershipError::MarketUnhealthy(error.to_string()))?;
        if cash.cents() < 0 {
            return Err(MembershipError::NegativeAdmissionCash);
        }
        self.state.setup.config.starting_cash = cash;
        Ok(())
    }

    pub fn join_market(
        &mut self,
        subject: OpaqueSubjectId,
        confirmed_rejoin: bool,
    ) -> Result<MarketMembership, MembershipError> {
        self.require_healthy()
            .map_err(|error| MembershipError::MarketUnhealthy(error.to_string()))?;
        if let Some(member) = self.state.memberships.members.get(&subject) {
            return Ok(member.clone());
        }
        if !self.fresh_initial_allocation && !confirmed_rejoin {
            return Err(MembershipError::RejoinConfirmationRequired);
        }
        let account_id = AccountId(
            self.state
                .accounts
                .keys()
                .next_back()
                .expect("市场始终保留创建者账户")
                .0
                .checked_add(1)
                .ok_or(MembershipError::AccountIdExhausted)?,
        );
        let cash = self.state.setup.config.starting_cash;
        let member = MarketMembership {
            account_id,
            admission_funding: AdmissionFunding {
                external_cash: cash,
            },
        };
        if let Some(binding) = &self.ingress {
            binding
                .source
                .register_player(account_id)
                .map_err(|error| MembershipError::IngressRegistration(error.to_string()))?;
        }
        self.state.accounts.insert(
            account_id,
            Account::new(account_id, AccountKind::Player, cash),
        );
        self.state.history_reads.insert(
            account_id,
            crate::experience::PersonalHistoryReadLedger::default(),
        );
        self.state
            .memberships
            .members
            .insert(subject, member.clone());
        Ok(member)
    }
}
