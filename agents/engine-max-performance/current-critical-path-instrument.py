"""为 382c1871 后的当前源码插入临时 critical-path span；采样后必须完整撤回。"""
from pathlib import Path
import base64, hashlib, json

root = Path.cwd()
topic = root / 'agents/engine-max-performance'
manifest_path = topic / 'current-critical-path-originals.json'
if manifest_path.exists():
    raise SystemExit('manifest 已存在；先执行 rollback-current-critical-profile.py')
entries = {}
names = []

def save(path):
    p = root / path
    source = p.read_bytes()
    entries[path] = {
        'sha256': hashlib.sha256(source).hexdigest(),
        'base64': base64.b64encode(source).decode(),
    }
    manifest_path.write_text(json.dumps({'originals': entries}, indent=2))
    return source

def insert_after_function(path, token, name, occurrence=1):
    index = len(names)
    names.append(name)
    source = save(path)
    text = source.decode()
    start = 0
    for _ in range(occurrence):
        start = text.index(token, start) + len(token)
    brace = text.index('{', start) + 1
    text = text[:brace] + f'\n    let _critical_profile = engine::performance_profile::span({index});\n' + text[brace:]
    (root / path).write_text(text)

def insert_after_impl_method(path, impl_token, method_token, name):
    index = len(names)
    names.append(name)
    source = save(path)
    text = source.decode()
    impl = text.index(impl_token)
    start = text.index(method_token, impl) + len(method_token)
    brace = text.index('{', start) + 1
    text = text[:brace] + f'\n        let _critical_profile = engine::performance_profile::span({index});\n' + text[brace:]
    (root / path).write_text(text)

# read-only start 三路 join 与其中最大的独立准备。
insert_after_function('packages/engine/src/session/pipeline/read_only_start.rs', 'fn prepare_with_observer(', 'read_only_prepare_join')
insert_after_function('packages/engine/src/session/envelope_projection.rs', 'fn validate_existing_live_projection_with_observer(', 'borrowed_existing_projection')
insert_after_function('packages/engine/src/session/pipeline/decision_resources.rs', 'fn seal(', 'seal_total')
insert_after_function('packages/engine/src/session/pipeline/continuous_matching_adapter.rs', 'fn prepare_incremental_continuous_inputs(', 'continuous_input_prepare')
insert_after_function('packages/engine/src/session/pipeline/account_validation_context.rs', 'fn build_account_validation_context(', 'account_validation_context')
insert_after_function('packages/engine/src/session/pipeline/ready_ingress.rs', 'fn capture_sources(', 'ready_ingress_capture_sources')
insert_after_function('packages/engine/src/session/pipeline/ready_ingress.rs', 'fn capture_roots(', 'ready_ingress_capture_roots')
insert_after_function('packages/engine/src/session/pipeline/stock_stream.rs', 'fn continuous_shards(', 'continuous_shadow_initialize')
insert_after_function('packages/engine/src/session/pipeline/candidate_commit.rs', 'fn prepare_candidate_commit', 'candidate_commit_prepare')
insert_after_function('packages/engine/src/session/pipeline/npc_tick_preparation.rs', 'fn queue_npc_for_next_tick(', 'npc_queue_next_tick')
insert_after_function('packages/engine/src/session/pipeline/decision_snapshot_capture.rs', 'fn capture_decision_snapshot_in_place(', 'npc_snapshot_capture')

# Protocol / server actor 的每 tick 发布路径。
insert_after_function('packages/engine/src/session/protocol/civil/session.rs', 'pub fn step_frame(', 'protocol_step_frame')
insert_after_function('packages/engine/src/session/protocol/civil/session.rs', 'fn prepare_frame(', 'protocol_prepare_frame')
insert_after_function('packages/engine/src/session/protocol/civil/session.rs', 'pub fn checkpoint(', 'protocol_checkpoint')
insert_after_function('packages/engine/src/session/protocol/civil/session.rs', 'pub fn tick_batch(', 'protocol_tick_batch')
insert_after_function('packages/engine/src/session/protocol/commit.rs', 'fn frame_after_step(', 'engine_frame_after_step')
insert_after_function('packages/engine/src/session/protocol/delta.rs', 'pub fn public_runtime_state(', 'public_runtime_state')
insert_after_function('packages/engine/src/session/protocol/delta.rs', 'pub fn tick_batch_delta(', 'tick_batch_delta')
insert_after_function('packages/engine/src/session/snapshot.rs', 'fn snapshot_inner(', 'snapshot_runtime')
insert_after_impl_method('packages/engine/src/session/protocol.rs', 'impl TickFrame {', 'pub fn validate(&self)', 'tick_frame_validate')
insert_after_function('apps/server/src/actor.rs', 'fn run_protocol_batch(', 'actor_protocol_batch')
insert_after_function('apps/server/src/actor.rs', 'fn broadcast_at(', 'actor_broadcast')

