use super::super::*;
use crate::error::ValidationFault;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SIMULATION_POLICY_ID: &str = "a-share-simulation";

/// 并行 tick 的权威运行时状态，在 quiet point 以 DTO 保存。
/// 不直接序列化 `EnvelopeLedger`，避免 tick 局部 terminal、守恒行与 local-key
/// 去重证据越过成功提交边界。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedRuntimeState {
    /// poisoned session 不允许生成存档；显式标记让恢复校验能够识别篡改。
    pub poisoned: bool,
    #[serde(with = "super::super::u64_decimal")]
    pub next_receipt_base: u64,
    pub live_envelopes: Vec<SavedLiveEnvelope>,
    /// 单 book 的公开可见性受理截点（数值事实，不复制竞价或连续订单集合）。
    #[serde(with = "super::super::book_sequence_map")]
    pub book_public_cutoff_sequences: BTreeMap<StockId, u64>,
    pub retail_projection_seen: Vec<SavedRetailReceiptIdentity>,
    /// 完整生产策略状态是权威事实；profile 从 enum value 派生，不重复保存。
    pub strategy_states: BTreeMap<AccountId, crate::strategy::StrategyState>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedLiveEnvelope {
    pub key: SavedEnvelopeKey,
    pub charged: SavedFeeComponents,
    /// D5 restore 链最后一层：desk 在途单的审计数值面（此前仅存 charged
    /// ——restore 的 runtime_shape 路径无 desk 权威可查，remaining 退 1 股
    /// 占位使续拍 desk 种子量错）。nominal 不存：由费用表+filled_value
    /// 决定性重算（runtime_step 尾重建同式）。
    pub remaining_qty: u32,
    pub filled_qty: u32,
    pub filled_value: Money,
    pub limit: Money,
    /// 买侧在途 purse（desk 权威占用；卖侧恒 0）——restore 的 live 资源
    /// 真值（此前 1 分占位过 fault2223）。
    pub purse: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedEnvelopeKey {
    pub account: AccountId,
    pub stock: StockId,
    pub order: OrderId,
    pub side: Side,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedFeeComponents {
    pub commission: Money,
    pub stamp_tax: Money,
    pub transfer_fee: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedRetailReceiptIdentity {
    #[serde(with = "super::super::u64_decimal")]
    pub index: u64,
    pub local_key: SavedReceiptLocalKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedReceiptLocalKey {
    pub journal: SavedJournalRank,
    pub source: SavedReceiptSource,
    pub transition: SavedReceiptTransition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SavedJournalRank {
    PreSeal,
    SealedBatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SavedReceiptSource {
    SealedIntent(#[serde(with = "super::super::u64_decimal")] u64),
    QuoteExpiry(u32),
    Auction(u32),
    DayEnd(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedReceiptTransition {
    pub envelope: SavedEnvelopeKey,
    #[serde(with = "super::super::u64_decimal")]
    pub ordinal: u64,
}

/// 从健康且已提交的 quiet point 捕获全部权威运行状态。
pub fn capture_runtime_state(session: &GameSession) -> Result<SavedRuntimeState, StepFatal> {
    session.require_healthy()?;
    if session.state.setup.simulation_policy_id != SIMULATION_POLICY_ID {
        return Err(invariant(format!(
            "simulation_policy_id {:?} 不允许写入当前存档",
            session.state.setup.simulation_policy_id
        )));
    }
    // save 侧完整校验与 restore 侧同源：≥512 行的并行审计必须归入会话 compute
    // registry（capture 入口持有 session，不得落入 caller 当前的池）。
    session
        .state
        .envelope_ledger
        .validate_complete_evidence_in(session.compute_context())?;
    if session.state.envelope_ledger.terminal_count() != 0 {
        return Err(invariant(
            "quiet-point envelope ledger retains terminal tick evidence".to_owned(),
        ));
    }
    if session.state.envelope_ledger.next_receipt_index() != session.state.next_receipt_base {
        return Err(invariant(format!(
            "envelope ledger cursor {} disagrees with next_receipt_base {}",
            session.state.envelope_ledger.next_receipt_index(),
            session.state.next_receipt_base
        )));
    }

    let live_envelopes = session
        .state
        .envelope_ledger
        .iter()
        .map(|(_, envelope)| {
            if envelope.origin() != pipeline::EnvelopeOrigin::TickStart
                || envelope.basis() != envelope.live()
                || envelope.spent() != pipeline::ResVec::ZERO
                || envelope.released() != pipeline::ResVec::ZERO
            {
                return Err(invariant(format!(
                    "envelope {:?} is not rebased at the save quiet point",
                    envelope.key()
                )));
            }
            Ok(SavedLiveEnvelope::from_envelope(envelope))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut rebased_ledger = session.state.envelope_ledger.clone();
    rebased_ledger.rebase_live_for_next_tick()?;
    if rebased_ledger != session.state.envelope_ledger {
        return Err(invariant(
            "save requires a fully rebased envelope-ledger quiet point".to_owned(),
        ));
    }

    let retail_projection_seen = session
        .state
        .retail_projection_seen
        .authoritative_identities()
        .into_iter()
        .map(|(index, key)| {
            Ok(SavedRetailReceiptIdentity {
                index,
                local_key: SavedReceiptLocalKey::from_runtime(&key)?,
            })
        })
        .collect::<Result<Vec<_>, StepFatal>>()?;

    let strategy_states = session
        .state
        .accounts
        .iter()
        .filter_map(|(account, value)| {
            value.strategy().map(|strategy| {
                strategy
                    .production_state()
                    .map(|state| (*account, state))
                    .map_err(|error| invariant(error.to_string()))
            })
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    let state = SavedRuntimeState {
        poisoned: false,
        next_receipt_base: session.state.next_receipt_base,
        live_envelopes,
        book_public_cutoff_sequences: if session.state.desk_book_mirror.is_empty() {
            session
                .state
                .markets
                .iter()
                .map(|(code, market)| (*code, market.auction_public_cutoff_sequence()))
                .collect()
        } else {
            // D5 attention：runtime 形态（desk 镜像在位）——公开截点自
            /// 镜像（desk 权威；session market 恒 0）。
            session
                .state
                .desk_book_mirror
                .iter()
                .map(|(code, mirror)| (*code, mirror.public_cutoff_serial))
                .collect()
        },
        retail_projection_seen,
        strategy_states,
    };
    validate_runtime_state(session, &state).map_err(session_error_as_invariant)?;

    let reconstructed = state
        .build_ledger(session)
        .map_err(session_error_as_invariant)?;
    let actual: Vec<_> = session
        .state
        .envelope_ledger
        .iter()
        .map(|(_, envelope)| envelope.clone())
        .collect();
    let projected: Vec<_> = reconstructed
        .iter()
        .map(|(_, envelope)| envelope.clone())
        .collect();
    if actual != projected {
        return Err(invariant("SavedLiveEnvelope DTO 无法无损恢复".to_owned()));
    }
    Ok(state)
}

/// 将 DTO 与已恢复的订单及确定性账户身份交叉校验；不修改或清空损坏数据。
pub fn validate_runtime_state(
    session: &GameSession,
    state: &SavedRuntimeState,
) -> Result<(), SessionError> {
    if state.poisoned {
        // fault2200：runtime_state 显式拒绝 poisoned 会话状态。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2200)));
    }
    if session.state.setup.simulation_policy_id != SIMULATION_POLICY_ID {
        // fault2201：simulation_policy_id 与引擎政策不符；ctx[0] 待 C 批数值化（原文本 "{:?}" 策略 id）。
        return Err(SessionError::InvalidSave(ValidationFault::new(
            2201,
            vec![0],
        )));
    }

    let expected_markets = session
        .state
        .markets
        .keys()
        .copied()
        .collect::<BTreeSet<_>>();
    if state
        .book_public_cutoff_sequences
        .keys()
        .copied()
        .collect::<BTreeSet<_>>()
        != expected_markets
    {
        return Err(SessionError::InvalidSave(ValidationFault::bare(2290)));
    }
    for (code, cutoff) in &state.book_public_cutoff_sequences {
        let market = &session.state.markets[code];
        // D5 attention：runtime 形态（desk 簿权威——restore 时 session 簿
        /// 空、next_sequence=0）——截点不与本簿序对拍（desk 恢复域接管）；
        /// 相位面（连续/盘前=0 截点）仍按 session 相位判。
        if (market.book_next_sequence() > 0 && *cutoff > market.book_next_sequence())
            || (matches!(
                session.phase(),
                TradingPhase::Continuous | TradingPhase::PreOpen
            ) && *cutoff != 0)
        {
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2291,
                vec![u64::from(code.0), *cutoff, market.book_next_sequence()],
            )));
        }
    }
    let ledger = state.build_ledger(session)?;
    validate_live_envelopes_against_orders(session, state, &ledger)?;

    let identities = state
        .retail_projection_seen
        .iter()
        .map(|identity| {
            validate_receipt_identity_domain(session, identity)?;
            identity
                .local_key
                .to_runtime()
                .map(|key| (identity.index, key))
        })
        .collect::<Result<Vec<_>, _>>()?;
    // fault2202：散户投影回执前缀重建失败；ctx[0] 待 C 批数值化
    // （原文 "saved retail projection receipt prefix is invalid: {error}"）。
    let seen = pipeline::RetailProjectionSeen::from_authoritative_identities(
        identities,
        state.next_receipt_base,
    )
    .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2202, vec![0])))?;
    // fault2203：散户投影规范身份转换失败；ctx[0] 待 C 批数值化
    // （原文 "saved retail projection canonical identity is invalid: {error}"）。
    let canonical = seen
        .authoritative_identities()
        .into_iter()
        .map(|(index, local_key)| {
            SavedReceiptLocalKey::from_runtime(&local_key)
                .map(|local_key| SavedRetailReceiptIdentity { index, local_key })
                .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2203, vec![0])))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if canonical.as_slice() != state.retail_projection_seen.as_slice() {
        // fault2204：散户投影身份非规范序。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2204)));
    }

    let expected_strategy_accounts: BTreeSet<_> = session
        .state
        .accounts
        .iter()
        .filter_map(|(account, value)| value.strategy().map(|_| *account))
        .collect();
    let saved_strategy_accounts: BTreeSet<_> = state.strategy_states.keys().copied().collect();
    if saved_strategy_accounts != expected_strategy_accounts {
        // fault2205：策略状态账户集合与 NPC 账户不精确一致。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2205)));
    }
    for (account, saved_state) in &state.strategy_states {
        let expected_profile = session
            .state
            .accounts
            .get(account)
            .ok_or_else(|| {
                // fault2206：策略状态引用缺失账户；ctx[0]=AccountId。
                SessionError::InvalidSave(ValidationFault::new(2206, vec![account.0]))
            })?
            .strategy()
            .ok_or_else(|| {
                // fault2207：策略状态引用无策略账户；ctx[0]=AccountId。
                SessionError::InvalidSave(ValidationFault::new(2207, vec![account.0]))
            })?
            .profile();
        if saved_state.profile() != expected_profile {
            // fault2208：策略状态改变确定性策略身份；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2208,
                vec![account.0],
            )));
        }
        let attention_probability = session
            .state
            .npc_attention
            .get(account)
            .ok_or_else(|| {
                // fault2209：策略状态账户无 NPC 关注状态；ctx[0]=AccountId。
                SessionError::InvalidSave(ValidationFault::new(2209, vec![account.0]))
            })?
            .base_probability;
        if saved_state.base_observation_probability().to_bits() != attention_probability.to_bits() {
            // fault2210：基础观察概率与 NPC 关注概率不一致；ctx[0]=AccountId，
            // ctx[1]/ctx[2]=两侧概率 IEEE754 位模式。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2210,
                vec![
                    account.0,
                    saved_state.base_observation_probability().to_bits(),
                    attention_probability.to_bits(),
                ],
            )));
        }
        // fault2211：策略状态重建失败；ctx[0]=AccountId，ctx[1] 待 C 批数值化（原文本 "{error}"）。
        saved_state.clone().into_strategy().map_err(|_| {
            SessionError::InvalidSave(ValidationFault::new(2211, vec![account.0, 0]))
        })?;
    }
    Ok(())
}

