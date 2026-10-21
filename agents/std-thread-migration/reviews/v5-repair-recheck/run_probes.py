"""临时源码复验；无论成功失败，finally 均按字节还原源码。"""
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parents[4]
OUT = Path(__file__).resolve().parent
START = time.monotonic()
FILES = [ROOT / p for p in ["packages/engine/src/runtime/desk.rs",
         "packages/engine/src/runtime/world.rs",
         "packages/engine/src/session/persistence/saved_runtime_tests.rs"]]
ORIGINAL = {p: p.read_bytes() for p in FILES}

PROBES = r'''

// 再复核临时用例：仅生成审查证据，执行后按字节还原。
fn recheck_stock_setup(count: u32) -> crate::SessionSetup {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    let template = setup.stocks[0].clone();
    setup.stocks = (0..count).map(|idx| {
        let mut stock = template.clone();
        stock.code = crate::StockCode(format!("{:06}", 600000 + idx));
        stock
    }).collect();
    setup
}

#[test]
fn recheck_idle_33_stock_json() {
    let mut session = GameSession::new(recheck_stock_setup(33), 42).expect("合法的 33 股局");
    serde_json::to_value(session.save().unwrap()).expect("步进前可序列化");
    session.step().expect("无交易的一拍");
    let save = session.save().expect("内存存档成功");
    assert!(save.resting_orders.values().all(Vec::is_empty));
    println!("空簿 next_order_id={}", save.next_order_id);
    serde_json::to_value(&save).expect("合法空簿局的存档必须可以序列化");
}

#[test]
fn recheck_two_stock_floor_survives_restore_and_new_order() {
    let mut session = GameSession::new(recheck_stock_setup(2), 42).unwrap();
    session.state.runtime_desk_serial_cursors = vec![7, 11];
    let save = session.save().unwrap();
    assert_eq!(save.next_order_id, (1u64 << 48) | 11);
    serde_json::to_value(&save).expect("两股游标仍在 wire 范围内");
    let mut restored = GameSession::restore(&save).expect("两股档恢复");
    restored.enqueue_player_intent(AccountId(0), Intent::PlaceLimit {
        code: StockId(1), side: Side::Buy,
        price: LimitPrice::Fixed(crate::Money::from_cents(1000)), qty: 100,
    }).unwrap();
    restored.step().expect("恢复后新单成功");
    let next = restored.save().unwrap();
    assert!(next.next_order_id > save.next_order_id);
    GameSession::restore(&next).expect("新单后的档恢复");
}
'''


def command(args, label, limit):
    remaining = 298 - (time.monotonic()-START)
    assert remaining > 0, "共享期限耗尽"
    p = subprocess.Popen(args, cwd=ROOT, env=dict(os.environ, ENGINE_STEP_RUNTIME="1"),
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
    try:
        output = p.communicate(timeout=min(limit, remaining))[0].decode(errors="replace")
        rc = p.returncode
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        output = p.communicate()[0].decode(errors="replace")
        rc = 124
    (OUT / (label + ".log")).write_text(output)
    print(label, "rc", rc, flush=True)
    return rc, output


def build(label):
    rc, output = command(["cargo", "test", "-p", "engine", "--lib", "--no-run", "-j", "32",
                          "--message-format=json"], label, 290)
    assert rc == 0, output[-1500:]
    artifacts = []
    for line in output.splitlines():
        if not line.startswith("{"):
            continue
        x = json.loads(line)
        if x.get("executable") and x.get("target", {}).get("name") == "engine":
            artifacts.append(x["executable"])
    return artifacts[-1]


def case(binary, name, label):
    return command([binary, name, "--test-threads=1", "--nocapture"], label, 9.5)


try:
    FILES[2].write_bytes(ORIGINAL[FILES[2]] + PROBES.encode())
    desk = ORIGINAL[FILES[0]].decode()
    for owner in ["taker", "m"]:
        for field, delta, component in [("charged_commission", "buy_commission_delta", "commission"),
                                         ("charged_transfer_fee", "buy_transfer_delta", "transfer_fee")]:
            old = f"{owner}.{field} += {delta};"
            assert desk.count(old) == 1
            new = (f"{owner}.{field} += FeeQuote::nominal(&self.config, Side::Buy, "
                   f"Money::from_cents(value)).expect(\"突变：分笔完整费用\").{component}.cents();")
            desk = desk.replace(old, new)
    FILES[0].write_text(desk)
    binary = build("mutation-charged-build")
    rc, output = case(binary, "restore_carries_true_fill_facts_and_continues_buy_with_cumulative_fees", "mutation-charged-red")
    assert rc == 101 and "2220" in output and "leg2" in output, output
    case(binary, "recheck_idle_33_stock_json", "probe-33-stocks")
    case(binary, "recheck_two_stock_floor_survives_restore_and_new_order", "probe-two-stock-floor")
    FILES[0].write_bytes(ORIGINAL[FILES[0]])
    world = ORIGINAL[FILES[1]].decode()
    old = "        if let Some(report) = self.dead_finals.take() {"
    assert world.count(old) == 1
    world = world.replace(old, "        self.worker_shutdown.store(true, Ordering::SeqCst);\n" + old)
    FILES[1].write_text(world)
    binary = build("mutation-shutdown-build")
    case(binary, "v5_shutdown_over_50ms_idle_window_applies_staged_refunds", "mutation-shutdown-loop")
    def repeat(i):
        rc, _ = case(binary, "v5_world_shutdown_reaches_all_groups_via_message_protocol", f"mutation-shutdown-world-{i:02}")
        return rc
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        results = list(pool.map(repeat, range(32)))
    (OUT / "mutation-shutdown-summary.json").write_text(json.dumps(dict(passed=results.count(0), failed=len(results)-results.count(0))) + "\n")
finally:
    for path, content in ORIGINAL.items():
        path.write_bytes(content)
    hashes = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in FILES}
    (OUT / "restored-sha256.json").write_text(json.dumps(hashes, indent=2) + "\n")
    assert all(p.read_bytes() == content for p, content in ORIGINAL.items())
    print("源码已按字节还原", flush=True)

binary = build("restored-build")
case(binary, "restore_carries_true_fill_facts_and_continues_buy_with_cumulative_fees", "restored-charged-green")
case(binary, "v5_adopt_partial_buy_seeds_exact_escrow_and_survives_quiet_save", "restored-adopt-green")
case(binary, "v5_shutdown_idle_window_tests", "restored-shutdown-green")
