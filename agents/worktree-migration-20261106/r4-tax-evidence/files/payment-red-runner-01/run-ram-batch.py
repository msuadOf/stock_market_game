# R4 付款三门目标红：单一 engine feature 图，三 exact 并发采集。
from pathlib import Path
import ast, concurrent.futures, hashlib, json, os, re, runpy, shutil, stat, subprocess, sys, threading, time

root = Path('/data1/<USER>/workplace/stock_market_game')
here = Path(__file__).resolve().parent
work = root / '.worktree/d3-p1-integration-2026-10-28'
out = here
owner = Path('/tmp/stock-market-game-validation-2026-10-27-1311109/engine-r4-payment-red-2026-10-29-01')
target = owner.parent / 'target'
guard = root / '.worktree/runtime-sync-e1/agents/perf-intraday-independent/implementation/e2/day-end-full-source-tmpfs-batch-r2/run-build-guard-tmpfs.mjs'
helper = root / '.worktree/main-merge-2026-10-25/scripts/run-with-deadline.mjs'
long_helper = helper.with_name('run-long-validation.mjs')
collector = root / 'agents/branch-completion/funds-production-resume-2026-10-27/p1-formal-red-2026-10-28/collect-child-status.py'
node = '/data1/<USER>/workplace/stock_market_game/.tmp/node24-env/node'
if len(sys.argv) != 4:
    raise RuntimeError('只能由launch-batch.py传入任务/发布/外部绝对截止')
deadline_ms, publication_ms, outer_ms = map(int, sys.argv[1:])
if publication_ms - deadline_ms != 3000 or outer_ms - publication_ms != 1000 or not 0 < outer_ms - time.time() * 1000 <= 300000:
    raise RuntimeError('共享300秒、前299秒发布或任务后2秒清理预留非法')
handoff = json.loads((here / 'slot-handoff.json').read_text())
if handoff.get('pendingSlotHandoff') is not False or handoff.get('rootAuthorized') is not True:
    raise RuntimeError('未取得前槽实际产物交接，禁止构建')
required_roles = []
if sorted(row['role'] for row in handoff['artifacts']) != sorted(required_roles):
    raise RuntimeError('本批无旧制品直接消费者，不接受继承制品')
if any(row.get('absent') is True for row in handoff['artifacts']):
    raise RuntimeError('feature前件不得缺席；不采用未知SHA或缺件fallback')
quota_helper = root / '.worktree/d3-p1-integration-2026-10-28/agents/branch-completion/wasm-w1/combined/fresh_binding.py'
quota = runpy.run_path(str(quota_helper))['tmp_user_quota']
if hashlib.sha256(Path(handoff['actualWaitReceipt']).read_bytes()).hexdigest() != handoff['actualWaitReceiptSha256']:
    raise RuntimeError('root直接前槽小SHA漂移')
# 历史制品不被本批复用；只保留前槽as-of摘要，不查角色、seal或归档继承链。
if (out / 'batch-receipt.json').exists():
    raise RuntimeError('本批原件已存在')
started = time.monotonic()
receipt = {'pythonPid': os.getpid(), 'actualPythonArgv': sys.argv,
           'actualParentArgv': Path(f'/proc/{os.getppid()}/cmdline').read_bytes().decode().split('\0')[:-1],
           'sharedTaskDeadlineUnixMs': deadline_ms, 'sharedPublicationDeadlineUnixMs': publication_ms, 'sharedOuterDeadlineUnixMs': outer_ms,
           'target': str(target), 'wholeTargetCopy': False, 'stages': [], 'tests': [], 'artifacts': [],
           'persistence': '单一RAMtarget与封存单件均非持久；root统一保留',
           'historicalInventoryScope': '其余历史只继承不可变前槽snapshot小SHA/map，as-of前槽；本批未逐stat/hash，未来实际使用时再核',
           'frontInventory': handoff['actualWaitReceipt'], 'frontInventorySha256': handoff['actualWaitReceiptSha256'],
           'artifactArchiveMaps': handoff['artifactArchiveMaps']}
owner_created = False
stop_sampling = threading.Event()
sample_errors = []
owned = {}
sampler = None

