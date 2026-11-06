# 仅刷新本人 engine 根源码的 mtime 使 Cargo 检查既有 fingerprint；不改源码字节。
from pathlib import Path
import hashlib
import json
import os
import sys
import time

ROOT = Path('/data1/<USER>/workplace/stock_market_game')
WORK = ROOT / '.worktree/d3-p1-integration-2026-10-28'
OWNER = Path('/tmp/stock-market-game-validation-2026-10-27-1311109/engine-r4-payment-red-2026-10-29-01')
TARGET = OWNER.parent / 'target'
SOURCE = 'packages/engine/src/lib.rs'


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def check_identity(row):
    path = Path(row['path'])
    if path.is_symlink():
        raise RuntimeError(f'本人源码不能为 symbolic link：{path}')
    stat = path.stat()
    actual = (stat.st_dev, stat.st_ino, stat.st_size, sha(path))
    expected = (row['dev'], row['inode'], row['bytes'], row['sha256'])
    if actual != expected:
        raise RuntimeError(f'本人源码 inode/字节变化，拒绝恢复：{path}')


def restore(receipt):
    receipt = Path(receipt)
    data = json.loads(receipt.read_text())
    expected = {str(WORK / SOURCE)}
    if data['work'] != str(WORK) or {row['path'] for row in data['sources']} != expected:
        raise RuntimeError('恢复 receipt 必须只属于固定 engine 源码')
    # 先核验唯一源码身份，避免并发修改时部分恢复掩盖问题。
    for row in data['sources']:
        check_identity(row)
    for row in data['sources']:
        path = Path(row['path'])
        os.utime(path, ns=(row['originalAtimeNs'], row['originalMtimeNs']))
        stat = path.stat()
        if (stat.st_atime_ns, stat.st_mtime_ns) != (row['originalAtimeNs'], row['originalMtimeNs']):
            raise RuntimeError(f'源码纳秒时间恢复失败：{path}')
    data['restored'] = True
    data['restoredAtUnixNs'] = time.time_ns()
    receipt.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')
    return data


def refresh(receipt, manifest):
    receipt = Path(receipt)
    manifest = Path(manifest)
    if manifest.parent != receipt.parent or manifest.name != 'product-source-manifest.json':
        raise RuntimeError('refresh 源码清单必须为同批固定 product-source-manifest.json')
    manifest_rows = json.loads(manifest.read_text())
    if not isinstance(manifest_rows, list):
        raise RuntimeError('源码清单必须是 product source rows 数组')
    matching = [row for row in manifest_rows if row.get('path') == SOURCE]
    if len(matching) != 1 or set(matching[0]) != {'path', 'sha256'}:
        raise RuntimeError('同批源码清单必须唯一绑定 engine 根源码 SHA')
    expected_sha = matching[0]['sha256']
    if not isinstance(expected_sha, str) or len(expected_sha) != 64:
        raise RuntimeError('engine 根源码 SHA 格式非法')

    path = WORK / SOURCE
    if path.is_symlink():
        raise RuntimeError(f'本人源码不能为 symbolic link：{path}')
    stat = path.stat()
    actual_sha = sha(path)
    if actual_sha != expected_sha:
        raise RuntimeError(f'本人源码与同批 product-source-manifest SHA 不符：{path}')

    # 在限定的 fingerprint 子树内查找 invoked.timestamp，并仅看 debug 直下的 timestamp。
    debug_root = TARGET / 'debug'
    fingerprint_root = debug_root / '.fingerprint'
    invoked = sorted(fingerprint_root.glob('engine-*/invoked.timestamp'))
    top_level = sorted(debug_root.glob('*.timestamp'))
    timestamps = invoked + top_level
    maximum = max((item.stat().st_mtime_ns for item in timestamps), default=0)
    row = {'path': str(path), 'dev': stat.st_dev, 'inode': stat.st_ino, 'bytes': stat.st_size,
           'sha256': actual_sha, 'originalAtimeNs': stat.st_atime_ns,
           'originalMtimeNs': stat.st_mtime_ns,
           'consideredTimestamps': [{'path': str(item), 'mtimeNs': item.stat().st_mtime_ns} for item in timestamps],
           'invokedMaxMtimeNs': maximum, 'coldNoInvokedTimestamp': not invoked,
           'requestedMtimeNs': max(time.time_ns(), maximum + 1)}
    data = {'kind': 'engine 根源码 mtime 失效与纳秒恢复记录', 'work': str(WORK),
            'sources': [row], 'restored': False}
    # 原始元数据先持久化；子进程被终止后由外监督恢复。
    with receipt.open('x') as stream:
        json.dump(data, stream, ensure_ascii=False, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    try:
        check_identity(row)
        os.utime(path, ns=(row['originalAtimeNs'], row['requestedMtimeNs']))
        changed = path.stat()
        if ((changed.st_dev, changed.st_ino, changed.st_size) != (row['dev'], row['inode'], row['bytes'])
                or changed.st_mtime_ns != row['requestedMtimeNs']
                or changed.st_mtime_ns <= maximum):
            raise RuntimeError(f'源码 identity 或 mtime 未满足既有 invoked 准入：{path}')
    except BaseException:
        restore(receipt)
        raise
    return data


if __name__ == '__main__':
    action, receipt, *extra = sys.argv[1:]
    if action == 'restore' and extra:
        raise RuntimeError('restore 不接受额外来源参数')
    if action == 'restore':
        result = restore(receipt)
    elif action == 'refresh' and len(extra) == 1:
        result = refresh(receipt, extra[0])
    else:
        raise RuntimeError('只接受 refresh <receipt> <product-source-manifest.json> 或 restore <receipt>')
    print(json.dumps({'action': action, 'sources': len(result['sources']),
                      'restored': result['restored']}, ensure_ascii=False))
