from pathlib import Path
from statistics import median
from math import ceil
import json,re,sys
root=Path(sys.argv[1])
paths=[root/f'{pair}-{host}.json' for pair in range(1,4) for host in ('native','wasm')]
receipts={}
for p in paths:
 if not p.is_file():raise SystemExit(f'缺少样本：{p}')
 r=json.loads(p.read_text())
 if r.get('failure') or r.get('errors') or [s.get('speed') for s in r.get('samples',[])] != ['1','Infinity']:
  raise SystemExit(f'失败或不完整的样本：{p}')
 if r.get('target')!=p.stem.split('-')[1] or (r['target']=='native' and r['actual'].get('deliveryMode')!='push'):
  raise SystemExit(f'样本宿主或投递模式不符：{p}')
 if r.get('setup')!=r['actual'].get('setup') or r.get('actual',{}).get('seedHits')!=1:
  raise SystemExit(f'实际 fixture 与记录不符：{p}')
 if receipts and r['setup']!=next(iter(receipts.values()))['setup']:
  raise SystemExit(f'样本使用了不同负载：{p}')
 receipts[p]=r
def qr(xs,p):
 return sorted(xs)[max(0,ceil(len(xs)*p)-1)] if xs else None
all_results={}
for p,r in receipts.items():
 print(p.name,'push:',r['actual'].get('deliveryMode'),'workers:',len(r['workers']))
 measures=[]
 for period in r['samples']:
  s,e=period['start'],period['end']
  duration=e['at']-s['at']
  v=e['visual']
  if not (duration>0 and 5<=s['tick']<e['tick']<r['setup']['ticks_per_day']-r['setup']['closing_auction_ticks']):
   raise SystemExit(f'无有效的首日界面推进：{p} {period["speed"]}')
  ui=(e['tick']-s['tick'])*1000/duration
  gaps=[b['at']-a['at'] for a,b in zip(v['raf'],v['raf'][1:])]
  mt=[m for m in e['metrics'] if m.get('running') and m['requested']['mode']==('fixed' if period['speed']=='1' else 'fastest') and m['at']-m['sample_duration_ms'] >= s['at'] and m['at']<=e['at']]
  if not mt or not all(m['sample_duration_ms']>0 for m in mt):raise SystemExit(f'缺少有效的权威速度摘样：{p} {period["speed"]}')
  weighted=sum(m['sample_ticks'] for m in mt)*1000/sum(m['sample_duration_ms'] for m in mt)
  before={x['name']:x['value'] for x in period['beforeMetrics']['metrics']};after={x['name']:x['value'] for x in period['afterMetrics']['metrics']}
  lt=[x for x in v['longTasks'] if x['start']>=s['at'] and x['start']+x['duration']<=e['at']]
  res={'speed':period['speed'],'tickRange':(s['tick'],e['tick']),'uiTicks/s':ui,'authorityApproxTicks/s':weighted,'authorityWindows':len(mt),'vsyncCallbacks/s':len(v['raf'])*1000/duration,'DOMTickChanges/s':len(v['commits'])*1000/duration,'gap95ms':qr(gaps,.95),'gap99ms':qr(gaps,.99),'gapMaxMs':max(gaps,default=0),'gaps>33.4ms':sum(g>33.4 for g in gaps),'longTasks':len(lt),'longMs':sum(t['duration'] for t in lt),'scriptMs/s':(after['ScriptDuration']-before['ScriptDuration'])*1e6/duration,'TaskMs/s':(after['TaskDuration']-before['TaskDuration'])*1e6/duration}
  measures.append(res)
  print(json.dumps(res))
 all_results[p.stem]=measures
for host in ['native','wasm']:
 rounds=[v for k,v in all_results.items() if k.split('-')[1]==host and len(v)==2]
 if len(rounds)!=3:continue
 for metric in ['uiTicks/s','authorityApproxTicks/s','vsyncCallbacks/s','DOMTickChanges/s','gap95ms','gap99ms','longTasks','scriptMs/s','TaskMs/s']:
  b=[r[0][metric] for r in rounds];m=[r[1][metric] for r in rounds]
  print(host,metric,'1×',round(median(b),3),'MAX',round(median(m),3),'MAX range',[round(min(m),3),round(max(m),3)],'median-paired-ratio',round(median([u/v for u,v in zip(m,b)]),3) if min(b)>0 else None)
