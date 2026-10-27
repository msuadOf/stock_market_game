//! 三宿主共用的公开公司身份故障数值投影。
use super::CompanyCatalogViewError;
use crate::ValidationFault;

/// 保留 typed source；只在宿主 wire 边界投影公开数值原因。
pub fn company_catalog_fault(error: &CompanyCatalogViewError) -> ValidationFault {
    match error {
        CompanyCatalogViewError::UnknownListedStock { company } => ValidationFault::new(
            crate::error::COMPANY_CATALOG_LISTED_STOCK_MISSING,
            vec![u64::from(company.0)],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn listed_stock_missing_keeps_exact_company_context() {
        let error = CompanyCatalogViewError::UnknownListedStock {
            company: crate::company::CompanyId(7),
        };
        assert_eq!(
            company_catalog_fault(&error),
            ValidationFault::new(3019, vec![7])
        );
    }
}