fn validate_receipt_identity_domain(
    session: &GameSession,
    identity: &SavedRetailReceiptIdentity,
) -> Result<(), SessionError> {
    let envelope = &identity.local_key.transition.envelope;
    if !session.state.accounts.contains_key(&envelope.account) {
        // fault2212：回执引用未知账户；ctx[0]=回执 index，ctx[1]=AccountId。
        return Err(SessionError::InvalidSave(ValidationFault::new(
            2212,
            vec![identity.index, envelope.account.0],
        )));
    }
    if !session.state.markets.contains_key(&envelope.stock) {
        // fault2213：回执引用未知股票；ctx[0]=回执 index，ctx[1]=StockId。
        return Err(SessionError::InvalidSave(ValidationFault::new(
            2213,
            vec![identity.index, u64::from(envelope.stock.0)],
        )));
    }
    if envelope.order.0 == 0 || envelope.order.0 >= session.state.next_order_id {
        // fault2214：回执订单 id 越界；ctx[0]=回执 index，ctx[1]=订单 id，ctx[2]=next_order_id 上界。
        return Err(SessionError::InvalidSave(ValidationFault::new(
            2214,
            vec![
                identity.index,
                envelope.order.0,
                session.state.next_order_id,
            ],
        )));
    }
    Ok(())
}

