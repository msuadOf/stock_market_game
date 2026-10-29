// 测试只访问真实 SaveSlot 的显式 variant；不提供旧 wire 字段或运行态兼容层。
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
