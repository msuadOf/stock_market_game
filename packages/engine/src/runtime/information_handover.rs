//! INFO 最后成功观察的只读移交数据；只按 AccountId 重分组，不执行本人观察。

use crate::calendar::CivilInstant;
use crate::company::CompanyReadFacts;
use crate::information::PublicLibrary;
use crate::observation::information_clock::InformationTradingAnchor;
use crate::strategy::MarketView;
use crate::{AccountId, StockId};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// 材料比较沿用 INFO 的真实值相等语义；Arc 只用于共享不可变输入。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InformationHandoverMaterial {
    pub(crate) through: CivilInstant,
    pub(crate) library: Arc<PublicLibrary>,
    pub(crate) facts: Arc<CompanyReadFacts>,
    pub(crate) market: Arc<MarketView>,
    pub(crate) anchors: BTreeMap<StockId, InformationTradingAnchor>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InformationHandoverSignature {
    pub(crate) material: Arc<InformationHandoverMaterial>,
    pub(crate) owners_and_codes: BTreeMap<AccountId, BTreeSet<StockId>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GroupInformationHandover {
    pub(crate) observed_through: Option<CivilInstant>,
    pub(crate) signature: Option<InformationHandoverSignature>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InformationHandover {
    pub(crate) observed_through: Option<CivilInstant>,
    pub(crate) signature: Option<InformationHandoverSignature>,
}

/// bool 由已验证的 PersonalPresence 提供；不能从账户 kind 猜测 Required。
fn required_owners(
    rosters: &BTreeMap<u16, Vec<(AccountId, bool)>>,
) -> Result<BTreeMap<u16, BTreeSet<AccountId>>, String> {
    let mut all_owners = BTreeSet::new();
    let mut required = BTreeMap::new();
    for (&group, roster) in rosters {
        let mut group_required = BTreeSet::new();
        for &(owner, is_required) in roster {
            if !all_owners.insert(owner) {
                return Err(format!(
                    "INFO 移交名册重复 AccountId={owner:?}，group={group}"
                ));
            }
            if is_required {
                group_required.insert(owner);
            }
        }
        required.insert(group, group_required);
    }
    Ok(required)
}

impl InformationHandover {
    pub(crate) fn merge(
        groups: Vec<(u16, GroupInformationHandover)>,
        rosters: &BTreeMap<u16, Vec<(AccountId, bool)>>,
    ) -> Result<Self, String> {
        let required = required_owners(rosters)?;
        let mut received = BTreeMap::new();
        for (group, handover) in groups {
            if !rosters.contains_key(&group) {
                return Err(format!("INFO 移交收到未知 group={group}"));
            }
            if received.insert(group, handover).is_some() {
                return Err(format!("INFO 移交重复 group={group} 回执"));
            }
        }
        if received.len() != rosters.len() {
            let missing: Vec<_> = rosters
                .keys()
                .filter(|group| !received.contains_key(*group))
                .copied()
                .collect();
            return Err(format!("INFO 移交缺少 group 回执：{missing:?}"));
        }

        let mut observed_through = None;
        let mut first_watermark = true;
        let mut signature: Option<InformationHandoverSignature> = None;
        let mut unsigned_required_groups = Vec::new();
        for (group, handover) in received {
            if first_watermark {
                observed_through = handover.observed_through;
                first_watermark = false;
            } else if observed_through != handover.observed_through {
                return Err(format!("INFO 移交观察水位不同，group={group}"));
            }
            let group_required = &required[&group];
            let Some(group_signature) = handover.signature else {
                if !group_required.is_empty() {
                    unsigned_required_groups.push(group);
                }
                continue;
            };
            if Some(group_signature.material.through) != handover.observed_through {
                return Err(format!(
                    "INFO 移交签名 through 与观察水位不同，group={group}"
                ));
            }
            let signed_owners: BTreeSet<_> =
                group_signature.owners_and_codes.keys().copied().collect();
            if &signed_owners != group_required {
                return Err(format!(
                    "INFO 移交签名 owner 集合与 Required 名册不同，group={group}"
                ));
            }
            match &mut signature {
                None => signature = Some(group_signature),
                Some(merged) => {
                    if merged.material.as_ref() != group_signature.material.as_ref() {
                        return Err(format!("INFO 移交观察材料不同，group={group}"));
                    }
                    for (owner, codes) in group_signature.owners_and_codes {
                        if merged.owners_and_codes.insert(owner, codes).is_some() {
                            return Err(format!(
                                "INFO 移交签名重复 AccountId={owner:?}，group={group}"
                            ));
                        }
                    }
                }
            }
        }
        // 全 None 是尚未观察；混合状态不能使 Required 的观察签名凭空丢失。
        if signature.is_some() && !unsigned_required_groups.is_empty() {
            return Err(format!(
                "INFO 移交部分 Required 组缺少观察签名：{unsigned_required_groups:?}"
            ));
        }
        Ok(Self {
            observed_through,
            signature,
        })
    }

    pub(crate) fn split(
        &self,
        new_rosters: &BTreeMap<u16, Vec<(AccountId, bool)>>,
    ) -> Result<Vec<(u16, GroupInformationHandover)>, String> {
        let required = required_owners(new_rosters)?;
        if let Some(signature) = &self.signature {
            if Some(signature.material.through) != self.observed_through {
                return Err("INFO 移交签名 through 与观察水位不同".into());
            }
            let next_required: BTreeSet<_> = required
                .values()
                .flat_map(|owners| owners.iter().copied())
                .collect();
            let signed_owners: BTreeSet<_> = signature.owners_and_codes.keys().copied().collect();
            if next_required != signed_owners {
                return Err(
                    "INFO 重分组后的 Required 集合与已观察 owner 不同，存在遗漏、新增或分类变化"
                        .into(),
                );
            }
        }
        let mut groups = Vec::with_capacity(new_rosters.len());
        for (&group, group_required) in &required {
            let signature = self
                .signature
                .as_ref()
                .map(|signature| InformationHandoverSignature {
                    material: Arc::clone(&signature.material),
                    owners_and_codes: group_required
                        .iter()
                        .map(|owner| (*owner, signature.owners_and_codes[owner].clone()))
                        .collect(),
                });
            groups.push((
                group,
                GroupInformationHandover {
                    observed_through: self.observed_through,
                    signature,
                },
            ));
        }
        Ok(groups)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::CivilDate;
    use crate::company::{CompanyId, CompanyKind};

    fn at(hour: u32) -> CivilInstant {
        CivilInstant::from_hms(CivilDate::from_iso("2031-04-01").unwrap(), hour, 0, 0).unwrap()
    }

    fn material() -> Arc<InformationHandoverMaterial> {
        Arc::new(InformationHandoverMaterial {
            through: at(19),
            library: Arc::new(PublicLibrary::new()),
            facts: Arc::new(
                CompanyReadFacts::new(
                    [(StockId(0), CompanyId(0))].into(),
                    [(CompanyId(0), (CompanyKind::Industrial, 10_000))].into(),
                )
                .unwrap(),
            ),
            market: Arc::new(MarketView {
                stocks: BTreeMap::new(),
                tick: 5,
                market_minute: 2,
            }),
            anchors: [(
                StockId(0),
                InformationTradingAnchor::TradingDay { ordinal: 0 },
            )]
            .into(),
        })
    }

    fn observed(owner: u64) -> GroupInformationHandover {
        GroupInformationHandover {
            observed_through: Some(at(19)),
            signature: Some(InformationHandoverSignature {
                material: material(),
                owners_and_codes: [(AccountId(owner), [StockId(0)].into())].into(),
            }),
        }
    }

    fn rosters() -> BTreeMap<u16, Vec<(AccountId, bool)>> {
        [
            (0, vec![(AccountId(1), true)]),
            (1, vec![(AccountId(2), true)]),
        ]
        .into()
    }

    fn merge_two() -> InformationHandover {
        InformationHandover::merge(vec![(1, observed(2)), (0, observed(1))], &rosters()).unwrap()
    }

    #[test]
    fn merge_accepts_arrival_order_and_equal_material_at_distinct_addresses() {
        let first = observed(1);
        let second = observed(2);
        assert!(!Arc::ptr_eq(
            &first.signature.as_ref().unwrap().material,
            &second.signature.as_ref().unwrap().material,
        ));
        let merged = InformationHandover::merge(vec![(1, second), (0, first)], &rosters()).unwrap();
        assert_eq!(merged.observed_through, Some(at(19)));
        let signature = merged.signature.unwrap();
        assert_eq!(signature.material.as_ref(), material().as_ref());
        assert_eq!(
            signature.owners_and_codes,
            [
                (AccountId(1), [StockId(0)].into()),
                (AccountId(2), [StockId(0)].into())
            ]
            .into(),
        );
    }

    #[test]
    fn split_regroups_by_account_and_keeps_absent_new_player_out_of_signature() {
        let source = merge_two();
        let before = source.clone();
        let target = [
            (4, vec![(AccountId(2), true), (AccountId(9), false)]),
            (7, vec![(AccountId(1), true)]),
            (8, vec![(AccountId(10), false)]),
        ]
        .into();
        let groups = source.split(&target).unwrap();
        assert_eq!(source, before);
        assert_eq!(
            groups.iter().map(|(group, _)| *group).collect::<Vec<_>>(),
            vec![4, 7, 8]
        );
        for (_, group) in &groups {
            assert_eq!(group.observed_through, source.observed_through);
            assert_eq!(
                group.signature.as_ref().unwrap().material,
                source.signature.as_ref().unwrap().material,
            );
        }
        assert_eq!(
            groups[0]
                .1
                .signature
                .as_ref()
                .unwrap()
                .owners_and_codes
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![AccountId(2)],
        );
        assert!(groups[2]
            .1
            .signature
            .as_ref()
            .unwrap()
            .owners_and_codes
            .is_empty());
        assert_eq!(InformationHandover::merge(groups, &target).unwrap(), source);
    }

    #[test]
    fn merge_rejects_duplicate_unknown_and_missing_group_receipts() {
        assert!(
            InformationHandover::merge(vec![(0, observed(1)), (0, observed(1))], &rosters())
                .is_err()
        );
        assert!(
            InformationHandover::merge(vec![(0, observed(1)), (2, observed(2))], &rosters())
                .is_err()
        );
        assert!(InformationHandover::merge(vec![(0, observed(1))], &rosters()).is_err());
    }

    #[test]
    fn merge_rejects_different_observation_material_or_watermark() {
        let mut different = observed(2);
        Arc::make_mut(&mut different.signature.as_mut().unwrap().material)
            .anchors
            .insert(
                StockId(0),
                InformationTradingAnchor::DayBefore { ordinal: 0 },
            );
        assert!(
            InformationHandover::merge(vec![(0, observed(1)), (1, different)], &rosters()).is_err()
        );

        let mut different = observed(2);
        different.observed_through = Some(at(20));
        Arc::make_mut(&mut different.signature.as_mut().unwrap().material).through = at(20);
        assert!(
            InformationHandover::merge(vec![(0, observed(1)), (1, different)], &rosters()).is_err()
        );

        let mut invalid = observed(2);
        invalid.observed_through = None;
        assert!(
            InformationHandover::merge(vec![(0, observed(1)), (1, invalid)], &rosters()).is_err()
        );

        let mut invalid = observed(1);
        invalid.observed_through = Some(at(20));
        let single = [(0, vec![(AccountId(1), true)])].into();
        assert!(InformationHandover::merge(vec![(0, invalid)], &single).is_err());
    }

    #[test]
    fn merge_rejects_missing_required_foreign_or_absent_signature_owner() {
        let mut missing = observed(2);
        missing.signature.as_mut().unwrap().owners_and_codes.clear();
        assert!(
            InformationHandover::merge(vec![(0, observed(1)), (1, missing)], &rosters()).is_err()
        );
        assert!(
            InformationHandover::merge(vec![(0, observed(2)), (1, observed(1))], &rosters())
                .is_err()
        );
        let mut absent = rosters();
        absent.get_mut(&1).unwrap()[0].1 = false;
        assert!(
            InformationHandover::merge(vec![(0, observed(1)), (1, observed(2))], &absent).is_err()
        );
    }

    #[test]
    fn merge_rejects_duplicate_roster_account_even_when_absent() {
        let duplicate = [
            (0, vec![(AccountId(1), false)]),
            (1, vec![(AccountId(1), false)]),
        ]
        .into();
        let empty = GroupInformationHandover {
            observed_through: None,
            signature: None,
        };
        assert!(
            InformationHandover::merge(vec![(0, empty.clone()), (1, empty)], &duplicate).is_err()
        );
    }

    #[test]
    fn merge_all_unobserved_allows_required_but_mixed_requires_empty_required_set() {
        let empty = GroupInformationHandover {
            observed_through: None,
            signature: None,
        };
        let unobserved =
            InformationHandover::merge(vec![(0, empty.clone()), (1, empty)], &rosters()).unwrap();
        assert_eq!(unobserved.signature, None);
        assert!(unobserved
            .split(&rosters())
            .unwrap()
            .iter()
            .all(|(_, group)| group.signature.is_none()));

        let empty = GroupInformationHandover {
            observed_through: Some(at(19)),
            signature: None,
        };
        assert!(
            InformationHandover::merge(vec![(0, observed(1)), (1, empty.clone())], &rosters())
                .is_err()
        );
        let mut absent = rosters();
        absent.get_mut(&1).unwrap()[0].1 = false;
        let mixed =
            InformationHandover::merge(vec![(1, empty), (0, observed(1))], &absent).unwrap();
        assert_eq!(mixed.signature.unwrap().owners_and_codes.len(), 1);
    }

    #[test]
    fn split_rejects_omitted_duplicate_new_required_or_reclassified_owner() {
        let source = merge_two();
        let omitted = [(0, vec![(AccountId(1), true)])].into();
        assert!(source.split(&omitted).is_err());
        let duplicate = [
            (0, vec![(AccountId(1), true)]),
            (1, vec![(AccountId(1), true), (AccountId(2), true)]),
        ]
        .into();
        assert!(source.split(&duplicate).is_err());
        let mut new_required = rosters();
        new_required.get_mut(&1).unwrap().push((AccountId(3), true));
        assert!(source.split(&new_required).is_err());
        let mut absent = rosters();
        absent.get_mut(&1).unwrap()[0].1 = false;
        assert!(source.split(&absent).is_err());
    }
}