/// 基础 SaveSlot 校验完成后，原子恢复全部权威运行状态。
pub fn restore_runtime_state(
    session: &mut GameSession,
    state: &SavedRuntimeState,
) -> Result<(), SessionError> {
    validate_runtime_state(session, state)?;
    let ledger = state.build_ledger(session)?;
    let identities = state
        .retail_projection_seen
        .iter()
        .map(|identity| {
            identity
                .local_key
                .to_runtime()
                .map(|key| (identity.index, key))
        })
        .collect::<Result<Vec<_>, _>>()?;
    // fault2202：散户投影回执前缀重建失败（同码复用）；ctx[0] 待 C 批数值化。
    let seen = pipeline::RetailProjectionSeen::from_authoritative_identities(
        identities,
        state.next_receipt_base,
    )
    .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2202, vec![0])))?;
    let strategies = state
        .strategy_states
        .iter()
        .map(|(account, saved)| {
            saved
                .clone()
                .into_strategy()
                .map(crate::account::StoredStrategy::production)
                .map(|strategy| (*account, strategy))
                // fault2211：策略状态重建失败（同码复用）；ctx[0]=AccountId，ctx[1] 待 C 批数值化。
                .map_err(|_| {
                    SessionError::InvalidSave(ValidationFault::new(2211, vec![account.0, 0]))
                })
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    let mut accounts = session
        .state
        .accounts
        .iter()
        .map(|(account, value)| {
            value
                .clone_for_shadow()
                .map(|value| (*account, value))
                // fault2215：账户影子克隆失败，无法原子恢复策略；ctx[0]=AccountId，ctx[1] 待 C 批数值化（原文本 "{error}"）。
                .map_err(|_| {
                    SessionError::InvalidSave(ValidationFault::new(2215, vec![account.0, 0]))
                })
        })
        .collect::<Result<AccountBook, _>>()?;
    for (account, strategy) in strategies {
        accounts
            .get_mut(&account)
            .ok_or_else(|| {
                // fault2206：策略状态引用缺失账户（同码复用）；ctx[0]=AccountId。
                SessionError::InvalidSave(ValidationFault::new(2206, vec![account.0]))
            })?
            .restore_strategy(Some(strategy));
    }
    for (code, cutoff) in &state.book_public_cutoff_sequences {
        session
            .state
            .markets
            .get_mut(code)
            .expect("完整键集已校验")
            .restore_auction_public_cutoff_sequence(*cutoff)
            .map_err(|_| {
                SessionError::InvalidSave(ValidationFault::new(
                    2291,
                    vec![u64::from(code.0), *cutoff],
                ))
            })?;
        // D5：镜像档的占位条目带上截点（save 侧镜像分支读
        /// mirror.public_cutoff_serial——往返一致性；desk 首拍重建）。
        if let Some(mirror) = session.state.desk_book_mirror.get_mut(code) {
            mirror.public_cutoff_serial = *cutoff;
        }
    }
    session.state.accounts = accounts;
    session.state.envelope_ledger = ledger;
    session.state.retail_projection_seen = seen;
    session.state.next_receipt_base = state.next_receipt_base;
    // restore重建的raw行没有serde proof。全部既有类型化fault（2216–2228）已按原优先级
    // 执行完毕后才在此建立每订单自己的证明；本循环不得先于任何typed检查触发。
    let config = session.state.setup.config.clone();
    for (key, envelope) in session.state.envelope_ledger.iter() {
        // D5 runtime 重建形态（build_ledger 的 saved-only 路径——
        /// nominal/limit 零标记）：desk 权威数据不在 restore 侧——证明
        /// 由下一 runtime_step 的重建（desk 权威 purse/费用）建立，此处
        /// 跳过（登记差异：restore 后首拍前的证明空窗）。
        let runtime_rebuilt = envelope.audit().nominal == pipeline::FeeComponents::ZERO
            && envelope.audit().limit == crate::money::Money::ZERO;
        if runtime_rebuilt {
            continue;
        }
        envelope.verified_live(&config).map_err(|_| {
            // fault2220：restore末尾财务证明建立失败（同码复用压平）；ctx为订单身份。
            SessionError::InvalidSave(ValidationFault::new(
                2220,
                vec![key.account.0, u64::from(key.stock.0), key.order.0],
            ))
        })?;
    }
    Ok(())
}

impl SavedRuntimeState {
    fn build_ledger(
        &self,
        session: &GameSession,
    ) -> Result<pipeline::EnvelopeLedger, SessionError> {
        // fault2216：活跃委托投影子错误压平；ctx[0] 待 C 批数值化
        // （原文 "saved live order projection is invalid: {error}"）。
        let projected = session
            .project_live_envelopes()
            .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2216, vec![0])))?
            .into_iter()
            .map(|envelope| (SavedEnvelopeKey::from_runtime(envelope.key()), envelope))
            .collect::<BTreeMap<_, _>>();
        let mut previous = None;
        let mut envelopes = Vec::with_capacity(self.live_envelopes.len());
        // D5 runtime 形态：restore 后旧簿恒空（desk 权威）——legacy 空
        // 且账本空时按 saved 自键重建（live 资源=remaining×price 买/余量
        // 股卖——runtime_step 尾重建的同式近似）。
        let runtime_shape = projected.is_empty()
            && session
                .state
                .markets
                .values()
                .all(|market| market.resting_order_count() == 0);
        for saved in &self.live_envelopes {
            if previous.as_ref().is_some_and(|key| key >= &saved.key) {
                // fault2217：活跃信封非规范键严格序。
                return Err(SessionError::InvalidSave(ValidationFault::bare(2217)));
            }
            previous = Some(saved.key.clone());
            if runtime_shape {
                let key = saved.key.to_runtime();
                // live 资源真值（买=purse 占用、卖=余量股数——与
                /// runtime_step 尾重建的 EchoResting 同源；此前 1 股/1 分
                /// 占位使 restore 后 desk 种子与公开口径失真）。
                let (cash, shares) = if key.side == Side::Buy {
                    (saved.purse, 0u32)
                } else {
                    (crate::money::Money::ZERO, saved.remaining_qty)
                };
                // nominal 自费用表重算（决定性——与 runtime_step 尾重建
                /// 同式；restore 尾的 verified_live 证明因此可建立，证明
                /// 空窗登记就此闭合）。
                let nominal = crate::runtime::fees::FeeQuote::nominal(
                    &session.state.setup.config,
                    key.side,
                    saved.filled_value,
                )
                .map_err(|_| {
                    SessionError::InvalidSave(ValidationFault::new(2219, vec![0]))
                })?;
                let audit = pipeline::EnvelopeAudit {
                    limit: saved.limit,
                    remaining_qty: saved.remaining_qty,
                    filled_qty: saved.filled_qty,
                    filled_value: saved.filled_value,
                    nominal: pipeline::FeeComponents {
                        commission: nominal.commission,
                        stamp_tax: nominal.stamp_tax,
                        transfer_fee: nominal.transfer_fee,
                    },
                    charged: saved.charged.to_runtime(),
                };
                let envelope =
                    pipeline::Envelope::tick_start_existing(key, cash, shares, audit);
                envelope
                    .validate()
                    .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2222, vec![0])))?;
                envelopes.push(envelope);
                continue;
            }
            let order_envelope = projected.get(&saved.key).ok_or_else(|| {
                // fault2218：活跃信封无匹配在场委托；ctx[0]=AccountId，ctx[1]=StockId，
                // ctx[2]=订单 id，ctx[3]=方向（Buy=0/Sell=1）。
                SessionError::InvalidSave(ValidationFault::new(
                    2218,
                    vec![
                        saved.key.account.0,
                        u64::from(saved.key.stock.0),
                        saved.key.order.0,
                        side_rank(saved.key.side),
                    ],
                ))
            })?;
            envelopes.push(saved.to_envelope(order_envelope)?);
        }
        // fault2219：信封账本重建子错误压平；ctx[0] 待 C 批数值化
        // （原文 "saved live envelope ledger is invalid: {error}"）。
        let ledger = pipeline::EnvelopeLedger::new(self.next_receipt_base, envelopes)
            .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2219, vec![0])))?;
        // fault2220：信封账本完整证据校验子错误压平；ctx[0] 待 C 批数值化
        // （原文 "saved live envelope ledger evidence is invalid: {error}"）。
        // 重建的 ledger 是全新缓存，≥512 行时必须走未缓存并行审计：使用
        // session 自身的 compute 上下文（restore_with_compute 与 save 路径同源），
        // 不落入 caller 当前的 global/foreign registry。
        ledger
            .validate_complete_evidence_in(session.compute_context())
            .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2220, vec![0])))?;
        Ok(ledger)
    }
}

