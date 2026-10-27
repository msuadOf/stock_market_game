use super::*;
use crate::runtime::protocol::{
    InformationPrepareError, InformationPreparePayload, PreparedInformationResult,
};
use crate::runtime::world::BeliefParticipantSnapshot;
use crate::StockId;

/// 私有候选尚未安装；before image 连 dirty 与引用一并保留，以限定 abort 作用域。
pub(super) struct PendingInformation {
    pub prepared: PreparedInformationResult,
    before: Vec<(LocalIdx, BeliefParticipantSnapshot, bool)>,
    pending_dirty: Vec<LocalIdx>,
    library: Arc<crate::information::PublicLibrary>,
    facts: Option<Arc<crate::company::CompanyReadFacts>>,
    observed_through: Option<crate::calendar::CivilInstant>,
}
impl GroupState {
    fn information_error(&self, error: InformationPrepareError) -> RuntimeError {
        RuntimeError::InformationPrepare {
            group: self.group,
            error,
        }
    }
    pub(super) fn prepare_information(
        &self,
        payload: &InformationPreparePayload,
        accounts: &BTreeMap<LocalIdx, GroupAccount>,
    ) -> Result<PendingInformation, RuntimeError> {
        let reject = |error| self.information_error(error);
        let mut personals = Vec::new();
        let mut before = Vec::new();
        if let Some(point) = &payload.observation {
            if self
                .information_observed_through
                .is_some_and(|prior| point.through < prior)
            {
                return Err(reject(InformationPrepareError::ObservationOrder));
            }
            if point.company_facts != payload.current_facts {
                return Err(reject(InformationPrepareError::ObservationFactsMismatch));
            }
            if point.market.tick != self.cur_tick {
                return Err(reject(InformationPrepareError::MarketTickMismatch {
                    expected: self.cur_tick,
                    actual: point.market.tick,
                }));
            }
            let projection = self.root_projection.as_ref().ok_or_else(|| {
                reject(InformationPrepareError::Identity {
                    detail: "信息观察缺少名册投影".into(),
                })
            })?;
            if self
                .accounts
                .iter()
                .any(|account| account.shares.len() != projection.catalog.len())
            {
                return Err(reject(InformationPrepareError::Identity {
                    detail: "组资产股票拓扑与构造期目录不同".into(),
                }));
            }
            // 完整拓扑来自构造期目录，不允许 payload 用删键自证完整性。
            let mut issuers = BTreeMap::new();
            let mut specs = BTreeMap::new();
            for (stock, _) in projection.catalog.iter() {
                let company = point
                    .company_facts
                    .issuer_of_stock(stock)
                    .map_err(|e| reject(e.into()))?;
                let spec = point
                    .company_facts
                    .company_spec(&company)
                    .map_err(|e| reject(e.into()))?;
                issuers.insert(stock, company);
                specs.insert(company, spec);
                if !point.as_of_trading_days.contains_key(&stock) {
                    return Err(reject(InformationPrepareError::MissingTradingDay { stock }));
                }
                if !point.market.stocks.contains_key(&stock) {
                    return Err(reject(InformationPrepareError::MissingMarket { stock }));
                }
            }
            for local in 0..self.accounts.len() {
                let slot = self.belief_states.get(local).and_then(Option::as_ref);
                let owner = projection
                    .account_ids
                    .get(self.roster_offset + local)
                    .copied();
                // 尾部 padding 不是真实账户；已有个人行却无 owner 仍须拒绝。
                if owner.is_none() && slot.is_none() {
                    continue;
                }
                let required = projection
                    .chain_params
                    .get(self.roster_offset + local)
                    .is_some_and(Option::is_some)
                    || self.decision.as_ref().is_some_and(|(profiles, _)| {
                        profiles
                            .get(local)
                            .is_some_and(|profile| profile.retail_style.is_some())
                    });
                let Some(snapshot) = slot else {
                    if required {
                        return Err(reject(InformationPrepareError::Identity {
                            detail: format!("NPC local={local} 缺少个人态"),
                        }));
                    }
                    continue;
                };
                let owner = owner.ok_or_else(|| {
                    reject(InformationPrepareError::Identity {
                        detail: format!("local={local} 无名册 AccountId"),
                    })
                })?;
                if snapshot.information.owner() != owner || snapshot.belief.npc() != owner {
                    return Err(reject(InformationPrepareError::Identity {
                        detail: format!("local={local}, AccountId={owner:?} 与个人态属主不同"),
                    }));
                }
                let account = accounts
                    .get(&(local as LocalIdx))
                    .unwrap_or(self.accounts[local].as_ref());
                let mut codes: std::collections::BTreeSet<StockId> = account
                    .shares
                    .iter()
                    .zip(&account.pending_sell_qty)
                    .enumerate()
                    .filter(|(_, (&shares, &pending))| shares > 0 || pending > 0)
                    .map(|(stock, _)| StockId(stock as u32))
                    .collect();
                codes.extend(snapshot.watchlist.stocks.keys().copied());
                codes.extend(self.plan_book.active_investor_codes(owner).copied());
                for stock in &codes {
                    if !issuers.contains_key(stock) {
                        return Err(reject(InformationPrepareError::Identity {
                            detail: format!("local={local} 的关注股票不在 live 目录：{stock:?}"),
                        }));
                    }
                }
                if !snapshot.attention.information_check_due(point.through) {
                    continue;
                }
                let result = crate::session::notices::deliver_information_for_npc(
                    owner,
                    point.through,
                    &|stock| point.as_of_trading_days.get(&stock).copied(),
                    &snapshot.attention,
                    &snapshot.information,
                    &snapshot.belief,
                    codes,
                    &|stock| issuers.get(&stock).copied(),
                    &|company| specs.get(company).copied(),
                    &payload.library,
                    &point.market,
                )
                .map_err(|e| {
                    reject(InformationPrepareError::Revaluation {
                        account: owner,
                        detail: e.to_string(),
                    })
                })?;
                if let Some((_, information, belief, attention)) = result {
                    let dirty = *self.belief_dirty.get(local).ok_or_else(|| {
                        reject(InformationPrepareError::Identity {
                            detail: format!("local={local} 缺少 dirty 槽"),
                        })
                    })?;
                    before.push((local as LocalIdx, snapshot.clone(), dirty));
                    personals.push((
                        local as LocalIdx,
                        owner,
                        BeliefParticipantSnapshot {
                            information,
                            belief,
                            attention,
                            watchlist: snapshot.watchlist.clone(),
                            price_memory: snapshot.price_memory.clone(),
                            history_reads: snapshot.history_reads.clone(),
                            retail_experience: snapshot.retail_experience.clone(),
                        },
                    ));
                }
            }
        }
        Ok(PendingInformation {
            prepared: PreparedInformationResult {
                personals,
                observed_through: payload.observation.as_ref().map(|p| p.through),
                library: Arc::clone(&payload.library),
                current_facts: Arc::clone(&payload.current_facts),
            },
            before,
            pending_dirty: self.belief_pending_dirty.clone(),
            library: Arc::clone(&self.library),
            facts: self.current_company_facts.clone(),
            observed_through: self.information_observed_through,
        })
    }
    pub(super) fn commit_information(&mut self, pending: PendingInformation) {
        for (local, _, snapshot) in pending.prepared.personals {
            self.belief_states[local as usize] = Some(snapshot);
            self.belief_dirty[local as usize] = true;
        }
        if pending.prepared.observed_through.is_some() {
            self.information_observed_through = pending.prepared.observed_through;
        }
        self.library = pending.prepared.library;
        self.current_company_facts = Some(pending.prepared.current_facts);
    }
    pub(super) fn abort_information(&mut self, pending: PendingInformation) {
        for (local, snapshot, dirty) in pending.before {
            self.belief_states[local as usize] = Some(snapshot);
            self.belief_dirty[local as usize] = dirty;
        }
        self.belief_pending_dirty = pending.pending_dirty;
        self.library = pending.library;
        self.current_company_facts = pending.facts;
        self.information_observed_through = pending.observed_through;
    }
}
