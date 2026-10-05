use serde::{Deserialize, Deserializer, Serializer};

use crate::accounting::{BusinessEventId, OpenItemId};
use crate::company::ContractId;

use super::{validate_id, CompanyId, CustomerFinanceError};

const CLAIM_PREFIX: &str = "company-claim:";

/// 客户对公司付款的来源类别；本金与每笔利息不共用一条合同号债务。
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum CompanyClaimSource {
    IndustrialReceivable {
        receivable: OpenItemId,
    },
    LoanPrincipal {
        loan: ContractId,
    },
    LoanInterest {
        loan: ContractId,
        #[serde(with = "business_event_decimal")]
        accrual: BusinessEventId,
    },
    RealEstatePresale {
        contract: ContractId,
    },
    RealEstateFinalReceivable {
        receivable: OpenItemId,
    },
    InsurancePremium {
        group: ContractId,
    },
}

impl CompanyClaimSource {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::IndustrialReceivable { .. } => "industrial-receivable",
            Self::LoanPrincipal { .. } => "loan-principal",
            Self::LoanInterest { .. } => "loan-interest",
            Self::RealEstatePresale { .. } => "real-estate-presale",
            Self::RealEstateFinalReceivable { .. } => "real-estate-final-receivable",
            Self::InsurancePremium { .. } => "insurance-premium",
        }
    }

    fn validate(&self) -> Result<(), CustomerFinanceError> {
        match self {
            Self::IndustrialReceivable { receivable }
            | Self::RealEstateFinalReceivable { receivable } => {
                validate_id(&receivable.0, "claim receivable")
            }
            Self::LoanPrincipal { loan } | Self::LoanInterest { loan, .. } => {
                validate_id(&loan.0, "claim loan")
            }
            Self::RealEstatePresale { contract } => {
                validate_id(&contract.0, "claim presale contract")
            }
            Self::InsurancePremium { group } => validate_id(&group.0, "claim premium group"),
        }
    }
}

/// 来源公司及强类型原义务身份；编码不是来源入账证明，也不创建新债务或现金。
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, Deserialize)]
#[serde(try_from = "ClaimFacts")]
pub struct CompanyClaimIdentity {
    company: CompanyId,
    source: CompanyClaimSource,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimFacts {
    company: CompanyId,
    source: CompanyClaimSource,
}

impl TryFrom<ClaimFacts> for CompanyClaimIdentity {
    type Error = CustomerFinanceError;
    fn try_from(facts: ClaimFacts) -> Result<Self, Self::Error> {
        Self::new(facts.company, facts.source)
    }
}

impl CompanyClaimIdentity {
    pub fn new(
        company: CompanyId,
        source: CompanyClaimSource,
    ) -> Result<Self, CustomerFinanceError> {
        validate_id(&company.0, "claim company")?;
        source.validate()?;
        Ok(Self { company, source })
    }

    pub fn company(&self) -> &CompanyId {
        &self.company
    }

    pub fn source(&self) -> &CompanyClaimSource {
        &self.source
    }

    pub(super) fn encode(&self) -> String {
        let mut encoded = format!("{CLAIM_PREFIX}{}:", self.source.kind_name());
        append_segment(&mut encoded, &self.company.0);
        match &self.source {
            CompanyClaimSource::IndustrialReceivable { receivable }
            | CompanyClaimSource::RealEstateFinalReceivable { receivable } => {
                append_segment(&mut encoded, &receivable.0)
            }
            CompanyClaimSource::LoanPrincipal { loan } => append_segment(&mut encoded, &loan.0),
            CompanyClaimSource::LoanInterest { loan, accrual } => {
                append_segment(&mut encoded, &loan.0);
                append_segment(&mut encoded, &accrual.value().to_string());
            }
            CompanyClaimSource::RealEstatePresale { contract } => {
                append_segment(&mut encoded, &contract.0)
            }
            CompanyClaimSource::InsurancePremium { group } => {
                append_segment(&mut encoded, &group.0)
            }
        }
        encoded
    }

