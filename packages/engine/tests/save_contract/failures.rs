//! Failure path：伪造引用/未来观察/丢失个人状态/错误日历 digest/篡改账本/
//! 资源超限/未知与缺失字段全部类型化拒绝，且拒绝不动原会话与源字节。

use super::{seasoned_json, seasoned_session};
use engine::session::{decode_save_slot, GameSession, SaveDecodeLimits, SessionError};
use serde_json::Value;

/// 序列化篡改档 → 解码 → 恢复；任何成功都是契约破坏（响亮失败）。
fn expect_rejection(value: &Value) -> SessionError {
    match restore_tampered(value) {
        Ok(_) => panic!("tampered save must be rejected"),
        Err(error) => error,
    }
}

/// 序列化篡改档 → 解码 → 恢复；返回第一处类型化拒绝。
fn restore_tampered(value: &Value) -> Result<GameSession, SessionError> {
    let bytes = serde_json::to_vec(value).expect("tampered JSON must serialize");
    let decoded = decode_save_slot(&bytes, &SaveDecodeLimits::default())?;
    GameSession::restore(&decoded)
}

fn first_key(map: &Value) -> String {
    map.as_object()
        .expect("must be an object")
        .keys()
        .next()
        .cloned()
        .expect("must be non-empty")
}

#[test]
fn missing_k7_fields_and_unknown_fields_are_generic_schema_rejections() {
    let base = seasoned_json();
    for field in [
        "company_operations",
        "closing_registry",
        "public_library",
        "ops_wiring",
        "disclosures",
        "plans",
        "information_states",
        "belief_books",
        "watchlists",
        "pending_plan_events",
        "civil_clock",
    ] {
        let mut missing = base.clone();
        missing
            .as_object_mut()
            .unwrap()
            .remove(field)
            .unwrap_or_else(|| panic!("field {field} must exist"));
        let error = expect_rejection(&missing);
        assert!(
            matches!(error, SessionError::InvalidSave(_)),
            "missing {field}: generic schema rejection, no legacy branch: {error:?}"
        );
    }

    // 冻结日历政策本体缺失 = 当前 schema 不合法。
    let mut no_policy = base.clone();
    no_policy["civil_clock"]
        .as_object_mut()
        .unwrap()
        .remove("policy")
        .unwrap();
    assert!(restore_tampered(&no_policy).is_err());

    // 旧格式头顶字段（版本号）不被识别：没有迁移器。
    let mut versioned = base;
    versioned["schema_version"] = Value::from(1);
    let error = expect_rejection(&versioned);
    assert!(matches!(error, SessionError::InvalidSave(_)));
}

#[test]
fn fake_publication_reference_is_rejected() {
    let mut tampered = seasoned_json();
    let states = &mut tampered["information_states"];
    let account = first_key(states);
    let companies = &mut states[&account]["companies"];
    let company = first_key(companies);
    let records = companies[&company].as_array_mut().expect("records array");
    let id = records[0]["id"].as_u64().expect("numeric publication id");
    records[0]["id"] = Value::from(id + 100_000);

    let error = expect_rejection(&tampered);
    assert!(matches!(error, SessionError::InvalidSave(_)), "{error:?}");
}

#[test]
fn unacquired_belief_report_is_rejected() {
    let mut tampered = seasoned_json();
    let books = &mut tampered["belief_books"];
    let account = first_key(books);
    let entries = &mut books[&account]["entries"];
    let code = first_key(entries);
    let used = entries[&code]["used_report_ids"]
        .as_array_mut()
        .expect("used report ids");
    assert!(
        !used.is_empty(),
        "fixture must hold a belief anchored on own-acquired reports"
    );
    used[0] = Value::from(999_999_u32);

    let error = expect_rejection(&tampered);
    assert!(matches!(error, SessionError::InvalidSave(_)), "{error:?}");
}

#[test]
fn future_observation_is_rejected() {
    let mut tampered = seasoned_json();
    let states = &mut tampered["information_states"];
    let account = first_key(states);
    let companies = &mut states[&account]["companies"];
    let company = first_key(companies);
    let records = companies[&company].as_array_mut().expect("records array");
    records[0]["observed_at"]["date"] = Value::from("2030-06-01");

    let error = expect_rejection(&tampered);
    assert!(matches!(error, SessionError::InvalidSave(_)), "{error:?}");
}

