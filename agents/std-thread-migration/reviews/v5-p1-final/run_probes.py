"""临时探针及 shutdown 突变；finally 按字节还原所有源码。"""
import ast, hashlib, json, os, signal, subprocess, time
from pathlib import Path
ROOT=Path.cwd(); OUT=ROOT/'agents/std-thread-migration/reviews/v5-p1-final'
WORLD=ROOT/'packages/engine/src/runtime/world.rs'
TESTS=ROOT/'packages/engine/src/session/persistence/saved_runtime_tests.rs'
ORIGINAL={p:p.read_bytes() for p in (WORLD,TESTS)}
START=time.monotonic(); END=START+298
old_script=ast.parse((ROOT/'agents/std-thread-migration/reviews/v5-repair-recheck/run_probes.py').read_text())
base=next(ast.literal_eval(x.value) for x in old_script.body if isinstance(x,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='PROBES' for t in x.targets))
base=base.replace('serde_json::to_value(&save).expect("合法空簿局的存档必须可以序列化");', '''let json = serde_json::to_vec(&save).expect("合法空簿局序列化");
    eprintln!("JSON 编码完成，bytes={}", json.len());
    let decoded: crate::SaveSlot = serde_json::from_slice(&json).expect("SaveSlot 反序列化");
    eprintln!("JSON 解码完成");
    assert_eq!(decoded.next_order_id, crate::orderbook::js_safe_u64::MAX);
    let restored = GameSession::restore(&decoded).expect("33 股空簿恢复");
    serde_json::to_vec(&restored.save().expect("恢复后再存档")).expect("恢复后再次序列化");
    eprintln!("33 股 SaveSlot JSON 往返和 restore 成功，next_order_id={}", decoded.next_order_id);''')
EXTRA=r'''

#[test]
fn final_recheck_31_stock_floor_and_wire_boundary() {
    let mut session = GameSession::new(recheck_stock_setup(31), 42).expect("合法 31 股局");
    let mask=(1u64<<48)-1;
    let max=crate::orderbook::js_safe_u64::MAX;
    session.state.runtime_desk_serial_cursors=(0..31).map(|idx| mask-idx).collect();
    let expected=(30u64<<48)|(mask-30);
    let cursors=session.save().expect("游标路径存档");
    assert_eq!(cursors.next_order_id,expected);
    for (idx,cursor) in session.state.runtime_desk_serial_cursors.iter().enumerate() {
        let last=((idx as u64)<<48)|(cursor-1);
        assert!(cursors.next_order_id>last);
        assert_eq!(((idx as u64)<<48)|cursor, (((idx as u64)<<48)|cursor).min(max));
    }
    serde_json::to_vec(&cursors).expect("游标路径 wire 域有效");
    session.state.runtime_desk_serial_cursors.clear();
    for idx in 0..31 {
        session.state.desk_book_mirror.insert(StockId(idx), crate::session::DeskBookMirror {
            next_serial:mask-u64::from(idx), last_price:Money::from_cents(1000), ..Default::default()
        });
    }
    let mirrors=session.save().expect("镜像路径存档");
    assert_eq!(mirrors.next_order_id,expected);
    serde_json::to_vec(&mirrors).expect("镜像路径 wire 域有效");
    for index in [30u64,31] {
        serde_json::to_vec(&OrderId((index<<48)|1)).expect("第 31/32 股身份仍在 wire 域内");
    }
    let unsafe_id=OrderId((32u64<<48)|1);
    let error=serde_json::to_vec(&unsafe_id).expect_err("第 33 股 desk OrderId 超 wire 域负控");
    assert!(error.to_string().contains("safe integer"));
    eprintln!("31 股 floor={}，逐股覆盖所有 serial<cursor；stock index31可编码，index32错误={}",expected,error);
}
'''

def command(args,label,limit):
    p=subprocess.Popen(args,cwd=ROOT,env=dict(os.environ,ENGINE_STEP_RUNTIME='1'),stdout=subprocess.PIPE,stderr=subprocess.STDOUT,start_new_session=True)
    t=time.monotonic()
    try:
        output=p.communicate(timeout=min(limit,max(.01,END-time.monotonic())))[0]; rc=p.returncode
    except subprocess.TimeoutExpired:
        os.killpg(p.pid,signal.SIGKILL); output=p.communicate()[0]; rc=124
    output=output.decode(errors='replace'); (OUT/f'{label}.log').write_text(output)
    print(label,'rc',rc,'seconds',round(time.monotonic()-t,3),flush=True)
    return rc,output

def build(label):
    rc,output=command(['cargo','test','-p','engine','--lib','--no-run','-j','32','--message-format=json'],label,290)
    assert rc==0,output
    a=[json.loads(s) for s in output.splitlines() if s.startswith('{')]
    return next(x['executable'] for x in a if x.get('executable') and x.get('target',{}).get('name')=='engine')

def case(binary,name,label,limit=9.5):
    return command([binary,name,'--test-threads=1','--nocapture'],label,limit)

try:
    TESTS.write_bytes(ORIGINAL[TESTS]+(base+EXTRA).encode())
    binary=build('probes-build')
    import shutil
    shutil.copy2(binary, OUT/'probe-binary')
    # 31 股完整边界 fixture 单列长验收；33 股另起独立长验收。
    for name in ('final_recheck_31_stock_floor_and_wire_boundary',):
        rc,output=case(binary,name,name,75); assert rc==0,output
    rc,output=case(binary,'recheck_two_stock_floor_survives_restore_and_new_order','two-stock-green'); assert rc==0,output
    world=ORIGINAL[WORLD].decode(); needle='    pub fn shutdown(mut self) -> Result<ShutdownReport, RuntimeError> {\n'
    assert world.count(needle)==1
    WORLD.write_bytes(world.replace(needle,needle+'        self.worker_shutdown.store(true, Ordering::SeqCst);\n').encode())
    binary=build('mutation-build')
    rc,output=case(binary,'v5_world_shutdown_stall_window_keeps_message_protocol','shutdown-red')
    assert rc==101 and 'SendFailed' in output,output
finally:
    for p,data in ORIGINAL.items(): p.write_bytes(data)
    assert all(p.read_bytes()==data for p,data in ORIGINAL.items())
    (OUT/'restored-sha256.json').write_text(json.dumps({str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in ORIGINAL},indent=2)+'\n')
    rc,output=command(['git','diff','--exit-code'],'restore-git-diff',9.5); assert rc==0,output
    print('源码逐字节还原，git diff --exit-code 成功',flush=True)
binary=build('restored-build')
rc,output=case(binary,'v5_world_shutdown_stall_window_keeps_message_protocol','shutdown-green'); assert rc==0,output
rc,output=case(binary,'restore_carries_true_fill_facts_and_continues_buy_with_cumulative_fees','fees-green'); assert rc==0,output
rc,output=command(['git','diff','--exit-code'],'final-git-diff',9.5); assert rc==0,output
