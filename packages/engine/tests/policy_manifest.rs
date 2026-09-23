//! 任务2（company-information-npc-intentions）：政策来源清单结构校验。
//!
//! fixture：`tests/fixtures/company-model/policy-sources.json`。只做机器可读结构
//! 校验——日期/适用区间/来源/状态/不支持合同映射，不校验自然语言措辞（由独立
//! 语义 review 负责）。负向用例在进程内构造畸形副本并断言拒绝。

use std::collections::BTreeMap;
use std::fmt;

const MANIFEST_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/company-model/policy-sources.json"
);

/// K3 显式列出必须存在 UnsupportedContract 映射的合同类别。
const REQUIRED_UNSUPPORTED: [&str; 5] = [
    "structured-derivatives",
    "reinsurance",
    "unit-linked-contracts",
    "participating-contracts",
    "other-measurement-models",
];

const COMPANY_KINDS: [&str; 4] = ["industrial", "bank", "insurance", "real_estate"];
const STATUSES: [&str; 3] = ["verified-official", "simulated-game-assumption", "blocked"];
const SOURCE_KINDS: [&str; 5] = [
    "standard",
    "rule",
    "notice",
    "interpretation",
    "game-assumption",
];

#[derive(Debug, PartialEq)]
struct ManifestError {
    code: &'static str,
    detail: String,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.detail)
    }
}

fn err(code: &'static str, detail: impl Into<String>) -> ManifestError {
    ManifestError {
        code,
        detail: detail.into(),
    }
}

fn as_str<'v>(value: &'v serde_json::Value, field: &str) -> Option<&'v str> {
    value
        .get(field)
        .and_then(|f| f.as_str())
        .filter(|s| !s.trim().is_empty())
}

fn as_non_null_str<'v>(value: &'v serde_json::Value, field: &str) -> Option<&'v str> {
    match value.get(field) {
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => Some(s),
        _ => None,
    }
}

