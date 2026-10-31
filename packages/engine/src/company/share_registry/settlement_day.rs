//! registry 物理自然日候选；税务日与生产市场的统一安装由上层完整事务负责。

use super::*;
use crate::calendar::{CalendarExchange, TradingCalendar};
use crate::orderbook::Side;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ShareUnitIdentity {
    Initial,
    ReDenominated { movement_event_id: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareDayFrontier {
    pub settled_on: CivilDate,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub receipt_count: u64,
    pub unit: ShareUnitIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpeningMovementRef {
    pub movement_event_id: String,
    pub registration_event_id: String,
    pub source_lots: Vec<Vec<String>>,
    pub from_unit: ShareUnitIdentity,
    pub to_unit: ShareUnitIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareConfirmationRef {
    pub account: AccountId,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub receipt_id: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ShareMarketSource {
    FinalConfirmations {
        receipts: Vec<ShareConfirmationRef>,
    },
    CalendarClosed {
        exchange: CalendarExchange,
        policy_digest: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum SharePostActionRef {
    Movement {
        event_id: String,
    },
    UnitMovement {
        event_id: String,
        from_unit: ShareUnitIdentity,
        to_unit: ShareUnitIdentity,
    },
    CaptureRegistration {
        event_id: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareSettlementBinding {
    pub predecessor: ShareDayFrontier,
    pub opening: Vec<OpeningMovementRef>,
    pub market_unit: ShareUnitIdentity,
    pub market_source: ShareMarketSource,
    pub post: Vec<SharePostActionRef>,
}

/// 非持久化的完整最终确认视图；由上层从权威确认日志与 StockCatalog 归一。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShareMarketConfirmation {
    pub account: AccountId,
    pub receipt_id: u64,
    pub stock: StockCode,
    pub day: CivilDate,
    pub side: Side,
    pub qty: u64,
}

pub struct ShareMarketSourceContext<'a> {
    pub calendar: &'a TradingCalendar,
    pub exchange: CalendarExchange,
    pub confirmations: &'a [ShareMarketConfirmation],
    pub treasury_account: Option<AccountId>,
    pub completed_through: Option<CivilDate>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenOpeningMovement {
    pub reference: OpeningMovementRef,
    pub request: ShareDayRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfirmedShareMarketClose {
    pub unit: ShareUnitIdentity,
    pub source: ShareMarketSource,
    pub request: ShareDayRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SharePostAction {
    Movement(ShareDayRequest),
    UnitMovement {
        request: ShareDayRequest,
        from_unit: ShareUnitIdentity,
        to_unit: ShareUnitIdentity,
    },
    CaptureRegistration {
        event_id: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShareSettlementDay {
    pub stock: StockCode,
    pub day: CivilDate,
    pub predecessor: ShareDayFrontier,
    pub opening: Vec<FrozenOpeningMovement>,
    pub actual_market_close: ConfirmedShareMarketClose,
    pub post_market_actions: Vec<SharePostAction>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PendingOpening {
    pub(super) predecessor: ShareDayFrontier,
    pub(super) opening: FrozenOpeningMovement,
}

impl ShareRegistry {
    pub fn settlement_frontier(&self) -> Result<ShareDayFrontier, ShareRegistryError> {
        if let Some(pending) = &self.pending_opening {
            return Ok(pending.predecessor.clone());
        }
        let unit = self.receipts.iter().rev().find_map(|receipt| {
            matches!(
                receipt.request.scope,
                MovementScope::ShareReDenomination { .. }
            )
            .then(|| ShareUnitIdentity::ReDenominated {
                movement_event_id: receipt.request.event_id.clone(),
            })
        });
        Ok(ShareDayFrontier {
            settled_on: self.settled_on,
            receipt_count: u64::try_from(self.receipts.len())
                .map_err(|_| error("settlement receipt count exceeds u64"))?,
            unit: match unit {
                Some(unit) => unit,
                None => ShareUnitIdentity::Initial,
            },
        })
    }

    pub fn pending_opening(&self) -> Option<&FrozenOpeningMovement> {
        self.pending_opening
            .as_ref()
            .map(|pending| &pending.opening)
    }

    pub fn current_unit(&self) -> ShareUnitIdentity {
        self.unit_at(self.receipts.len())
    }

    pub fn prepare_opening_movement(
        &self,
        predecessor: &ShareDayFrontier,
        opening: &FrozenOpeningMovement,
    ) -> Result<Self, ShareRegistryError> {
        self.validate()?;
        if self.pending_opening.is_some()
            || *predecessor != self.settlement_frontier()?
            || opening.request.day.days_since(self.settled_on) != 1
            || opening.request.event_id != opening.reference.movement_event_id
            || !opening_scope_matches(&opening.request.scope, &opening.reference)
            || !opening_changes_match(&opening.request)
        {
            return Err(error(
                "opening predecessor/day/scope is invalid or already pending",
            ));
        }
        let mut candidate = self.clone();
        candidate.apply_movement(opening.request.clone())?;
        candidate.pending_opening = Some(PendingOpening {
            predecessor: predecessor.clone(),
            opening: opening.clone(),
        });
        candidate.validate()?;
        Ok(candidate)
    }

    pub(super) fn validate_pending_opening(&self) -> Result<(), ShareRegistryError> {
        let Some(pending) = &self.pending_opening else {
            return Ok(());
        };
        let index = usize::try_from(pending.predecessor.receipt_count)
            .map_err(|_| error("opening frontier exceeds usize"))?;
        let opening = &pending.opening;
        if index.checked_add(1) != Some(self.receipts.len())
            || pending.predecessor.settled_on != self.settled_on
            || opening.request.day.days_since(self.settled_on) != 1
            || pending.predecessor.unit != self.unit_at(index)
            || self
                .receipts
                .get(index.wrapping_sub(1))
                .is_some_and(|receipt| receipt.request.day != pending.predecessor.settled_on)
            || self
                .registrations
                .iter()
                .any(|snapshot| snapshot.event_id == opening.request.event_id)
            || opening.request.event_id != opening.reference.movement_event_id
            || !opening_scope_matches(&opening.request.scope, &opening.reference)
            || !opening_changes_match(&opening.request)
            || self.receipts[index].request != opening.request
        {
            return Err(error(
                "pending opening does not bind its predecessor and physical suffix",
            ));
        }
        if matches!(
            opening.request.scope,
            MovementScope::ShareReDenomination { .. }
        ) {
            check_transition(
                &pending.predecessor.unit,
                &opening.reference.from_unit,
                &opening.reference.to_unit,
                &opening.request.event_id,
            )?;
        } else if opening.reference.from_unit != pending.predecessor.unit {
            return Err(error("pending opening unit differs from predecessor"));
        }
        self.validate_frozen_opening(&opening.reference, index, &self.receipts[index])?;
        Ok(())
    }

    pub fn prepare_settlement_day(
        &self,
        request: &ShareSettlementDay,
        sources: &ShareMarketSourceContext<'_>,
    ) -> Result<Self, ShareRegistryError> {
        let prepare = || -> Result<Self, ShareRegistryError> {
            self.validate()?;
            self.validate_market_sources(sources)?;
            if request.stock != self.stock
                || request.actual_market_close.request.day != request.day
                || !matches!(
                    request.actual_market_close.request.scope,
                    MovementScope::PublicMarket
                )
            {
                return Err(error(
                    "stock/day/market scope does not match the typed settlement day",
                ));
            }
            for opening in &request.opening {
                if opening.request.day != request.day
                    || opening.request.event_id != opening.reference.movement_event_id
                    || !opening_scope_matches(&opening.request.scope, &opening.reference)
                    || !opening_changes_match(&opening.request)
                {
                    return Err(error(
                        "opening request does not bind its declared day/phase/movement reference",
                    ));
                }
            }
            let binding = request.binding();
            let mut market = request.actual_market_close.request.clone();
            market.scope = MovementScope::SettlementDayMarket {
                binding: binding.clone(),
            };
            if let Some(stored) = self.receipt_by_event(&market.event_id) {
                if stored.request != market {
                    return Err(error("complete settlement retry changed market, unit, source or action positions"));
                }
                for opening in &request.opening {
                    if self
                        .receipt_by_event(&opening.request.event_id)
                        .is_none_or(|stored| stored.request != opening.request)
                    {
                        return Err(error(
                            "complete settlement retry changed its frozen opening movement",
                        ));
                    }
                }
                for action in &request.post_market_actions {
                    if let Some(movement) = action.movement() {
                        if self
                            .receipt_by_event(&movement.event_id)
                            .is_none_or(|stored| stored.request != *movement)
                        {
                            return Err(error(
                                "complete settlement retry changed a post-market movement",
                            ));
                        }
                    }
                }
                return Ok(self.clone());
            }
            if request.predecessor != self.settlement_frontier()?
                || request.day.days_since(self.settled_on) != 1
            {
                return Err(error(
                    "typed settlement predecessor/frontier/unit or natural day is stale",
                ));
            }
            let mut identities: BTreeSet<String> = self
                .receipts
                .iter()
                .map(|receipt| receipt.request.event_id.clone())
                .chain(
                    self.registrations
                        .iter()
                        .map(|snapshot| snapshot.event_id.clone()),
                )
                .collect();
            let mut candidate = self.clone();
            let already_applied = match &self.pending_opening {
                Some(pending) => {
                    if request.opening.as_slice() != [pending.opening.clone()] {
                        return Err(error("settlement opening differs from pending suffix"));
                    }
                    candidate.pending_opening = None;
                    true
                }
                None => false,
            };
            for opening in &request.opening {
                if already_applied {
                    continue;
                }
                if opening.request.day != request.day
                    || opening.reference.movement_event_id != opening.request.event_id
                    || !opening_scope_matches(&opening.request.scope, &opening.reference)
                    || !identities.insert(opening.request.event_id.clone())
                {
                    return Err(error(
                        "opening movement identity/day/scope is invalid or reused",
                    ));
                }
                candidate.apply_movement(opening.request.clone()).map_err(|cause| error(&format!(
                    "opening={} registration={} from={:?} to={:?} source_groups={:?}: {cause}",
                    opening.reference.movement_event_id, opening.reference.registration_event_id,
                    opening.reference.from_unit, opening.reference.to_unit, opening.reference.source_lots)))?;
            }
            if !identities.insert(market.event_id.clone()) {
                return Err(error("market identity is reused in another phase"));
            }
            candidate.apply_movement(market)?;
            for action in &request.post_market_actions {
                match action {
                    SharePostAction::Movement(movement)
                    | SharePostAction::UnitMovement {
                        request: movement, ..
                    } => {
                        let post_unit = candidate.unit_at(candidate.receipts.len());
                        let correct_scope = match action {
                            SharePostAction::Movement(_) => matches!(
                                movement.scope,
                                MovementScope::NonTradingTransfer { .. }
                                    | MovementScope::IssuerRepurchaseCancellation { .. }
                            ),
                            SharePostAction::UnitMovement { .. } => {
                                matches!(movement.scope, MovementScope::ShareReDenomination { .. })
                            }
                            SharePostAction::CaptureRegistration { .. } => false,
                        };
                        if !correct_scope
                            || movement.day != request.day
                            || !identities.insert(movement.event_id.clone())
                        {
                            return Err(post_movement_error(
                                movement,
                                &post_unit,
                                "post-market movement has wrong scope/day or a reused identity",
                            ));
                        }
                        candidate
                            .apply_movement(movement.clone())
                            .map_err(|cause| {
                                post_movement_error(movement, &post_unit, &cause.to_string())
                            })?;
                    }
                    SharePostAction::CaptureRegistration { event_id } => {
                        if event_id.trim().is_empty() || !identities.insert(event_id.clone()) {
                            return Err(error("capture registration identity is empty or reused"));
                        }
                        candidate.registrations.push(RegistrationSnapshot {
                            event_id: event_id.clone(),
                            stock: candidate.stock.clone(),
                            issuer: candidate.issuer,
                            registered_on: request.day,
                            issued_shares: candidate.issued_shares,
                            settled_receipts: u64::try_from(candidate.receipts.len())
                                .map_err(|_| error("capture receipt count exceeds u64"))?,
                            issuer_repurchase_account: candidate.issuer_repurchase_account.clone(),
                            holdings: candidate.holdings.clone(),
                        });
                    }
                }
            }
            candidate.settled_on = request.day;
            candidate.validate()?;
            candidate.validate_market_sources(sources)?;
            Ok(candidate)
        };
        prepare().map_err(|cause| ShareRegistryError::InvalidFact {
            detail: format!(
                "stock={} day={} market={} phase=complete-day: {cause}",
                request.stock.0, request.day, request.actual_market_close.request.event_id
            ),
        })
    }

    fn unit_at(&self, receipt_count: usize) -> ShareUnitIdentity {
        self.receipts[..receipt_count]
            .iter()
            .rev()
            .find_map(|receipt| {
                matches!(
                    receipt.request.scope,
                    MovementScope::ShareReDenomination { .. }
                )
                .then(|| ShareUnitIdentity::ReDenominated {
                    movement_event_id: receipt.request.event_id.clone(),
                })
            })
            .unwrap_or(ShareUnitIdentity::Initial)
    }

    pub(super) fn validate_settlement_bindings(&self) -> Result<(), ShareRegistryError> {
        let receipt_ids: BTreeSet<_> = self
            .receipts
            .iter()
            .map(|receipt| receipt.request.event_id.as_str())
            .collect();
        let mut typed_movement_ids = BTreeSet::new();
        for receipt in &self.receipts {
            if let MovementScope::SettlementDayMarket { binding } = &receipt.request.scope {
                typed_movement_ids.insert(receipt.request.event_id.as_str());
                typed_movement_ids.extend(
                    binding
                        .opening
                        .iter()
                        .map(|opening| opening.movement_event_id.as_str()),
                );
                for action in &binding.post {
                    if let SharePostActionRef::Movement { event_id }
                    | SharePostActionRef::UnitMovement { event_id, .. } = action
                    {
                        typed_movement_ids.insert(event_id.as_str());
                    }
                }
            }
        }
        if let Some(snapshot) = self
            .registrations
            .iter()
            .find(|snapshot| typed_movement_ids.contains(snapshot.event_id.as_str()))
        {
            return Err(error(&format!("stock={} capture={} day={} phase=typed-identity: registration collides with a typed movement", self.stock.0, snapshot.event_id, snapshot.registered_on)));
        }
        for (market_index, receipt) in self.receipts.iter().enumerate() {
            let MovementScope::SettlementDayMarket { binding } = &receipt.request.scope else {
                continue;
            };
            let verify = || -> Result<(), ShareRegistryError> {
                match &binding.market_source {
                    ShareMarketSource::FinalConfirmations { receipts } => {
                        let mut ids = BTreeSet::new();
                        if receipts
                            .iter()
                            .any(|reference| !ids.insert(reference.receipt_id))
                        {
                            return Err(error(
                                "typed market confirmation reference identity is repeated",
                            ));
                        }
                    }
                    ShareMarketSource::CalendarClosed { policy_digest, .. }
                        if policy_digest.trim().is_empty() =>
                    {
                        return Err(error("calendar-closed source policy binding is empty"));
                    }
                    ShareMarketSource::CalendarClosed { .. } => {}
                }
                let start = usize::try_from(binding.predecessor.receipt_count)
                    .map_err(|_| error("opening receipt frontier exceeds usize"))?;
                if start > market_index
                    || market_index - start != binding.opening.len()
                    || receipt
                        .request
                        .day
                        .days_since(binding.predecessor.settled_on)
                        != 1
                    || self
                        .receipts
                        .get(start.wrapping_sub(1))
                        .is_some_and(|prior| prior.request.day != binding.predecessor.settled_on)
                    || binding.predecessor.unit != self.unit_at(start)
                {
                    return Err(error(
                        "typed predecessor date/receipt frontier/unit does not resolve",
                    ));
                }
                let mut unit = binding.predecessor.unit.clone();
                let mut identities = BTreeSet::new();
                for (offset, opening) in binding.opening.iter().enumerate() {
                    let stored = &self.receipts[start + offset];
                    if stored.request.day != receipt.request.day
                        || stored.request.event_id != opening.movement_event_id
                        || !opening_scope_matches(&stored.request.scope, opening)
                        || !opening_changes_match(&stored.request)
                        || !identities.insert(opening.movement_event_id.as_str())
                    {
                        return Err(error(
                            "opening reference does not resolve to its exact ordered movement",
                        ));
                    }
                    if matches!(
                        stored.request.scope,
                        MovementScope::ShareReDenomination { .. }
                    ) {
                        check_transition(
                            &unit,
                            &opening.from_unit,
                            &opening.to_unit,
                            &stored.request.event_id,
                        )?;
                    } else if unit != opening.from_unit {
                        return Err(error(
                            "stock distribution opening does not retain the current unit",
                        ));
                    }
                    self.validate_frozen_opening(opening, start + offset, stored)
                        .map_err(|cause| {
                            error(&format!(
                        "opening={} registration={} from={:?} to={:?} source_groups={:?}: {cause}",
                        opening.movement_event_id, opening.registration_event_id,
                        opening.from_unit, opening.to_unit, opening.source_lots))
                        })?;
                    unit = opening.to_unit.clone();
                }
                if binding.market_unit != unit
                    || binding.market_unit != self.unit_at(market_index)
                    || !identities.insert(receipt.request.event_id.as_str())
                {
                    return Err(error("market close is not in the opening result unit"));
                }
                let mut cursor = market_index + 1;
                let mut captures = BTreeSet::new();
                for action in &binding.post {
                    match action {
                        SharePostActionRef::CaptureRegistration { event_id } => {
                            let snapshot = self.registration(event_id).ok_or_else(|| {
                                error(&format!("capture={event_id} frontier={cursor} unit={unit:?} has no immutable snapshot"))
                            })?;
                            if receipt_ids.contains(event_id.as_str())
                                || !identities.insert(event_id.as_str())
                                || !captures.insert(event_id.as_str())
                                || snapshot.registered_on != receipt.request.day
                                || usize::try_from(snapshot.settled_receipts).ok() != Some(cursor)
                                || self.unit_at(cursor) != unit
                            {
                                return Err(error(&format!("capture={event_id} expected_frontier={cursor} stored_frontier={} unit={unit:?}: capture date/receipt frontier/unit/identity disagrees with its action position", snapshot.settled_receipts)));
                            }
                        }
                        SharePostActionRef::Movement { event_id }
                        | SharePostActionRef::UnitMovement { event_id, .. } => {
                            let stored = self.receipts.get(cursor).ok_or_else(|| {
                                error(&format!("post={event_id} frontier={cursor} unit={unit:?}: movement reference exceeds stored history"))
                            })?;
                            if stored.request.event_id != *event_id
                                || stored.request.day != receipt.request.day
                                || !identities.insert(event_id.as_str())
                            {
                                return Err(post_movement_error(&stored.request, &unit,
                                    &format!("expected_post={event_id} frontier={cursor}: movement reference has wrong identity/day/order")));
                            }
                            match action {
                                SharePostActionRef::UnitMovement {
                                    from_unit, to_unit, ..
                                } => {
                                    if !matches!(
                                        stored.request.scope,
                                        MovementScope::ShareReDenomination { .. }
                                    ) {
                                        return Err(post_movement_error(&stored.request, &unit,
                                            "post unit movement does not reference a re-denomination"));
                                    }
                                    check_transition(&unit, from_unit, to_unit, event_id)?;
                                    unit = to_unit.clone();
                                }
                                SharePostActionRef::Movement { .. } => {
                                    if !matches!(
                                        stored.request.scope,
                                        MovementScope::NonTradingTransfer { .. }
                                            | MovementScope::IssuerRepurchaseCancellation { .. }
                                    ) {
                                        return Err(post_movement_error(&stored.request, &unit,
                                            "ordinary post movement has an opening/market/unit scope"));
                                    }
                                }
                                SharePostActionRef::CaptureRegistration { .. } => unreachable!(),
                            }
                            cursor += 1;
                        }
                    }
                }
                if self
                    .receipts
                    .get(cursor)
                    .is_some_and(|next| next.request.day <= receipt.request.day)
                    || self
                        .registrations
                        .iter()
                        .filter(|snapshot| snapshot.registered_on == receipt.request.day)
                        .any(|snapshot| !captures.contains(snapshot.event_id.as_str()))
                {
                    return Err(error(
                        "typed day contains an unlisted market/post/capture fact",
                    ));
                }
                Ok(())
            };
            verify().map_err(|cause| ShareRegistryError::InvalidFact {
                detail: format!(
                    "stock={} day={} market={} phase=replay: {cause}",
                    self.stock.0, receipt.request.day, receipt.request.event_id
                ),
            })?;
        }
        Ok(())
    }

    pub(super) fn is_typed_movement_identity(&self, event: &str) -> bool {
        self.receipts.iter().any(|receipt| {
            let MovementScope::SettlementDayMarket { binding } = &receipt.request.scope else {
                return false;
            };
            receipt.request.event_id == event
                || binding
                    .opening
                    .iter()
                    .any(|opening| opening.movement_event_id == event)
                || binding.post.iter().any(|action| match action {
                    SharePostActionRef::Movement { event_id }
                    | SharePostActionRef::UnitMovement { event_id, .. } => event_id == event,
                    SharePostActionRef::CaptureRegistration { .. } => false,
                })
        })
    }

    fn validate_frozen_opening(
        &self,
        reference: &OpeningMovementRef,
        index: usize,
        receipt: &ShareDayReceipt,
    ) -> Result<(), ShareRegistryError> {
        let snapshot = self
            .registration(&reference.registration_event_id)
            .ok_or_else(|| error("opening registration reference is unknown"))?;
        let frontier = usize::try_from(snapshot.settled_receipts)
            .map_err(|_| error("opening registration frontier exceeds usize"))?;
        if frontier > index
            || self.unit_at(frontier) != reference.from_unit
            || snapshot.registered_on >= receipt.request.day
            || reference.source_lots.len() != receipt.request.changes.len()
        {
            return Err(error(
                "opening original registration day/frontier/unit/source groups do not match",
            ));
        }
        let current = self.replay_registration_to(snapshot, index)?;
        if current.holdings != snapshot.holdings || current.issued_shares != snapshot.issued_shares
        {
            return Err(error(
                "opening frozen original lots changed after registration",
            ));
        }
        let mut seen = BTreeSet::new();
        for (group, change) in reference.source_lots.iter().zip(&receipt.request.changes) {
            if group.is_empty() {
                return Err(error("opening source lot group is empty"));
            }
            let holding = snapshot
                .holdings
                .iter()
                .find(|holding| holding.holder == change.holder)
                .ok_or_else(|| {
                    error("opening source holder is absent from original registration")
                })?;
            let mut before = 0u64;
            let mut restriction = None;
            for id in group {
                if !seen.insert(id.as_str()) {
                    return Err(error(
                        "opening original lot is repeated across source groups",
                    ));
                }
                let lot = holding
                    .lots
                    .iter()
                    .find(|lot| lot.id == *id)
                    .ok_or_else(|| {
                        error("opening source lot does not belong to the registered holder")
                    })?;
                before = before
                    .checked_add(lot.qty)
                    .ok_or_else(|| error("opening source lot quantity overflow"))?;
                if change.change > 0 {
                    match restriction {
                        None => restriction = Some(&lot.restriction),
                        Some(prior) if prior == &lot.restriction => {}
                        Some(_) => {
                            return Err(error("opening positive source group mixes restrictions"))
                        }
                    }
                }
            }
            let after = i128::from(before)
                .checked_add(change.change)
                .ok_or_else(|| error("opening source group target overflow"))?;
            u64::try_from(after)
                .map_err(|_| error("opening source group target is outside u64"))?;
            if let Some(selection) = &change.consume {
                if selection.iter().any(|entry| !group.contains(&entry.lot_id)) {
                    return Err(error(
                        "opening lot consumption crosses its frozen source group",
                    ));
                }
            } else if change.change < 0 && group.len() != holding.lots.len() {
                return Err(error(
                    "opening partial source bucket reduction requires explicit lot selection",
                ));
            }
            if let Some(acquisition) = &change.acquisition {
                let original = restriction
                    .ok_or_else(|| error("opening acquisition lacks original restriction facts"))?;
                let expected = match original {
                    ShareRestriction::Restricted { release_on, .. }
                        if *release_on < receipt.request.day =>
                    {
                        &ShareRestriction::Unrestricted
                    }
                    other => other,
                };
                if &acquisition.restriction != expected {
                    return Err(error("opening acquisition restriction does not preserve the existing inheritance rule"));
                }
            }
        }
        let original_lot_count = snapshot
            .holdings
            .iter()
            .map(|holding| holding.lots.len())
            .sum::<usize>();
        if seen.len() != original_lot_count {
            return Err(error(
                "opening source groups do not cover all frozen original lots",
            ));
        }
        Ok(())
    }

    /// 外域来源校验须由 prepare 与完整 SaveSlot 恢复提供冻结 context；独立 serde 不代替它。
    pub fn validate_market_sources(
        &self,
        sources: &ShareMarketSourceContext<'_>,
    ) -> Result<(), ShareRegistryError> {
        let mut all_ids = BTreeSet::new();
        for row in sources.confirmations {
            if row.qty == 0 || row.stock.0.trim().is_empty() || !all_ids.insert(row.receipt_id) {
                return Err(error(
                    "market source context contains invalid or repeated confirmation identities",
                ));
            }
        }
        for receipt in &self.receipts {
            let MovementScope::SettlementDayMarket { binding } = &receipt.request.scope else {
                continue;
            };
            let verify = || -> Result<(), ShareRegistryError> {
                if sources
                    .completed_through
                    .is_none_or(|completed| receipt.request.day > completed)
                {
                    return Err(error("market source day has not actually completed"));
                }
                let trading = sources
                    .calendar
                    .is_trading_day(sources.exchange, receipt.request.day)
                    .map_err(|cause| {
                        error(&format!("frozen market calendar rejected day: {cause}"))
                    })?;
                let rows: Vec<_> = sources
                    .confirmations
                    .iter()
                    .filter(|row| row.stock == self.stock && row.day == receipt.request.day)
                    .collect();
                match &binding.market_source {
                    ShareMarketSource::CalendarClosed {
                        exchange,
                        policy_digest,
                    } => {
                        if trading
                            || *exchange != sources.exchange
                            || policy_digest != sources.calendar.policy().content_digest()
                            || !rows.is_empty()
                            || receipt
                                .request
                                .changes
                                .iter()
                                .any(|change| change.change != 0)
                        {
                            return Err(error("calendar-closed source disagrees with frozen policy or contains market movements"));
                        }
                    }
                    ShareMarketSource::FinalConfirmations { receipts } => {
                        if !trading {
                            return Err(error(
                                "final market confirmations claim an exchange-closed day",
                            ));
                        }
                        let expected: BTreeSet<_> = rows
                            .iter()
                            .map(|row| (row.account, row.receipt_id))
                            .collect();
                        let supplied: BTreeSet<_> = receipts
                            .iter()
                            .map(|row| (row.account, row.receipt_id))
                            .collect();
                        if supplied.len() != receipts.len() || supplied != expected {
                            return Err(error("market confirmation references are orphaned, repeated or omit final confirmations"));
                        }
                        let mut net = BTreeMap::<HolderId, i128>::new();
                        for row in rows {
                            let holder = if Some(row.account) == sources.treasury_account {
                                HolderId::IssuerTreasury
                            } else {
                                HolderId::Account(row.account)
                            };
                            let signed = if row.side == Side::Buy {
                                i128::from(row.qty)
                            } else {
                                -i128::from(row.qty)
                            };
                            let sum = net.entry(holder).or_default();
                            *sum = sum
                                .checked_add(signed)
                                .ok_or_else(|| error("final confirmation holder net overflow"))?;
                        }
                        let changes: BTreeMap<_, _> = receipt
                            .request
                            .changes
                            .iter()
                            .map(|change| (change.holder.clone(), change.change))
                            .collect();
                        for holder in net.keys().chain(changes.keys()) {
                            if net.get(holder).copied().unwrap_or(0)
                                != changes.get(holder).copied().unwrap_or(0)
                            {
                                return Err(error("final confirmation quantity does not match the market holder net change"));
                            }
                        }
                    }
                }
                Ok(())
            };
            verify().map_err(|cause| ShareRegistryError::InvalidFact {
                detail: format!(
                    "stock={} day={} market={} phase=market-source: {cause}",
                    self.stock.0, receipt.request.day, receipt.request.event_id
                ),
            })?;
        }
        Ok(())
    }
}

fn post_movement_error(
    request: &ShareDayRequest,
    unit: &ShareUnitIdentity,
    detail: &str,
) -> ShareRegistryError {
    let lots: Vec<_> = request
        .changes
        .iter()
        .flat_map(|change| {
            change
                .acquisition
                .iter()
                .map(|acquisition| acquisition.lot_id.as_str())
                .chain(
                    change
                        .consume
                        .iter()
                        .flatten()
                        .map(|selection| selection.lot_id.as_str()),
                )
        })
        .collect();
    error(&format!(
        "post={} unit={unit:?} lots={lots:?}: {detail}",
        request.event_id
    ))
}

fn opening_scope_matches(scope: &MovementScope, reference: &OpeningMovementRef) -> bool {
    match scope {
        MovementScope::ShareReDenomination { .. } => true,
        MovementScope::NonTradingTransfer { .. } => reference.from_unit == reference.to_unit,
        _ => false,
    }
}

fn opening_changes_match(request: &ShareDayRequest) -> bool {
    !matches!(request.scope, MovementScope::NonTradingTransfer { .. })
        || request.changes.iter().all(|change| change.change >= 0)
}

fn check_transition(
    current: &ShareUnitIdentity,
    from: &ShareUnitIdentity,
    to: &ShareUnitIdentity,
    event: &str,
) -> Result<(), ShareRegistryError> {
    if from != current
        || *to
            != (ShareUnitIdentity::ReDenominated {
                movement_event_id: event.into(),
            })
    {
        return Err(error(&format!(
            "unit transition movement={event} current={current:?} from={from:?} to={to:?} does not bind the actual movement receipt identity"
        )));
    }
    Ok(())
}

impl ShareSettlementDay {
    fn binding(&self) -> ShareSettlementBinding {
        ShareSettlementBinding {
            predecessor: self.predecessor.clone(),
            opening: self
                .opening
                .iter()
                .map(|opening| opening.reference.clone())
                .collect(),
            market_unit: self.actual_market_close.unit.clone(),
            market_source: self.actual_market_close.source.clone(),
            post: self
                .post_market_actions
                .iter()
                .map(|action| match action {
                    SharePostAction::Movement(request) => SharePostActionRef::Movement {
                        event_id: request.event_id.clone(),
                    },
                    SharePostAction::UnitMovement {
                        request,
                        from_unit,
                        to_unit,
                    } => SharePostActionRef::UnitMovement {
                        event_id: request.event_id.clone(),
                        from_unit: from_unit.clone(),
                        to_unit: to_unit.clone(),
                    },
                    SharePostAction::CaptureRegistration { event_id } => {
                        SharePostActionRef::CaptureRegistration {
                            event_id: event_id.clone(),
                        }
                    }
                })
                .collect(),
        }
    }
}

impl SharePostAction {
    fn movement(&self) -> Option<&ShareDayRequest> {
        match self {
            Self::Movement(request) | Self::UnitMovement { request, .. } => Some(request),
            Self::CaptureRegistration { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests;