impl SavedLiveEnvelope {
    fn from_envelope(envelope: &pipeline::Envelope) -> Self {
        let audit = envelope.audit();
        Self {
            key: SavedEnvelopeKey::from_runtime(envelope.key()),
            charged: SavedFeeComponents::from_runtime(audit.charged),
            remaining_qty: audit.remaining_qty,
            filled_qty: audit.filled_qty,
            filled_value: audit.filled_value,
            limit: audit.limit,
            purse: envelope.live().cash,
        }
    }

    fn to_envelope(
        &self,
        projected: &pipeline::Envelope,
    ) -> Result<pipeline::Envelope, SessionError> {
        if projected.key() != &self.key.to_runtime() {
            // fault2221：信封与在场委托身份不一致；ctx[0]=AccountId，ctx[1]=StockId，
            // ctx[2]=订单 id，ctx[3]=方向（Buy=0/Sell=1）。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2221,
                vec![
                    self.key.account.0,
                    u64::from(self.key.stock.0),
                    self.key.order.0,
                    side_rank(self.key.side),
                ],
            )));
        }
        let mut audit = projected.audit();
        audit.charged = self.charged.to_runtime();
        let envelope = pipeline::Envelope::tick_start_existing(
            self.key.to_runtime(),
            projected.live().cash,
            projected.live().shares,
            audit,
        );
        // fault2222：活跃信封校验子错误压平；ctx[0] 待 C 批数值化
        // （原文 "saved live envelope is invalid: {error}"）。
        envelope
            .validate()
            .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2222, vec![0])))?;
        if envelope.live() == pipeline::ResVec::ZERO {
            // fault2223：活跃信封无在途资源；ctx[0]=AccountId，ctx[1]=StockId，
            // ctx[2]=订单 id，ctx[3]=方向（Buy=0/Sell=1）。
            let key = envelope.key();
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2223,
                vec![
                    key.account.0,
                    u64::from(key.stock.0),
                    key.order.0,
                    side_rank(key.side),
                ],
            )));
        }
        Ok(envelope)
    }
}

