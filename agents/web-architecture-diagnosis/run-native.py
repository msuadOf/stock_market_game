# 依次启动三个独立 session，记录进程资源采样和原始输出。
import subprocess,time,json,statistics,os
path='agents/web-architecture-diagnosis/native-engine-results.json'
runs=[]
for n in range(3):
    p=subprocess.Popen(['target/release/bare-world-diagnostic','agents/web-architecture-diagnosis/default-setup.json'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    samples=[]
    while p.poll() is None:
        ps=subprocess.run(['ps','-p',str(p.pid),'-o','%cpu=,rss='],capture_output=True,text=True)
        if ps.returncode==0 and ps.stdout.strip():
            fields=ps.stdout.split()
            if len(fields)>=2:
                threads=subprocess.run(['ps','-M','-p',str(p.pid)],capture_output=True,text=True)
                samples.append({'cpu_percent':float(fields[0]),'threads':max(0,len(threads.stdout.splitlines())-1),'rss_kib':int(fields[1])})
        time.sleep(0.5)
    stdout,stderr=p.communicate()
    runs.append({'run':n+1,'exit_code':p.returncode,'stdout':stdout,'stderr':stderr,'samples':samples})
    with open(path,'w') as f: json.dump({'runs':runs},f,ensure_ascii=False,indent=2)
    if p.returncode: raise SystemExit(p.returncode)
