//! StockProcessing 独占的逐股账本：同一行共同保存 envelope、守恒事实与 terminal 标记。
//! candidate 失败由外层丢弃；转换、费用与守恒检查仍调用一般 ledger 的同一组 helper。
use super::ledger::{ConservationState, EnvelopeReceipt, ReceiptKind};
use super::{
    ledger_candidate, ledger_conservation, ledger_validation, Envelope, EnvelopeKey,
    EnvelopeOrigin, ReceiptLocalKey, ResVec, StepFatal,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub(super) struct PrivateStockLedger {
    rows: Vec<PrivateRow>,
    created: BTreeMap<EnvelopeKey, PrivateRow>,
    seen_local_keys: BTreeSet<ReceiptLocalKey>,
    next_receipt_index: u64,
}

#[derive(Clone, Debug)]
struct PrivateRow {
    envelope: Envelope,
    conservation: ConservationState,
    terminal: bool,
}

impl PrivateStockLedger {
    pub(super) fn new(envelopes: impl IntoIterator<Item = Envelope>) -> Result<Self, StepFatal> {
        // tick 起点的行只排序一次；Envelope 自己的 key 同时用于索引，不复制另一份 key 树。
        let mut rows: Vec<_> = envelopes
            .into_iter()
            .map(|envelope| PrivateRow {
                envelope,
                conservation: ConservationState::EMPTY,
                terminal: false,
            })
            .collect();
        rows.sort_unstable_by(|left, right| left.envelope.key().cmp(right.envelope.key()));
        if rows
            .windows(2)
            .any(|pair| pair[0].envelope.key() == pair[1].envelope.key())
        {
            return Err(ledger_validation::invariant("duplicate envelope"));
        }
        Ok(Self {
            rows,
            created: BTreeMap::new(),
            seen_local_keys: BTreeSet::new(),
            next_receipt_index: 0,
        })
    }

    fn row(&self, key: &EnvelopeKey) -> Option<&PrivateRow> {
        if let Some(row) = self.created.get(key) {
            return Some(row);
        }
        self.rows
            .binary_search_by(|row| row.envelope.key().cmp(key))
            .ok()
            .map(|index| &self.rows[index])
    }

    fn row_mut(&mut self, key: &EnvelopeKey) -> Option<&mut PrivateRow> {
        if let Some(row) = self.created.get_mut(key) {
            return Some(row);
        }
        let index = self
            .rows
            .binary_search_by(|row| row.envelope.key().cmp(key))
            .ok()?;
        Some(&mut self.rows[index])
    }

    fn ordered_rows(&self) -> impl Iterator<Item = &PrivateRow> {
        let mut initial = self.rows.iter().peekable();
        let mut created = self.created.values().peekable();
        std::iter::from_fn(move || match (initial.peek(), created.peek()) {
            (Some(left), Some(right)) if left.envelope.key() <= right.envelope.key() => {
                initial.next()
            }
            (Some(_), Some(_)) => created.next(),
            (Some(_), None) => initial.next(),
            (None, Some(_)) => created.next(),
            (None, None) => None,
        })
    }

    pub(super) fn get(&self, key: &EnvelopeKey) -> Result<&Envelope, StepFatal> {
        self.row(key)
            .filter(|row| !row.terminal)
            .map(|row| &row.envelope)
            .ok_or_else(|| ledger_validation::invariant("unknown envelope"))
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&EnvelopeKey, &Envelope)> {
        self.ordered_rows()
            .filter(|row| !row.terminal)
            .map(|row| (row.envelope.key(), &row.envelope))
    }

    #[cfg(test)]
    pub(super) fn terminal_count(&self) -> usize {
        self.ordered_rows().filter(|row| row.terminal).count()
    }

    pub(super) fn insert_created_for_stock_round(
        &mut self,
        envelopes: impl IntoIterator<Item = Envelope>,
    ) -> Result<(), StepFatal> {
        let mut seen = BTreeSet::new();
        let mut prepared = Vec::new();
        for envelope in envelopes {
            envelope.validate()?;
            if envelope.origin() != EnvelopeOrigin::CreatedAtValidation {
                return Err(ledger_validation::invariant(
                    "插入的 envelope 并非由 AccountValidation 创建",
                ));
            }
            if envelope.live() == ResVec::ZERO {
                return Err(ledger_validation::invariant(
                    "插入的 AccountValidation envelope 缺少 live resources",
                ));
            }
            let key = envelope.key().clone();
            if !seen.insert(key.clone()) || self.row(&key).is_some() {
                return Err(ledger_validation::invariant(
                    "插入的 AccountValidation envelope key 与账本证据冲突",
                ));
            }
            prepared.push((key, envelope));
        }
        for (key, envelope) in prepared {
            self.created.insert(
                key,
                PrivateRow {
                    envelope,
                    conservation: ConservationState::EMPTY,
                    terminal: false,
                },
            );
        }
        Ok(())
    }

    pub(super) fn apply_private_for_stock_round(
        &mut self,
        receipts: &mut [EnvelopeReceipt],
        terminal_keys: &[EnvelopeKey],
    ) -> Result<(), StepFatal> {
        let normalized = ledger_candidate::normalize(&mut self.next_receipt_index, receipts)?;
        for receipt in &normalized {
            if !self.seen_local_keys.insert(receipt.local_key.clone()) {
                return Err(ledger_validation::invariant("duplicate receipt local key"));
            }
            let row = self
                .row_mut(&receipt.envelope)
                .ok_or_else(|| ledger_validation::invariant("unknown envelope audit"))?;
            let audit = row.envelope.audit();
            ledger_validation::validate(receipt, audit, (!row.terminal).then_some(&row.envelope))?;
            let audit_after = ledger_validation::next_audit(receipt, audit)?;
            row.envelope.apply(
                receipt.delta,
                audit_after,
                matches!(receipt.kind, ReceiptKind::PriceResolved { .. }),
            )?;
            ledger_conservation::record_state(&mut row.conservation, receipt)?;
        }
        // 与一般 ledger 一样，normalized receipts 在 terminal removal 前安装到私有 outbox。
        receipts.clone_from_slice(&normalized);
        for key in terminal_keys {
            let row = self
                .row_mut(key)
                .filter(|row| !row.terminal)
                .ok_or_else(|| ledger_validation::invariant("unknown terminal envelope"))?;
            if row.envelope.live() != ResVec::ZERO {
                return Err(ledger_validation::invariant(
                    "terminal envelope still has live resources",
                ));
            }
            row.terminal = true;
        }
        Ok(())
    }

    pub(super) fn validate_complete_evidence(&self) -> Result<(), StepFatal> {
        // 原完整门禁先处理全部 live 再处理 terminal；不能混成按 key 排序的一次循环。
        let ordered: Vec<_> = self
            .ordered_rows()
            .filter(|row| !row.terminal)
            .chain(self.ordered_rows().filter(|row| row.terminal))
            .map(|row| (&row.envelope, &row.conservation))
            .collect();
        ledger_conservation::validate_owned_rows(&ordered)?;
        if self
            .ordered_rows()
            .any(|row| !row.terminal && row.envelope.live() == ResVec::ZERO)
        {
            return Err(ledger_validation::invariant(
                "live envelope row has no live resources",
            ));
        }
        if self
            .ordered_rows()
            .any(|row| row.terminal && row.envelope.live() != ResVec::ZERO)
        {
            return Err(ledger_validation::invariant(
                "terminal envelope row still has live resources",
            ));
        }
        for (envelope, _) in ordered {
            envelope.validate()?;
        }
        // audit 属于 Envelope，同键 conservation 属于同一 PrivateRow；三者不能独立丢行或漂移。
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ledger, ledger_tests};
    use super::PrivateStockLedger;
    use crate::Money;
    use std::collections::BTreeMap;

    #[test]
    fn private_receipt_transitions_match_the_general_ledger_including_prior_fees_and_rejections() {
        use super::super::{
            Envelope, EnvelopeAudit, EnvelopeLedger, FeeComponents, ReceiptKind, ResVec,
        };
        let key = ledger_tests::key();
        let paid = FeeComponents {
            commission: Money::from_cents(500),
            transfer_fee: Money::from_cents(1),
            stamp_tax: Money::ZERO,
        };
        let audit = EnvelopeAudit {
            limit: Money::from_cents(1000),
            remaining_qty: 100,
            filled_qty: 100,
            filled_value: Money::from_cents(100_000),
            nominal: paid,
            charged: paid,
        };
        let envelope =
            Envelope::tick_start_existing(key.clone(), Money::from_cents(100_001), 0, audit);
        for malformed in 0..8 {
            let mut private = PrivateStockLedger::new([envelope.clone()]).unwrap();
            let mut reference = EnvelopeLedger::new(0, [envelope.clone()]).unwrap();
            let mut release = ledger_tests::expiry_release(key.clone(), envelope.live());
            release.qty_before = 100;
            release.qty_after = 100;
            release.value_before = audit.filled_value;
            release.value_after = audit.filled_value;
            release.charged_before = paid;
            release.charged_after = paid;
            match malformed {
                0 | 6 => {}
                1 => release.delta.spent.cash = Money::from_cents(1),
                2 => release.envelope.order = crate::OrderId(99),
                3 => release.qty_after = 99,
                4 => release.deliver_cash = Money::from_cents(1),
                5 => release.charged_after.commission = Money::from_cents(501),
                7 => release.kind = ReceiptKind::Fill,
                _ => unreachable!(),
            }
            let mut actual = vec![release.clone()];
            if malformed == 6 {
                actual.push(release);
            }
            let mut expected = actual.clone();
            let terminal = std::slice::from_ref(&key);
            let expected_result = reference.apply_private_for_stock_round(&mut expected, terminal);
            let actual_result = private.apply_private_for_stock_round(&mut actual, terminal);
            assert_eq!(
                actual_result, expected_result,
                "receipt 变体 {malformed} 的显式错误必须一致"
            );
            assert_eq!(actual_result.is_ok(), malformed == 0);
            if actual_result.is_ok() {
                assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
                assert_eq!(private.terminal_count(), reference.terminal_count());
                assert_eq!(private.iter().count(), reference.iter().count());
                assert_eq!(
                    private.row(&key).unwrap().envelope.audit(),
                    audit,
                    "不能丢弃先前的累计费用"
                );
                assert_eq!(private.row(&key).unwrap().envelope.live(), ResVec::ZERO);
                private.validate_complete_evidence().unwrap();
                reference.validate_complete_evidence().unwrap();
            }
        }
    }

    #[test]
    fn constructor_does_not_build_the_general_multi_map_ledger() {
        ledger::LEDGER_CONSTRUCTION_COUNT.with(|count| count.set(0));

        let key = ledger_tests::key();
        let stock_ledger =
            PrivateStockLedger::new([ledger_tests::existing(key.clone(), Money::from_cents(100))])
                .unwrap();

        assert_eq!(
            stock_ledger.get(&key).unwrap().live().cash,
            Money::from_cents(100)
        );
        ledger::LEDGER_CONSTRUCTION_COUNT.with(|count| assert_eq!(count.get(), 0));
    }

    #[test]
    fn private_stock_ledger_preserves_duplicate_rejection_and_canonical_live_order() {
        let first_key = ledger_tests::key();
        let second_key = ledger_tests::second_key();
        let stock_ledger = PrivateStockLedger::new([
            ledger_tests::existing(second_key.clone(), Money::from_cents(200)),
            ledger_tests::existing(first_key.clone(), Money::from_cents(100)),
        ])
        .unwrap();

        let actual_keys: Vec<_> = stock_ledger.iter().map(|(key, _)| key.clone()).collect();
        let expected_keys: Vec<_> = BTreeMap::from([
            (first_key, Money::from_cents(100)),
            (second_key, Money::from_cents(200)),
        ])
        .into_keys()
        .collect();
        assert_eq!(actual_keys, expected_keys);

        let duplicate_key = ledger_tests::key();
        assert!(PrivateStockLedger::new([
            ledger_tests::existing(duplicate_key.clone(), Money::from_cents(100)),
            ledger_tests::existing(duplicate_key, Money::from_cents(200)),
        ])
        .is_err());
    }

    #[test]
    fn inserted_rows_merge_with_tick_start_rows_in_canonical_key_order() {
        use super::super::{Envelope, EnvelopeKey, EnvelopeOrigin};

        let first_key = ledger_tests::key();
        let middle_key = ledger_tests::second_key();
        let last_key = EnvelopeKey {
            account: crate::AccountId(2),
            stock: crate::StockCode("600003".to_owned()),
            order: crate::OrderId(3),
            side: crate::Side::Buy,
        };
        let mut stock_ledger = PrivateStockLedger::new([
            ledger_tests::existing(last_key.clone(), Money::from_cents(300)),
            ledger_tests::existing(first_key.clone(), Money::from_cents(100)),
        ])
        .unwrap();
        let inserted = Envelope::created_at_validation(
            middle_key.clone(),
            Money::from_cents(200),
            0,
            ledger_tests::audit_with_remaining(1),
        );
        assert_eq!(inserted.origin(), EnvelopeOrigin::CreatedAtValidation);

        stock_ledger
            .insert_created_for_stock_round([inserted])
            .unwrap();

        let actual_keys: Vec<_> = stock_ledger.iter().map(|(key, _)| key.clone()).collect();
        assert_eq!(actual_keys, vec![first_key, middle_key, last_key]);
    }

    #[test]
    fn created_insertion_rejects_batch_duplicates_and_terminal_keys_atomically() {
        use super::super::{Envelope, EnvelopeKey};

        let duplicate_key = EnvelopeKey {
            account: crate::AccountId(2),
            stock: crate::StockCode("600003".to_owned()),
            order: crate::OrderId(3),
            side: crate::Side::Buy,
        };
        let duplicate = Envelope::created_at_validation(
            duplicate_key.clone(),
            Money::from_cents(100),
            0,
            ledger_tests::audit_with_remaining(1),
        );
        let mut duplicate_ledger = PrivateStockLedger::new([]).unwrap();
        assert!(duplicate_ledger
            .insert_created_for_stock_round([duplicate.clone(), duplicate])
            .is_err());
        assert!(duplicate_ledger.get(&duplicate_key).is_err());
        assert_eq!(duplicate_ledger.iter().count(), 0);

        let terminal_key = ledger_tests::key();
        let terminal_envelope =
            ledger_tests::existing(terminal_key.clone(), Money::from_cents(100));
        let mut terminal_ledger = PrivateStockLedger::new([terminal_envelope.clone()]).unwrap();
        let mut release =
            ledger_tests::expiry_release(terminal_key.clone(), terminal_envelope.live());
        release.qty_before = terminal_envelope.audit().remaining_qty;
        release.qty_after = terminal_envelope.audit().remaining_qty;
        release.value_before = terminal_envelope.audit().filled_value;
        release.value_after = terminal_envelope.audit().filled_value;
        release.charged_before = terminal_envelope.audit().charged;
        release.charged_after = terminal_envelope.audit().charged;
        terminal_ledger
            .apply_private_for_stock_round(&mut [release], std::slice::from_ref(&terminal_key))
            .unwrap();
        assert_eq!(terminal_ledger.terminal_count(), 1);

        let prefix_key = EnvelopeKey {
            account: crate::AccountId(2),
            stock: crate::StockCode("600003".to_owned()),
            order: crate::OrderId(3),
            side: crate::Side::Buy,
        };
        let prefix = Envelope::created_at_validation(
            prefix_key.clone(),
            Money::from_cents(100),
            0,
            ledger_tests::audit_with_remaining(1),
        );
        let terminal_replacement = Envelope::created_at_validation(
            terminal_key.clone(),
            Money::from_cents(100),
            0,
            ledger_tests::audit_with_remaining(1),
        );
        assert!(terminal_ledger
            .insert_created_for_stock_round([prefix, terminal_replacement])
            .is_err());
        assert!(terminal_ledger.get(&prefix_key).is_err());
        assert!(terminal_ledger.get(&terminal_key).is_err());
        assert_eq!(terminal_ledger.terminal_count(), 1);
    }

    #[test]
    fn aggregate_cash_and_share_overflow_errors_match_the_general_ledger() {
        use super::super::{Envelope, EnvelopeKey, EnvelopeLedger};

        let cash_overflow = [
            Envelope::tick_start_existing(
                ledger_tests::key(),
                Money::from_cents(i64::MAX),
                0,
                ledger_tests::audit(),
            ),
            Envelope::tick_start_existing(
                ledger_tests::second_key(),
                Money::from_cents(1),
                0,
                ledger_tests::audit(),
            ),
        ];
        let share_overflow = [
            Envelope::tick_start_existing(
                ledger_tests::second_key(),
                Money::ZERO,
                u32::MAX,
                ledger_tests::audit(),
            ),
            Envelope::tick_start_existing(
                EnvelopeKey {
                    account: crate::AccountId(2),
                    stock: crate::StockCode("600003".to_owned()),
                    order: crate::OrderId(3),
                    side: crate::Side::Sell,
                },
                Money::ZERO,
                1,
                ledger_tests::audit(),
            ),
        ];

        for (envelopes, expected_description) in [
            (
                &cash_overflow[..],
                "overflow in add with operand 9223372036854775807 + 1",
            ),
            (&share_overflow[..], "shares add"),
        ] {
            let later_key = EnvelopeKey {
                account: crate::AccountId(3),
                stock: crate::StockCode("600003".to_owned()),
                order: crate::OrderId(4),
                side: crate::Side::Buy,
            };
            let later = ledger_tests::existing(later_key.clone(), Money::from_cents(1));
            let mut private =
                PrivateStockLedger::new(envelopes.iter().cloned().chain([later.clone()])).unwrap();
            let mut general =
                EnvelopeLedger::new(0, envelopes.iter().cloned().chain([later])).unwrap();
            // 第三行故意不守恒；必须仍报告前两行的 aggregate 溢出。
            private
                .row_mut(&later_key)
                .unwrap()
                .conservation
                .sealed_released
                .cash = Money::from_cents(1);
            general
                .conservation
                .get_mut(&later_key)
                .unwrap()
                .sealed_released
                .cash = Money::from_cents(1);
            let private_error = private.validate_complete_evidence().unwrap_err();
            let general_error = general.validate_complete_evidence().unwrap_err();
            assert_eq!(private_error, general_error);
            assert!(matches!(private_error,
                super::super::StepFatal::InvariantViolation { description, location }
                    if description == expected_description && location == "pipeline::ResVec"
            ));
        }
    }
}
