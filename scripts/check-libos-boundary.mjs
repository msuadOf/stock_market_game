import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// 本门禁是无第三方依赖的 Rust 词法检查，不是 rustc AST；不能识别 proc macro 展开、间接包装或任意用户自定义等待 API。
// 新增平台原语时须同步扩展规则；它以可定位的已知泄漏阻断迁移，不能替代完整架构审查。
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const roots = ['packages/engine/src/runtime', 'packages/engine/src/session'];

// 专用测试源仅在父模块以 cfg(test) 声明时豁免；验证/诊断文件逐项列明用途。
const testFiles = [
  ['packages/engine/src/runtime/calendar_drain_tests.rs', 'runtime calendar drain 行为测试'],
  ['packages/engine/src/runtime/calendar_misroute_tests.rs', 'runtime calendar misroute 行为测试'],
  ['packages/engine/src/runtime/day_end_public_views_tests.rs', 'runtime 日终 public view 行为测试'],
  ['packages/engine/src/runtime/desk/borrowed_receiver_tests.rs', 'desk borrowed receiver 行为测试'],
  ['packages/engine/src/runtime/desk/repurchase_group_fixture.rs', '仅供 cfg(test) 回购测试使用的 fixture'],
  ['packages/engine/src/runtime/desk_repurchase_transport_tests.rs', 'desk repurchase transport 行为测试'],
  ['packages/engine/src/runtime/drive/typed_info_delivery_tests.rs', 'drive typed info delivery 行为测试'],
  ['packages/engine/src/runtime/group/admin_transaction/repurchase_source_tests.rs', 'group repurchase source 行为测试'],
  ['packages/engine/src/runtime/publication_tests.rs', 'runtime publication 行为测试'],
  ['packages/engine/src/runtime/seal_tests.rs', 'runtime seal 行为测试'],
  ['packages/engine/src/runtime/skeleton_tests.rs', 'runtime skeleton 行为测试'],
  ['packages/engine/src/runtime/world/admin_transaction/calendar_information_intersection_tests.rs', 'world calendar information intersection 行为测试'],
  ['packages/engine/src/runtime/world/admin_transaction/funds_cost_tests.rs', 'world funds cost 行为测试'],
  ['packages/engine/src/runtime/world/admin_transaction/information_tests.rs', 'world information 行为测试'],
  ['packages/engine/src/runtime/world/committed_calendar_tests.rs', 'world committed calendar 行为测试'],
  ['packages/engine/src/runtime/world/owned_worker_exit_tests.rs', 'world owned worker exit 行为测试'],
  ['packages/engine/src/runtime/world/shutdown_verification.rs', 'verification-harness 下的 shutdown 诊断辅助代码'],
  ['packages/engine/src/session/company_source_runtime_tests.rs', 'session company source runtime 行为测试'],
  ['packages/engine/src/session/decision_chain/company_source_tests.rs', 'session decision chain company source 行为测试'],
  ['packages/engine/src/session/pipeline/executor_perturbation_tests.rs', 'session executor perturbation 行为测试'],
  ['packages/engine/src/session/pipeline/ledger_conservation_tests.rs', 'session ledger conservation 行为测试'],
  ['packages/engine/src/session/pipeline/npc_decisions_quota_tests.rs', 'session NPC decisions quota 行为测试'],
  ['packages/engine/src/session/pipeline/npc_decisions_tests.rs', 'session NPC decisions 行为测试'],
  ['packages/engine/src/session/pipeline/plan_quota_dead_zone_tests.rs', 'session plan quota dead zone 行为测试'],
  ['packages/engine/src/session/pipeline/ready_ingress_tests.rs', 'session ready ingress 行为测试'],
  ['packages/engine/src/session/pipeline/repair_lifecycle_receipt_tests.rs', 'session repair lifecycle receipt 行为测试'],
  ['packages/engine/src/session/plan_chain_candidates/adaptive_tests.rs', 'session plan chain adaptive 行为测试'],
  ['packages/engine/src/session/plan_root_quota_tests.rs', 'session plan root quota 行为测试'],
  ['packages/engine/src/session/plan_root_quota_wave_tests.rs', 'session plan root quota wave 行为测试'],
  ['packages/engine/src/session/protocol/civil/core_first_tests.rs', 'session civil core first 行为测试'],
  ['packages/engine/src/session/protocol/civil/day_end_npc_tests.rs', 'session civil day end NPC 行为测试'],
  ['packages/engine/src/session/protocol/civil/single_owner_tests.rs', 'session civil single owner 行为测试'],
].map(([file, reason]) => ({ file, reason }));

