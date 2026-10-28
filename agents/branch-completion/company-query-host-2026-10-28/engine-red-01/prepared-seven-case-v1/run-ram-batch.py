# 六项公司只读查询首红；仅 engine 七个短 case，不包含三宿主验收。
from pathlib import Path
import concurrent.futures, hashlib, json, os, re, subprocess, sys, time

root = Path('/data1/baiyifan/workplace/stock_market_game')
work = root / '.worktree/company-query-host-2026-10-28'
out = Path(__file__).resolve().parent
owner = Path('/tmp/stock-market-game-validation-2026-10-27-1311109/company-query-engine-red-01-2026-10-28')
target = owner.parent / 'target'
guard = root / '.worktree/runtime-sync-e1/agents/perf-intraday-independent/implementation/e2/day-end-full-source-tmpfs-batch-r2/run-build-guard-tmpfs.mjs'
helper = root / '.worktree/main-merge-2026-10-25/scripts/run-with-deadline.mjs'
long_helper = helper.with_name('run-long-validation.mjs')
node = '/home/linuxbrew/.linuxbrew/Cellar/node/25.8.2/bin/node'
# 任何实际执行原件一旦存在，本目录不可再次启动；抢占失败不得进入 finally 写回。
allowed = {'run-ram-batch.py', 'plan-template.json', 'collect-child-status.py', 'review.md', 'source-freeze.json', 'launch-batch.py', 'slot-handoff.json', 'absolute-supervisor.mjs', '__pycache__'}
existing = [entry.name for entry in out.iterdir() if entry.name not in allowed]
if existing:
    raise RuntimeError('已有批次原件，拒绝重用目录：' + repr(existing))
with (out / 'execution-claimed.json').open('x') as stream:
    json.dump({'pid': os.getpid(), 'argv': sys.argv}, stream)
started = time.monotonic()
if len(sys.argv) != 3:
    raise RuntimeError('必须由共享截止 launcher 启动')
deadline = int(sys.argv[1]) / 1000
outer_deadline = int(sys.argv[2]) / 1000
if outer_deadline - deadline != 1 or not 0 < outer_deadline - time.time() <= 300:
    raise RuntimeError('共享299/300秒绝对截止非法')
receipt = {'pythonPid': os.getpid(), 'actualPythonArgv': sys.argv, 'actualParentArgv': Path(f'/proc/{os.getppid()}/cmdline').read_bytes().decode().split('\0')[:-1], 'target': str(target), 'wholeTargetCopy': False, 'persistence': 'RAM target 与封存单件均非持久；root统一保留复用', 'stages': []}

def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def resources():
    memory = dict(line.split(':', 1) for line in Path('/proc/meminfo').read_text().splitlines())
    def available(path):
        row = os.statvfs(path)
        return row.f_bavail * row.f_frsize
    return {'outputAvailableBytes': available(target), 'sourceAvailableBytes': available(work), 'memAvailable': memory['MemAvailable'].strip()}

def remain_ms(maximum):
    value = min(maximum, int((deadline - time.time()) * 1000))
    if value <= 0:
        raise RuntimeError('共享299秒工作窗耗尽，不能重赠截止')
    return value

def fingerprint(plan):
    def one(row):
        actual = sha(work / row['path'])
        if actual != row['sha256']:
            raise RuntimeError('源码漂移：' + row['path'])
        return {'path': row['path'], 'sha256': actual}
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        return list(pool.map(one, plan['productSources']))

def run_logged(label, argv, long=False, environment=None):
    supervisor = long_helper if long else helper
    command = [node, str(supervisor), str(remain_ms(299000 if long else 10000)), '--', *map(str, argv)]
    begin = time.monotonic()
    with (out / (label + '.stdout')).open('xb') as stdout, (out / (label + '.stderr')).open('xb') as stderr:
        child = subprocess.Popen(command, cwd=work, env=environment, stdout=stdout, stderr=stderr)
        print(json.dumps({'stage': label, 'pid': child.pid, 'parentPid': os.getpid(), 'argv': command}), flush=True)
        result = child.wait()
    row = {'stage': label, 'actualArgv': command, 'pid': child.pid, 'exit': result, 'seconds': time.monotonic() - begin}
    receipt['stages'].append(row)
    return row