# 临时 module 与 probe 输出。
save('packages/engine/src/lib.rs')
lib = root / 'packages/engine/src/lib.rs'
lib.write_text(lib.read_text() + '\n// 本任务临时插桩；采样后撤回，不属于发布 API。\npub mod performance_profile;\n')
module = root / 'packages/engine/src/performance_profile.rs'
if module.exists():
    raise SystemExit('临时 module 已存在')
module.write_text('''//! 临时 critical path wall time；父子 span 重叠，不能直接求和。
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
''' + f'const NAMES: [&str; {len(names)}] = ' + json.dumps(names, ensure_ascii=False) + ';\n' + f'''static TIMES: [AtomicU64; {len(names)}] = [const {{ AtomicU64::new(0) }}; {len(names)}];
static CALLS: [AtomicU64; {len(names)}] = [const {{ AtomicU64::new(0) }}; {len(names)}];
static CPU_TIMES: [AtomicU64; {len(names)}] = [const {{ AtomicU64::new(0) }}; {len(names)}];
pub struct Span {{ index: usize, started: Instant, cpu_started: u64 }}
pub fn span(index: usize) -> Span {{ Span {{ index, started: Instant::now(), cpu_started: cpu_ns() }} }}
impl Drop for Span {{
    fn drop(&mut self) {{
        CPU_TIMES[self.index].fetch_add(cpu_ns().saturating_sub(self.cpu_started), Ordering::Relaxed);
        TIMES[self.index].fetch_add(self.started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        CALLS[self.index].fetch_add(1, Ordering::Relaxed);
    }}
}}
pub fn reset() {{
    for value in TIMES.iter().chain(CALLS.iter()).chain(CPU_TIMES.iter()) {{ value.store(0, Ordering::Relaxed); }}
}}
pub fn snapshot() -> serde_json::Value {{
    serde_json::Value::Array(NAMES.iter().enumerate().map(|(index, name)| {{
        serde_json::json!({{"stage": name, "calls": CALLS[index].load(Ordering::Relaxed),
            "cpu_ms": CPU_TIMES[index].load(Ordering::Relaxed) as f64 / 1_000_000.0,
            "wall_ms": TIMES[index].load(Ordering::Relaxed) as f64 / 1_000_000.0}})
    }}).collect())
}}
#[repr(C)] struct Timespec {{ seconds: i64, nanos: i64 }}
extern "C" {{ fn clock_gettime(clock: i32, time: *mut Timespec) -> i32; }}
fn cpu_ns() -> u64 {{
    let mut time = Timespec {{ seconds: 0, nanos: 0 }};
    assert_eq!(unsafe {{ clock_gettime(3, &mut time) }}, 0, "clock_gettime 失败");
    time.seconds as u64 * 1_000_000_000 + time.nanos as u64
}}
''')

save('agents/engine-max-performance/production-server-probe/src/main.rs')
probe = root / 'agents/engine-max-performance/production-server-probe/src/main.rs'
text = probe.read_text()
text = text.replace('        let started = Instant::now();', '        engine::performance_profile::reset();\n        let started = Instant::now();')
text = text.replace('            "path": "production_server_actor_fastest",', '            "critical_path_profile": engine::performance_profile::snapshot(),\n            "path": "production_server_actor_fastest",')
probe.write_text(text)
manifest_path.write_text(json.dumps({'originals': entries, 'names': names, 'module': 'packages/engine/src/performance_profile.rs'}, indent=2))
print(f'已插桩 {len(names)} 个 span，保存 {len(entries)} 份源码')
