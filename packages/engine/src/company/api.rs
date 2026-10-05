use super::CompanyId;
use crate::accounting::AccountingAmount;
use crate::calendar::CivilDate;

#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum FundamentalSource {
    SimpleGenerated,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeriodAmounts {
    pub revenue: AccountingAmount,
    pub fixed_expense: AccountingAmount,
    pub variable_expense: AccountingAmount,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum CompanyReportAvailability {
    Available,
    BeforeOpening,
    NotYetSettled,
    PeriodNotRepresented,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimpleDisclosureCandidate {
    pub company: CompanyId,
    pub period_start: CivilDate,
    pub period_end: CivilDate,
    pub amounts: PeriodAmounts,
    pub explanation: super::simple::period::PeriodChangeExplanation,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "command", deny_unknown_fields)]
pub enum CompanyCommand {
    RestartRevenue {
        company: CompanyId,
        revenue: AccountingAmount,
        source: String,
    },
}