def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def remain_ms(maximum=299000):
    value = min(maximum, int(deadline_ms - time.time() * 1000))
    if value <= 0:
        raise RuntimeError('共享297秒任务窗耗尽；后2秒仅供owned终止、scratch清理与发布')
    return value

def write_json(path, value):
    with Path(path).open('x') as stream:
        stream.write(json.dumps(value, ensure_ascii=False, indent=2) + '\n')

def git(*args):
    return subprocess.check_output(['git', '-C', str(work), *args], text=True).strip()

def resources():
    def available(path):
        row = os.statvfs(path)
        return row.f_bavail * row.f_frsize
    return {'outputAvailableBytes': available(target), 'sourceAvailableBytes': available(work), 'tmpUserQuota': quota()}

def identity(pid):
    try:
        raw = Path(f'/proc/{pid}/stat').read_text()
    except (FileNotFoundError, ProcessLookupError):
        return None
    fields = raw[raw.rfind(')') + 2:].split()
    return {'pid': int(pid), 'ppid': int(fields[1]), 'state': fields[0], 'birth': fields[19],
            'cpuTicks': int(fields[11]) + int(fields[12]), 'threads': int(fields[17])}

def sample_tree():
    rows = {}
    for entry in Path('/proc').iterdir():
        if entry.name.isdecimal():
            row = identity(entry.name)
            if row is not None:
                rows[row['pid']] = row
    descendants = {os.getpid()}
    while True:
        before = len(descendants)
        descendants.update(row['pid'] for row in rows.values() if row['ppid'] in descendants)
        if before == len(descendants):
            break
    result = [row for pid, row in rows.items() if pid in descendants and pid != os.getpid()]
    for row in result:
        owned[(row['pid'], row['birth'])] = row
    return result

def sampling_loop():
    try:
        with (out / 'batch-resources.jsonl').open('x') as stream:
            while not stop_sampling.is_set():
                stream.write(json.dumps({'elapsedSeconds': time.monotonic() - started,
                                         'processes': sample_tree(), **resources()}) + '\n')
                stream.flush()
                stop_sampling.wait(0.5)
    except BaseException as error:
        sample_errors.append(repr(error))

def fingerprint(rows):
    def one(row):
        actual = sha(work / row['path'])
        if actual != row['sha256']:
            raise RuntimeError('源码漂移：' + row['path'])
        return {'path': row['path'], 'sha256': actual}
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        return list(pool.map(one, rows))

def run_logged(label, argv, long=False, cwd=None):
    supervisor = long_helper if long else helper
    command = [node, str(supervisor), str(remain_ms(299000 if long else 10000)), '--', *map(str, argv)]
    begin = time.monotonic()
    with (out / (label + '.stdout')).open('xb') as stdout, (out / (label + '.stderr')).open('xb') as stderr:
        child = subprocess.Popen(command, cwd=work if cwd is None else cwd, stdout=stdout, stderr=stderr)
        row = identity(child.pid)
        if row is not None:
            owned[(row['pid'], row['birth'])] = row
        print(json.dumps({'stage': label, 'pid': child.pid, 'argv': command}), flush=True)
        code = child.wait()
    result = {'stage': label, 'actualArgv': command, 'pid': child.pid, 'exit': code, 'seconds': time.monotonic() - begin}
    receipt['stages'].append(result)
    return result