impl SavedEnvelopeKey {
    fn from_runtime(key: &pipeline::EnvelopeKey) -> Self {
        Self {
            account: key.account,
            stock: key.stock.clone(),
            order: key.order,
            side: key.side,
        }
    }

    fn to_runtime(&self) -> pipeline::EnvelopeKey {
        pipeline::EnvelopeKey {
            account: self.account,
            stock: self.stock.clone(),
            order: self.order,
            side: self.side,
        }
    }
}

impl Ord for SavedEnvelopeKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let side_rank = |side: Side| match side {
            Side::Buy => 0_u8,
            Side::Sell => 1_u8,
        };
        (
            &self.account,
            &self.stock,
            &self.order,
            side_rank(self.side),
        )
            .cmp(&(
                &other.account,
                &other.stock,
                &other.order,
                side_rank(other.side),
            ))
    }
}

impl PartialOrd for SavedEnvelopeKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl SavedFeeComponents {
    fn from_runtime(fees: pipeline::FeeComponents) -> Self {
        Self {
            commission: fees.commission,
            stamp_tax: fees.stamp_tax,
            transfer_fee: fees.transfer_fee,
        }
    }

    fn to_runtime(self) -> pipeline::FeeComponents {
        pipeline::FeeComponents {
            commission: self.commission,
            stamp_tax: self.stamp_tax,
            transfer_fee: self.transfer_fee,
        }
    }

