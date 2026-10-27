# ordinary 税日正式候选验证：复用已审 R4/P1 外部监督模板，唯一 RAM target。
from pathlib import Path
import concurrent.futures
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time

work = Path(__file__).resolve().parents[4]
root = Path('/data1/baiyifan/workplace/stock_market_game')
node = root / '.tmp/node24-env/node'
target = Path('/tmp/stock-market-game-validation-2026-10-27-1311109/target')
helper_repository = root / '.worktree/main-merge-2026-10-25'
helper = helper_repository / 'scripts/run-with-deadline.mjs'
long_helper = helper.with_name('run-long-validation.mjs')
guard = root / '.worktree/runtime-sync-e1/agents/perf-intraday-independent/implementation/e2/day-end-full-source-tmpfs-batch-r2/run-build-guard-tmpfs.mjs'
child_status_wrapper = root / 'agents/branch-completion/funds-production-resume-2026-10-27/p1-formal-red-2026-10-28/collect-child-status.py'
unit_prefix = 'company::cash_dividend_tax::day_boundary::tests::'
unit_names = ['actual_reg_prepare_zero_market_day_advances_tax_and_binds_exact_plan_capture', 'missing_watermark_and_omitted_confirmations_reject_without_mutating_inputs', 'complete_lot_attribution_rejects_90_physical_100_tax_and_equal_total_spoofs', 'calendar_closed_and_absent_holder_change_still_record_exactly_one_zero_tax_day', 'any_current_or_historical_nonmarket_movement_is_explicitly_unsupported', 'actual_world_fills_normalize_into_reg_and_exact_tax_net_acquisition_and_disposition', 'equal_tail_cannot_move_a_historical_sale_across_tax_holding_period_boundary', 'stock_batch_keeps_one_registry_and_rejects_duplicate_tax_book_accounts', 'late_first_buyer_opens_empty_tax_book_at_own_frontier_after_existing_registry_history', 'depleted_acquisition_and_opening_lot_sources_remain_bound_to_registry_history', 'opening_requires_actual_frontier_and_batch_uses_account_identity']

def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def source_manifest():
    names = subprocess.check_output(
        ['git', 'ls-files', '-co', '--exclude-standard'], cwd=work, text=True
    ).splitlines()
    names = sorted(set(name for name in names if (
        name in ('Cargo.lock', 'Cargo.toml', '.cargo/config.toml')
        or name.endswith('/Cargo.toml')
        or (name.startswith('packages/engine/') and name.endswith('.rs'))
    )))
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        return list(pool.map(lambda name: {'path': name, 'sha256': sha(work / name)}, names))


def run_logged(out, label, command, maximum_ms=10000):
    remaining = min(maximum_ms, int((deadline - time.time()) * 1000))
    if remaining <= 0:
        raise RuntimeError('共享执行 299 秒已耗尽，不能重赠 deadline')
    supervisor = long_helper if maximum_ms > 10000 else helper
    argv = [str(node), str(supervisor), str(remaining), '--', *map(str, command)]
    started = time.monotonic()
    with (out / (label + '.stdout')).open('xb') as stdout, (out / (label + '.stderr')).open('xb') as stderr:
        result = subprocess.run(argv, cwd=work, stdout=stdout, stderr=stderr)
    return {'name': label, 'argv': argv, 'exit': result.returncode, 'wallSeconds': time.monotonic() - started}


if len(sys.argv) != 3 or sys.argv[1] not in ('prepare', 'run') or sys.argv[2] not in ('formal-green-01', 'formal-green-02', 'formal-green-03'):
    raise RuntimeError('用法：run-source-formal.py prepare|run formal-green-03')
