import concurrent.futures
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

root = Path.cwd()
out = root / '.tmp/perf-binding'

def command(args, cwd=root):
    return subprocess.check_output(args, cwd=cwd, stderr=subprocess.STDOUT)

def write_new(path, data):
    with path.open('xb') as stream:
        stream.write(data if isinstance(data, bytes) else data.encode())

write_new(out / 'initial-status.txt', command(['git', 'status', '--porcelain=v1']))
write_new(out / 'initial-tracked.diff', command(['git', 'diff', '--binary', 'HEAD']))
write_new(out / 'ledger.txt', (root / 'agents/std-thread-migration/evidence/perf-p2/perf-phase1-final.txt').read_bytes())
write_new(out / 'prior-closure.md', (root / 'agents/std-thread-migration/evidence/perf-p2/binary-provenance-closure.md').read_bytes())
write_new(out / 'source-difference.patch', command(['git', 'diff', '1f5863cd', '6dcc25b4']))
write_new(out / 'host-toolchain.txt', command(['rustc', '-vV']) + command(['cargo', '-V']) + command(['uname', '-a']) + command(['nproc']))

def rebuild(short):
    worktree = root / '.worktree' / ('perf-bind-' + short)
    evidence = out / short
    evidence.mkdir()
    for name, args in {
        'commit.txt': ['git', 'show', '-s', '--format=fuller', 'HEAD'],
        'tree.txt': ['git', 'rev-parse', 'HEAD^{tree}'],
        'source-files.txt': ['git', 'ls-tree', '-r', '--full-tree', 'HEAD'],
        'status-before.txt': ['git', 'status', '--porcelain=v1', '--untracked-files=all'],
    }.items():
        write_new(evidence / name, command(args, worktree))
    subprocess.run(['git', 'archive', '--format=tar', '--output=' + str(evidence / 'source.tar'), 'HEAD'], cwd=worktree, check=True)
    write_new(evidence / 'source-archive.sha256', hashlib.file_digest((evidence / 'source.tar').open('rb'), 'sha256').hexdigest() + '  source.tar\n')
    env = os.environ.copy()
    removed = {}
    for key in ['CARGO_TARGET_DIR', 'CARGO_BUILD_TARGET', 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS']:
        if key in env:
            removed[key] = env.pop(key)
    # 未记录的 profile override 不冒充历史构建参数；显式记录实际继承值。
    selected_env = {k: v for k, v in env.items() if k.startswith('CARGO_PROFILE_') or k in ['RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_RUSTFLAGS']}
    args = ['cargo', 'build', '--offline', '--locked', '--release', '--example', 'driver_semantics_probe', '--example', 'runtime_s5_probe', '-p', 'engine', '-j', '16', '-vv']
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    with (evidence / 'rebuild-log.txt').open('xb') as log:
        log.write(('开始 UTC: ' + started + '\n工作目录: ' + str(worktree) + '\n命令: ' + ' '.join(args) + '\n独立 target: ' + str(worktree / 'target') + '\n移除环境变量: ' + json.dumps(removed) + '\n继承构建 override: ' + json.dumps(selected_env) + '\nCargo jobs: 16；两提交并行，总上限 32 jobs；profile.release: thin LTO/codegen-units=16\n').encode())
        log.flush()
        result = subprocess.run(args, cwd=worktree, env=env, stdout=log, stderr=subprocess.STDOUT)
        log.write(('\n结束 UTC: ' + datetime.datetime.now(datetime.timezone.utc).isoformat() + '\n退出码: ' + str(result.returncode) + '\n').encode())
    hashes = {}
    if result.returncode == 0:
        binaries = evidence / 'binaries'
        binaries.mkdir()
        for name in ['driver_semantics_probe', 'runtime_s5_probe']:
            source = worktree / 'target/release/examples' / name
            shutil.copy2(source, binaries / name)
            hashes[name] = hashlib.file_digest(source.open('rb'), 'sha256').hexdigest()
    write_new(evidence / 'result.json', json.dumps({'commit': command(['git', 'rev-parse', 'HEAD'], worktree).decode().strip(), 'exit_code': result.returncode, 'hashes': hashes}, indent=2) + '\n')
    write_new(evidence / 'status-after.txt', command(['git', 'status', '--porcelain=v1', '--untracked-files=all'], worktree))
    print(short, result.returncode, hashes, flush=True)

with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
    list(pool.map(rebuild, ['1f5863cd', '6dcc25b4']))
