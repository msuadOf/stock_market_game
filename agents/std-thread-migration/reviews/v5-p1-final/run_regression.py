"""终复核长验收：独立 case 进程、共享五分钟 deadline、多核分片。"""
import collections, concurrent.futures, json, os, resource, signal, subprocess, time
from pathlib import Path
ROOT=Path.cwd(); OUT=ROOT/'agents/std-thread-migration/reviews/v5-p1-final'
START=time.monotonic(); END=START+298; WORKERS=32

def invoke(args,env,limit=9.5):
    p=subprocess.Popen(args,cwd=ROOT,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,start_new_session=True)
    try:
        data=p.communicate(timeout=min(limit,max(.01,END-time.monotonic())))[0]; rc=p.returncode
    except subprocess.TimeoutExpired:
        os.killpg(p.pid,signal.SIGKILL); data=p.communicate()[0]; rc=124
    return rc,data.decode(errors='replace')

artifacts=[]
for package in ('engine','server'):
    for line in (OUT/f'build-{package}.jsonl').read_text().splitlines():
        x=json.loads(line)
        if x.get('executable') and x.get('profile',{}).get('test') and f'/{"packages/engine" if package=="engine" else "apps/server"}/' in x['target']['src_path']:
            artifacts.append((package,x['target']['name'],x['target']['kind'],x['executable']))
tasks=[]; counts=collections.defaultdict(collections.Counter)
for package,name,kind,binary in artifacts:
    for mode in (['0','1'] if package=='engine' and 'rlib' in kind else ['1']):
        group='lib-'+mode if package=='engine' and 'rlib' in kind else package
        env=dict(os.environ,ENGINE_STEP_RUNTIME=mode)
        rc,listing=invoke([binary,'--list','--format=terse'],env); assert rc==0,(name,listing)
        rc,ignored=invoke([binary,'--list','--ignored','--format=terse'],env); assert rc==0,(name,ignored)
        ignored={l[:-6] for l in ignored.splitlines() if l.endswith(': test')}
        cases=[l[:-6] for l in listing.splitlines() if l.endswith(': test')]
        counts[group]['ignored']+=len(ignored)
        tasks.extend((group,name,binary,c,env) for c in cases if c not in ignored)
print(json.dumps(dict(workers=WORKERS,cpu=os.cpu_count(),case_deadline_s=9.5,batch_deadline_s=299,binaries=len(artifacts),cases=len(tasks))),flush=True)
def run(t):
    group,name,binary,case,env=t; start=time.monotonic()
    rc,output=invoke([binary,case,'--exact','--test-threads=1'],env)
    return dict(group=group,binary=name,case=case,rc=rc,seconds=round(time.monotonic()-start,3),output=output)
with (OUT/'cases.jsonl').open('w') as log:
    for workers,phase in [(32,[t for t in tasks if 'export_bindings' not in t[3]]),(4,[t for t in tasks if 'export_bindings' in t[3]])]:
        print('阶段并发',workers,'cases',len(phase),flush=True)
        with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
            for i,result in enumerate(pool.map(run,phase),1):
                status='passed' if result['rc']==0 else 'timeout' if result['rc']==124 else 'failed'
                counts[result['group']][status]+=1
                if result['group']=='lib-1': counts['engine'][status]+=1
                log.write(json.dumps(result,ensure_ascii=False)+'\n'); log.flush()
                if result['rc']: print(json.dumps({k:v for k,v in result.items() if k!='output'}),flush=True)
                if i%500==0: print('完成',i,'秒',round(time.monotonic()-START,1),flush=True)
counts['engine']['ignored']+=counts['lib-1']['ignored']
u=resource.getrusage(resource.RUSAGE_CHILDREN)
summary=dict(counts={k:dict(v) for k,v in counts.items()},seconds=round(time.monotonic()-START,3),child_cpu_seconds=round(u.ru_utime+u.ru_stime,3))
(OUT/'summary.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n')
print(json.dumps(summary,ensure_ascii=False),flush=True)