    fn total(self) -> Result<Money, SessionError> {
        // fault2224：费用审计合计子错误压平；ctx[0] 待 C 批数值化
        // （原文 "saved fee audit total is invalid: {error}"）。
        self.commission
            .add(self.stamp_tax)
            .and_then(|subtotal| subtotal.add(self.transfer_fee))
            .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2224, vec![0])))
    }

    fn validate_nonnegative(self, _label: &str) -> Result<(), SessionError> {
        if [self.commission, self.stamp_tax, self.transfer_fee]
            .into_iter()
            .any(|amount| amount.cents() < 0)
        {
            // fault2225：费用审计含负分量；ctx[0] 待 C 批数值化（原文本 "{label}"：nominal/charged）。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2225,
                vec![0],
            )));
        }
        Ok(())
    }
}

fn validate_live_envelopes_against_orders(
    session: &GameSession,
    state: &SavedRuntimeState,
    ledger: &pipeline::EnvelopeLedger,
) -> Result<(), SessionError> {
    // fault2216：活跃委托投影子错误压平（同码复用）；ctx[0] 待 C 批数值化。
    let expected = session
        .project_live_envelopes()
        .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2216, vec![0])))?
        .into_iter()
        .map(|envelope| (envelope.key().clone(), envelope))
        .collect::<BTreeMap<_, _>>();
    let actual = ledger
        .iter()
        .map(|(key, envelope)| (key.clone(), envelope))
        .collect::<BTreeMap<_, _>>();
    // D5 runtime 形态：旧簿恒空（desk 权威）——expected 空而 actual==saved
    // 时为合法（在场委托面=desk，restore 侧由 build_ledger 的 saved 重建
    // 路径承载）。
    let runtime_shape = expected.is_empty() && actual.len() == state.live_envelopes.len();
    if !runtime_shape
        && (actual.len() != state.live_envelopes.len() || actual.len() != expected.len())
    {
        // fault2226：活跃信封集合与在场委托所有权不精确一致。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2226)));
    }
    if runtime_shape {
        return Ok(());
    }
    for key in expected.keys() {
        let saved = actual.get(key).ok_or_else(|| {
            // fault2227：在场委托无已存信封；ctx[0]=AccountId，ctx[1]=StockId，
            // ctx[2]=订单 id，ctx[3]=方向（Buy=0/Sell=1）。
            SessionError::InvalidSave(ValidationFault::new(
                2227,
                vec![
                    key.account.0,
                    u64::from(key.stock.0),
                    key.order.0,
                    side_rank(key.side),
                ],
            ))
        })?;
        let saved_audit = saved.audit();
        let audit = CumulativeFeeAudit {
            side: key.side,
            filled_value: saved_audit.filled_value,
            charged: SavedFeeComponents::from_runtime(saved_audit.charged),
        };
        // runtime 形态（desk 簿权威——ledger 非空而旧簿空）分段计费。
        let desk_staged = !session.state.desk_book_mirror.is_empty()
            || (session
                .state
                .markets
                .values()
                .all(|market| market.resting_order_count() == 0)
                && session.state.envelope_ledger.iter().next().is_some());
        if !audit.validate(&session.state.setup.config, desk_staged)? {
            // fault2228：信封累计收费审计不一致；ctx[0]=AccountId，ctx[1]=StockId，
            // ctx[2]=订单 id，ctx[3]=方向（Buy=0/Sell=1）。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2228,
                vec![
                    key.account.0,
                    u64::from(key.stock.0),
                    key.order.0,
                    side_rank(key.side),
                ],
            )));
        }
    }
    Ok(())
}