#[test]
fn missing_personal_state_is_rejected() {
    // 丢一本信念簿：三图键集失配。
    let mut tampered = seasoned_json();
    let books = &mut tampered["belief_books"];
    let account = first_key(books);
    books.as_object_mut().unwrap().remove(&account).unwrap();
    let error = expect_rejection(&tampered);
    assert!(matches!(error, SessionError::InvalidSave(_)), "{error:?}");

    // 整个信息集表被删空档位（缺字段）同理拒绝。
    let mut missing_map = seasoned_json();
    missing_map
        .as_object_mut()
        .unwrap()
        .remove("information_states")
        .unwrap();
    assert!(restore_tampered(&missing_map).is_err());
}

#[test]
fn tampered_calendar_policy_is_rejected() {
    let mut tampered = seasoned_json();
    tampered["civil_clock"]["policy"]["simulated_fallback"]["digest"] =
        Value::from("0000000000000000");

    let error = expect_rejection(&tampered);
    assert!(
        matches!(
            error,
            SessionError::CivilClock(engine::session::CivilClockError::Calendar(
                engine::calendar::CalendarError::DigestMismatch { .. }
            ))
        ),
        "a calendar policy whose facts digest fails recomputation must be a typed digest rejection: {error:?}"
    );
}

/// 递归找到第一处 `journal`（公司账套嵌套较深；按结构而非路径定位）。
fn find_journal_mut(value: &mut Value) -> Option<&mut Value> {
    match value {
        Value::Object(map) => {
            if map.contains_key("journal") {
                map.get_mut("journal")
            } else {
                map.values_mut().find_map(find_journal_mut)
            }
        }
        Value::Array(items) => items.iter_mut().find_map(find_journal_mut),
        _ => None,
    }
}

#[test]
fn tampered_company_books_are_rejected() {
    let mut tampered = seasoned_json();
    let journal =
        find_journal_mut(&mut tampered).expect("the fixture must carry posted company journals");
    // 翻转第一条分录行的借贷方向：复式平衡被破坏，恢复重放必须显式失败。
    let line = &mut journal["batches"][0][0]["lines"][0];
    let flipped = match line["side"].as_str() {
        Some("Debit") => "Credit",
        Some("Credit") => "Debit",
        other => panic!("unexpected posting side: {other:?}"),
    };
    line["side"] = Value::from(flipped);

    let error = expect_rejection(&tampered);
    assert!(matches!(error, SessionError::InvalidSave(_)), "{error:?}");
}

#[test]
fn oversized_payloads_are_typed_resource_rejections() {
    let bytes = serde_json::to_vec(&seasoned_json()).unwrap();
    let tiny_bytes = SaveDecodeLimits {
        max_total_bytes: 8,
        max_companies: 256,
    };
    let error = match decode_save_slot(&bytes, &tiny_bytes) {
        Ok(_) => panic!("an oversized payload must be rejected before decoding"),
        Err(error) => error,
    };
    assert!(matches!(error, SessionError::ResourceLimit(_)), "{error:?}");

    let tiny_companies = SaveDecodeLimits {
        max_total_bytes: engine::MAX_SAVE_DECODE_BYTES,
        max_companies: 1,
    };
    let error = match decode_save_slot(&bytes, &tiny_companies) {
        Ok(_) => panic!("a save above the company cap must be rejected"),
        Err(error) => error,
    };
    assert!(matches!(error, SessionError::ResourceLimit(_)), "{error:?}");
}

#[test]
fn rejections_leave_the_running_session_untouched() {
    let session = seasoned_session();
    let before = serde_json::to_vec(&session.save()).unwrap();

    let mut base = serde_json::to_value(session.save()).unwrap();
    base["schema_version"] = Value::from(1);
    assert!(restore_tampered(&base).is_err());
    let mut future = serde_json::to_value(session.save()).unwrap();
    future["civil_clock"]["current_date"] = Value::from("2031-01-01");
    assert!(restore_tampered(&future).is_err());

    let after = serde_json::to_vec(&session.save()).unwrap();
    assert_eq!(
        before, after,
        "rejected restores must leave the running session byte-identical"
    );
}