owner_created = False
try:
    plan = json.loads((out / 'plan-template.json').read_text())
    receipt['before'] = resources()
    if receipt['before']['outputAvailableBytes'] < 20 * 1024 ** 3:
        raise RuntimeError('RAM 输出余量不足20GiB；source低余只记录，不能冒称满足')
    before = fingerprint(plan)
    with (out / 'source-before.json').open('x') as stream:
        stream.write(json.dumps(before, indent=2) + '\n')
    for row in plan['runnerSources']:
        if sha(Path(row['path'])) != row['sha256']:
            raise RuntimeError('runner 漂移：' + row['path'])
    owner.mkdir(exist_ok=False)
    owner_created = True
    (owner / 'scratch').mkdir()
    prior = plan['protectedPrior']
    if prior.get('pendingSlotHandoff'):
        raise RuntimeError('尚未收到前槽实际 default binary 绑定，禁止构建覆盖')
    old = Path(prior['path'])
    if sha(old) != prior['sha256'] or old.stat().st_size != prior['bytes']:
        raise RuntimeError('前槽实际 default unit 身份漂移')
    if 'sealedExisting' in prior:
        sealed = Path(prior['sealedExisting'])
        if sha(sealed) != prior['sha256'] or sealed.stat().st_size != prior['bytes'] or old.stat().st_ino == sealed.stat().st_ino:
            raise RuntimeError('已保护前槽 default 单件身份不符；禁止重复复制冒充保护')
        receipt['protectedPrior'] = {**prior, 'sealed': str(sealed), 'distinctInode': True, 'reusedExistingSeal': True, 'persistence': '仅RAM非持久，root统一保留'}
    else:
        sealed = owner / 'sealed-prior-default-unit'
        copied = run_logged('seal-single-prior-default-unit', ['cp', '--reflink=never', '-p', old, sealed], long=True)
        if copied['exit'] or sha(sealed) != prior['sha256'] or old.stat().st_ino == sealed.stat().st_ino:
            raise RuntimeError('前槽 default 单件封存失败；不得覆盖')
        receipt['protectedPrior'] = {**prior, 'sealed': str(sealed), 'distinctInode': True, 'reusedExistingSeal': False, 'persistence': '仅RAM非持久，root统一保留'}
    plan['sharedDeadlineUnixMs'] = int(deadline * 1000)
    plan_path = out / 'default-plan.json'
    with plan_path.open('x') as stream:
        stream.write(json.dumps(plan, indent=2) + '\n')
    receipt['actualPlanSha256'] = sha(plan_path)
    build = run_logged('default-build', [node, guard, plan_path], long=True)
    if build['exit']:
        raise RuntimeError('真实 default 构建失败，不用旧 binary 跑新源码')
    artifacts = []
    for line in (out / 'default-guard.stdout').read_text().splitlines():
        row = json.loads(line)
        if row.get('reason') == 'compiler-artifact' and row.get('executable'):
            artifact = Path(row['executable'])
            remain_ms(299000)
            if not artifact.is_relative_to(target):
                raise RuntimeError('实际产物不在统一 RAM target')
            artifacts.append({'path': str(artifact), 'sha256': sha(artifact), 'fresh': row['fresh'], 'target': row['target']['name'], 'features': row['features'], 'persistence': 'RAM非持久'})
    if len(artifacts) != 1 or artifacts[0]['target'] != 'engine':
        raise RuntimeError('default lib必须恰一实际unit')
    actual = Path(artifacts[0]['path'])
    sealed_red = owner / 'sealed-company-query-red-unit'
    copied = run_logged('seal-single-origin-core-red-unit', ['cp', '--reflink=never', '-p', actual, sealed_red], long=True)
    if copied['exit'] or sha(sealed_red) != artifacts[0]['sha256'] or actual.stat().st_ino == sealed_red.stat().st_ino:
        raise RuntimeError('公司只读 red actual unit 单件保存失败；禁止用未来覆盖后的产物追认红')
    receipt['sealedRed'] = {'actualArtifact': str(actual), 'path': str(sealed_red), 'sha256': artifacts[0]['sha256'], 'bytes': sealed_red.stat().st_size, 'distinctInode': True, 'persistence': '仅RAM非持久，交root保留至红绿复核完成'}
    receipt['artifacts'] = artifacts
    with (out / 'artifact-binding.json').open('x') as stream:
        stream.write(json.dumps({'sourceCount': len(before), 'artifacts': artifacts}, indent=2) + '\n')
    listed = run_logged('actual-test-list', [artifacts[0]['path'], '--list', '--format=terse'])
    if listed['exit']:
        raise RuntimeError('实际 unit test list 失败')
    available = set(line.removesuffix(': test') for line in (out / 'actual-test-list.stdout').read_text().splitlines() if line.endswith(': test'))
    names = ['session::protocol::company_reads::tests::company_views_preserve_nonempty_existing_domain_results_and_save_bytes', 'session::protocol::company_reads::tests::global_outstanding_is_filtered_to_the_authenticated_owner', 'session::protocol::company_reads::tests::unhealthy_sessions_reject_before_reading_company_facts', 'session::protocol::company_reads::tests::actual_owner_query_returns_the_existing_domain_view_without_state_changes', 'session::protocol::company_reads::tests::unknown_owner_is_rejected_before_any_domain_query', 'session::protocol::company_reads::tests::company_identity_uses_the_actual_catalog_namespace', 'session::protocol::company_reads::tests::requests_have_no_client_selected_owner_and_reject_unknown_fields']
    if any(name not in available for name in names):
        raise RuntimeError('真实 unit 未注册全部七个公司只读 exact case')
    def run_case(full_name):
        label = full_name.split('::')[-1]
        result = run_logged(label, [sys.executable, '/data1/baiyifan/workplace/stock_market_game/agents/branch-completion/funds-production-resume-2026-10-27/p1-formal-red-2026-10-28/collect-child-status.py', out / (label + '.child-status.json'), artifacts[0]['path'], full_name, '--exact', '--test-threads=1', '--nocapture'])
        child_status = json.loads((out / (label + '.child-status.json')).read_text()) if result['exit'] == 0 else None
        stdout = (out / (label + '.stdout')).read_text()
        stderr = (out / (label + '.stderr')).read_text()
        summaries = re.findall(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed;', stdout)
        passed = result['exit'] == 0 and child_status['exit'] == 0 and summaries == [('ok', '1', '0')]
        failed = result['exit'] == 0 and child_status['exit'] == 101 and summaries == [('FAILED', '0', '1')] and ('panicked at ' in stderr)
        expected = 'green' if label == 'requests_have_no_client_selected_owner_and_reject_unknown_fields' else 'red'
        panic_lines = {'company_views_preserve_nonempty_existing_domain_results_and_save_bytes': 66, 'global_outstanding_is_filtered_to_the_authenticated_owner': 100, 'unhealthy_sessions_reject_before_reading_company_facts': 115, 'actual_owner_query_returns_the_existing_domain_view_without_state_changes': 132, 'unknown_owner_is_rejected_before_any_domain_query': 142, 'company_identity_uses_the_actual_catalog_namespace': 151}
        targeted = expected == 'green' or f'protocol/company_reads/tests.rs:{panic_lines[label]}:' in stderr
        if label in ['company_views_preserve_nonempty_existing_domain_results_and_save_bytes', 'global_outstanding_is_filtered_to_the_authenticated_owner']:
            targeted = targeted and '公司只读注册尚未实现' in stderr
        return {**result, 'expected': expected, 'targetedPanic': targeted, 'name': full_name, 'childStatus': child_status, 'binarySha256': artifacts[0]['sha256'], 'summary': summaries, 'exactPassed': passed, 'exactAssertionFailed': failed, 'stderrSha256': sha(out / (label + '.stderr'))}
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        receipt['tests'] = list(pool.map(run_case, names))
    remain_ms(299000)
    after = fingerprint(plan)
    remain_ms(299000)
    with (out / 'source-after.json').open('x') as stream:
        stream.write(json.dumps(after, indent=2) + '\n')
    receipt['sourceCount'] = len(before)
    receipt['sourceUnchanged'] = before == after
    receipt['passed'] = sum(row['exactPassed'] for row in receipt['tests'])
    receipt['assertionFailed'] = sum(row['exactAssertionFailed'] for row in receipt['tests'])
    if receipt['passed'] != 1 or receipt['assertionFailed'] != 6 or any(not row['targetedPanic'] or not (row['exactPassed'] if row['expected'] == 'green' else row['exactAssertionFailed']) for row in receipt['tests']):
        raise RuntimeError('未取得指定六个query断言红与严格请求一个绿；拒绝fixture提前panic')
    remain_ms(299000)
    receipt['status'] = '公司只读engine首红实际完成，逐例分类，不表示三宿主已注册或生产结算已支持'
except BaseException as error:
    receipt['status'] = '失败'
    receipt['error'] = repr(error)
    print(str(error), file=sys.stderr)
    sys.exit(1)
finally:
    receipt['after'] = resources()
    receipt['wallSeconds'] = time.monotonic() - started
    scratch = owner / 'scratch'
    cleanup_start = time.monotonic()
    if owner_created and scratch.exists():
        import shutil
        shutil.rmtree(scratch)
    receipt['scratchCleanup'] = {'path': str(scratch), 'ownedByThisBatch': owner_created, 'removed': owner_created and not scratch.exists(), 'seconds': time.monotonic() - cleanup_start}
    receipt['scratchDisposition'] = '本批scratch已实际清理；保护单件binary仅RAM非持久由root保留'
    if time.time() >= deadline and receipt.get('status') != '失败':
        receipt['status'] = '失败'
        receipt['error'] = '工作/校验/发布未在299秒内完成，禁止称成功'
        receipt['workDeadlineExceeded'] = True
    with (out / 'batch-receipt.json').open('x') as stream:
        stream.write(json.dumps(receipt, ensure_ascii=False, indent=2) + '\n')
    if time.time() >= deadline:
        # 成功回执若在发布时越界，额外排他失败标志使本批无效，不回写已发布原件。
        with (out / 'work-deadline-failed.json').open('x') as stream:
            json.dump({'status': '失败', 'reason': 'receipt发布未在299秒工作窗内完成'}, stream)
        receipt['workDeadlineExceeded'] = True
    print(json.dumps({'status': receipt.get('status'), 'wallSeconds': receipt['wallSeconds'], 'target': str(target)}, ensure_ascii=False), flush=True)
    if receipt.get('workDeadlineExceeded'):
        sys.exit(1)