mode, batch = sys.argv[1:]
out = Path(__file__).resolve().parent / batch
owner = target.parent / ('tax-source-' + batch)
if mode == 'prepare':
    out.mkdir(exist_ok=True)
    environment = {'CARGO_TARGET_DIR': str(target), 'CARGO_INCREMENTAL': '0', 'CARGO_BUILD_JOBS': '8', 'TMPDIR': str(owner / 'scratch'), 'RUSTFLAGS': '', 'CARGO_ENCODED_RUSTFLAGS': ''}
    command = ['cargo', 'test', '--offline', '-p', 'engine', '--lib', '--no-run', '-j', '8', '--message-format=json-render-diagnostics']
    if sha(child_status_wrapper) != 'c0a06ed34615572d91f8636999ed99bfea3aec7d0a70c458851cf3f9c95fd3f7':
        raise RuntimeError('已审 child status wrapper 漂移')
    node_version = subprocess.check_output([node, '--version'], text=True).strip()
    if node_version != 'v24.18.0':
        raise RuntimeError('冻结 Node 版本不符：' + node_version)
    runners = [Path(__file__).resolve(), helper, long_helper, guard, child_status_wrapper]
    plan = {
        'name': 'tax', 'cwd': str(work), 'target': str(target), 'command': command,
        'cargoCommand': command, 'cargoJobs': 8, 'environment': environment,
        'deadlineMs': 300000, 'noProgressMs': 60000, 'sampleMs': 500,
        'cargoLogPaths': [str(out / 'tax-guard.stdout'), str(out / 'tax-guard.stderr')],
        'buildReceiptPath': str(out / 'unused-build-receipt.json'),
        'mainHelperRepository': str(helper_repository),
        'mainHelperHead': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=helper_repository, text=True).strip(),
        'productSources': source_manifest(),
        'runnerSources': [{'path': str(path), 'sha256': sha(path)} for path in runners],
        'nodePath': str(node), 'nodeVersion': node_version,
    }
    with (out / 'plan-template.json').open('x') as stream:
        stream.write(json.dumps(plan, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({'batch': batch, 'productSources': len(plan['productSources']), 'planSha256': sha(out / 'plan-template.json'), 'cargoJobs': 8, 'testWorkers': 8}))
    sys.exit(0)

started = time.monotonic()
deadline = time.time() + 299
# 先独占取得本次 receipt；重复运行在 try/finally 外直接拒绝，绝不覆盖旧件。
receipt_file = (out / 'batch-receipt.json').open('x')
owner_created = False
receipt = {'batch': batch, 'target': str(target), 'cargoJobs': 8, 'testWorkers': 8, 'outerDeadlineMs': 300000, 'executionDeadlineMs': 299000, 'ordinaryCaseAndCommandDeadlineMs': 10000, 'actualArgv': sys.argv, 'actualParentArgv': Path(f'/proc/{os.getppid()}/cmdline').read_bytes().decode().split('\0')[:-1], 'stages': []}
try:
    if str(long_helper) not in receipt['actualParentArgv'] or '300000' not in receipt['actualParentArgv']:
        raise RuntimeError('本批必须由指定 main run-long-validation.mjs 300000 外监督启动')
    plan = json.loads((out / 'plan-template.json').read_text())
    for row in plan['runnerSources']:
        if sha(Path(row['path'])) != row['sha256']:
            raise RuntimeError('runner 漂移：' + row['path'])
    if str(node) != plan['nodePath'] or subprocess.check_output([node, '--version'], text=True).strip() != plan['nodeVersion']:
        raise RuntimeError('实际 Node path/version 与冻结不符')
    before = source_manifest()
    if before != plan['productSources']:
        raise RuntimeError('冻结后产品源漂移')
    disk = os.statvfs(target)
    receipt['targetAvailableBytesBefore'] = disk.f_bavail * disk.f_frsize
    if receipt['targetAvailableBytesBefore'] < 20 * 1024 ** 3:
        raise RuntimeError('统一 RAM target 余量不足 20GiB')
    owner.mkdir(exist_ok=False)
    owner_created = True
    (owner / 'scratch').mkdir()
    # 上一槽 E2 完成后才提供真实 default unit 绑定，不沿用旧 R4 制品。
    prior_path = out / 'prior-artifact.json'
    prior = json.loads(prior_path.read_text())
    receipt['priorBinding'] = {'path': str(prior_path), 'sha256': sha(prior_path)}
    old = Path(prior['path'])
    if not old.is_relative_to(target) or prior['features'] != []:
        raise RuntimeError('仅保护同 RAM target 内 E2 实际 default unit')
    if sha(old) != prior['sha256'] or old.stat().st_size != prior['bytes']:
        raise RuntimeError('E2 实际 default unit 与新鲜绑定不符')
    if 'sealedExisting' in prior:
        sealed = Path(prior['sealedExisting'])
        if not sealed.is_relative_to(target.parent) or sealed.is_relative_to(target):
            raise RuntimeError('既有 seal 必须在同 RAM 根且在 Cargo target 外')
        if sha(sealed) != prior['sha256'] or sealed.stat().st_size != prior['bytes'] or old.stat().st_ino == sealed.stat().st_ino:
            raise RuntimeError('既有单件 seal SHA/大小/独立 inode 不符')
        receipt['protectedPrior'] = {**prior, 'sealed': str(sealed), 'distinctInode': True, 'reusedExistingSeal': True, 'persistence': '仅RAM非持久，root统一保留'}
    else:
        protection_dir = target.parent / ('tax-source-protection-' + batch)
        protection_dir.mkdir(exist_ok=False)
        sealed = protection_dir / 'sealed-prior-default-unit'
        copied = run_logged(out, 'seal-single-prior-default-unit', ['cp', '--reflink=never', '-p', old, sealed], 299000)
        receipt['stages'].append(copied)
        if copied['exit'] or sha(sealed) != prior['sha256'] or sealed.stat().st_size != prior['bytes'] or old.stat().st_ino == sealed.stat().st_ino:
            raise RuntimeError('单件保护副本 SHA/大小/独立 inode 不符')
        receipt['protectedPrior'] = {**prior, 'sealed': str(sealed), 'distinctInode': True, 'reusedExistingSeal': False, 'persistence': '仅RAM非持久，root统一保留'}
    plan['protectedPrior'] = receipt['protectedPrior']
    plan['sharedDeadlineUnixMs'] = int(deadline * 1000)
    plan_path = out / 'build-plan.json'
    plan_path.write_text(json.dumps(plan, ensure_ascii=False, indent=2) + '\n')
    receipt['buildPlanSha256'] = sha(plan_path)
    build = run_logged(out, 'build', [str(node), str(guard), str(plan_path)], 299000)
    receipt['stages'].append(build)
    if build['exit']:
        raise RuntimeError('真实构建失败；禁止拿旧 binary 跑新源码')
    artifacts = {}
    for line in (out / 'tax-guard.stdout').read_text().splitlines():
        row = json.loads(line)
        if row.get('reason') == 'compiler-artifact' and row.get('executable'):
            path = Path(row['executable'])
            if not path.is_relative_to(target):
                raise RuntimeError('产物不在统一 RAM target')
            artifacts[row['target']['name']] = {'path': str(path), 'sha256': sha(path), 'fresh': row['fresh']}
    if set(artifacts) != {'engine'}:
        raise RuntimeError('实际构建未交付恰好 engine unit 产物')
    receipt['artifacts'] = artifacts
    cases = [('engine', unit_prefix + name, name) for name in unit_names]

    def run_case(case):
        kind, full_name, name = case
        artifact = artifacts[kind]
        child_status_path = out / (name + '.child-status.json')
        result = run_logged(out, name, ['python3', child_status_wrapper, child_status_path,
                                      artifact['path'], full_name, '--exact', '--test-threads=1', '--nocapture'])
        stdout = (out / (name + '.stdout')).read_text()
        stderr = (out / (name + '.stderr')).read_text()
        summary = re.findall(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed;', stdout)
        child_status = json.loads(child_status_path.read_text()) if child_status_path.exists() else None
        result.update({'binarySha256': artifact['sha256'], 'summary': summary,
                       'childStatus': child_status, 'supervisorCompleted': result['exit'] == 0})
        result['matchedExpectedBehavior'] = result['exit'] == 0 and child_status is not None and child_status['exit'] == 0 and summary == [('ok', '1', '0')]
        return result

    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        receipt['tests'] = list(pool.map(run_case, cases))
    if sha(sealed) != prior['sha256'] or sealed.stat().st_size != prior['bytes']:
        raise RuntimeError('本批构建后 E2 单件保护制品漂移')
    receipt['priorProtectedUnchanged'] = True
    after = source_manifest()
    receipt['sourceUnchanged'] = before == after
    receipt['productSources'] = before
    receipt['allExpectedResults'] = all(row['matchedExpectedBehavior'] for row in receipt['tests'])
    if not receipt['sourceUnchanged'] or not receipt['allExpectedResults']:
        raise RuntimeError('源码漂移或真实用例未满足本批预期，详见逐项原件')
    receipt['status'] = 'ordinary 税日真实 engine unit 11 项绿（仍无生产 caller）'
except BaseException as error:
    receipt['status'] = '失败'
    receipt['error'] = repr(error)
    print(str(error), file=sys.stderr)
    sys.exit(1)
finally:
    if owner_created:
        try:
            # 本目录只存本批 Cargo 临时件；统一 target 与所有日志/产物均不在其中。
            shutil.rmtree(owner)
            receipt['ownTemporaryDirectoryCleaned'] = True
        except OSError as error:
            receipt['ownTemporaryDirectoryCleaned'] = False
            receipt['cleanupError'] = repr(error)
            receipt['status'] = '失败'
    receipt['wallSeconds'] = time.monotonic() - started
    if time.time() >= deadline:
        receipt['status'] = '失败'
        receipt['error'] = '工作/清理/receipt 发布必须在共享 299 秒内完成'
        receipt['workDeadlineExceeded'] = True
    receipt_file.write(json.dumps(receipt, ensure_ascii=False, indent=2) + '\n')
    receipt_file.close()
    if time.time() >= deadline:
        with (out / 'work-deadline-failed.json').open('x') as stream:
            json.dump({'status': '失败', 'reason': 'receipt 发布越过共享 299 秒'}, stream)
        receipt['status'] = '失败'
    print(json.dumps({'status': receipt['status'], 'wallSeconds': receipt['wallSeconds']}, ensure_ascii=False), flush=True)
    if receipt['status'] == '失败':
        sys.exit(1)
