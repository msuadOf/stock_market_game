"""为当前 NPC next-tick queue 插入临时嵌套计时；采样后必须 rollback。"""
from pathlib import Path
import base64, hashlib, json
root=Path.cwd(); topic=root/'agents/engine-max-performance'; manifest_path=topic/'npc-queue-originals.json'
if manifest_path.exists(): raise SystemExit('manifest exists')
entries={}
def save(path):
 p=root/path; b=p.read_bytes(); entries[path]={'sha256':hashlib.sha256(b).hexdigest(),'base64':base64.b64encode(b).decode()}; manifest_path.write_text(json.dumps({'originals':entries},indent=2)); return b
# module: names 0..4
save('packages/engine/src/lib.rs')
lib=root/'packages/engine/src/lib.rs'; lib.write_text(lib.read_text()+'\n// 临时 NPC queue 计时；采样后撤回。\npub mod performance_profile;\n')
module=root/'packages/engine/src/performance_profile.rs'
if module.exists(): raise SystemExit('module exists')
names=['queue_capture','queue_stream','queue_projection_callback','queue_receive_callback','npc_decide_account']
module.write_text('''//! 临时 NPC queue 计时；CPU 为同步调用线程，不含 child worker。
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
#[repr(C)] struct T {{ s: i64, n: i64 }}
extern "C" {{ fn clock_gettime(c: i32, t: *mut T) -> i32; }}
fn cpu_ns() -> u64 {{
    let mut t = T {{ s: 0, n: 0 }};
    assert_eq!(unsafe {{ clock_gettime(3, &mut t) }}, 0);
    t.s as u64 * 1_000_000_000 + t.n as u64
}}
''')
# instrument queue
save('packages/engine/src/session/pipeline/npc_tick_preparation.rs')
p=root/'packages/engine/src/session/pipeline/npc_tick_preparation.rs'; s=p.read_text()
old='''    let captured =
        capture_decision_snapshot(session).map_err(|error| invariant(&error.to_string()))?;
'''
new='''    let captured = {
        let _profile = crate::performance_profile::span(0);
        capture_decision_snapshot(session).map_err(|error| invariant(&error.to_string()))
    }?;
'''
assert old in s; s=s.replace(old,new,1)
old='''    let stream_error = stream_npc_decisions(captured.snapshot.clone(), &config, |mut result| {
        let projection = project_npc_account(session, &captured, &mut result.output)
            .map_err(|error| error.to_string())?;
        let projected = projected_ordered_intents(&result.output, &projection)
            .map_err(|error| error.to_string())?;
        let offset = intents.len();
        for (owner, intent) in projected.intents {
            intents.push(
                session
                    .receive_private_intent(owner, intent)
                    .map_err(|error| error.to_string())?,
            );
        }
        dependencies.extend(
            projected
                .dependencies
                .into_iter()
                .map(|(before, after)| (offset + before, offset + after)),
        );
        Ok::<_, String>(())
    });
'''
new='''    let stream_error = {
        let _profile = crate::performance_profile::span(1);
        stream_npc_decisions(captured.snapshot.clone(), &config, |mut result| {
            let projection = {
                let _profile = crate::performance_profile::span(2);
                project_npc_account(session, &captured, &mut result.output)
                    .map_err(|error| error.to_string())?
            };
            let projected = projected_ordered_intents(&result.output, &projection)
                .map_err(|error| error.to_string())?;
            let offset = intents.len();
            for (owner, intent) in projected.intents {
                let _profile = crate::performance_profile::span(3);
                intents.push(
                    session
                        .receive_private_intent(owner, intent)
                        .map_err(|error| error.to_string())?,
                );
            }
            dependencies.extend(
                projected
                    .dependencies
                    .into_iter()
                    .map(|(before, after)| (offset + before, offset + after)),
            );
            Ok::<_, String>(())
        })
    };
'''
assert old in s; s=s.replace(old,new,1); p.write_text(s)
# decide account
save('packages/engine/src/session/pipeline/npc_decisions.rs')
p=root/'packages/engine/src/session/pipeline/npc_decisions.rs'; s=p.read_text(); old='''    let input = snapshot
        .account(account)
        .map_err(NpcDecisionSourceError::Snapshot)?;
'''; new='''    let _profile = crate::performance_profile::span(4);
    let input = snapshot
        .account(account)
        .map_err(NpcDecisionSourceError::Snapshot)?;
'''
assert old in s; s=s.replace(old,new,1); p.write_text(s)
# probe
save('agents/engine-max-performance/production-server-probe/src/main.rs')
p=root/'agents/engine-max-performance/production-server-probe/src/main.rs'; s=p.read_text(); s=s.replace('        let started = Instant::now();','        engine::performance_profile::reset();\n        let started = Instant::now();').replace('            "path": "production_server_actor_fastest",','            "npc_queue_profile": engine::performance_profile::snapshot(),\n            "path": "production_server_actor_fastest",'); p.write_text(s)
manifest_path.write_text(json.dumps({'originals':entries,'module':'packages/engine/src/performance_profile.rs','names':names},indent=2)); print('instrumented',len(names))
