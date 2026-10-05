use super::core::{CompanyOperationsHashCache, OperatingCompany, PaymentFailureRecord};
use super::history::HistoryMeta;
use super::{CompanyOperations, FlowParams, IndustryBooks, OperationsError};
use crate::calendar::CivilDate;
use crate::company::events::ShockParams;
use crate::company::rng::OperatingRng;
use crate::company::{CompanyId, CompanySpec, IndustryId};
use std::collections::BTreeMap;

impl CompanyOperations {
    pub fn validate_restored(&self) -> Result<(), OperationsError> {
        self.shock_params.validate()?;
        self.validate_payment_history()?;
        CompanySpec::validate_set(
            &self
                .companies
                .values()
                .map(|company| company.spec.clone())
                .collect::<Vec<_>>(),
        )?;
        for (id, company) in &self.companies {
            if id != &company.spec.id {
                return Err(OperationsError::InvalidRestoredState {
                    detail: format!(
                        "公司 map key {id:?} 与 spec 身份 {:?} 不一致",
                        company.spec.id
                    ),
                });
            }
            let checked = (|| {
                let matching = matches!(
                    (&company.books, &company.params),
                    (IndustryBooks::Industrial(_), FlowParams::Industrial(_))
                        | (IndustryBooks::Bank(_), FlowParams::Bank(_))
                        | (IndustryBooks::Insurance(_), FlowParams::Insurance(_))
                        | (IndustryBooks::RealEstate(_), FlowParams::RealEstate(_))
                );
                if company.spec.kind != company.books.kind() || !matching {
                    return Err(OperationsError::KindFlowMismatch {
                        company: id.clone(),
                        kind: company.spec.kind,
                        flow: company.params.variant_name(),
                    });
                }
                company.params.validate_durations(&company.spec)?;
                if let Some(bank) = company.books.as_bank() {
                    bank.ecl_policy().validate()?;
                }
                company.books.validate_owner_state()
            })();
            checked.map_err(|cause| OperationsError::InvalidRestoredOwner {
                company: id.clone(),
                cause: Box::new(cause),
            })?;
        }
        Ok(())
    }
}

impl<'de> serde::Deserialize<'de> for CompanyOperations {
    fn deserialize<Decoder: serde::Deserializer<'de>>(
        decoder: Decoder,
    ) -> Result<Self, Decoder::Error> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct State {
            #[serde(with = "crate::session::u64_decimal")]
            seed: u64,
            shock_params: ShockParams,
            scheduler: super::OperatingScheduler,
            market_rng: OperatingRng,
            industry_rngs: BTreeMap<IndustryId, OperatingRng>,
            companies: BTreeMap<CompanyId, OperatingCompany>,
            #[serde(deserialize_with = "crate::company::persistence::required_nullable")]
            next_expected: Option<CivilDate>,
            #[serde(deserialize_with = "crate::company::persistence::required_nullable")]
            history: Option<HistoryMeta>,
            payment_failures: BTreeMap<CivilDate, Vec<PaymentFailureRecord>>,
        }
        let state = State::deserialize(decoder)?;
        let restored = Self {
            seed: state.seed,
            shock_params: state.shock_params,
            scheduler: state.scheduler,
            market_rng: state.market_rng,
            industry_rngs: state.industry_rngs,
            companies: state.companies,
            next_expected: state.next_expected,
            history: state.history,
            payment_failures: state.payment_failures,
            hash_projection_cache: CompanyOperationsHashCache::default(),
        };
        restored
            .validate_restored()
            .map_err(serde::de::Error::custom)?;
        Ok(restored)
    }
}