    pub(super) fn decode(encoded: &str) -> Result<Option<Self>, CustomerFinanceError> {
        let Some(body) = encoded.strip_prefix(CLAIM_PREFIX) else {
            return Ok(None);
        };
        let (kind, mut remaining) = body
            .split_once(':')
            .ok_or_else(|| invalid("missing claim source category"))?;
        let company = CompanyId(read_segment(&mut remaining)?.to_string());
        let source = match kind {
            "industrial-receivable" => CompanyClaimSource::IndustrialReceivable {
                receivable: OpenItemId(read_segment(&mut remaining)?.to_string()),
            },
            "loan-principal" => CompanyClaimSource::LoanPrincipal {
                loan: ContractId(read_segment(&mut remaining)?.to_string()),
            },
            "loan-interest" => {
                let loan = ContractId(read_segment(&mut remaining)?.to_string());
                let accrual = parse_accrual(read_segment(&mut remaining)?)?;
                CompanyClaimSource::LoanInterest { loan, accrual }
            }
            "real-estate-presale" => CompanyClaimSource::RealEstatePresale {
                contract: ContractId(read_segment(&mut remaining)?.to_string()),
            },
            "real-estate-final-receivable" => CompanyClaimSource::RealEstateFinalReceivable {
                receivable: OpenItemId(read_segment(&mut remaining)?.to_string()),
            },
            "insurance-premium" => CompanyClaimSource::InsurancePremium {
                group: ContractId(read_segment(&mut remaining)?.to_string()),
            },
            _ => return Err(invalid("unknown claim source category")),
        };
        if !remaining.is_empty() {
            return Err(invalid("trailing claim identity data"));
        }
        let identity = Self::new(company, source)?;
        if identity.encode() != encoded {
            return Err(invalid("claim identity does not use canonical encoding"));
        }
        Ok(Some(identity))
    }
}

fn append_segment(encoded: &mut String, value: &str) {
    encoded.push_str(&value.len().to_string());
    encoded.push(':');
    encoded.push_str(value);
}

fn read_segment<'text>(remaining: &mut &'text str) -> Result<&'text str, CustomerFinanceError> {
    let (length_text, body) = remaining
        .split_once(':')
        .ok_or_else(|| invalid("missing identity segment byte length"))?;
    let length = length_text
        .parse::<usize>()
        .map_err(|_| invalid("identity byte length is outside usize range"))?;
    if length.to_string() != length_text {
        return Err(invalid(
            "identity byte length must use canonical decimal notation",
        ));
    }
    let value = body
        .get(..length)
        .ok_or_else(|| invalid("identity segment is truncated or splits a UTF-8 character"))?;
    *remaining = body
        .get(length..)
        .ok_or_else(|| invalid("invalid UTF-8 identity segment boundary"))?;
    Ok(value)
}

fn parse_accrual(text: &str) -> Result<BusinessEventId, CustomerFinanceError> {
    let value = text
        .parse::<u64>()
        .map_err(|_| invalid("claim accrual source is outside u64 range"))?;
    if value.to_string() != text {
        return Err(invalid(
            "claim accrual source must use canonical u64 decimal text",
        ));
    }
    Ok(BusinessEventId::new(value))
}

fn invalid(detail: &str) -> CustomerFinanceError {
    CustomerFinanceError::InvalidClaimIdentity {
        detail: detail.to_string(),
    }
}