/// 校验 ISO `YYYY-MM-DD` 日期可表示（含闰年）；非法返回 Err。
fn check_iso_date(date: &str, entry: &str, field: &str) -> Result<(), ManifestError> {
    let malformed = || {
        err(
            "invalid-date",
            format!("entry {entry} field {field}: {date:?}"),
        )
    };
    let (year, month, day) = match date.split('-').collect::<Vec<_>>()[..] {
        [y, m, d] => {
            let invalid_shape = y.len() != 4
                || m.len() != 2
                || d.len() != 2
                || !y
                    .bytes()
                    .chain(m.bytes())
                    .chain(d.bytes())
                    .all(|b| b.is_ascii_digit());
            if invalid_shape {
                return Err(malformed());
            }
            (y.parse::<u32>(), m.parse::<u32>(), d.parse::<u32>())
        }
        _ => return Err(malformed()),
    };
    let (year, month, day) = (
        year.map_err(|_| malformed())?,
        month.map_err(|_| malformed())?,
        day.map_err(|_| malformed())?,
    );
    if !(1..=12).contains(&month) || day == 0 {
        return Err(malformed());
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_len = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day > month_len {
        return Err(malformed());
    }
    Ok(())
}

fn sources(value: &serde_json::Value) -> Result<Vec<serde_json::Value>, ManifestError> {
    value
        .get("sources")
        .and_then(|s| s.as_array())
        .cloned()
        .ok_or_else(|| err("missing-field", "top-level `sources` array"))
}

fn check_source_entry(
    entry: &serde_json::Value,
    retrieval_date: &str,
) -> Result<String, ManifestError> {
    let id =
        as_str(entry, "id").ok_or_else(|| err("missing-field", "source entry without `id`"))?;
    for field in ["title", "issuer", "retrieval_date"] {
        if as_str(entry, field).is_none() {
            return Err(err("missing-field", format!("source {id}: field {field}")));
        }
    }
    let kind = as_str(entry, "kind").unwrap_or_default();
    if !SOURCE_KINDS.contains(&kind) {
        return Err(err("invalid-kind", format!("source {id}: kind {kind:?}")));
    }
    let status = as_str(entry, "status").unwrap_or_default();
    if !STATUSES.contains(&status) {
        return Err(err(
            "invalid-status",
            format!("source {id}: status {status:?}"),
        ));
    }
    let retrieval = as_str(entry, "retrieval_date").unwrap();
    check_iso_date(retrieval, id, "retrieval_date")?;
    if retrieval != retrieval_date {
        return Err(err(
            "retrieval-date-mismatch",
            format!("source {id}: {retrieval}"),
        ));
    }
    match status {
        "verified-official" => {
            // 官方核验至少要能溯源：原文 URL 必填；文号或官方附件（全文 PDF/docx）
            // 二者其一必须存在——交易所规则存在全文可得而通知文号未核验的情况。
            // 未来生效条目另有更严的 future-without-notice 门（强制文号+发布日期）。
            if as_str(entry, "url").is_none() {
                return Err(err(
                    "missing-source",
                    format!("source {id}: {status} needs url"),
                ));
            }
            if as_str(entry, "document_number").is_none()
                && as_str(entry, "attachment_url").is_none()
            {
                return Err(err(
                    "missing-source",
                    format!("source {id}: {status} needs document_number or attachment_url"),
                ));
            }
        }
        "simulated-game-assumption" => {
            let has_rationale = entry
                .get("game_assumptions")
                .and_then(|a| a.as_array())
                .is_some_and(|a| !a.is_empty());
            if !has_rationale {
                return Err(err(
                    "game-assumption-without-rationale",
                    format!("source {id}"),
                ));
            }
        }
        "blocked" => {
            let has_attempts = entry
                .get("attempts")
                .and_then(|a| a.as_array())
                .is_some_and(|a| !a.is_empty());
            let has_reason = as_str(entry, "blocked_reason").is_some();
            if !has_attempts || !has_reason {
                return Err(err("blocked-without-attempts", format!("source {id}")));
            }
        }
        _ => unreachable!("status validated above"),
    }
    if let Some(issue) = as_str(entry, "issue_date") {
        check_iso_date(issue, id, "issue_date")?;
    }
    Ok(id.to_string())
}

/// 校验来源条目与其适用区间；返回 (scope → [(from, to, id)]) 供重叠检查。
fn check_sources(
    value: &serde_json::Value,
    retrieval_date: &str,
) -> Result<WindowsByScope, ManifestError> {
    let mut windows = BTreeMap::new();
    let mut seen = std::collections::HashSet::new();
    for entry in sources(value)? {
        let id = check_source_entry(&entry, retrieval_date)?;
        if !seen.insert(id.clone()) {
            return Err(err("duplicate-id", format!("source {id}")));
        }
        for window in entry
            .get("windows")
            .and_then(|w| w.as_array())
            .unwrap_or(&vec![])
        {
            let scope = as_str(window, "scope").ok_or_else(|| {
                err(
                    "missing-field",
                    format!("source {id}: window without `scope`"),
                )
            })?;
            let from = as_str(window, "from").ok_or_else(|| {
                err(
                    "missing-field",
                    format!("source {id} window {scope}: `from`"),
                )
            })?;
            check_iso_date(from, &id, "window.from")?;
            let to = as_non_null_str(window, "to");
            if let Some(to) = to {
                check_iso_date(to, &id, "window.to")?;
                if from > to {
                    return Err(err(
                        "inverted-window",
                        format!("source {id} scope {scope}: {from}>{to}"),
                    ));
                }
            }
            let future_official = from > retrieval_date
                && as_str(&entry, "status") == Some("verified-official")
                && as_str(&entry, "kind") != Some("game-assumption");
            if future_official {
                let formal = as_str(&entry, "document_number").is_some()
                    && as_str(&entry, "url").is_some()
                    && as_str(&entry, "issue_date").is_some();
                if !formal {
                    return Err(err(
                        "future-without-notice",
                        format!("source {id}: {from} 生效但缺正式通知引用"),
                    ));
                }
            }
            windows
                .entry(scope.to_string())
                .or_insert_with(Vec::new)
                .push((from.to_string(), to.map(str::to_string), id.clone()));
        }
    }
    Ok(windows)
}

type WindowsByScope = BTreeMap<String, Vec<(String, Option<String>, String)>>;

fn check_no_overlaps(windows: &WindowsByScope) -> Result<(), ManifestError> {
    for (scope, list) in windows {
        let mut sorted = list.clone();
        sorted.sort();
        for pair in sorted.windows(2) {
            let (prev_from, prev_to, prev_id) = &pair[0];
            let (next_from, _, next_id) = &pair[1];
            let prev_end = prev_to.clone().unwrap_or_else(|| "9999-12-31".to_string());
            if prev_end.as_str() >= next_from.as_str() {
                return Err(err(
                    "overlap",
                    format!(
                        "scope {scope}: {prev_id}[{prev_from},{prev_end}] vs {next_id}[{next_from}]"
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn check_unsupported(
    value: &serde_json::Value,
    known_ids: &std::collections::HashSet<String>,
) -> Result<(), ManifestError> {
    let list = value
        .get("unsupported_contracts")
        .and_then(|u| u.as_array())
        .ok_or_else(|| err("missing-field", "top-level `unsupported_contracts` array"))?;
    let mut seen = std::collections::HashSet::new();
    for item in list {
        let id = as_str(item, "id")
            .ok_or_else(|| err("missing-field", "unsupported entry without `id`"))?;
        if as_str(item, "label").is_none() {
            return Err(err("missing-field", format!("unsupported {id}: `label`")));
        }
        let related = item
            .get("related_source_ids")
            .and_then(|r| r.as_array())
            .ok_or_else(|| {
                err(
                    "missing-field",
                    format!("unsupported {id}: `related_source_ids`"),
                )
            })?;
        if related.is_empty() {
            return Err(err(
                "missing-field",
                format!("unsupported {id}: empty `related_source_ids`"),
            ));
        }
        for src in related.iter().filter_map(|s| s.as_str()) {
            if !known_ids.contains(src) {
                return Err(err(
                    "unknown-source-ref",
                    format!("unsupported {id}: source {src}"),
                ));
            }
        }
        seen.insert(id.to_string());
    }
    for required in REQUIRED_UNSUPPORTED {
        if !seen.contains(required) {
            return Err(err(
                "missing-unsupported",
                format!("K3 required {required}"),
            ));
        }
    }
    Ok(())
}

fn check_calendar(
    value: &serde_json::Value,
    known_ids: &std::collections::HashSet<String>,
) -> Result<(), ManifestError> {
    let cal = value
        .get("calendar")
        .ok_or_else(|| err("missing-field", "top-level `calendar`"))?;
    let fixed = [
        ("default_start_date", "2030-01-01"),
        ("runtime_min_start", "2000-01-01"),
        ("runtime_max_end", "2099-12-31"),
        ("init_only_min_start", "1998-01-01"),
        ("day_count_basis", "ACT/365F"),
    ];
    for (field, expected) in fixed {
        let actual =
            as_str(cal, field).ok_or_else(|| err("missing-field", format!("calendar.{field}")))?;
        if actual != expected {
            return Err(err(
                "calendar-frozen-value",
                format!("calendar.{field}={actual:?}, expected {expected:?}"),
            ));
        }
    }
    for coverage in cal
        .get("official_coverage")
        .and_then(|o| o.as_array())
        .unwrap_or(&vec![])
    {
        let year = coverage
            .get("year")
            .and_then(|y| y.as_u64())
            .unwrap_or_default();
        if !(1998..=2099).contains(&year) {
            return Err(err(
                "invalid-date",
                format!("official_coverage year {year}"),
            ));
        }
        for notice in coverage
            .get("notice_source_ids")
            .and_then(|n| n.as_array())
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|n| n.as_str())
        {
            if !known_ids.contains(notice) {
                return Err(err(
                    "unknown-source-ref",
                    format!("calendar year {year}: source {notice}"),
                ));
            }
        }
    }
    let fallback = cal
        .get("simulated_fallback")
        .ok_or_else(|| err("missing-field", "calendar.simulated_fallback"))?;
    if as_str(fallback, "status") != Some("simulated-game-assumption") {
        return Err(err("invalid-status", "calendar.simulated_fallback.status"));
    }
    if fallback
        .get("game_assumptions")
        .and_then(|a| a.as_array())
        .is_none_or(|a| a.is_empty())
    {
        return Err(err(
            "game-assumption-without-rationale",
            "calendar.simulated_fallback",
        ));
    }
    Ok(())
}

fn check_business_events(
    value: &serde_json::Value,
    known_ids: &std::collections::HashSet<String>,
) -> Result<(), ManifestError> {
    let events = value
        .get("business_events")
        .and_then(|b| b.as_array())
        .ok_or_else(|| err("missing-field", "top-level `business_events` array"))?;
    let mut kinds = std::collections::HashSet::new();
    for event in events {
        let kind = as_str(event, "company_kind").unwrap_or_default();
        if !COMPANY_KINDS.contains(&kind) {
            return Err(err("invalid-kind", format!("business_event kind {kind:?}")));
        }
        if as_str(event, "event").is_none() {
            return Err(err("missing-field", "business_event without `event`"));
        }
        let status = as_str(event, "status").unwrap_or_default();
        if !STATUSES.contains(&status) {
            return Err(err(
                "invalid-status",
                format!("business_event {kind}: status {status:?}"),
            ));
        }
        let refs = event
            .get("source_ids")
            .and_then(|s| s.as_array())
            .ok_or_else(|| {
                err(
                    "missing-field",
                    format!("business_event {kind}: `source_ids`"),
                )
            })?;
        let is_game_assumption = status == "simulated-game-assumption";
        if !is_game_assumption && refs.is_empty() {
            return Err(err(
                "missing-source",
                format!("business_event {kind} without source"),
            ));
        }
        for src in refs.iter().filter_map(|s| s.as_str()) {
            if !known_ids.contains(src) {
                return Err(err(
                    "unknown-source-ref",
                    format!("business_event {kind}: source {src}"),
                ));
            }
        }
        kinds.insert(kind.to_string());
    }
    for kind in COMPANY_KINDS {
        if !kinds.contains(kind) {
            return Err(err("missing-coverage", format!("company kind {kind}")));
        }
    }
    Ok(())
}

fn validate(value: &serde_json::Value) -> Result<(), ManifestError> {
    let retrieval_date = as_str(value, "retrieval_date")
        .ok_or_else(|| err("missing-field", "top-level `retrieval_date`"))?;
    check_iso_date(retrieval_date, "manifest", "retrieval_date")?;
    let windows = check_sources(value, retrieval_date)?;
    check_no_overlaps(&windows)?;
    let known: std::collections::HashSet<String> = sources(value)?
        .iter()
        .filter_map(|e| as_str(e, "id").map(str::to_string))
        .collect();
    check_unsupported(value, &known)?;
    check_calendar(value, &known)?;
    check_business_events(value, &known)?;
    Ok(())
}

fn load_manifest() -> serde_json::Value {
    let raw = std::fs::read_to_string(MANIFEST_PATH)
        .unwrap_or_else(|e| panic!("读取 fixture 失败 {MANIFEST_PATH}: {e}"));
    serde_json::from_str(&raw).expect("fixture 必须是合法 JSON")
}

#[test]
fn manifest_fixture_is_valid() {
    let manifest = load_manifest();
    validate(&manifest).unwrap_or_else(|e| panic!("fixture 结构校验失败: {e}"));
    // 2030 默认开局使用财会〔2026〕11 号列报：境内上市公司 2029-01-01 生效窗口必须存在。
    let cas30 = manifest["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"].as_str() == Some("cas-30-presentation-2026"))
        .expect("fixture 必须包含 cas-30-presentation-2026");
    assert_eq!(cas30["document_number"].as_str(), Some("财会〔2026〕11号"));
    assert!(cas30["windows"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |w| w["scope"].as_str() == Some("presentation:listed-domestic")
                && w["from"].as_str() == Some("2029-01-01")
        ));
}

#[test]
fn overlapping_applicability_windows_are_rejected() {
    let mut manifest = load_manifest();
    let with_windows = manifest["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["windows"].as_array().is_some_and(|w| !w.is_empty()))
        .expect("fixture 必须存在带适用区间的来源")
        .clone();
    let mut clone = with_windows;
    clone["id"] = "test-overlap".into();
    let scope = clone["windows"][0]["scope"].as_str().unwrap().to_string();
    let from = clone["windows"][0]["from"].as_str().unwrap().to_string();
    clone["windows"] = serde_json::json!([{ "scope": scope, "from": from, "to": null }]);
    manifest["sources"].as_array_mut().unwrap().push(clone);
    let error = validate(&manifest).expect_err("同 scope 重叠区间必须被拒绝");
    assert_eq!(error.code, "overlap", "unexpected: {error}");
}

#[test]
fn verified_official_without_url_is_rejected() {
    let mut manifest = load_manifest();
    let entry = manifest["sources"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["status"].as_str() == Some("verified-official"))
        .expect("fixture 必须有 verified-official 条目");
    entry.as_object_mut().unwrap().remove("url");
    let error = validate(&manifest).expect_err("verified-official 缺 url 必须被拒绝");
    assert_eq!(error.code, "missing-source", "unexpected: {error}");
}

#[test]
fn future_effective_official_without_formal_notice_is_rejected() {
    let mut manifest = load_manifest();
    let retrieval_date = manifest["retrieval_date"].as_str().unwrap().to_string();
    manifest["sources"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": "test-future",
            "kind": "standard",
            "title": "虚构未来准则",
            "issuer": "测试",
            "retrieval_date": retrieval_date,
            "status": "verified-official",
            "url": "https://kjs.mof.gov.cn/zhengcefabu/fake-for-test.htm",
            "document_number": "测试令第1号",
            "windows": [{ "scope": "test:future", "from": "2031-01-01", "to": null }]
        }));
    let error =
        validate(&manifest).expect_err("未来生效的官方条目必须引用正式通知（文号+原文+发布日期）");
    assert_eq!(error.code, "future-without-notice", "unexpected: {error}");
}

#[test]
fn missing_k3_unsupported_contract_mapping_is_rejected() {
    let mut manifest = load_manifest();
    manifest["unsupported_contracts"]
        .as_array_mut()
        .unwrap()
        .retain(|u| u["id"].as_str() != Some("reinsurance"));
    let error = validate(&manifest).expect_err("K3 列明的再保险必须存在 UnsupportedContract 映射");
    assert_eq!(error.code, "missing-unsupported", "unexpected: {error}");
}
