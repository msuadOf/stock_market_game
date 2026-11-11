#!/usr/bin/env python3
"""验证第三方快照与固定来源清单的文件内容和路径一致。"""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parents[3]
source = root / 'vendor' / 'web-thread' / 'upstream'
manifest = json.loads((root / 'vendor' / 'web-thread' / 'sha256.json').read_text())
actual = {str(p.relative_to(source)): hashlib.sha256(p.read_bytes()).hexdigest()
          for p in source.rglob('*') if p.is_file()}
if actual != manifest:
    missing = sorted(manifest.keys() - actual.keys())
    extra = sorted(actual.keys() - manifest.keys())
    changed = sorted(k for k in actual.keys() & manifest.keys() if actual[k] != manifest[k])
    raise SystemExit(f'快照校验失败：missing={missing}, extra={extra}, changed={changed}')
print(f'快照校验通过：{len(actual)} 个文件')
