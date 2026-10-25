//! 当前 selected 公司来源的公开身份投影，不读取或修改财务与报告。

use super::GameSession;
use crate::company::{CompanyId, CompanySpec};
use crate::stock::StockId;

/// 本局实际公司身份；CompanyId 与 StockId 属于不同命名空间。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct CompanyCatalogEntry {
    pub id: CompanyId,
    pub code: String,
    pub name: String,
    /// 未上市实体为 None；上市代码必须能在本局 StockCatalog 中解析。
    pub listed_stock: Option<StockId>,
}

/// 身份事实失配；宿主 wire 映射由宿主边界另行定义。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CompanyCatalogViewError {
    #[error("公司 {company} 的上市证券不在当前 StockCatalog 中")]
    UnknownListedStock { company: CompanyId },
}

impl GameSession {
    /// 读取当前 selected 来源，保留实际 CompanyId；不生成新状态。
    pub fn company_catalog_view(
        &self,
    ) -> Result<Vec<CompanyCatalogEntry>, CompanyCatalogViewError> {
        let project = |(id, spec): (&CompanyId, &CompanySpec)| {
            let listed_stock = spec
                .listed_stock
                .as_ref()
                .map(|code| {
                    self.stock_catalog()
                        .id(&code.0)
                        .ok_or(CompanyCatalogViewError::UnknownListedStock { company: *id })
                })
                .transpose()?;
            Ok(CompanyCatalogEntry {
                id: *id,
                code: spec.code.clone(),
                name: spec.name.clone(),
                listed_stock,
            })
        };
        match self.company_system() {
            Some(system) => system.issuers().iter().map(project).collect(),
            None => self
                .state
                .company_registry
                .iter()
                .map(|(id, company)| (id, company.spec()))
                .map(project)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::company::config::CompanySystemConfig;
    use crate::session::npc_working_quote_tests::quote_setup;
    use crate::{Money, StockCatalog};

    include!("../../test-support/simple_company.rs");

    #[test]
    fn unknown_listed_stock_is_an_error_for_both_selected_sources() {
        for simple in [false, true] {
            let mut setup = quote_setup(0);
            setup.npcs.inst_count = 0;
            setup.history_len = 1;
            if simple {
                let CompanySystemConfig::Simple(mut config) =
                    simple_company_fixture!(crate; ["600888"])
                else {
                    unreachable!()
                };
                config.prehistory_periods = 2;
                setup.company_system = Some(CompanySystemConfig::Simple(config));
                setup.dividend_tax_mode =
                    Some(crate::company::CashDividendTaxMode::FlatWithholding);
                setup.flat_withholding_bp = Some(1_000);
                setup.par_value_per_share = Some(Money::from_cents(100));
            }
            let mut game = GameSession::new(setup, 42).unwrap();
            // 公开装配／恢复入口已经拒绝未知映射；局部破坏只验证查询防线。
            game.state.stocks = StockCatalog::from_metas(Vec::new()).unwrap();
            assert_eq!(
                game.company_catalog_view(),
                Err(CompanyCatalogViewError::UnknownListedStock {
                    company: CompanyId(0),
                }),
            );
        }
    }
}