mod business_event_decimal {
    use super::*;
    pub fn serialize<Output: Serializer>(
        event: &BusinessEventId,
        serializer: Output,
    ) -> Result<Output::Ok, Output::Error> {
        serializer.serialize_str(&event.value().to_string())
    }
    pub fn deserialize<'de, Input: Deserializer<'de>>(
        deserializer: Input,
    ) -> Result<BusinessEventId, Input::Error> {
        let text = String::deserialize(deserializer)?;
        parse_accrual(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::AccountingAmount;
    use crate::calendar::CivilDate;
    use crate::company::customer_finance::{CustomerFinanceBook, CustomerFinanceEventId, DebtId};
    use crate::company::{CounterpartyId, DebtCreditor};

    fn claim(company: &str, receivable: &str) -> CompanyClaimIdentity {
        CompanyClaimIdentity::new(
            CompanyId(company.to_string()),
            CompanyClaimSource::IndustrialReceivable {
                receivable: OpenItemId(receivable.to_string()),
            },
        )
        .expect("明确公司应收身份")
    }

    #[test]
    fn company_claim_id_roundtrip_is_stable_and_preserves_unicode_and_delimiters() {
        for (company, receivable) in [("甲:1😀", "AR:1:尾款😀"), (" a:b ", " 1:2 ")] {
            let identity = claim(company, receivable);
            let debt = DebtId::for_company_claim(&identity).expect("联合身份编码");
            assert_eq!(
                DebtId::for_company_claim(&identity).expect("重复编码"),
                debt
            );
            assert_eq!(
                debt.company_claim().expect("严格解码"),
                Some(identity.clone())
            );
            let restored: CompanyClaimIdentity =
                serde_json::from_str(&serde_json::to_string(&identity).expect("强类型身份序列化"))
                    .expect("严格身份恢复");
            assert_eq!(restored, identity);
        }
    }

    #[test]
    fn company_claim_id_has_no_company_or_delimiter_collisions() {
        let mut ids = std::collections::BTreeSet::new();
        for (company, receivable) in [
            ("issuer-a", "AR-2"),
            ("issuer-b", "AR-2"),
            ("a:b", "c"),
            ("a", "b:c"),
            ("a:1", "2:c"),
            ("a", "1:2:c"),
        ] {
            assert!(
                ids.insert(
                    DebtId::for_company_claim(&claim(company, receivable)).expect("联合身份编码")
                ),
                "不同公司或原文组合不能碰撞"
            );
        }
    }

    #[test]
    fn claim_source_kind_and_each_loan_accrual_are_independent() {
        let mut ids = std::collections::BTreeSet::new();
        for source in [
            CompanyClaimSource::IndustrialReceivable {
                receivable: OpenItemId("same-id".into()),
            },
            CompanyClaimSource::LoanPrincipal {
                loan: ContractId("same-id".into()),
            },
            CompanyClaimSource::LoanInterest {
                loan: ContractId("same-id".into()),
                accrual: BusinessEventId::new(12),
            },
            CompanyClaimSource::LoanInterest {
                loan: ContractId("same-id".into()),
                accrual: BusinessEventId::new(13),
            },
            CompanyClaimSource::LoanInterest {
                loan: ContractId("same-id".into()),
                accrual: BusinessEventId::new(u64::MAX),
            },
            CompanyClaimSource::RealEstatePresale {
                contract: ContractId("same-id".into()),
            },
            CompanyClaimSource::RealEstateFinalReceivable {
                receivable: OpenItemId("same-id".into()),
            },
            CompanyClaimSource::InsurancePremium {
                group: ContractId("same-id".into()),
            },
        ] {
            let identity =
                CompanyClaimIdentity::new(CompanyId("issuer".into()), source).expect("明确来源");
            let debt = DebtId::for_company_claim(&identity).expect("来源编码");
            assert_eq!(debt.company_claim().expect("严格解码"), Some(identity));
            assert!(ids.insert(debt), "本金、每笔利息及其他行业来源不能覆盖彼此");
        }
    }

    #[test]
    fn two_issuers_same_receivable_share_one_cash_balance_and_restore_idempotently() {
        let customer = CounterpartyId("shared-customer".into());
        let mut book = CustomerFinanceBook::new();
        book.register_customer(customer.clone(), AccountingAmount::from_cents(150))
            .expect("只有一份现金");
        let first = claim("issuer-a", "AR-2");
        let second = claim("issuer-b", "AR-2");
        let opened = CivilDate::from_iso("2030-01-01").expect("日期");
        let due = CivilDate::from_iso("2030-01-02").expect("日期");
        let first_id = book
            .register_company_claim(
                &customer,
                &first,
                opened,
                due,
                AccountingAmount::from_cents(100),
            )
            .expect("第一发行人债务");
        let second_id = book
            .register_company_claim(
                &customer,
                &second,
                opened,
                due,
                AccountingAmount::from_cents(100),
            )
            .expect("第二发行人债务");
        assert_ne!(first_id, second_id);
        let before_duplicate = book.clone();
        assert!(matches!(
            book.register_company_claim(
                &customer,
                &first,
                opened,
                due,
                AccountingAmount::from_cents(100)
            ),
            Err(CustomerFinanceError::DuplicateDebt { .. })
        ));
        assert_eq!(book, before_duplicate);
        let event = CustomerFinanceEventId("pay".into());
        let payments = book
            .repay_due(&customer, due, event.clone())
            .expect("共同现金按登记序支付");
        assert_eq!(
            payments
                .iter()
                .map(|payment| (
                    payment.debt.clone(),
                    payment.creditor.clone(),
                    payment.amount
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    first_id,
                    DebtCreditor::Company(first.company().clone()),
                    AccountingAmount::from_cents(100)
                ),
                (
                    second_id,
                    DebtCreditor::Company(second.company().clone()),
                    AccountingAmount::from_cents(50)
                ),
            ]
        );
        assert_eq!(
            book.customer(&customer).expect("客户").cash(),
            AccountingAmount::ZERO
        );
        let mut restored: CustomerFinanceBook =
            serde_json::from_str(&serde_json::to_string(&book).expect("真实账簿"))
                .expect("严格恢复");
        assert_eq!(
            restored
                .repay_due(&customer, due, event)
                .expect("恢复后幂等"),
            payments
        );
        assert_eq!(restored, book);
    }

    #[test]
    fn typed_claim_restore_requires_valid_identity_and_decimal_accrual_source() {
        for invalid in [
            r#"{"source":{"IndustrialReceivable":{"receivable":"AR-2"}}}"#,
            r#"{"company":"issuer"}"#,
            r#"{"company":"issuer","source":{"IndustrialReceivable":{}}}"#,
            r#"{"company":"issuer","source":{"LoanInterest":{"loan":"LN-1"}}}"#,
            r#"{"company":"issuer","source":{"IndustrialReceivable":{"receivable":"AR-2"}},"unexpected":true}"#,
            r#"{"company":"issuer","source":{"IndustrialReceivable":{"receivable":"AR-2","unexpected":true}}}"#,
            r#"{"company":"issuer","company":"other","source":{"IndustrialReceivable":{"receivable":"AR-2"}}}"#,
            r#"{"company":"issuer","source":{"LoanInterest":{"loan":"LN-1","loan":"LN-2","accrual":"12"}}}"#,
        ] {
            let _: serde_json::Value =
                serde_json::from_str(invalid).expect("合法JSON，仅业务结构非法");
            assert!(
                serde_json::from_str::<CompanyClaimIdentity>(invalid).is_err(),
                "缺字段／未知字段／重复字段须明确拒绝：{invalid}"
            );
        }
        assert!(CompanyClaimIdentity::new(
            CompanyId(" ".into()),
            CompanyClaimSource::IndustrialReceivable {
                receivable: OpenItemId("AR-2".into())
            }
        )
        .is_err());
        assert!(CompanyClaimIdentity::new(
            CompanyId("issuer".into()),
            CompanyClaimSource::LoanPrincipal {
                loan: ContractId(" ".into())
            }
        )
        .is_err());
        for invalid in ["12", "\"012\"", "\"+12\"", "\"18446744073709551616\""] {
            let input = format!("{{\"company\":\"issuer\",\"source\":{{\"LoanInterest\":{{\"loan\":\"LN-1\",\"accrual\":{invalid}}}}}}}");
            assert!(serde_json::from_str::<CompanyClaimIdentity>(&input).is_err());
        }
        let identity = CompanyClaimIdentity::new(
            CompanyId("issuer".into()),
            CompanyClaimSource::LoanInterest {
                loan: ContractId("LN-1".into()),
                accrual: BusinessEventId::new(u64::MAX),
            },
        )
        .expect("全域来源id");
        let encoded = serde_json::to_string(&identity).expect("身份序列化");
        assert!(encoded.contains("\"18446744073709551615\""));
        assert_eq!(
            serde_json::from_str::<CompanyClaimIdentity>(&encoded).expect("无损恢复"),
            identity
        );
    }

    #[test]
    fn encoded_company_claim_cannot_register_with_another_creditor() {
        let customer = CounterpartyId("customer".into());
        let mut book = CustomerFinanceBook::new();
        book.register_customer(customer.clone(), AccountingAmount::from_cents(100))
            .expect("独立现金");
        let before = book.clone();
        let opened = CivilDate::from_iso("2030-01-01").expect("日期");
        let due = CivilDate::from_iso("2030-01-02").expect("日期");
        let attempted = book.register_debt(
            &customer,
            DebtId("company-claim:industrial-receivable:8:issuer-a4:AR-2".into()),
            opened,
            due,
            AccountingAmount::from_cents(100),
            DebtCreditor::Company(CompanyId("issuer-b".into())),
        );
        assert!(attempted.is_err(), "编码来源公司必须等于实际债权人");
        assert_eq!(book, before);
    }

    #[test]
    fn restored_company_claim_cannot_belong_to_another_creditor() {
        let customer = CounterpartyId("customer".into());
        let mut book = CustomerFinanceBook::new();
        book.register_customer(customer.clone(), AccountingAmount::from_cents(100))
            .expect("独立现金");
        let opened = CivilDate::from_iso("2030-01-01").expect("日期");
        let due = CivilDate::from_iso("2030-01-02").expect("日期");
        book.register_debt(
            &customer,
            DebtId("named-external-debt".into()),
            opened,
            due,
            AccountingAmount::from_cents(100),
            DebtCreditor::Company(CompanyId("issuer-b".into())),
        )
        .expect("显式自定义债务");
        let mut forged = serde_json::to_value(book).expect("合法账簿");
        let debt = forged["customers"]["customer"]["debts"]
            .as_object_mut()
            .expect("债务map")
            .remove("named-external-debt")
            .expect("债务");
        let encoded = "company-claim:industrial-receivable:8:issuer-a4:AR-2";
        let mut debt = debt;
        debt["id"] = serde_json::json!(encoded);
        forged["customers"]["customer"]["debts"][encoded] = debt;
        assert!(
            serde_json::from_value::<CustomerFinanceBook>(forged).is_err(),
            "恢复也必须核查编码来源公司"
        );
    }

    #[test]
    fn encoded_claim_decoder_rejects_noncanonical_truncated_and_ambiguous_parts() {
        for invalid in [
            "company-claim:unknown:8:issuer-a4:AR-2",
            "company-claim:industrial-receivable:08:issuer-a4:AR-2",
            "company-claim:industrial-receivable:+8:issuer-a4:AR-2",
            "company-claim:industrial-receivable:999999999999999999999999:issuer-a4:AR-2",
            "company-claim:industrial-receivable:8:issuer",
            "company-claim:industrial-receivable:0:4:AR-2",
            "company-claim:industrial-receivable:1:甲4:AR-2",
            "company-claim:industrial-receivable:8:issuer-a4:AR-2trailing",
            "company-claim:loan-interest:8:issuer-a2:LN",
            "company-claim:loan-interest:8:issuer-a2:LN2:01",
            "company-claim:loan-interest:8:issuer-a2:LN3:+12",
            "company-claim:loan-interest:8:issuer-a2:LN20:18446744073709551616",
        ] {
            assert!(
                DebtId(invalid.into()).company_claim().is_err(),
                "非法编码应拒绝：{invalid}"
            );
        }
        assert_eq!(
            DebtId("named-external-claim".into())
                .company_claim()
                .expect("明确具名债务不属公司编码"),
            None
        );
    }
}