/// 活动委托的累计收费审计；GameConfig 与 EnvelopeAudit 仍各自拥有费率和运行时事实。
/// 此处只校验累计边界，不能从累计分量重建逐笔收费优先级。
pub(super) struct CumulativeFeeAudit {
    pub(super) side: Side,
    pub(super) filled_value: Money,
    pub(super) charged: SavedFeeComponents,
}

impl CumulativeFeeAudit {
    pub(super) fn nominal(&self, config: &GameConfig) -> Result<SavedFeeComponents, SessionError> {
        if self.filled_value.cents() < 0 {
            // fault2229：已成交金额不可为负。
            return Err(SessionError::InvalidSave(ValidationFault::bare(2229)));
        }
        if self.filled_value == Money::ZERO {
            return Ok(SavedFeeComponents::default());
        }
        Ok(SavedFeeComponents {
            // fault2230：累计佣金计算子错误压平；ctx[0] 待 C 批数值化
            // （原文 "saved cumulative commission is invalid: {error}"）。
            commission: config
                .commission(self.filled_value)
                .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2230, vec![0])))?,
            stamp_tax: match self.side {
                Side::Buy => Money::ZERO,
                // fault2231：累计印花税计算子错误压平；ctx[0] 待 C 批数值化
                // （原文 "saved cumulative stamp tax is invalid: {error}"）。
                Side::Sell => config
                    .stamp_tax(self.filled_value)
                    .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2231, vec![0])))?,
            },
            // fault2232：累计过户费计算子错误压平；ctx[0] 待 C 批数值化
            // （原文 "saved cumulative transfer fee is invalid: {error}"）。
            transfer_fee: config
                .transfer_fee(self.filled_value)
                .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2232, vec![0])))?,
        })
    }

    /// 返回累计分量是否一致，由遍历调用方附加委托身份错误上下文。
    /// `desk_staged`：runtime 形态（desk 分段计费——累计 charged=Σ分笔
    /// nominal，最低佣金下限使 Σ≥累计式 nominal（口径差登记：fault2228
    /// 的 Buy 硬等对分段模型不成立，放宽为分量上界+总额不越成交额）。
    pub(super) fn validate(
        &self,
        config: &GameConfig,
        desk_staged: bool,
    ) -> Result<bool, SessionError> {
        let nominal = self.nominal(config)?;
        let charged = self.charged;
        nominal.validate_nonnegative("nominal")?;
        charged.validate_nonnegative("charged")?;
        if self.side == Side::Buy {
            if !desk_staged {
                return Ok(charged == nominal);
            }
            // 分段计费上界：Σ分笔费用 ∈ [累计式 nominal, nominal+成交额]
            //（min 佣金下限的分段富余不可精确重建——笔数未存；每笔富余
            // ≤最低佣金<费用额+该笔成交额，Σ上界以 nominal+总成交额承载
            //——终审修正：此前 .max(upper) 写法恒真、上界零校验。stamp
            // 买腿恒零；commission/transfer 双边制度面均收）。
            let upper = |component: Money| -> Result<Money, SessionError> {
                component.add(self.filled_value).map_err(|_| {
                    // fault2229 同族：分段上界组装溢出（分量+成交额）。
                    SessionError::InvalidSave(ValidationFault::bare(2229))
                })
            };
            return Ok(charged.stamp_tax == Money::ZERO
                && charged.commission >= nominal.commission
                && charged.commission <= upper(nominal.commission)?
                && charged.transfer_fee >= nominal.transfer_fee
                && charged.transfer_fee <= upper(nominal.transfer_fee)?);
        }
        let component_bounds_hold = charged.commission <= nominal.commission
            && charged.stamp_tax <= nominal.stamp_tax
            && charged.transfer_fee <= nominal.transfer_fee;
        let charged_total = charged.total()?;
        Ok(component_bounds_hold
            && charged_total <= nominal.total()?
            && charged_total <= self.filled_value)
    }
}

