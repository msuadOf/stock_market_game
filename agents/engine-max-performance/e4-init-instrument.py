#!/usr/bin/env python3
"""E4 诊断插桩：拆解零 op 初始化路径（from_post_expiry → process_continuous_stock_step(input, 0)）内部成本。

apply：按唯一锚点插入 crate::performance_profile::span 计时，写入 manifest。
rollback：按 manifest 原字节恢复全部源码并删除临时 module 与 manifest。
插桩构建不用于正式基准；verify-build 对诊断 probe 预期失败。
"""
import base64
import hashlib
import json
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[2]
manifest_path = Path(__file__).resolve().parent / 'e4-init-rollback.json'

NAMES = [
    'e4_step_zero_total', 'e4_init_snapshots_total', 'e4_orders_clone',
    'e4_snap_loop', 'e4_book_pass', 'e4_ledger_new',
    'e4_complete_evidence', 'e4_private_market_validate',
]
FILES = [
    'packages/engine/src/session/pipeline/incremental_continuous_stock_shadow.rs',
    'packages/engine/src/session/pipeline/continuous_matching.rs',
    'packages/engine/src/lib.rs',
    'agents/engine-max-performance/production-server-probe/src/main.rs',
]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def apply():
    if manifest_path.exists():
        raise SystemExit('manifest 已存在，先 rollback')
    entries = {}
    for rel in FILES:
        data = (root / rel).read_bytes()
        entries[rel] = {
            'sha256': hashlib.sha256(data).hexdigest(),
            'base64': base64.b64encode(data).decode(),
        }

    def replace(rel, old, new):
        path = root / rel
        text = path.read_text()
        if text.count(old) != 1:
            raise SystemExit(f'{rel} 锚点匹配数 {text.count(old)} != 1：{old[:70]!r}')
        path.write_text(text.replace(old, new))

    shadow = FILES[0]
    matching = FILES[1]

    replace(shadow,
            '            let initialized = process_continuous_stock_step(input, 0)?;',
            '''            let initialized = {
                let _profile = crate::performance_profile::span(0);
                process_continuous_stock_step(input, 0)?
            };''')

    replace(matching,
            '            validate_initial_snapshots(&input.market, &input.envelopes)?;',
            '''            {
                let _profile = crate::performance_profile::span(1);
                validate_initial_snapshots(&input.market, &input.envelopes)?;
            }''')

    replace(matching,
            '    let orders = market.resting_orders();',
            '''    let orders = {
        let _profile = crate::performance_profile::span(2);
        market.resting_orders()
    };''')

    replace(matching,
            '''    for snapshot in snapshots {
        snapshot.envelope.validate()?;
        if snapshot.envelope.origin() != EnvelopeOrigin::TickStart {
            return Err(invariant(
                "报价过期后的 Continuous 输入包含同 tick 创建的 envelope",
            ));
        }
        if snapshot.envelope.key().stock != *market.code() {
            return Err(invariant("continuous envelope belongs to another stock"));
        }
        let key = snapshot.envelope.key().order;
        if by_order.insert(key, snapshot).is_some() {
            return Err(invariant("duplicate continuous envelope snapshot"));
        }
    }''',
            '''    {
        let _profile = crate::performance_profile::span(3);
        for snapshot in snapshots {
            snapshot.envelope.validate()?;
            if snapshot.envelope.origin() != EnvelopeOrigin::TickStart {
                return Err(invariant(
                    "报价过期后的 Continuous 输入包含同 tick 创建的 envelope",
                ));
            }
            if snapshot.envelope.key().stock != *market.code() {
                return Err(invariant("continuous envelope belongs to another stock"));
            }
            let key = snapshot.envelope.key().order;
            if by_order.insert(key, snapshot).is_some() {
                return Err(invariant("duplicate continuous envelope snapshot"));
            }
        }
    }''')

    replace(matching,
            '''    for order in &orders {
        let snapshot = by_order
            .remove(&order.id)
            .ok_or_else(|| invariant("resting order has no continuous envelope snapshot"))?;
        validate_snapshot(snapshot, order)?;
    }''',
            '''    {
        let _profile = crate::performance_profile::span(4);
        for order in &orders {
            let snapshot = by_order
                .remove(&order.id)
                .ok_or_else(|| invariant("resting order has no continuous envelope snapshot"))?;
            validate_snapshot(snapshot, order)?;
        }
    }''')

    replace(matching,
            '            PrivateStockLedger::new(ledger_envelopes)?',
            '''            {
                let _profile = crate::performance_profile::span(5);
                PrivateStockLedger::new(ledger_envelopes)?
            }''')

    replace(matching,
            '            self.ledger.validate_complete_evidence()?;',
            '''            {
                let _profile = crate::performance_profile::span(6);
                self.ledger.validate_complete_evidence()?;
            }''')

    replace(matching,
            '            validate_private_market_ledger(&self.output.market, &self.ledger, &self.config)?;',
            '''            {
                let _profile = crate::performance_profile::span(7);
                validate_private_market_ledger(&self.output.market, &self.ledger, &self.config)?;
            }''')

    lib = root / FILES[2]
    text = lib.read_text()
    if 'pub mod performance_profile;' in text:
        raise SystemExit('lib.rs 已含临时 module 声明')
    lib.write_text(text + '\n// 本任务临时插桩；采样后撤回，不属于发布 API。\npub mod performance_profile;\n')

    module = root / 'packages/engine/src/performance_profile.rs'
    if module.exists():
        raise SystemExit('临时 module 已存在')
    module.write_text('''//! 临时 E4 零 op 初始化路径 sub-span 计时；CPU 为执行线程，含 Rayon worker。
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
''' + f'const NAMES:[&str;{len(NAMES)}]={json.dumps(NAMES, ensure_ascii=False)};\n' + f'''static WALL:[AtomicU64;{len(NAMES)}]=[const{{AtomicU64::new(0)}};{len(NAMES)}];
static CALLS:[AtomicU64;{len(NAMES)}]=[const{{AtomicU64::new(0)}};{len(NAMES)}];
static CPU:[AtomicU64;{len(NAMES)}]=[const{{AtomicU64::new(0)}};{len(NAMES)}];
pub struct Span{{index:usize,started:Instant,cpu:u64}}
pub fn span(index:usize)->Span{{Span{{index,started:Instant::now(),cpu:cpu_ns()}}}}
impl Drop for Span{{fn drop(&mut self){{
 CPU[self.index].fetch_add(cpu_ns().saturating_sub(self.cpu),Ordering::Relaxed);
 WALL[self.index].fetch_add(self.started.elapsed().as_nanos() as u64,Ordering::Relaxed);
 CALLS[self.index].fetch_add(1,Ordering::Relaxed);
}}}}
pub fn reset(){{for value in WALL.iter().chain(CALLS.iter()).chain(CPU.iter()){{value.store(0,Ordering::Relaxed)}}}}
pub fn snapshot()->serde_json::Value{{serde_json::Value::Array(NAMES.iter().enumerate().map(|(index,name)|serde_json::json!({{"stage":name,"calls":CALLS[index].load(Ordering::Relaxed),"wall_ms":WALL[index].load(Ordering::Relaxed) as f64/1_000_000.0,"cpu_ms":CPU[index].load(Ordering::Relaxed) as f64/1_000_000.0}})).collect())}}
#[repr(C)] struct Timespec{{seconds:i64,nanos:i64}}
extern "C" {{fn clock_gettime(clock:i32,time:*mut Timespec)->i32;}}
fn cpu_ns()->u64{{
 let mut time=Timespec{{seconds:0,nanos:0}};
 assert_eq!(unsafe{{clock_gettime(3,&mut time)}},0);
 time.seconds as u64*1_000_000_000+time.nanos as u64
}}
''')

    probe = root / FILES[3]
    text = probe.read_text()
    for anchor in ('        let started = Instant::now();',
                   '            "path": "production_server_actor_fastest", "ticks": committed,'):
        if text.count(anchor) != 1:
            raise SystemExit(f'probe 锚点匹配数 {text.count(anchor)} != 1：{anchor!r}')
    text = text.replace('        let started = Instant::now();',
                        '        engine::performance_profile::reset();\n        let started = Instant::now();')
    text = text.replace('            "path": "production_server_actor_fastest", "ticks": committed,',
                        '            "critical_path_profile": engine::performance_profile::snapshot(),\n            "path": "production_server_actor_fastest", "ticks": committed,')
    probe.write_text(text)

    manifest_path.write_text(json.dumps({'originals': entries, 'names': NAMES,
                                         'module': 'packages/engine/src/performance_profile.rs'}, indent=2))
    print(f'已插桩 {len(NAMES)} 个 span，保存 {len(entries)} 份源码')


def rollback():
    if not manifest_path.exists():
        raise SystemExit('manifest 不存在')
    manifest = json.loads(manifest_path.read_text())
    for rel, entry in manifest['originals'].items():
        path = root / rel
        data = base64.b64decode(entry['base64'])
        if hashlib.sha256(data).hexdigest() != entry['sha256']:
            raise SystemExit(f'{rel} manifest sha256 不一致，中止')
        path.write_bytes(data)
        if sha256(path) != entry['sha256']:
            raise SystemExit(f'{rel} 恢复后 sha256 不一致，中止')
    module = root / manifest['module']
    if module.exists():
        module.unlink()
    manifest_path.unlink()
    print(f'已按 manifest 恢复 {len(manifest["originals"])} 份源码并删除临时 module')


if __name__ == '__main__':
    if len(sys.argv) != 2 or sys.argv[1] not in ('apply', 'rollback'):
        raise SystemExit('用法：e4-init-instrument.py apply|rollback')
    (apply if sys.argv[1] == 'apply' else rollback)()