// 必要兼容导出仅可逐条精确授权，不可豁免所在文件。
const exactExceptions = [
  // 格式：{ file, line, category, reason }。当前生产源没有已审核的必要例外。
];

const rules = [
  ['平台线程', /\b(?:spawn|scope|Builder|sleep|park|park_timeout|yield_now)\b/],
  ['平台通道', /\b(?:channel|sync_channel|Sender|Receiver|SyncSender|SyncReceiver|TryRecvError|RecvTimeoutError)\b/],
  ['平台等待或轮询', /\b(?:recv_timeout|try_recv|spin_loop|yield_now|park_timeout|recv_deadline|worker_recv_spin|worker_recv)\b/],
  ['旧等待模块或实验策略选择', /\b(?:wasm_wait|libos-worker-experiment)\b/],
];

async function collect(dir) {
  const entries = await readdir(path.join(root, dir), { withFileTypes: true });
  const nested = await Promise.all(entries.map((entry) => {
    const file = path.posix.join(dir, entry.name);
    return entry.isDirectory() ? collect(file) : entry.isFile() && entry.name.endsWith('.rs') ? [file] : [];
  }));
  return nested.flat();
}

// 词法遮罩保留字符偏移；字符串和注释里的示例文本不会被当作 Rust 代码。
function maskCommentsAndStrings(source, maskStrings = true) {
  const chars = source.split('');
  let i = 0;
  while (i < chars.length) {
    if (chars[i] === '/' && chars[i + 1] === '/') {
      while (i < chars.length && chars[i] !== '\n') chars[i++] = ' ';
      continue;
    }
    if (chars[i] === '/' && chars[i + 1] === '*') {
      chars[i++] = ' ';
      chars[i++] = ' ';
      let depth = 1;
      while (i < chars.length && depth > 0) {
        if (chars[i] === '/' && chars[i + 1] === '*') { chars[i++] = ' '; chars[i++] = ' '; depth += 1; }
        else if (chars[i] === '*' && chars[i + 1] === '/') { chars[i++] = ' '; chars[i++] = ' '; depth -= 1; }
        else { if (chars[i] !== '\n') chars[i] = ' '; i += 1; }
      }
      continue;
    }
    // Rust raw string 前缀的内容也屏蔽；边界按成对井号处理。
    const raw = source.slice(i).match(/^(?:b)?r(#+)?"/);
    if (raw) {
      const hashes = raw[1]?.length ?? 0;
      const endToken = `"${'#'.repeat(hashes)}`;
      const end = source.indexOf(endToken, i + raw[0].length);
      const stop = end < 0 ? chars.length : end + endToken.length;
      while (i < stop) { if (maskStrings && chars[i] !== '\n') chars[i] = ' '; i += 1; }
      continue;
    }
    if (chars[i] === '"' || chars[i] === "'") {
      // 生命周期 apostrophe（'a）不是字符字面量。
      if (chars[i] === "'" && /^'\w/.test(source.slice(i, i + 3)) && source[i + 2] !== "'") { i += 1; continue; }
      const quote = chars[i++];
      while (i < chars.length) {
        if (chars[i] === '\\') { if (maskStrings) chars[i] = ' '; i += 1; if (i < chars.length && chars[i] !== '\n' && maskStrings) chars[i] = ' '; i += 1; }
        else if (chars[i] === quote) { if (maskStrings) chars[i] = ' '; i += 1; break; }
        else { if (chars[i] !== '\n' && maskStrings) chars[i] = ' '; i += 1; }
      }
      continue;
    }
    i += 1;
  }
  return chars.join('');
}

// 仅从代码位置提取属性；字符串字面量里的伪 cfg 文本不会被识别为属性。
export function rustAttributes(source) {
  const structure = maskCommentsAndStrings(source);
  const attributes = [];
  const startPattern = /#\s*!?\s*\[/g;
  let match;
  while ((match = startPattern.exec(structure))) {
    const open = structure.indexOf('[', match.index);
    let depth = 1;
    let end = open + 1;
    for (; end < structure.length && depth > 0; end += 1) {
      if (structure[end] === '[') depth += 1;
      else if (structure[end] === ']') depth -= 1;
    }
    if (depth !== 0) continue;
    const close = end - 1;
    attributes.push({ start: match.index, innerStart: open + 1, end, inner: source.slice(open + 1, close) });
    startPattern.lastIndex = end;
  }
  return attributes;
}

export function platformCfgOccurrences(source) {
  const occurrences = [];
  const forbidden = /(?:target_family\s*=\s*"wasm"|target_arch\s*=\s*"wasm32"|feature\s*=\s*"libos-worker-experiment")/g;
  for (const attribute of rustAttributes(source)) {
    const content = maskCommentsAndStrings(attribute.inner, false);
    if (!/^\s*(?:cfg|cfg_attr)\s*\(/.test(content)) continue;
    let match;
    forbidden.lastIndex = 0;
    while ((match = forbidden.exec(content))) {
      occurrences.push({ category: '平台条件编译', offset: attribute.innerStart + match.index });
    }
  }
  return occurrences;
}

function location(source, offset) {
  const before = source.slice(0, offset);
  const line = before.split('\n').length;
  return { line, column: offset - before.lastIndexOf('\n') };
}

function splitCfgArguments(expression) {
  const args = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index < expression.length; index += 1) {
    if (expression[index] === '(') depth += 1;
    else if (expression[index] === ')') depth -= 1;
    else if (expression[index] === ',' && depth === 0) {
      args.push(expression.slice(start, index).trim());
      start = index + 1;
    }
  }
  const tail = expression.slice(start).trim();
  if (tail) args.push(tail);
  return args;
}

function hasUnsafeNegativeOrAlternative(expression, marker) {
  const compact = expression.replace(/\s+/g, '');
  const call = /^([a-z_]+)\((.*)\)$/.exec(compact);
  if (!call) return false;
  const args = splitCfgArguments(call[2]);
  if ((call[1] === 'not' || call[1] === 'any') && compact.includes(marker)) return true;
  return args.some((argument) => hasUnsafeNegativeOrAlternative(argument, marker));
}

export function hasPositiveTestGate(expression) {
  const compact = expression.replace(/\s+/g, '');
  if (compact === 'test') return true;
  if (!compact.startsWith('all(') || !compact.endsWith(')')) return false;
  const args = splitCfgArguments(compact.slice(4, -1));
  if (!args.includes('test')) return false;
  // `all(test, ...)` 确定包含 test；排除会反向否定 test 或另开 any 分支的表达式。
  return !hasUnsafeNegativeOrAlternative(compact, 'test');
}

function hasPositiveFeatureGate(expression, feature) {
  const compact = expression.replace(/\s+/g, '');
  if (!compact.startsWith('all(') || !compact.endsWith(')')) return false;
  const required = `feature="${feature}"`;
  const args = splitCfgArguments(compact.slice(4, -1));
  if (!args.includes(required)) return false;
  return !hasUnsafeNegativeOrAlternative(compact, required);
}

function isExactException(file, category, lineText) {
  return exactExceptions.some((entry) => entry.file === file && entry.category === category
    && entry.line === lineText && typeof entry.reason === 'string' && entry.reason.trim().length > 0);
}

function cfgTestSpans(code) {
  const spans = [];
  const attr = /#\s*!?\s*\[\s*cfg\s*\(([^\]]*)\)\s*\]/gs;
  let match;
  while ((match = attr.exec(code))) {
    const testOnly = hasPositiveTestGate(match[1]);
    if (!testOnly) continue;
    if (/^#\s*!/.test(match[0])) {
      spans.push([match.index, enclosingScopeEnd(code, match.index)]);
      continue;
    }
    let cursor = attr.lastIndex;
    // 吸收紧邻的其他属性，再定位带 cfg(test) 的 item 本体。
    while (code.slice(cursor).match(/^\s*#\s*\[/)) {
      const next = code.slice(cursor).match(/^\s*#\s*\[[^\]]*\]/s);
      if (!next) break;
      cursor += next[0].length;
    }
    const head = code.slice(cursor);
    const item = head.match(/^\s*(?:(?:pub(?:\([^)]*\))?)\s+)?(?:async\s+)?(?:unsafe\s+)?(?:mod|fn|const|static|struct|enum|trait|type|impl|use)\b/);
    if (!item) continue;
    const itemStart = cursor + item.index;
    const open = code.indexOf('{', itemStart);
    const semi = code.indexOf(';', itemStart);
    let end = semi >= 0 && (open < 0 || semi < open) ? semi + 1 : open + 1;
    if (open >= 0 && end === open + 1) {
      let depth = 1;
      for (; end < code.length && depth > 0; end += 1) {
        if (code[end] === '{') depth += 1;
        else if (code[end] === '}') depth -= 1;
      }
    }
    spans.push([match.index, end]);
  }
  return spans;
}

function inSpans(offset, spans) {
  return spans.some(([start, end]) => offset >= start && offset < end);
}

function enclosingScopeEnd(code, offset) {
  const stack = [];
  for (let index = 0; index < offset; index += 1) {
    if (code[index] === '{') stack.push(index);
    else if (code[index] === '}') stack.pop();
  }
  if (stack.length === 0) return code.length;
  let depth = 1;
  for (let index = stack.at(-1) + 1; index < code.length; index += 1) {
    if (code[index] === '{') depth += 1;
    else if (code[index] === '}' && --depth === 0) return index + 1;
  }
  return code.length;
}

export function aliasRules(code) {
  const found = [];
  // use 声明允许跨行；记录 thread/mpsc 模块别名及通道类型别名。
  const imports = /\buse\s+([^;]+);/gs;
  let match;
  while ((match = imports.exec(code))) {
    const body = match[1]
      .replace(/std\s*::\s*\{([^}]*)\}/g, (_, inner) => inner.split(',').map((part) => `std::${part}`).join(','))
      .replace(/std\s*::\s*sync\s*::\s*\{([^}]*)\}/g, (_, inner) => inner.split(',').map((part) => `std::sync::${part}`).join(','));
    if (!/\bstd\s*::\s*(?:thread|sync\s*::\s*mpsc)\b/.test(body)) continue;
    const target = /std\s*::\s*(thread|sync\s*::\s*mpsc)(.*)$/s.exec(body);
    if (!target) continue;
    const selected = target[2].match(/\{([^}]*)\}/s)?.[1];
    if (!selected) {
      const alias = target[2].match(/\bas\s+(\w+)/)?.[1];
      const finalName = target[2].match(/::\s*(\w+)\s*$/)?.[1];
      const name = alias ?? finalName ?? (target[1] === 'thread' ? 'thread' : 'mpsc');
      found.push({ name, category: target[1] === 'thread' ? '平台线程' : '平台通道', offset: match.index });
    } else {
      for (const part of selected.split(',')) {
        const original = part.trim().split(' as ')[0].trim();
    if (original === 'self') {
          found.push({
            name: part.trim().match(/\bas\s+(\w+)/)?.[1] ?? (target[1] === 'thread' ? 'thread' : 'mpsc'),
            category: target[1] === 'thread' ? '平台线程' : '平台通道',
            offset: match.index,
          });
          continue;
        }
        if (!/^(spawn|scope|Builder|sleep|park|park_timeout|yield_now)$/.test(original)
          && !/^(channel|sync_channel|Sender|Receiver|SyncSender|SyncReceiver|TryRecvError|RecvTimeoutError)$/.test(original)) continue;
        const name = part.trim().match(/\bas\s+(\w+)/)?.[1] ?? original;
        const category = /^(spawn|scope|Builder|sleep|park|park_timeout|yield_now)$/.test(original) ? '平台线程' : '平台通道';
        found.push({ name, category, offset: match.index });
      }
    }
  }
  return found;
}

export function resolveModuleFile(declarationFile, moduleName, pathAttribute, knownFiles) {
  if (pathAttribute) {
    return path.posix.normalize(path.posix.join(path.posix.dirname(declarationFile), pathAttribute));
  }
  const declarationDirectory = path.posix.dirname(declarationFile);
  const moduleDirectory = path.posix.basename(declarationFile) === 'mod.rs'
    ? declarationDirectory
    : path.posix.join(declarationDirectory, path.posix.basename(declarationFile, '.rs'));
  return [
    path.posix.join(moduleDirectory, `${moduleName}.rs`),
    path.posix.join(moduleDirectory, moduleName, 'mod.rs'),
  ].find((candidate) => knownFiles.has(candidate));
}

export function sourceOccurrences(source) {
  const code = maskCommentsAndStrings(source);
  const hits = [];
  const imported = aliasRules(code);
  for (const [category, pattern] of rules) {
    pattern.lastIndex = 0;
    let match;
    while ((match = pattern.exec(code))) {
      const prefix = code.slice(Math.max(0, match.index - 64), match.index);
      const qualified = category === '旧等待模块或实验策略选择'
        || /(?:std\s*::\s*(?:thread|sync\s*::\s*mpsc)|(?:std\s*::\s*)?(?:mpsc|thread)\s*::)\s*$/.test(prefix)
        || imported.some((item) => item.category === category
          && new RegExp(`\\b${item.name}\\s*::\\s*$`).test(prefix));
      const plainForbidden = category === '平台等待或轮询' || category === '旧等待模块或实验策略选择';
      if (qualified || plainForbidden) hits.push({ category, offset: match.index });
    }
  }
  hits.push(...platformCfgOccurrences(source));
  hits.push(...imported.map((item) => ({ category: item.category, offset: item.offset })));
  return hits;
}

const invokedAsScript = Boolean(process.argv[1]) && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedAsScript) {
const files = (await Promise.all(roots.map(collect))).flat().sort();
const sources = new Map();
for (const file of files) sources.set(file, await readFile(path.join(root, file), 'utf8'));
// session.rs 声明的测试模块用于验证白名单归属，但不纳入 runtime/session 代码泄漏扫描。
const moduleSources = new Map(sources);
moduleSources.set('packages/engine/src/session.rs', await readFile(path.join(root, 'packages/engine/src/session.rs'), 'utf8'));

// 每个测试文件必须在仓库源码中被 cfg(test) module 声明；仅这些已验证测试模块可豁免整份测试文件。
const testFileSet = new Set();
const setupErrors = [];
function hasGatedModuleDeclaration(file, gate) {
  for (const [declarationFile, source] of moduleSources) {
    const structure = maskCommentsAndStrings(source);
    const attributes = rustAttributes(source);
    for (let index = 0; index < attributes.length; index += 1) {
      const first = attributes[index];
      const group = [first];
      let cursor = first.end;
      while (/^\s*$/.test(structure.slice(cursor, attributes[index + 1]?.start ?? cursor)) && attributes[index + 1]
        && /^#\s*\[/.test(structure.slice(attributes[index + 1].start))) {
        index += 1;
        group.push(attributes[index]);
        cursor = attributes[index].end;
      }
      const following = structure.slice(cursor).match(/^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;/);
      if (!following) continue;
      const rawAttributes = group.map((attribute) => attribute.inner);
      const gated = rawAttributes.some((inner) => {
        const cfg = /^\s*cfg\s*\(([\s\S]*)\)\s*$/.exec(inner);
        return cfg && (gate === 'test' ? hasPositiveTestGate(cfg[1]) : hasPositiveFeatureGate(cfg[1], gate));
      });
      if (!gated) continue;
      const pathAttribute = rawAttributes.map((inner) => /^\s*path\s*=\s*"([^"]+)"\s*$/.exec(inner)?.[1]).find(Boolean);
      const resolved = resolveModuleFile(declarationFile, following[1], pathAttribute, new Set(moduleSources.keys()));
      if (resolved === file) return true;
    }
  }
  return false;
}

