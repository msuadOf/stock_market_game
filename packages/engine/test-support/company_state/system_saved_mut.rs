// 测试只访问真实 SaveSlot 的显式 variant；不提供旧 wire 字段或运行态兼容层。
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
