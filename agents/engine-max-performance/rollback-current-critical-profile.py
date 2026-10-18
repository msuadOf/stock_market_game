"""按 manifest 原字节撤回 current critical-path 插桩。"""
from pathlib import Path
import base64, hashlib, json
root = Path.cwd()
manifest = json.loads((root / 'agents/engine-max-performance/current-critical-path-originals.json').read_text())
for path, entry in manifest['originals'].items():
    payload = base64.b64decode(entry['base64'])
    assert hashlib.sha256(payload).hexdigest() == entry['sha256']
    (root / path).write_bytes(payload)
module = root / manifest['module']
if module.exists():
    module.unlink()
(root / 'agents/engine-max-performance/current-critical-path-originals.json').unlink()
print(f"已恢复 {len(manifest['originals'])} 份源码并删除临时 module")
