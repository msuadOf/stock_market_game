//! 验证 Escrow 证据 producer 的公开接口。
//!
//! 只从 crate public API 导入，避免创建与生产类型不兼容的第二份 evidence 模块。

#[test]
fn full_update_stream_projection_is_exposed_by_the_engine_crate() {
    let _projector = engine::verification_evidence::UpdateStreamProjector::new();
    let _project_update = engine::verification_evidence::UpdateStreamProjector::project_update;
    let empty: [engine::verification_evidence::RuntimeUpdateRef<'_>; 0] = [];

    assert_eq!(
        engine::verification_evidence::project_update_stream(&empty).unwrap_err(),
        engine::verification_evidence::EvidenceError::InvalidUpdate {
            detail: "full runtime update stream contains no updates".to_owned(),
        }
    );
}
