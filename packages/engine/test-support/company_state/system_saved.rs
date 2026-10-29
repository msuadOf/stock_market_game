// 测试只访问真实 SaveSlot 的显式 variant；不提供旧 wire 字段或运行态兼容层。
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
