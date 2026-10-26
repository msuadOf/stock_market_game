"""校验真实源码、依赖和 binary 绑定后运行；外层 supervisor 统一约束10秒。"""
from pathlib import Path
import hashlib
import json
import os
import subprocess
import sys
import time
out = Path('/data1/baiyifan/workplace/stock_market_game/.tmp/tax-entitlement-k1')
label, suite = sys.argv[1:]
receipt = json.loads((out/(label+'-artifact.json')).read_text())
binary = Path(receipt.get('binary_path',str(out/'light-harness')))
def verify():
    for item in receipt['files'] + receipt['dependencies']:
        if hashlib.sha256(Path(item['path']).read_bytes()).hexdigest() != item['sha256']:
            raise RuntimeError('源码或依赖改变：'+item['path'])
    if hashlib.sha256(binary.read_bytes()).hexdigest() != receipt['binary_sha256']:
        raise RuntimeError('binary已改变')
verify()
argv = [str(binary), suite, '--test-threads=8', '--nocapture']
started = time.monotonic()
result = subprocess.run(argv, env={**os.environ, 'TS_RS_EXPORT_DIR':str(out/'bindings')})
verify()
run = {'label':label,'argv':argv,'binary_sha256':receipt['binary_sha256'],'source_verified_before_and_after':True,'exit':result.returncode,'wall_seconds':time.monotonic()-started}
(out/(label+'-run.json')).write_text(json.dumps(run,ensure_ascii=False,indent=2)+'\n')
raise SystemExit(result.returncode)
