// 测试只访问真实 SaveSlot 的显式 variant；不提供旧 wire 字段或运行态兼容层。
fn legacy_saved(
    save: &engine::SaveSlot,
) -> (
    &engine::company::operations::CompanyOperations,
    &engine::accounting::closing::ClosingEngine,
    &Vec<engine::session::GroupStructure>,
) {
    match &save.company_state {
        engine::session::SavedSessionCompanies::LegacyOperations {
            operations,
            closing,
            groups,
        } => (operations, closing, groups),
        _ => panic!("测试输入必须显式选择 LegacyOperations"),
    }
}

fn legacy_saved_mut(
    save: &mut engine::SaveSlot,
) -> (
    &mut engine::company::operations::CompanyOperations,
    &mut engine::accounting::closing::ClosingEngine,
    &mut Vec<engine::session::GroupStructure>,
) {
    match &mut save.company_state {
        engine::session::SavedSessionCompanies::LegacyOperations {
            operations,
            closing,
            groups,
        } => (operations, closing, groups),
        _ => panic!("测试输入必须显式选择 LegacyOperations"),
    }
}

fn system_saved(
    save: &engine::SaveSlot,
) -> (
    &engine::company::CompanySystem,
    &engine::session::SessionCorporateActions,
) {
    match &save.company_state {
        engine::session::SavedSessionCompanies::CompanySystem {
            system,
            corporate_actions,
        } => (system, corporate_actions),
        _ => panic!("测试输入必须显式选择 CompanySystem"),
    }
}

fn system_saved_mut(
    save: &mut engine::SaveSlot,
) -> (
    &mut engine::company::CompanySystem,
    &mut engine::session::SessionCorporateActions,
) {
    match &mut save.company_state {
        engine::session::SavedSessionCompanies::CompanySystem {
            system,
            corporate_actions,
        } => (system, corporate_actions),
        _ => panic!("测试输入必须显式选择 CompanySystem"),
    }
}
