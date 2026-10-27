# I-D 组合限域构建；外层共享300000ms监督，磁盘低20GiB时停止自有整组。
import concurrent.futures, hashlib, json, os, pathlib, shutil, signal, subprocess, sys, time
repo = pathlib.Path(sys.argv[1]); output = pathlib.Path(sys.argv[2]); output.mkdir(parents=True, exist_ok=True)
started = time.monotonic()
argv = ['cargo','test','-p','engine','--lib','--test','plan_identity','--test','plan_identity_behavior','--test','plan_allocation','--test','save_contract','--no-run','--message-format=json-render-diagnostics','-j','8']
(output/'build-command.json').write_text(json.dumps({'argv':argv,'cwd':str(repo),'jobs':8,'debug':0,'deadline_ms':300000,'target':os.environ['CARGO_TARGET_DIR']},indent=2)+'\n')
def run(argv, label, stdout):
    print(json.dumps({'phase':label,'state':'started','elapsed_s':time.monotonic()-started,'argv':argv}),flush=True)
    child = subprocess.Popen(argv,cwd=repo,stdout=stdout)
    with (output/(label+'-process-samples.jsonl')).open('w') as log:
        while child.poll() is None:
            available = shutil.disk_usage(repo).free
            if available < 20*1024**3:
                print(json.dumps({'phase':label,'failure':'available_disk_below_20GiB','bytes':available}),file=sys.stderr,flush=True)
                os.killpg(os.getpgrp(),signal.SIGTERM)
                raise RuntimeError('低磁盘停止自有批次')
            stats = {}
            for path in pathlib.Path('/proc').iterdir():
                if not path.name.isdigit(): continue
                try:
                    raw=(path/'stat').read_text(); tail=raw[raw.rfind(')')+2:].split()
                    stats[int(path.name)]=(int(tail[1]),tail,path)
                except (FileNotFoundError,ProcessLookupError,PermissionError):continue
            owned={child.pid}
            for _ in range(12):
                extra={pid for pid,(ppid,_,_) in stats.items() if ppid in owned}
                if extra<=owned:break
                owned|=extra
            rows=[]
            for pid in owned:
                if pid not in stats:continue
                ppid,tail,path=stats[pid]
                try:
                    states={}
                    for task in (path/'task').iterdir():
                        try:
                            value=(task/'stat').read_text();state=value[value.rfind(')')+2:].split()[0];states[state]=states.get(state,0)+1
                        except (FileNotFoundError,ProcessLookupError):continue
                    rows.append({'pid':pid,'ppid':ppid,'comm':(path/'comm').read_text().strip(),'state':tail[0],'cpu_ticks':int(tail[11])+int(tail[12]),'threads':states,'rss_pages':int(tail[21]),'wchan':(path/'wchan').read_text().strip()})
                except (FileNotFoundError,ProcessLookupError,PermissionError):continue
            log.write(json.dumps({'elapsed_s':time.monotonic()-started,'available_bytes':available,'processes':rows})+'\n');log.flush();time.sleep(1)
    result=child.wait()
    print(json.dumps({'phase':label,'state':'completed','exit':result,'elapsed_s':time.monotonic()-started}),flush=True)
    if result:sys.exit(result)
with (output/'cargo-artifacts.jsonl').open('w') as stdout:run(argv,'cargo-five-binaries',stdout)
artifacts=[json.loads(line) for line in (output/'cargo-artifacts.jsonl').read_text().splitlines()]
artifacts=[a for a in artifacts if a.get('reason')=='compiler-artifact']
engine=[pathlib.Path(f) for a in artifacts if a['target']['name']=='engine' and not a['profile']['test'] for f in a['filenames'] if f.endswith('.rlib')]
serde=[pathlib.Path(f) for a in artifacts if a['target']['name']=='serde_json' for f in a['filenames'] if f.endswith('.rlib')]
assert len(engine)==len(serde)==1
producer=pathlib.Path('/data1/baiyifan/workplace/stock_market_game/agents/branch-completion/plan-identity-combined-producer.rs')
producer_binary=output/'combined-producer'
run(['rustc','--edition=2021','-C','debuginfo=0','-C','codegen-units=16',str(producer),'-L','dependency='+str(engine[0].parent),'--extern','engine='+str(engine[0]),'--extern','serde_json='+str(serde[0]),'-o',str(producer_binary)],'rustc-producer',None)
paths=[pathlib.Path(a['executable']) for a in artifacts if a.get('executable') and a['profile']['test']]+engine+[producer_binary,producer]
assert len([p for p in paths if p==producer_binary])==1
assert len([a for a in artifacts if a.get('executable') and a['profile']['test']])==5
def seal(path):
    with path.open('rb') as source:digest=hashlib.file_digest(source,'sha256').hexdigest()
    return {'path':str(path),'bytes':path.stat().st_size,'sha256':digest}
with concurrent.futures.ThreadPoolExecutor(max_workers=5) as pool:sealed=list(pool.map(seal,paths))
(output/'build-artifact-seals.json').write_text(json.dumps({'elapsed_s':time.monotonic()-started,'artifacts':sealed,'case_binaries':5,'cargo_jobs':8,'hash_threads':5},indent=2)+'\n')
print(json.dumps({'phase':'build-and-seal','exit':0,'elapsed_s':time.monotonic()-started}),flush=True)
