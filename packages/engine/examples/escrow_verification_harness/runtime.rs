use crate::{
    cli::{Config, MergeDimension, Mode},
    digest::digest_hex,
};
use engine::{
    session::protocol::{ProtocolSession, TickFrame},
    verification_evidence::{RuntimeUpdateRef, UpdateProjection},
    AccountId, CivilDate, Event, FloatAllocation, GameConfig, HotParams, InstParams, Intent, Money,
    NpcSetup, RetailParams, SaveSlot, SecurityCategory, SessionSetup, Side, StockCode,
    StockExchange, StockSpec, StrategyParams, SIMULATION_POLICY_ID,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
};

const CAPTURE_SCHEMA: &str = "escrow-runtime-evidence-capture";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CaptureStatus {
    Pass,
}

#[derive(Clone, Debug, Serialize)]
pub struct ArtifactReceipt {
    byte_length: String,
    sha256: String,
}

#[derive(Clone, Debug, Serialize)]
struct ArtifactFile {
    file: &'static str,
    receipt: ArtifactReceipt,
}

impl ArtifactReceipt {
    fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            byte_length: bytes.len().to_string(),
            sha256: digest_hex(bytes),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct RuntimeConfiguration {
    scenario: String,
    seed: String,
    budget: String,
    actual_rayon_threads: String,
    repeat: String,
    mode: Mode,
    requested_scheduler_merge_disabled: Option<MergeDimension>,
}

#[derive(Clone, Debug, Serialize)]
struct RuntimeCoverage {
    tick_from: String,
    tick_to: String,
    tick_frames: String,
    civil_updates: String,
    auction_completed_events: String,
    day_boundary_events: String,
    stock_codes: Vec<String>,
    account_ids: Vec<String>,
    restore_slots: Vec<RestoreSlotReceipt>,
}

#[derive(Clone, Debug, Serialize)]
struct RestoreSlotReceipt {
    slot: String,
    saved: ArtifactReceipt,
    restored: ArtifactReceipt,
    uninterrupted_continuation: ArtifactReceipt,
    restored_continuation: ArtifactReceipt,
    restored_conservation: Vec<engine::verification_evidence::ConservationSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
struct PublicPayloadOrderProbe {
    scope: &'static str,
    account_payload_order: Vec<String>,
    stock_payload_order: Vec<String>,
    event_payload_order: Vec<String>,
    canonicalized_bytes: ArtifactReceipt,
    probe_disabled_sort_changed_bytes: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
struct UnavailableCapture {
    available: bool,
    reason: &'static str,
}

#[derive(Clone, Debug, Serialize)]
struct ProducerReadiness {
    update_projection_schema: &'static str,
    update_projection_count: String,
    full_update_stream_ordinal_scope: UnavailableCapture,
    determinism_observation: Option<Value>,
    /// 固定 null 占位：密封语料投影面已随 escrow 语料工具删除；保留该键以
    /// 维持现行 capture 报告的 JSON 键集稳定。
    corpus_projection: Option<Value>,
    conservation_snapshots: Vec<Value>,
}

#[derive(Clone, Debug, Serialize)]
struct TypedBlocker {
    code: &'static str,
    required_for: &'static str,
    current_symbol: &'static str,
    minimum_missing_seam: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct CaptureReport {
    schema: &'static str,
    schema_version: u32,
    status: CaptureStatus,
    configuration: RuntimeConfiguration,
    authority_path: &'static str,
    artifacts: BTreeMap<&'static str, ArtifactFile>,
    runtime_coverage: RuntimeCoverage,
    scheduler_precanonical_order: Value,
    public_payload_order_probe: PublicPayloadOrderProbe,
    producer_readiness: ProducerReadiness,
    blockers: Vec<TypedBlocker>,
    scope: &'static str,
    negative_control: Option<Value>,
}

struct CaptureArtifact {
    name: &'static str,
    file: &'static str,
    bytes: Vec<u8>,
    receipt: ArtifactReceipt,
}

impl CaptureArtifact {
    fn from_bytes(name: &'static str, file: &'static str, bytes: Vec<u8>) -> Self {
        let receipt = ArtifactReceipt::from_bytes(&bytes);
        Self {
            name,
            file,
            bytes,
            receipt,
        }
    }