for (const entry of testFiles) {
  const source = sources.get(entry.file);
  if (!source || !entry.reason.trim()) { setupErrors.push(`测试白名单无效：${entry.file}（缺文件或理由）`); continue; }
  const code = maskCommentsAndStrings(source);
  const testNamed = /(?:#\s*\[\s*test\s*\]|#\s*\[\s*tokio::test\s*\])/.test(code);
  const fixture = entry.file.endsWith('repurchase_group_fixture.rs');
  const diagnostic = entry.file.endsWith('/shutdown_verification.rs');
  if (!testNamed && !fixture && !diagnostic) { setupErrors.push(`测试白名单未通过结构校验：${entry.file}（未发现 test item）`); continue; }
  const declared = diagnostic
    ? hasGatedModuleDeclaration(entry.file, 'verification-harness')
    : hasGatedModuleDeclaration(entry.file, 'test');
  if (!declared) {
    setupErrors.push(`测试白名单未通过模块范围校验：${entry.file}（未找到 cfg(test) module 声明）`);
    continue;
  }
  testFileSet.add(entry.file);
}

const violations = [...setupErrors];
for (const file of files) {
  const source = sources.get(file);
  const code = maskCommentsAndStrings(source);
  const testSpans = cfgTestSpans(code);
  for (const hit of sourceOccurrences(source)) {
    if (inSpans(hit.offset, testSpans) || testFileSet.has(file)) continue;
    const { line, column } = location(source, hit.offset);
    const lineStart = source.lastIndexOf('\n', hit.offset - 1) + 1;
    const lineEnd = source.indexOf('\n', hit.offset);
    const lineText = source.slice(lineStart, lineEnd < 0 ? source.length : lineEnd).trim();
    if (!isExactException(file, hit.category, lineText)) {
      violations.push(`${file}:${line}:${column}: ${hit.category}: ${lineText}`);
    }
  }
}

if (violations.length) {
  process.stderr.write(`LibOS 词法边界检查发现 ${violations.length} 项待处理问题：\n${violations.join('\n')}\n`);
  process.exitCode = 1;
} else {
  process.stdout.write(`LibOS 词法边界检查完成：扫描 ${files.length} 个 runtime/session Rust 文件；检查线程、通道导入及别名、等待原语、平台条件编译和旧策略标记。\n`);
}
}
