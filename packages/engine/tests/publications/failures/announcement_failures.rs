//! 拒绝路径套件（公告面）：发生日之前 / 相位外 / 非次相位拒绝；
//! 无默认日期兜底（缺失 published_at = 显式反序列化失败）。
//!
//! 共享基线（`base`）在 `super`。

use super::{base, COMPANY};
use crate::fixture::d;
use engine::calendar::CivilInstant;
use engine::company::{CompanyId, ShockKind};
use engine::information::{AnnouncedEvent, AnnouncementRequest, InformationError};

/// 公告时序：发生日之前 / 相位外 / 非次相位（晚一天）拒绝。
#[test]
fn announcement_timing_rejected() {
    let mut library = base().library;
    let event = AnnouncedEvent {
        kind: ShockKind::CreditDeterioration,
        amplitude_bp: 500,
        starts_on: d("2030-04-20"),
        expires_on: d("2030-04-25"),
    };
    let phase = CivilInstant::from_hms(d("2030-04-20"), 18, 0, 0).unwrap();
    // 发生日之前公布。
    let mut req = AnnouncementRequest {
        company: CompanyId(COMPANY.to_string()),
        occurred_on: d("2030-04-21"),
        published_at: phase,
        event: event.clone(),
    };
    assert!(matches!(
        library.publish_announcement(req.clone()).unwrap_err(),
        InformationError::AnnouncementBeforeOccurrence { .. }
    ));
    // 相位外（20:00）。
    req.occurred_on = d("2030-04-20");
    req.published_at = CivilInstant::from_hms(d("2030-04-20"), 20, 0, 0).unwrap();
    assert!(matches!(
        library.publish_announcement(req.clone()).unwrap_err(),
        InformationError::PublicationOutsidePhase { .. }
    ));
    // 非次相位：晚一天（正确 18:00 相位但日期 > 发生日）。
    req.published_at = CivilInstant::from_hms(d("2030-04-21"), 18, 0, 0).unwrap();
    assert!(matches!(
        library.publish_announcement(req).unwrap_err(),
        InformationError::AnnouncementNotNextPhase { .. }
    ));
}

/// 无默认日期兜底：序列化缺失 published_at 字段 = 显式反序列化失败，
/// 绝不静默补今天/补 18:00。
#[test]
fn missing_instant_field_is_a_deserialize_error() {
    let base = base();
    let id = base.library.publication_ids()[0];
    let report = base
        .library
        .report(id, base.q1_instant)
        .expect("readable")
        .clone();
    let mut value = serde_json::to_value(&report).expect("serializes");
    let object = value
        .as_object_mut()
        .expect("report serializes to an object");
    assert!(
        object.remove("published_at").is_some(),
        "fixture has the field"
    );
    assert!(
        serde_json::from_value::<engine::information::PublishedReport>(value).is_err(),
        "missing published_at must fail loudly, not default-fill"
    );
}
