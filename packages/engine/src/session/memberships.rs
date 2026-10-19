//! 市场成员关系（B3 远程登录批次取回自 18725d07）。
//!
//! 共享市场的经济成员事实：每个认证主体（`OpaqueSubjectId`）对应一个玩家
//! 账户与一次性入场资金。单人本地局恒有 `local-owner` 成员（账户 0）。
//!
//! B3 适配注记（与 18725d07 的差异，均为 main std-thread 形态的显式对齐）：
//! - `SessionSetup.issuer_repurchase_enabled`（发行人回购专户）随公司系统
//!   退役，`validate` 的回购专户分支本批裁剪；公司线重集成（B4b）恢复
//!   setup 字段后一并恢复该分支。
//! - `SessionError::InvalidSave` 在 main 为 `ValidationFault`（数值码）形态，
//!   成员事实校验失败统一映射 fault2020（快照账户集合与 setup 不完全一致
//!   的多玩家扩展语义）。
//! - `MarketMembership.account_id` 维持 canonical u64 十进制字符串 serde
//!   （18725d07 存档契约，全 u64 域）；main 的 `AccountId` 全局 serde 仍是
//!   js_safe number（上限 2^53-1）。成员账户号由 join_market 顺序递增分配，
//!   正常运行远低于两域边界；若真越界，账户域序列化显式报错而非静默截断。

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

    /// 存档成员事实校验（fault2020 域：账户集合必须恰好为 NPC 与市场成员账户）。
    pub(super) fn validate(&self, save: &SaveSlot) -> Result<(), SessionError> {
        let npc_count = u64::from(save.setup.npcs.retail_count)
            + u64::from(save.setup.npcs.inst_count)
            + u64::from(save.setup.npcs.hot_count);
        let mut accounts = BTreeSet::new();
        for (subject, member) in &self.members {
            if OpaqueSubjectId::new(subject.0.clone()).is_err() {
                // fault2020：成员主体标识非法（空串或含控制字符）。
                return Err(SessionError::InvalidSave(
                    crate::error::ValidationFault::bare(2020),
                ));
            }
            if member.admission_funding.external_cash.cents() < 0
                || !accounts.insert(member.account_id)
                || !save.snapshot.accounts.contains_key(&member.account_id)
                || (member.account_id.0 > 0 && member.account_id.0 <= npc_count)
            {
                // fault2020：市场成员账户或入场资金事实无效（负入场资金/
                // 重复经济账户/快照缺失成员账户/成员账户占用 NPC 区间）。
                return Err(SessionError::InvalidSave(
                    crate::error::ValidationFault::new(2020, vec![member.account_id.0, npc_count]),
                ));
            }
        }
        if !accounts.contains(&AccountId(0)) {
            // fault2020：初始账户 0 缺少经济成员关系。
            return Err(SessionError::InvalidSave(
                crate::error::ValidationFault::bare(2020),
            ));
        }
        let expected: BTreeSet<_> = accounts
            .into_iter()
            .chain((1..=npc_count).map(AccountId))
            .collect();
        // B3 裁剪注记：18725d07 在此按 setup.issuer_repurchase_enabled 追加
        // 发行人回购专户（NPC 序列后确定性账户）；main 现状无公司系统，
        // 该分支随 B4b 恢复 setup 字段后一并恢复。
        if expected != save.snapshot.accounts.keys().copied().collect() {
            // fault2020：账户集合必须恰好为 NPC 与市场成员账户。
            return Err(SessionError::InvalidSave(
                crate::error::ValidationFault::bare(2020),
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
        // std-thread runtime 适配（main 形态）：世界名册（RuntimeDriver 的
        // id_map）在驱动器构造时固化，动态加入的成员账户不在既有名册内、
        // 外部意图路由会显式失败。join_market 是静点（不在 tick 中），此处
        // 丢弃常驻驱动器，下一 step_frame 经 from_session 以新名册重建——
        // 与 runtime_discard_driver_flag（投影段失败丢弃、重试重建）同机制。
        self.runtime_driver = None;
        Ok(member)
    }
}