impl SavedReceiptLocalKey {
    fn from_runtime(key: &pipeline::ReceiptLocalKey) -> Result<Self, StepFatal> {
        let encoded = serde_json::to_value(key).map_err(|error| {
            invariant(format!(
                "cannot project authoritative receipt local key: {error}"
            ))
        })?;
        let mirror: RuntimeReceiptLocalKey = serde_json::from_value(encoded).map_err(|error| {
            invariant(format!(
                "authoritative receipt local key has an unsupported shape: {error}"
            ))
        })?;
        Ok(mirror.into_saved_receipt_local_key())
    }

    fn to_runtime(&self) -> Result<pipeline::ReceiptLocalKey, SessionError> {
        let journal = match self.journal {
            SavedJournalRank::PreSeal => pipeline::JournalRank::PreSeal,
            SavedJournalRank::SealedBatch => pipeline::JournalRank::SealedBatch,
        };
        let source = match self.source {
            SavedReceiptSource::SealedIntent(value) => pipeline::ReceiptSource::SealedIntent(value),
            SavedReceiptSource::QuoteExpiry(value) => pipeline::ReceiptSource::QuoteExpiry(value),
            SavedReceiptSource::Auction(value) => pipeline::ReceiptSource::Auction(value),
            SavedReceiptSource::DayEnd(value) => pipeline::ReceiptSource::DayEnd(value),
        };
        pipeline::ReceiptLocalKey::new(
            journal,
            source,
            pipeline::ReceiptTransition {
                envelope: self.transition.envelope.to_runtime(),
                ordinal: self.transition.ordinal,
            },
        )
        // fault2233：回执本地键重建子错误压平；ctx[0] 待 C 批数值化
        // （原文 "saved receipt local key is invalid: {error}"）。
        .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2233, vec![0])))
    }
}

#[derive(Deserialize)]
struct RuntimeReceiptLocalKey {
    journal: SavedJournalRank,
    source: RuntimeReceiptSource,
    transition: RuntimeReceiptTransition,
}

#[derive(Deserialize)]
enum RuntimeReceiptSource {
    SealedIntent(u64),
    QuoteExpiry(u32),
    Auction(u32),
    DayEnd(u32),
}

#[derive(Deserialize)]
struct RuntimeReceiptTransition {
    envelope: SavedEnvelopeKey,
    ordinal: u64,
}

impl RuntimeReceiptLocalKey {
    fn into_saved_receipt_local_key(self) -> SavedReceiptLocalKey {
        SavedReceiptLocalKey {
            journal: self.journal,
            source: match self.source {
                RuntimeReceiptSource::SealedIntent(value) => {
                    SavedReceiptSource::SealedIntent(value)
                }
                RuntimeReceiptSource::QuoteExpiry(value) => SavedReceiptSource::QuoteExpiry(value),
                RuntimeReceiptSource::Auction(value) => SavedReceiptSource::Auction(value),
                RuntimeReceiptSource::DayEnd(value) => SavedReceiptSource::DayEnd(value),
            },
            transition: SavedReceiptTransition {
                envelope: self.transition.envelope,
                ordinal: self.transition.ordinal,
            },
        }
    }
}

fn invariant(description: String) -> StepFatal {
    StepFatal::InvariantViolation {
        description,
        location: "session::persistence::saved_runtime".to_owned(),
    }
}

fn session_error_as_invariant(error: SessionError) -> StepFatal {
    invariant(error.to_string())
}

/// 信封方向数值化：Buy=0/Sell=1（与 SavedEnvelopeKey::Ord 的 side_rank 序一致）。
fn side_rank(side: Side) -> u64 {
    match side {
        Side::Buy => 0,
        Side::Sell => 1,
    }
}
