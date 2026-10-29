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