    fn report_entry(&self) -> (&'static str, ArtifactFile) {
        (
            self.name,
            ArtifactFile {
                file: self.file,
                receipt: self.receipt.clone(),
            },
        )
    }
}

fn capture_artifacts(
    authoritative_state: Vec<u8>,
    event_stream: Vec<u8>,
    save_slot: Vec<u8>,
    receipts: Vec<u8>,
) -> Vec<CaptureArtifact> {
    // Writer 顺序沿用旧契约，报告仍投影到 BTreeMap 的规范键顺序。
    vec![
        CaptureArtifact::from_bytes(
            "authoritative_state",
            "authoritative-state.json",
            authoritative_state,
        ),
        CaptureArtifact::from_bytes("event_stream", "event-stream.json", event_stream),
        CaptureArtifact::from_bytes("save_slot", "save-slot.json", save_slot),
        CaptureArtifact::from_bytes("receipts", "receipts.json", receipts),
    ]
}

pub struct CaptureBundle {
    pub report: CaptureReport,
    artifacts: Vec<CaptureArtifact>,
}

#[derive(Clone, Debug, Serialize)]
struct CollectorRow {
    identity: String,
    payload: Value,
}

#[derive(Clone)]
struct CollectorRows {
    accounts: Vec<CollectorRow>,
    stocks: Vec<CollectorRow>,
    completions: Vec<CollectorRow>,
}

struct RestoreSlotCapture {
    slot: String,
    saved: Vec<u8>,
    restored: Vec<u8>,
    uninterrupted_continuation: Vec<u8>,
    restored_continuation: Vec<u8>,
    restored_conservation: Vec<engine::verification_evidence::ConservationSnapshot>,
}

#[path = "committed.rs"]
mod committed;
use committed::RuntimeCapture;

pub fn execute(config: &Config) -> Result<CaptureBundle, String> {
    committed::execute(config)
}

fn assemble(
    config: &Config,
    actual_threads: usize,
    capture: RuntimeCapture,
) -> Result<CaptureBundle, String> {
    if capture.restore_slots.len() != 2 {
        return Err("runtime capture did not exercise exactly two restore slots".to_owned());
    }
    if capture.auction_completed == 0 || capture.day_boundaries == 0 || capture.civil_updates == 0 {
        return Err(
            "runtime capture missed auction, day-boundary, or CivilUpdate coverage".to_owned(),
        );
    }

    let authoritative_state = serde_json::to_vec(&capture.snapshots)
        .map_err(|error| format!("authoritative snapshot stream serialization failed: {error}"))?;
    let event_stream = serde_json::to_vec(&capture.updates)
        .map_err(|error| format!("event projection stream serialization failed: {error}"))?;
    let save_slot = serde_json::to_vec(&capture.final_save)
        .map_err(|error| format!("final SaveSlot serialization failed: {error}"))?;
    let receipts = serde_json::to_vec(&capture.receipts)
        .map_err(|error| format!("committed receipt serialization failed: {error}"))?;

    let rows = collector_rows(&capture)?;
    let (precanonical, collector_bytes, disabled_changed) =
        collect_rows(rows, config.mode, config.disabled_merge)?;
    let stock_codes = capture
        .final_save
        .setup
        .stocks
        .iter()
        .map(|stock| stock.code.0.clone())
        .collect();
    let account_ids = capture
        .final_save
        .snapshot
        .accounts
        .keys()
        .map(|account| account.0.to_string())
        .collect();
    let restore_slots = capture
        .restore_slots
        .iter()
        .map(|slot| RestoreSlotReceipt {
            slot: slot.slot.clone(),
            saved: ArtifactReceipt::from_bytes(&slot.saved),
            restored: ArtifactReceipt::from_bytes(&slot.restored),
            uninterrupted_continuation: ArtifactReceipt::from_bytes(
                &slot.uninterrupted_continuation,
            ),
            restored_continuation: ArtifactReceipt::from_bytes(&slot.restored_continuation),
            restored_conservation: slot.restored_conservation.clone(),
        })
        .collect();

    let executor_order = committed::identity_orders(&capture.executor_records)?;
    let observation = observation(
        config,
        &capture,
        &executor_order,
        &authoritative_state,
        &event_stream,
        &receipts,
        &save_slot,
    )?;
    let artifacts = capture_artifacts(authoritative_state, event_stream, save_slot, receipts);
    let report = CaptureReport {
        schema: CAPTURE_SCHEMA,
        schema_version: 1,
        status: CaptureStatus::Pass,
        configuration: RuntimeConfiguration {
            scenario: config.scenario.clone(),
            seed: config.seed.to_string(),
            budget: config.budget.label().to_owned(),
            actual_rayon_threads: actual_threads.to_string(),
            repeat: config.repeat.to_string(),
            mode: config.mode,
            requested_scheduler_merge_disabled: config.disabled_merge,
        },
        authority_path: "ProtocolSession::step_frame_with_commit_evidence -> GameSession::step -> pipeline::execute_authoritative_tick",
        artifacts: artifacts.iter().map(CaptureArtifact::report_entry).collect(),
        runtime_coverage: RuntimeCoverage {
            tick_from: capture.tick_from.to_string(),
            tick_to: capture.tick_to.to_string(),
            tick_frames: capture.tick_frames.to_string(),
            civil_updates: capture.civil_updates.to_string(),
            auction_completed_events: capture.auction_completed.to_string(),
            day_boundary_events: capture.day_boundaries.to_string(),
            stock_codes,
            account_ids,
            restore_slots,
        },
        scheduler_precanonical_order: serde_json::json!({
            "available": true,
            "scope": "actual production dispatch and completion vectors before canonical merge",
            "account_shards": executor_order.accounts,
            "stock_shards": executor_order.stocks,
            "completion_order": executor_order.completions,
            "records": capture.executor_records,
        }),
        public_payload_order_probe: PublicPayloadOrderProbe {
            scope: "real serialized public-runtime account, stock, and event payloads; explicitly not production executor pre-canonical shard evidence",
            account_payload_order: precanonical.accounts,
            stock_payload_order: precanonical.stocks,
            event_payload_order: precanonical.completions,
            canonicalized_bytes: ArtifactReceipt::from_bytes(&collector_bytes),
            probe_disabled_sort_changed_bytes: disabled_changed,
        },
        producer_readiness: ProducerReadiness {
            update_projection_schema: "verification_evidence::UpdateProjection",
            update_projection_count: capture.updates.len().to_string(),
            full_update_stream_ordinal_scope: UnavailableCapture {
                available: true,
                reason: "one UpdateStreamProjector carries all TickFrame and CivilUpdate ordinals",
            },
            determinism_observation: Some(observation),
            corpus_projection: None,
            conservation_snapshots: capture.conservation.iter().map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?,
        },
        blockers: Vec::new(),
        scope: "runtime determinism capture only; historical corpus and performance require separate escrow verification acceptance",
        negative_control: None,
    };
    Ok(CaptureBundle { report, artifacts })
}

struct PrecanonicalIdentities {
    accounts: Vec<String>,
    stocks: Vec<String>,
    completions: Vec<String>,
}

fn observation(
    config: &Config,
    capture: &RuntimeCapture,
    order: &PrecanonicalIdentities,
    authoritative_state: &[u8],
    event_stream: &[u8],
    receipts: &[u8],
    save_slot: &[u8],
) -> Result<Value, String> {
    use engine::verification_evidence::*;
    struct Sha256;
    impl Sha256Provider for Sha256 {
        fn digest_hex(&self, bytes: &[u8]) -> String {
            digest_hex(bytes)
        }
    }
    let restore_slots = capture
        .restore_slots
        .iter()
        .map(|slot| RestoreSlotBytes {
            slot: &slot.slot,
            saved: &slot.saved,
            restored: &slot.restored,
            uninterrupted_continuation: &slot.uninterrupted_continuation,
            restored_continuation: &slot.restored_continuation,
        })
        .collect::<Vec<_>>();
    let result = project_observation(
        ObservationInput {
            scenario: &config.scenario,
            seed: config.seed,
            budget: config.budget.label(),
            repeat: config.repeat,
            mode: match config.mode {
                Mode::Canonical => ObservationMode::Canonical,
                Mode::Perturbed => ObservationMode::Perturbed,
                Mode::NegativeControl => ObservationMode::NegativeControl,
            },
            canonical_merge_disabled: config.disabled_merge.map(|dimension| match dimension {
                MergeDimension::Completion => "completion",
            }),
            artifacts: ObservationArtifactBytes {
                authoritative_state,
                event_stream,
                receipts,
                save_slot: Some(save_slot),
            },
            precanonical_order: PrecanonicalOrderInput {
                account_shards: &order.accounts,
                stock_shards: &order.stocks,
                completion_order: &order.completions,
            },
            finalizers: &capture.finalizers,
            conservation: &capture.conservation,
            restore_slots: Some(&restore_slots),
        },
        &Sha256,
    )
    .map_err(|error| format!("determinism observation: {error}"))?;
    serde_json::to_value(result).map_err(|error| error.to_string())
}

fn collector_rows(capture: &RuntimeCapture) -> Result<CollectorRows, String> {
    let accounts = capture
        .final_save
        .snapshot
        .accounts
        .iter()
        .map(|(account, state)| {
            Ok(CollectorRow {
                identity: format!("account:{}", account.0),
                payload: serde_json::to_value(state)
                    .map_err(|error| format!("account collector serialization failed: {error}"))?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let stocks = capture
        .final_save
        .snapshot
        .markets
        .iter()
        .map(|(stock, state)| {
            Ok(CollectorRow {
                identity: format!("stock:{}", stock.0),
                payload: serde_json::to_value(state)
                    .map_err(|error| format!("stock collector serialization failed: {error}"))?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let completions = capture
        .updates
        .iter()
        .flat_map(|update| update.events.iter())
        .map(|fact| {
            let (tick, phase, entity, ordinal) = &fact.comparison_event_key;
            CollectorRow {
                identity: format!("{tick}/{phase}/{entity}/{ordinal}"),
                payload: fact.event.clone(),
            }
        })
        .collect::<Vec<_>>();
    for (dimension, count) in [
        ("account", accounts.len()),
        ("stock", stocks.len()),
        ("completion", completions.len()),
    ] {
        if count < 2 {
            return Err(format!(
                "collector {dimension} dimension has fewer than two real identities"
            ));
        }
    }
    Ok(CollectorRows {
        accounts,
        stocks,
        completions,
    })
}

fn collect_rows(
    mut rows: CollectorRows,
    mode: Mode,
    disabled: Option<MergeDimension>,
) -> Result<(PrecanonicalIdentities, Vec<u8>, Option<bool>), String> {
    if mode != Mode::Canonical {
        rows.accounts.reverse();
        rows.stocks.reverse();
        rows.completions.reverse();
    }
    let precanonical = PrecanonicalIdentities {
        accounts: identities(&rows.accounts),
        stocks: identities(&rows.stocks),
        completions: identities(&rows.completions),
    };
    let canonical_bytes = merge_bytes(rows.clone(), None)?;
    let output = merge_bytes(rows, disabled)?;
    let disabled_changed = disabled.map(|_| output != canonical_bytes);
    if mode == Mode::NegativeControl && disabled_changed != Some(true) {
        return Err("disabled canonical merge did not change real collector bytes".to_owned());
    }
    Ok((precanonical, output, disabled_changed))
}

fn merge_bytes(
    mut rows: CollectorRows,
    disabled: Option<MergeDimension>,
) -> Result<Vec<u8>, String> {
    rows.accounts
        .sort_by(|left, right| left.identity.cmp(&right.identity));
    rows.stocks
        .sort_by(|left, right| left.identity.cmp(&right.identity));
    if disabled != Some(MergeDimension::Completion) {
        rows.completions
            .sort_by(|left, right| left.identity.cmp(&right.identity));
    }
    serde_json::to_vec(&serde_json::json!({
        "accounts": rows.accounts,
        "stocks": rows.stocks,
        "completions": rows.completions,
    }))
    .map_err(|error| format!("collector merge serialization failed: {error}"))
}

fn identities(rows: &[CollectorRow]) -> Vec<String> {
    rows.iter().map(|row| row.identity.clone()).collect()
}

fn frozen_setup() -> Result<SessionSetup, String> {
    let setup = SessionSetup {
        company_operations: None,
        groups: Vec::new(),
        stocks: vec![
            stock(
                "600101",
                StockExchange::Shanghai,
                SecurityCategory::MainBoard,
                1_120,
            ),
            stock(
                "000812",
                StockExchange::Shenzhen,
                SecurityCategory::MainBoard,
                885,
            ),
        ],
        npcs: NpcSetup {
            retail_count: 4,
            inst_count: 2,
            hot_count: 2,
            retail_cash_median: Money::from_cents(20_000_000),
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.8,
                order_size_mean: 200,
                chase_prob: 0.4,
            },
            inst: InstParams {
                margin: 0.02,
                order_size: 500,
            },
            hot: HotParams {
                lookback: 3,
                trend_threshold: 0.01,
                order_size: 300,
            },
        },
        ticks_per_day: 12,
        auction_ticks: 3,
        closing_auction_ticks: 2,
        history_len: 24,
        t1_enabled: true,
        float_allocation: FloatAllocation::Random,
        start_date: CivilDate::from_iso("2030-01-02").map_err(|error| error.to_string())?,
        simulation_policy_id: SIMULATION_POLICY_ID.to_owned(),
    };
    setup
        .validate()
        .map_err(|error| format!("frozen escrow verification setup is invalid: {error}"))?;
    Ok(setup)
}

fn stock(
    code: &str,
    exchange: StockExchange,
    category: SecurityCategory,
    initial_price: i64,
) -> StockSpec {
    StockSpec {
        code: StockCode(code.to_owned()),
        exchange,
        initial_price: Money::from_cents(initial_price),
        category,
        limit_pct: category.limit_pct(),
        tick: Money::from_cents(1),
        total_shares: 10_000_000,
        float_shares: 2_000_000,
    }
}

struct CaptureArtifactWriter {
    output: PathBuf,
}

impl CaptureArtifactWriter {
    fn create(output: &Path) -> Result<Self, String> {
        fs::create_dir(output).map_err(|error| {
            format!(
                "output directory {} must be new and creatable: {error}",
                output.display()
            )
        })?;
        Ok(Self {
            output: output.to_path_buf(),
        })
    }

    // 顺序写入保持 artifact 字节契约；失败可能留下部分输出，不承诺原子发布。
    fn write(&self, bundle: &CaptureBundle) -> Result<(), String> {
        self.write_json("capture.json", &bundle.report)?;
        let capture_bytes = fs::read(self.output.join("capture.json"))
            .map_err(|error| format!("cannot read capture report for receipt: {error}"))?;
        self.write_json(
            "capture-receipt.json",
            &serde_json::json!({
                "schema": "escrow-capture-receipt", "schema_version": 1,
                "file": "capture.json",
                "sha256": digest_hex(&capture_bytes),
                "byte_length": capture_bytes.len().to_string(),
            }),
        )?;
        for artifact in &bundle.artifacts {
            self.write_bytes(artifact.file, &artifact.bytes)?;
        }
        Ok(())
    }

    fn write_json(&self, name: &str, value: &impl Serialize) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|error| format!("capture report serialization failed: {error}"))?;
        self.write_bytes(name, &bytes)
    }

    fn write_bytes(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.output.join(name);
        fs::write(&path, bytes).map_err(|error| format!("cannot write {}: {error}", path.display()))
    }
}

pub fn write_bundle(bundle: &CaptureBundle, output: &Path) -> Result<(), String> {
    CaptureArtifactWriter::create(output)?.write(bundle)
}

impl CaptureBundle {
    pub fn passed(&self) -> bool {
        self.report.status == CaptureStatus::Pass
    }
}

struct WorkspacePathPolicy {
    workspace: PathBuf,
    temp_root: PathBuf,
    output: PathBuf,
    inputs: BTreeMap<String, PathBuf>,
}

impl WorkspacePathPolicy {
    fn validate(config: &Config) -> Result<Self, String> {
        let workspace = std::env::var("ESCROW_WORKSPACE_ROOT")
            .map_err(|_| "ESCROW_WORKSPACE_ROOT must name the shared repository root".to_owned())?;
        let workspace =
            canonical_existing_directory("ESCROW_WORKSPACE_ROOT", Path::new(&workspace))?;
        let temp_root = canonical_workspace_temp_root(&workspace)?;
        let output = validate_new_output(&config.output, &temp_root)?;
        let mut policy = Self {
            workspace,
            temp_root,
            output,
            inputs: BTreeMap::new(),
        };
        for name in ["CARGO_TARGET_DIR", "TMPDIR", "TMP", "TEMP"] {
            let value = std::env::var(name).map_err(|_| format!("{name} must be set"))?;
            let path = canonical_existing_directory(name, Path::new(&value))?;
            policy.validate_input(name, &path)?;
            policy.inputs.insert(name.to_owned(), path);
        }
        Ok(policy)
    }

    fn validate_input(&self, name: &str, path: &Path) -> Result<(), String> {
        require_temp_descendant(name, path, &self.temp_root)?;
        verify_writable_directory(name, path)
    }

    fn paths(self) -> BTreeMap<String, String> {
        let mut paths = BTreeMap::new();
        paths.insert(
            "ESCROW_WORKSPACE_ROOT".to_owned(),
            self.workspace.display().to_string(),
        );
        for (name, path) in self.inputs {
            paths.insert(name, path.display().to_string());
        }
        paths.insert("OUTPUT".to_owned(), self.output.display().to_string());
        paths
    }
}

pub fn validate_workspace_paths(config: &Config) -> Result<BTreeMap<String, String>, String> {
    Ok(WorkspacePathPolicy::validate(config)?.paths())
}

fn canonical_existing_directory(name: &str, path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!(
            "{name} must be an absolute path: {}",
            path.display()
        ));
    }
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("cannot canonicalize {name} {}: {error}", path.display()))?;
    if !canonical.is_dir() {
        return Err(format!("{name} {} is not a directory", canonical.display()));
    }
    Ok(canonical)
}

fn canonical_workspace_temp_root(workspace: &Path) -> Result<PathBuf, String> {
    let temp_root = canonical_existing_directory("workspace .tmp", &workspace.join(".tmp"))?;
    if temp_root == workspace || !temp_root.starts_with(workspace) {
        return Err(format!(
            "workspace .tmp must resolve below canonical workspace root {}",
            workspace.display()
        ));
    }
    Ok(temp_root)
}

fn require_temp_descendant(name: &str, path: &Path, temp_root: &Path) -> Result<(), String> {
    if path == temp_root || !path.starts_with(temp_root) {
        return Err(format!(
            "{name} must resolve to a descendant of workspace temp root {}",
            temp_root.display()
        ));
    }
    Ok(())
}

fn validate_new_output(output: &Path, temp_root: &Path) -> Result<PathBuf, String> {
    if !output.is_absolute()
        || output
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(format!(
            "--output must be an absolute normalized new directory below {}",
            temp_root.display()
        ));
    }
    if output.exists() {
        return Err(format!(
            "--output must not already exist: {}",
            output.display()
        ));
    }
    let parent = output
        .parent()
        .ok_or_else(|| "--output has no parent directory".to_owned())?;
    let parent = canonical_existing_directory("--output parent", parent)?;
    require_temp_descendant("--output parent", &parent, temp_root)?;
    verify_writable_directory("--output parent", &parent)?;
    let name = output
        .file_name()
        .ok_or_else(|| "--output has no final directory name".to_owned())?;
    let resolved = parent.join(name);
    require_temp_descendant("--output", &resolved, temp_root)?;
    Ok(resolved)
}

fn verify_writable_directory(name: &str, path: &Path) -> Result<(), String> {
    let probe = path.join(format!(
        ".escrow-verification-write-probe-{}",
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|error| format!("{name} {} is not writable: {error}", path.display()))?;
    file.write_all(b"workspace-local write probe")
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("{name} write probe failed: {error}"))?;
    drop(file);
    fs::remove_file(&probe).map_err(|error| {
        format!(
            "{name} write probe cleanup failed for {}: {error}",
            probe.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Budget, SCENARIO};

    fn config(mode: Mode, disabled_merge: Option<MergeDimension>) -> Config {
        Config {
            scenario: SCENARIO.to_owned(),
            seed: 41,
            budget: Budget::Two,
            repeat: 0,
            mode,
            disabled_merge,
            output: "/does/not/write/in/unit/test".into(),
        }
    }

    fn assert_artifact_receipts(bundle: &CaptureBundle) {
        assert_eq!(bundle.artifacts.len(), 4);
        for artifact in &bundle.artifacts {
            assert_eq!(
                artifact.receipt.byte_length,
                artifact.bytes.len().to_string()
            );
            assert_eq!(artifact.receipt.sha256, digest_hex(&artifact.bytes));
            let report_entry = &bundle.report.artifacts[artifact.name];
            assert_eq!(report_entry.file, artifact.file);
            assert_eq!(
                report_entry.receipt.byte_length,
                artifact.receipt.byte_length
            );
            assert_eq!(report_entry.receipt.sha256, artifact.receipt.sha256);
        }
    }

    #[test]
    fn capture_artifact_receipts_bind_empty_and_large_bytes() {
        for bytes in [Vec::new(), vec![0x5a; 131_072]] {
            let artifact = CaptureArtifact::from_bytes("receipts", "receipts.json", bytes.clone());
            assert_eq!(artifact.bytes, bytes);
            assert_eq!(artifact.receipt.byte_length, bytes.len().to_string());
            assert_eq!(artifact.receipt.sha256, digest_hex(&bytes));
            let (name, entry) = artifact.report_entry();
            assert_eq!(name, "receipts");
            assert_eq!(entry.file, "receipts.json");
            assert_eq!(entry.receipt.byte_length, artifact.receipt.byte_length);
            assert_eq!(entry.receipt.sha256, artifact.receipt.sha256);
        }
    }

    struct WriterTestDirectory(PathBuf);

    impl WriterTestDirectory {
        fn create(case: &str) -> Self {
            let registered = std::env::var("TMPDIR")
                .expect("Writer tests require registered workspace-local TMPDIR");
            let parent = canonical_existing_directory("Writer test TMPDIR", Path::new(&registered))
                .expect("Writer test TMPDIR must be an existing absolute directory");
            assert!(parent
                .ancestors()
                .any(|path| path.file_name().is_some_and(|name| name == ".tmp")));
            let root = parent.join(format!("capture-writer-{}-{case}", std::process::id()));
            fs::create_dir(&root).expect("create fresh Writer test directory");
            Self(root)
        }
    }

    impl Drop for WriterTestDirectory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("clean Writer test directory");
        }
    }

    fn writer_bundle() -> CaptureBundle {
        // 仅验证 Writer I/O：固定短字节与 DTO，不执行市场，也不声称真实 runtime coverage。
        let artifacts = capture_artifacts(
            br#"{"state":"fixture"}"#.to_vec(),
            b"[]".to_vec(),
            br#"{"save":"fixture"}"#.to_vec(),
            b"[]".to_vec(),
        );
        CaptureBundle {
            report: CaptureReport {
                schema: CAPTURE_SCHEMA,
                schema_version: 1,
                status: CaptureStatus::Pass,
                configuration: RuntimeConfiguration {
                    scenario: "writer-io-fixture".to_owned(),
                    seed: "0".to_owned(),
                    budget: "1".to_owned(),
                    actual_rayon_threads: "1".to_owned(),
                    repeat: "0".to_owned(),
                    mode: Mode::Canonical,
                    requested_scheduler_merge_disabled: None,
                },
                authority_path: "Writer I/O fixture only",
                artifacts: artifacts
                    .iter()
                    .map(CaptureArtifact::report_entry)
                    .collect(),
                runtime_coverage: RuntimeCoverage {
                    tick_from: "0".to_owned(),
                    tick_to: "0".to_owned(),
                    tick_frames: "0".to_owned(),
                    civil_updates: "0".to_owned(),
                    auction_completed_events: "0".to_owned(),
                    day_boundary_events: "0".to_owned(),
                    stock_codes: Vec::new(),
                    account_ids: Vec::new(),
                    restore_slots: Vec::new(),
                },
                scheduler_precanonical_order: Value::Null,
                public_payload_order_probe: PublicPayloadOrderProbe {
                    scope: "Writer I/O fixture only",
                    account_payload_order: Vec::new(),
                    stock_payload_order: Vec::new(),
                    event_payload_order: Vec::new(),
                    canonicalized_bytes: ArtifactReceipt::from_bytes(&[]),
                    probe_disabled_sort_changed_bytes: None,
                },
                producer_readiness: ProducerReadiness {
                    update_projection_schema: "Writer I/O fixture only",
                    update_projection_count: "0".to_owned(),
                    full_update_stream_ordinal_scope: UnavailableCapture {
                        available: false,
                        reason: "Writer I/O fixture only",
                    },
                    determinism_observation: None,
                    corpus_projection: None,
                    conservation_snapshots: Vec::new(),
                },
                blockers: Vec::new(),
                scope: "Writer I/O fixture only; no runtime acceptance claim",
                negative_control: None,
            },
            artifacts,
        }
    }

    #[test]
    fn capture_writer_persists_exact_bytes_and_receipts_and_refuses_existing_output() {
        let directory = WriterTestDirectory::create("complete");
        let output = directory.0.join("output");
        let bundle = writer_bundle();
        write_bundle(&bundle, &output).unwrap();
        let report_bytes = fs::read(output.join("capture.json")).unwrap();
        assert_eq!(
            report_bytes,
            serde_json::to_vec_pretty(&bundle.report).unwrap()
        );
        let report: Value = serde_json::from_slice(&report_bytes).unwrap();
        for artifact in &bundle.artifacts {
            let persisted = fs::read(output.join(artifact.file)).unwrap();
            assert_eq!(persisted, artifact.bytes);
            assert_eq!(report["artifacts"][artifact.name]["file"], artifact.file);
            assert_eq!(
                report["artifacts"][artifact.name]["receipt"]["byte_length"],
                persisted.len().to_string()
            );
            assert_eq!(
                report["artifacts"][artifact.name]["receipt"]["sha256"],
                digest_hex(&persisted)
            );
        }
        let receipt: Value =
            serde_json::from_slice(&fs::read(output.join("capture-receipt.json")).unwrap())
                .unwrap();
        assert_eq!(receipt["file"], "capture.json");
        assert_eq!(receipt["byte_length"], report_bytes.len().to_string());
        assert_eq!(receipt["sha256"], digest_hex(&report_bytes));
        assert!(write_bundle(&bundle, &output)
            .unwrap_err()
            .contains("must be new and creatable"));
        assert_eq!(fs::read(output.join("capture.json")).unwrap(), report_bytes);
    }

    #[test]
    fn capture_writer_keeps_prior_files_and_stops_later_files_on_partial_write_failure() {
        let directory = WriterTestDirectory::create("partial");
        let output = directory.0.join("output");
        let writer = CaptureArtifactWriter::create(&output).unwrap();
        let bundle = writer_bundle();
        fs::create_dir(output.join("event-stream.json")).unwrap();
        let error = writer.write(&bundle).unwrap_err();
        assert!(error.contains("cannot write"));
        assert!(error.contains("event-stream.json"));
        assert!(output.join("capture.json").is_file());
        assert!(output.join("capture-receipt.json").is_file());
        assert_eq!(
            fs::read(output.join("authoritative-state.json")).unwrap(),
            bundle.artifacts[0].bytes
        );
        assert!(output.join("event-stream.json").is_dir());
        assert!(!output.join("save-slot.json").exists());
        assert!(!output.join("receipts.json").exists());
    }

    #[test]
    fn real_protocol_capture_has_committed_evidence_without_claiming_full_acceptance() {
        let bundle = execute(&config(Mode::Canonical, None)).unwrap();
        assert!(bundle.passed());
        assert_artifact_receipts(&bundle);
        assert!(bundle
            .report
            .scope
            .contains("historical corpus and performance require separate"));
        assert!(
            bundle
                .report
                .runtime_coverage
                .tick_frames
                .parse::<u64>()
                .unwrap()
                >= 13
        );
        assert_eq!(bundle.report.runtime_coverage.civil_updates, "1");
        assert!(bundle.report.runtime_coverage.auction_completed_events != "0");
        assert_eq!(bundle.report.runtime_coverage.day_boundary_events, "1");
        assert_eq!(bundle.report.runtime_coverage.restore_slots.len(), 2);
        for slot in &bundle.report.runtime_coverage.restore_slots {
            assert_eq!(slot.saved.sha256, slot.restored.sha256);
            assert!(
                slot.uninterrupted_continuation
                    .byte_length
                    .parse::<usize>()
                    .unwrap()
                    > 0
            );
            assert!(
                slot.restored_continuation
                    .byte_length
                    .parse::<usize>()
                    .unwrap()
                    > 0
            );
        }
        assert!(bundle
            .report
            .producer_readiness
            .determinism_observation
            .is_some());
        assert_eq!(
            bundle
                .report
                .producer_readiness
                .conservation_snapshots
                .len(),
            13
        );
        assert_eq!(bundle.report.producer_readiness.corpus_projection, None);
        assert!(
            bundle
                .report
                .producer_readiness
                .full_update_stream_ordinal_scope
                .available
        );
        assert_eq!(
            bundle.report.scheduler_precanonical_order["available"],
            true
        );
        assert!(bundle.report.blockers.is_empty());
        let receipts: Vec<Value> = serde_json::from_slice(
            &bundle
                .artifacts
                .iter()
                .find(|artifact| artifact.name == "receipts")
                .unwrap()
                .bytes,
        )
        .unwrap();
        assert_eq!(receipts.len(), 13);
        assert!(receipts
            .iter()
            .any(|tick| tick["receipts"].as_array().unwrap().len() >= 8));
        assert_eq!(bundle.report.runtime_coverage.account_ids.len(), 9);
    }

    #[test]
    fn restored_continuations_validate_their_own_committed_receipt_chains() {
        let capture = committed::capture_short_restore_runtime(17).unwrap();
        assert_eq!(capture.restore_slots.len(), 2);
        let mut restored_receipt_count = 0;
        for slot in capture.restore_slots {
            let continuation: Value = serde_json::from_slice(&slot.restored_continuation).unwrap();
            restored_receipt_count += continuation[1]["receipts"].as_array().unwrap().len();
            assert_eq!(slot.restored_conservation.len(), 1);
            let snapshot = &slot.restored_conservation[0];
            assert_eq!(snapshot.tick, continuation[0]["tick"].as_u64().unwrap());
            assert_eq!(snapshot.seed, "17");
            assert_eq!(snapshot.scenario, crate::cli::SCENARIO);
            assert!(!snapshot.accounts.is_empty());
        }
        assert!(restored_receipt_count > 0);
    }

    #[test]
    fn public_payload_probe_uses_real_identities_and_negative_controls_diverge() {
        let capture = committed::capture_runtime(43).unwrap();
        let rows = collector_rows(&capture).unwrap();
        let canonical = collect_rows(rows.clone(), Mode::Canonical, None).unwrap();
        let perturbed = collect_rows(rows.clone(), Mode::Perturbed, None).unwrap();
        assert_eq!(canonical.1, perturbed.1);
        assert_ne!(canonical.0.accounts, perturbed.0.accounts);
        assert_ne!(canonical.0.stocks, perturbed.0.stocks);
        assert_ne!(canonical.0.completions, perturbed.0.completions);
        for dimension in [MergeDimension::Completion] {
            let negative =
                collect_rows(rows.clone(), Mode::NegativeControl, Some(dimension)).unwrap();
            assert_eq!(negative.2, Some(true));
            assert_ne!(negative.1, canonical.1);
        }
    }

    #[test]
    fn perturbation_gate_disabled_merges_reject_real_execution_without_partial_publication() {
        for dimension in [MergeDimension::Completion] {
            let bundle = execute(&config(Mode::NegativeControl, Some(dimension))).unwrap();
            assert!(bundle.passed());
            assert_artifact_receipts(&bundle);
            let witness = bundle.report.negative_control.as_ref().unwrap();
            assert_eq!(witness["detected"], true);
            assert_eq!(witness["kind"], "typed-rejection-with-rollback");
            assert_eq!(witness["enabled_merge_same_step_passed"], true);
            assert_eq!(witness["failed_step_published_events"], "0");
            assert_eq!(witness["before_failed_step"], witness["after_failed_step"]);
            assert_eq!(
                bundle.report.producer_readiness.determinism_observation,
                None
            );
            assert!(bundle
                .report
                .public_payload_order_probe
                .probe_disabled_sort_changed_bytes
                .is_none());
        }
    }

    #[cfg(unix)]
    #[test]
    fn workspace_path_validation_rejects_parent_escape_and_outside_symlink() {
        use std::os::unix::fs::symlink;

        let cargo_target = std::env::var("CARGO_TARGET_DIR")
            .expect("focused tests must use the registered workspace-local CARGO_TARGET_DIR");
        let temp_root = Path::new(&cargo_target)
            .ancestors()
            .find(|path| path.file_name().is_some_and(|name| name == ".tmp"))
            .expect("CARGO_TARGET_DIR must be below workspace .tmp")
            .to_path_buf();
        let process_temp = std::env::var("TMPDIR")
            .expect("focused tests must use the registered workspace-local TMPDIR");
        let process_temp = canonical_existing_directory("test TMPDIR", Path::new(&process_temp))
            .expect("test TMPDIR must be an existing absolute directory");
        require_temp_descendant("test TMPDIR", &process_temp, &temp_root)
            .expect("test TMPDIR must resolve below workspace .tmp");
        let case_root = process_temp.join(format!("path-validation-{}", std::process::id()));
        fs::create_dir(&case_root).expect("fresh path-validation directory");

        let parent_escape = case_root.join("../escaped-output");
        assert!(validate_new_output(&parent_escape, &temp_root)
            .unwrap_err()
            .contains("normalized"));

        let outside_target = temp_root
            .parent()
            .expect("workspace .tmp has a repository parent");
        let link = case_root.join("outside-link");
        symlink(outside_target, &link).expect("create repository-local test symlink");
        assert!(validate_new_output(&link.join("capture"), &temp_root)
            .unwrap_err()
            .contains("descendant"));

        let fake_workspace = case_root.join("fake-workspace");
        let outside_temp = case_root.join("outside-temp");
        fs::create_dir(&fake_workspace).expect("create fake workspace");
        fs::create_dir(&outside_temp).expect("create temp root outside fake workspace");
        symlink(&outside_temp, fake_workspace.join(".tmp"))
            .expect("create escaping workspace temp-root symlink");
        assert!(canonical_workspace_temp_root(&fake_workspace)
            .unwrap_err()
            .contains("canonical workspace root"));

        fs::remove_file(fake_workspace.join(".tmp"))
            .expect("remove escaping workspace temp-root symlink");
        fs::remove_dir(&fake_workspace).expect("remove fake workspace");
        fs::remove_dir(&outside_temp).expect("remove outside temp root");
        fs::remove_file(&link).expect("remove path-validation symlink");
        fs::remove_dir(&case_root).expect("remove empty path-validation directory");
    }

    #[test]
    fn workspace_directory_inputs_must_be_absolute() {
        assert!(canonical_existing_directory("relative", Path::new("."))
            .unwrap_err()
            .contains("absolute path"));
    }
}
