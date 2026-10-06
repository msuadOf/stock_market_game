"""临时拆解 capture_decision_snapshot 内部；采样后 rollback。"""
from pathlib import Path
import base64,hashlib,json
root=Path.cwd(); topic=root/'agents/engine-max-performance'; manifest=topic/'npc-snapshot-originals.json'
if manifest.exists(): raise SystemExit('manifest exists')
entries={}
def save(path):
 p=root/path; b=p.read_bytes(); entries[path]={'sha256':hashlib.sha256(b).hexdigest(),'base64':base64.b64encode(b).decode()}; manifest.write_text(json.dumps({'originals':entries},indent=2)); return b
names=['validate_market_view','build_market_view','attention_mutate','working_order_capture','behavior_market','prepared_accounts','retail_experience_install','retail_analysis','decision_snapshot_new','experience_capture']
save('packages/engine/src/lib.rs'); p=root/'packages/engine/src/lib.rs'; text=p.read_text(); assert 'pub mod performance_profile;' not in text; p.write_text(text+'\n// 临时 snapshot 内部计时；采样后撤回。\npub mod performance_profile;\n')
module=root/'packages/engine/src/performance_profile.rs'
if module.exists(): raise SystemExit('module exists')
module.write_text('''//! 临时 decision snapshot 内部计时；CPU 为同步调用线程，不含 child worker。
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
''' + f'const NAMES:[&str;{len(names)}]={json.dumps(names)};\n' + f'''static WALL:[AtomicU64;{len(names)}]=[const{{AtomicU64::new(0)}};{len(names)}];
static CALLS:[AtomicU64;{len(names)}]=[const{{AtomicU64::new(0)}};{len(names)}];
static CPU:[AtomicU64;{len(names)}]=[const{{AtomicU64::new(0)}};{len(names)}];
pub struct Span{{index:usize,started:Instant,cpu:u64}}
pub fn span(index:usize)->Span{{Span{{index,started:Instant::now(),cpu:cpu_ns()}}}}
impl Drop for Span{{fn drop(&mut self){{
 CPU[self.index].fetch_add(cpu_ns().saturating_sub(self.cpu),Ordering::Relaxed);
 WALL[self.index].fetch_add(self.started.elapsed().as_nanos() as u64,Ordering::Relaxed);
 CALLS[self.index].fetch_add(1,Ordering::Relaxed);
}}}}
pub fn reset(){{for x in WALL.iter().chain(CALLS.iter()).chain(CPU.iter()){{x.store(0,Ordering::Relaxed)}}}}
pub fn snapshot()->serde_json::Value{{serde_json::Value::Array(NAMES.iter().enumerate().map(|(i,n)|serde_json::json!({{"stage":n,"calls":CALLS[i].load(Ordering::Relaxed),"wall_ms":WALL[i].load(Ordering::Relaxed) as f64/1_000_000.0,"cpu_ms":CPU[i].load(Ordering::Relaxed) as f64/1_000_000.0}})).collect())}}
#[repr(C)] struct Timespec {{ seconds: i64, nanos: i64 }}
extern "C" {{ fn clock_gettime(clock: i32, time: *mut Timespec) -> i32; }}
fn cpu_ns() -> u64 {{
    let mut time = Timespec {{ seconds: 0, nanos: 0 }};
    assert_eq!(unsafe {{ clock_gettime(3, &mut time) }}, 0);
    time.seconds as u64 * 1_000_000_000 + time.nanos as u64
}}
''')
save('packages/engine/src/session/pipeline/decision_snapshot_capture.rs'); p=root/'packages/engine/src/session/pipeline/decision_snapshot_capture.rs'; s=p.read_text()
# Replace function body segments by inserting spans with scopes.
s=s.replace('''    let tick = shadow.state.tick;
    let phase = shadow.phase();
    validate_market_view_inputs(shadow)?;
    let market = shadow.build_market_view();
''','''    let tick = shadow.state.tick;
    let phase = shadow.phase();
    {
        let _profile = crate::performance_profile::span(0);
        validate_market_view_inputs(shadow)?;
    }
    let market = {
        let _profile = crate::performance_profile::span(1);
        shadow.build_market_view()
    };
''',1)
s=s.replace('''    let attention_results = shadow.state.npc_attention.mutate_existing_parallel(
''','''    let attention_results = {
        let _profile = crate::performance_profile::span(2);
        shadow.state.npc_attention.mutate_existing_parallel(
''',1)
s=s.replace('''    )?;
    let mut accepted_due_npc_ids = Vec::new();
''','''    )?
    };
    let mut accepted_due_npc_ids = Vec::new();
''',1)
s=s.replace('''        shadow.working_orders_for_accounts(&accepted_due_npc_ids.iter().copied().collect())
    };
''','''        {
            let _profile = crate::performance_profile::span(3);
            shadow.working_orders_for_accounts(&accepted_due_npc_ids.iter().copied().collect())
        }
    };
''',1)
s=s.replace('''        .then(|| build_behavior_market_checked(shadow))
        .transpose()?;
''','''        .then(|| {
            let _profile = crate::performance_profile::span(4);
            build_behavior_market_checked(shadow)
        })
        .transpose()?;
''',1)
s=s.replace('''    let prepared = accepted_due_npc_ids
        .par_iter()
''','''    let prepared = {
        let _profile = crate::performance_profile::span(5);
        accepted_due_npc_ids
        .par_iter()
''',1)
s=s.replace('''        .collect::<Result<Vec<_>, _>>()?;
    let mut accounts = BTreeMap::new();
''','''        .collect::<Result<Vec<_>, _>>()?
    };
    let mut accounts = BTreeMap::new();
''',1)
s=s.replace('''    shadow
        .state
        .retail_experience
        .replace_existing_shared_parallel(experience_updates)
        .map_err(DecisionSnapshotCaptureError::MissingAccount)?;
''','''    {
        let _profile = crate::performance_profile::span(6);
        shadow
            .state
            .retail_experience
            .replace_existing_shared_parallel(experience_updates)
            .map_err(DecisionSnapshotCaptureError::MissingAccount)?;
    }
''',1)
s=s.replace('''    let mut retail_analysis = shadow
        .capture_retail_analysis(&accepted_due_npc_ids, &market)
        .map_err(DecisionSnapshotCaptureError::RetailAnalysis)?;
''','''    let mut retail_analysis = {
        let _profile = crate::performance_profile::span(7);
        shadow
            .capture_retail_analysis(&accepted_due_npc_ids, &market)
            .map_err(DecisionSnapshotCaptureError::RetailAnalysis)?
    };
''',1)
s=s.replace('''    let snapshot = DecisionSnapshot::new(
''','''    let snapshot = {
        let _profile = crate::performance_profile::span(8);
        DecisionSnapshot::new(
''',1)
needle='''        shadow.state.seed,
        phase,
        market_minute,
        market,
        behavior_market,
        accepted_due_npc_ids,
        accounts,
    )
    .map_err(DecisionSnapshotCaptureError::Snapshot)?
    .with_urgency_policy(shadow.state.urgency_policy)?;
'''
replacement='''        shadow.state.seed,
        phase,
        market_minute,
        market,
        behavior_market,
        accepted_due_npc_ids,
        accounts,
    )
    .map_err(DecisionSnapshotCaptureError::Snapshot)?
    .with_urgency_policy(shadow.state.urgency_policy)?;
    };
'''
assert needle in s, 'DecisionSnapshot::new argument block not found'
s=s.replace(needle,replacement,1)
s=s.replace('''        let Some(mut experience) = session.state.retail_experience.get(&account).cloned() else {
''','''        let _profile = crate::performance_profile::span(9);
        let Some(mut experience) = session.state.retail_experience.get(&account).cloned() else {
''',1)
p.write_text(s)
save('agents/engine-max-performance/production-server-probe/src/main.rs'); p=root/'agents/engine-max-performance/production-server-probe/src/main.rs'; s=p.read_text(); s=s.replace('        let started = Instant::now();','        engine::performance_profile::reset();\n        let started = Instant::now();').replace('            "path": "production_server_actor_fastest",','            "npc_snapshot_profile": engine::performance_profile::snapshot(),\n            "path": "production_server_actor_fastest",'); p.write_text(s)
manifest.write_text(json.dumps({'originals':entries,'module':'packages/engine/src/performance_profile.rs','names':names},indent=2)); print('instrumented',len(names))