try:
    template = json.loads((here / 'plan-template.json').read_text())
    cases = json.loads((here / 'cases-plan.json').read_text())
    contract = json.loads((here / 'expected-red-contracts.json').read_text())
    contracts = {case['name']: {**case, 'group': group['group'], 'requiredError': group.get('requiredError')}
                 for group in contract['groupedCases'] for case in group['cases']}
    if len(contracts) != 3:
        raise RuntimeError('付款三门目标契约不完整')
    if sorted(contracts) != sorted(case for batch in cases['batches'] for target in batch['targets'] for case in target['cases']):
        raise RuntimeError('三个实际选定 exact 必须逐项对应目标契约')
    if cases['buildOrder'] != ['feature'] or [row['name'] for row in cases['batches']] != ['feature']:
        raise RuntimeError('只允许一个feature构建图')
    if [[(target['name'], len(target['cases'])) for target in batch['targets']] for batch in cases['batches']] != [[('engine', 3)]]:
        raise RuntimeError('付款三门必须仅从单一engine lib图执行')
    if git('rev-parse', 'HEAD') != template['candidate'] or git('rev-parse', 'HEAD^{tree}') != template['candidateTree'] or git('status', '--porcelain=v1') != template['candidateStatus']:
        raise RuntimeError('候选HEAD/tree/clean冻结不符')
    receipt['candidate'] = template['candidate']
    receipt['candidateTree'] = template['candidateTree']
    receipt['before'] = resources()
    if receipt['before']['outputAvailableBytes'] < 20 * 1024 ** 3 or receipt['before']['tmpUserQuota']['availableBytes'] < 5 * 1024 ** 3:
        raise RuntimeError('RAM输出不足20GiB或真实本用户quota不足5GiB，禁止构建')
    for row in template['runnerSources']:
        if sha(row['path']) != row['sha256']:
            raise RuntimeError('runner冻结漂移：' + row['path'])
    before = fingerprint(template['productSources'])
    web_before = fingerprint(template['webProductSources'])
    write_json(out / 'web-source-before.json', web_before)
    write_json(out / 'source-before.json', before)
    owner.mkdir(exist_ok=False)
    owner_created = True
    me = identity(os.getpid())
    write_json(out / 'exclusive-owner-claim.json', {'wrapperPid': os.getppid(), 'pythonPid': os.getpid(), 'pythonBirthTicks': me['birth'], 'owner': str(owner), 'scratch': str(owner / 'scratch')})
    (owner / 'scratch').mkdir()
    sampler = threading.Thread(target=sampling_loop, name='engine-payment-red-resource-sampler', daemon=True)
    sampler.start()
    metadata = run_logged('metadata', ['env', 'CARGO_TARGET_DIR=' + str(target), 'CARGO_INCREMENTAL=0',
        'cargo', 'metadata', '--format-version', '1', '--no-deps', '--locked'])
    if metadata['exit']:
        raise RuntimeError('本树真实 Cargo metadata 失败')
    actual_metadata = json.loads((out / 'metadata.stdout').read_text())
    if Path(actual_metadata['workspace_root']).resolve() != work or Path(actual_metadata['target_directory']).resolve() != target:
        raise RuntimeError('本树真实 metadata workspace/target 错位')
    manifests = sorted(str(Path(row['manifest_path']).resolve().relative_to(work)) for row in actual_metadata['packages'])
    if any(name not in {row['path'] for row in before} for name in manifests):
        raise RuntimeError('完整source清单漏workspace manifest')
    receipt['metadata'] = {'sha256': sha(out / 'metadata.stdout'), 'manifests': manifests,
        'workspaceMembers': actual_metadata['workspace_members'], 'workspaceDefaultMembers': actual_metadata['workspace_default_members']}
    receipt['protectedPriors'] = []
    write_json(out / 'product-source-manifest.json', before)
    refreshed = run_logged('refresh-owned-source-times', [sys.executable, str(here / 'owned-native-source.py'), 'refresh', str(out / 'owned-source-times.json'), str(out / 'product-source-manifest.json')])
    if refreshed['exit']:
        raise RuntimeError('本人源码时间refresh失败')
    for batch in cases['batches']:
        remain_ms()
        name = batch['name']
        plan = {**template, 'name': name, 'command': batch['command'], 'cargoCommand': batch['command'],
                'sharedDeadlineUnixMs': deadline_ms,
                'cargoLogPaths': [str(out / (name + '-guard.stdout')), str(out / (name + '-guard.stderr'))],
                'buildReceiptPath': str(out / (name + '-unused-build-receipt.json'))}
        plan_path = out / (name + '-plan.json')
        write_json(plan_path, plan)
        result = run_logged(name + '-build', [node, str(here / 'build-verified.mjs'), plan_path, 'native'], long=True)
        if result['exit']:
            raise RuntimeError(name + '真实构建失败，不用旧binary代跑')
        artifacts = []
        expected_features = ['simulation-diagnostics', 'verification-harness']
        messages = [json.loads(line) for line in (out / (name + '-guard.stdout')).read_text().splitlines()]
        if [m for m in messages if m.get('reason') == 'build-finished'] != [{'reason': 'build-finished', 'success': True}]:
            raise RuntimeError(name + ' Cargo未唯一成功完成')
        for message in messages:
            if message.get('reason') != 'compiler-artifact' or message['target']['name'] != 'engine':
                continue
            target_name = message['target']['name']
            expected_src = work / ('packages/engine/src/lib.rs' if target_name == 'engine' else 'packages/engine/tests/' + target_name + '.rs')
            if sorted(message['features']) != expected_features:
                raise RuntimeError('实际engine features不等冻结配置')
            if Path(message['target']['src_path']) != expected_src or message['fresh'] is not False or message['profile']['opt_level'] != '0' or message['profile']['debuginfo'] != 2:
                raise RuntimeError('必须本候选src/fresh:false/debug2实际制品')
            if message.get('executable') and message['profile']['test'] is True:
                actual = Path(message['executable'])
                if message['target']['kind'] != ['rlib'] or not actual.is_relative_to(target):
                    raise RuntimeError('实际unit targetKind或唯一target路径不等冻结')
                artifacts.append({'configuration': name, 'path': str(actual), 'sha256': sha(actual), 'bytes': actual.stat().st_size,
                    'fresh': message['fresh'], 'target': target_name, 'targetKind': message['target']['kind'], 'testProfile': True,
                    'features': message['features'], 'actualPlanSha256': sha(plan_path), 'cargoArtifact': message})
            elif target_name == 'engine' and message['profile']['test'] is False and message['target']['kind'] == ['rlib']:
                receipt.setdefault('normalArtifacts', []).append(message)
            else:
                raise RuntimeError('出现未声明的engine artifact类别')
        if sorted(a['target'] for a in artifacts) != ['engine']:
            raise RuntimeError('必须精确匹配唯一feature unit制品')
        for message in receipt.get('normalArtifacts', []):
            for filename in message['filenames']:
                actual = Path(filename)
                if actual.suffix not in ['.rlib', '.rmeta'] or not actual.is_relative_to(target):
                    raise RuntimeError('正常engine库输出路径错位')
                receipt.setdefault('normalArtifactFiles', []).append({'path': filename, 'sha256': sha(actual), 'bytes': actual.stat().st_size, 'dev': actual.stat().st_dev, 'inode': actual.stat().st_ino, 'mtimeNs': actual.stat().st_mtime_ns, 'cargoArtifact': message})
        write_json(out / (name + '-artifact-candidates-before-acceptance.json'), artifacts)
        cases_to_run = []
        for entry in batch['targets']:
            artifact = next(artifact for artifact in artifacts if artifact['target'] == entry['name'])
            current = Path(artifact['path']).stat()
            artifact['executionBinding'] = {'path': artifact['path'], 'sha256': artifact['sha256'], 'dev': current.st_dev, 'inode': current.st_ino, 'bytes': current.st_size, 'mtimeNs': current.st_mtime_ns, 'scope': '仅本批直接执行，跨批无复用需求，不新增大seal'}
            receipt['artifacts'].append(artifact)
            list_label = name + '-' + artifact['target'] + '-actual-test-list'
            listed = run_logged(list_label, [artifact['path'], '--list', '--format=terse'])
            available = [line.removesuffix(': test') for line in (out / (list_label + '.stdout')).read_text().splitlines() if line.endswith(': test')]
            if listed['exit'] or len(entry['cases']) != len(set(entry['cases'])) or any(available.count(case) != 1 for case in entry['cases']):
                raise RuntimeError(name + '/' + artifact['target'] + '实际target未注册选定完整exact')
            cases_to_run.extend((full_name, artifact) for full_name in entry['cases'])
        def run_case(item):
            full_name, artifact = item
            label = name + '-' + artifact['target'] + '-' + full_name.split('::')[-1]
            status_path = out / (label + '.child-status.json')
            case_flags = ['--exact', '--test-threads=1', '--nocapture']
            child_argv = [sys.executable, collector, status_path, artifact['path'], full_name, *case_flags]
            if contracts[full_name].get('environment'):
                child_argv = ['env', *[key + '=' + value for key, value in contracts[full_name]['environment'].items()], *child_argv]
            result = run_logged(label, child_argv)
            child_status = None
            status_error = None
            if status_path.exists():
                try:
                    child_status = json.loads(status_path.read_text())
                except (OSError, json.JSONDecodeError) as error:
                    status_error = repr(error)
            else:
                status_error = 'collector未发布child-status原件'
            stdout = (out / (label + '.stdout')).read_text()
            summaries = re.findall(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;', stdout)
            expected_argv = [artifact['path'], full_name, *case_flags]
            status_valid = isinstance(child_status, dict) and child_status.get('argv') == expected_argv
            passed = result['exit'] == 0 and status_valid and child_status.get('exit') == 0 and summaries == [('ok', '1', '0', '0')]
            raw_business_red = result['exit'] == 0 and status_valid and child_status.get('exit') == 101 and summaries == [('FAILED', '0', '1', '0')]
            expected = contracts[full_name]
            stderr = (out / (label + '.stderr')).read_text()
            panic_text = stdout if expected.get('panicChannel') == 'stdout' else stderr
            panic_headers = re.findall(r"panicked at ([^:\n]+):(\d+):(\d+):", panic_text)
            target_panic = len(panic_headers) == 1 and panic_headers[0][0] == expected['sourcePath'] and expected['requiredEarliestPanic'] in panic_text and all(marker in panic_text for marker in expected.get('requiredAdditionalMarkers', []))
            if expected.get('requiresEmptyStderr'):
                target_panic = target_panic and not stderr
            business_red = raw_business_red and target_panic
            expected_matched = passed if expected.get('expected') == 'green' else business_red
            row = {**result, 'name': full_name, 'configuration': name, 'target': artifact['target'], 'childStatus': child_status,
                   'childStatusError': status_error, 'summary': summaries, 'businessCaseRed': business_red,
                   'rawBusinessCaseRed': raw_business_red, 'targetPanicMatched': target_panic, 'group': expected['group'], 'expectedTarget': expected,
                   'binarySha256': artifact['sha256'], 'exactPassed': passed, 'expectedMatched': expected_matched,
                   'stdoutSha256': sha(out / (label + '.stdout')), 'stderrSha256': sha(out / (label + '.stderr'))}
            # 每项结果先落盘；超时、helper/collector与异常harness不是可聚合的业务红。
            write_json(out / (label + '.case-result.json'), row)
            if status_error is not None or not (passed or raw_business_red):
                raise RuntimeError('case执行证据或基础设施失败，停止后续配置：' + json.dumps(row, ensure_ascii=False))
            return row
        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            receipt['tests'].extend(pool.map(run_case, cases_to_run))
        if any(not row['businessCaseRed'] or not row['expectedMatched'] for row in receipt['tests'] if row['configuration'] == name):
            raise RuntimeError('付款三门必须全部由指定源码marker与唯一目标panic构成businessCaseRed')
    after = fingerprint(template['productSources'])
    web_after = fingerprint(template['webProductSources'])
    write_json(out / 'web-source-after.json', web_after)
    receipt['webSourceUnchanged'] = web_before == web_after
    if not receipt['webSourceUnchanged']:
        raise RuntimeError('本次Web源码或strict配置漂移')
    write_json(out / 'source-after.json', after)
    receipt['sourceCount'] = len(before)
    receipt['sourceUnchanged'] = before == after
    if git('rev-parse', 'HEAD') != template['candidate'] or git('rev-parse', 'HEAD^{tree}') != template['candidateTree'] or git('status', '--porcelain=v1') != template['candidateStatus']:
        raise RuntimeError('候选源码HEAD/status在执行中漂移')
    for row in template['runnerSources']:
        if sha(row['path']) != row['sha256']:
            raise RuntimeError('执行后runner漂移：' + row['path'])
    for artifact in receipt['artifacts']:
        if sha(artifact['path']) != artifact['sha256']:
            raise RuntimeError('本批实际产物被意外覆盖')
    if len(receipt['tests']) != 3 or not all(row['businessCaseRed'] and row['expectedMatched'] for row in receipt['tests']):
        raise RuntimeError('付款三门须全部取得指定目标业务红；fixture、编译或超时错误不算目标红')
    remain_ms()
    receipt['status'] = '付款三门指定目标业务红，待root核验；不是生产实现或整树验收'

except BaseException as error:
    receipt['status'] = '失败'
    receipt['error'] = repr(error)
    print(str(error), file=sys.stderr)
finally:
    stop_sampling.set()
    if sampler is not None:
        sampler.join(timeout=max(0, min(0.6, (publication_ms - time.time() * 1000 - 500) / 1000)))
        if sampler.is_alive():
            sample_errors.append('资源采样线程未在共享工作截止前退出')
    receipt['samplingErrors'] = sample_errors
    if sample_errors:
        receipt['status'] = '失败'
    scratch = owner / 'scratch'
    cleanup_started = time.monotonic()
    # helper若自身清理失败，不能先删除仍可能被活owned使用的TMPDIR。
    termination_ok = False
    try:
        write_json(out / 'observed-owned-before-cleanup.json', list(owned.values()))
        cleanup_budget = int(publication_ms - time.time() * 1000 - 250)
        if cleanup_budget <= 0 or (sampler is not None and sampler.is_alive()):
            raise RuntimeError('owned终止与最终发布预算不足；保留scratch供root显式收口')
        cleanup_result = out / 'owned-termination.json'
        command = [node, str(helper), str(min(10000, cleanup_budget)), '--', node, str(here / 'terminate-owned.mjs'),
                   str(out / 'observed-owned-before-cleanup.json'), str(cleanup_result), str(publication_ms - 300)]
        with (out / 'owned-termination.stdout').open('xb') as stdout, (out / 'owned-termination.stderr').open('xb') as stderr:
            cleanup = subprocess.run(command, cwd=work, stdout=stdout, stderr=stderr)
        receipt['ownedTerminationCommand'] = command
        receipt['ownedTerminationExit'] = cleanup.returncode
        termination_ok = cleanup.returncode == 0 and json.loads(cleanup_result.read_text())['alive'] == []
        if not termination_ok:
            raise RuntimeError('owned终止未确认，保留scratch；本批失败且交root显式收口')
        # 原subreaper确认ECHILD后由外层唯一清理scratch与恢复源码时间。
    except BaseException as cleanup_error:
        receipt['status'] = '失败'
        receipt['cleanupError'] = repr(cleanup_error)
        print('最终清理失败：' + str(cleanup_error), file=sys.stderr)
    receipt['scratchCleanup'] = {'path': str(scratch), 'ownedByThisBatch': owner_created, 'terminationConfirmed': termination_ok,
                                 'removed': False, 'seconds': time.monotonic() - cleanup_started}
    alive = []
    for (pid, birth), previous in sorted(owned.items()):
        current = identity(pid)
        if current is not None and current['birth'] == birth and current['state'] not in ('Z', 'X'):
            alive.append(current)
    receipt['observedOwnedProcesses'] = list(owned.values())
    receipt['ownedAlive'] = alive
    if alive:
        receipt['status'] = '失败'
        receipt['ownedExitFailure'] = '已观察owned子进程仍存活，不能宣称清理完成'
    receipt['after'] = resources()
    receipt['wallSecondsIncludingCleanup'] = time.monotonic() - started
    if time.time() * 1000 >= publication_ms:
        receipt['status'] = '失败'
        receipt['workDeadlineExceeded'] = True
    write_json(out / 'batch-receipt.json', receipt)
    if time.time() * 1000 >= publication_ms:
        write_json(out / 'work-deadline-failed.json', {'status': '失败', 'reason': '最终发布超过299秒工作截止'})
        receipt['status'] = '失败'
    print(json.dumps({'status': receipt['status'], 'wallSecondsIncludingCleanup': receipt['wallSecondsIncludingCleanup']}, ensure_ascii=False), flush=True)
    if receipt['status'] == '失败':
        sys.exit(1)
