use super::*;

pub(super) fn summary_chart(
    kind: CompanyKind,
) -> Result<crate::accounting::AccountChart, SimpleFinanceError> {
    let base = match kind {
        CompanyKind::Industrial => crate::company::industrial::industrial_account_chart(),
        CompanyKind::Bank => crate::company::bank::bank_account_chart(),
        CompanyKind::Insurance => crate::company::insurance::insurance_account_chart(),
        CompanyKind::RealEstate => crate::company::real_estate::real_estate_account_chart(),
    };
    Ok(crate::accounting::reports::simple_summary::account_chart(
        base,
    )?)
}

impl SimpleFinanceState {
    pub fn industry(&self) -> IndustryPresentation {
        crate::information::industry_presentation(self.kind)
    }
}
