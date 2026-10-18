from pathlib import Path
import base64,hashlib,json
root=Path.cwd(); m=json.loads((root/'agents/engine-max-performance/npc-snapshot-originals.json').read_text())
for path,e in m['originals'].items():
 b=base64.b64decode(e['base64']); assert hashlib.sha256(b).hexdigest()==e['sha256']; (root/path).write_bytes(b)
p=root/m['module'];
if p.exists(): p.unlink()
(root/'agents/engine-max-performance/npc-snapshot-originals.json').unlink(); print('restored',len(m['originals']))
