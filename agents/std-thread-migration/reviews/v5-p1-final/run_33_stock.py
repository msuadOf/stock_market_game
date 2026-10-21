"""33 股 wire 边界独立长验收；JSON 单流往返必须顺序执行。"""
import json, os, resource, signal, subprocess, time
from pathlib import Path
OUT=Path(__file__).resolve().parent
binary=OUT/'probe-binary'
assert binary.is_file(), '必须先完成普通 World 版本的探针构建'
t=time.monotonic()
with (OUT/'33-stock-long.log').open('wb') as log:
    p=subprocess.Popen([str(binary),'recheck_idle_33_stock_json','--test-threads=1','--nocapture'],env=dict(os.environ,ENGINE_STEP_RUNTIME='1'),stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
    try: rc=p.wait(timeout=295)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid,signal.SIGKILL); p.wait(); rc=124
u=resource.getrusage(resource.RUSAGE_CHILDREN)
s=dict(rc=rc,seconds=round(time.monotonic()-t,3),child_cpu_seconds=round(u.ru_utime+u.ru_stime,3),deadline_s=299)
(OUT/'33-stock-summary.json').write_text(json.dumps(s,indent=2)+'\n')
print(json.dumps(s),flush=True)
raise SystemExit(rc)
