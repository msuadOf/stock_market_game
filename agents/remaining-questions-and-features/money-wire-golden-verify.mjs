import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

const [baselineDirectory, currentDirectory, fixture, partition] = process.argv.slice(2)
assert.ok(baselineDirectory && currentDirectory && fixture)
const stepFixture = ['0', '3', '6'].includes(fixture)
if (stepFixture) assert.ok(partition === 'digest' || /^[0-3]$/.test(partition), 'step 核验必须选择 0–3 的五个 tick 分片或独立 digest')

function tokens(raw) {
  const result = []
  const expression = /"(?:[^"\\]|\\.)*"|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?|true|false|null|[{}\[\]:,]/gy
  let cursor = 0
  while (cursor < raw.length) {
    while (/\s/.test(raw[cursor])) cursor += 1
    if (cursor === raw.length) break
    expression.lastIndex = cursor
    const match = expression.exec(raw)
    assert.ok(match, `无法解析原文 token：${cursor}`)
    result.push({ text: match[0], start: cursor, end: expression.lastIndex })
    cursor = expression.lastIndex
  }
  let index = 0
  function visit(path) {
    const token = result[index++]
    assert.ok(token)
    token.path = path
    if (token.text === '{') {
      if (result[index].text === '}') { index += 1; return }
      for (;;) {
        const key = result[index++]
        assert.ok(key.text.startsWith('"'))
        assert.equal(result[index++].text, ':')
        visit([...path, JSON.parse(key.text)])
        const separator = result[index++].text
        if (separator === '}') break
        assert.equal(separator, ',')
      }
    } else if (token.text === '[') {
      if (result[index].text === ']') { index += 1; return }
      let item = 0
      for (;;) {
        visit([...path, item++])
        const separator = result[index++].text
        if (separator === ']') break
        assert.equal(separator, ',')
      }
    }
  }
  visit([])
  assert.equal(index, result.length)
  return result
}

const approvedPatterns = [
  /^\/(?:2\/)?setup\/config\/(?:starting_cash|commission_min)$/,
  /^\/(?:2\/)?setup\/npcs\/retail_cash_median$/,
  /^\/(?:2\/)?setup\/stocks\/\d+\/(?:initial_price|tick)$/,
  /^\/(?:1|(?:2\/)?snapshot)\/markets\/\d+\/(?:last_price|last_close|best_bid|best_ask)$/,
  /^\/(?:1|(?:2\/)?snapshot)\/accounts\/\d+\/(?:cash|reserved_cash)$/,
  /^\/(?:1|(?:2\/)?snapshot)\/accounts\/\d+\/positions\/\d+\/(?:invested_cents|recovered_cents)$/,
  /^\/(?:1|(?:2\/)?snapshot)\/daily_candles\/\d+\/\d+\/(?:open|high|low|close)$/,
  /^\/(?:1|(?:2\/)?snapshot)\/active_daily_candles\/\d+\/(?:open|high|low|close)$/,
  /^\/(?:2\/)?market_minute_closes\/\d+\/\d+\/close$/,
  /^\/(?:2\/)?price_history\/\d+\/\d+$/,
  /^\/(?:0\/)?\d+\/(?:Trade|OrderAccepted)\/price$/,
  /^\/(?:0\/)?\d+\/AuctionCompleted\/clearing_price$/,
  /^\/(?:0\/)?\d+\/AuctionTick\/indicative_price$/,
  /^\/(?:0\/)?\d+\/PriceTick\/last_price$/,
  /^\/(?:0\/)?\d+\/PriceTick\/daily_candle\/(?:open|high|low|close)$/,
  /^\/(?:0\/)?\d+\/PriceTick\/(?:bids|asks)\/\d+\/0$/,
  /^\/(?:0\/)?\d+\/DayBoundary\/closed_daily_candles\/\d+\/(?:open|high|low|close)$/,
  /^\/retail_experience\/\d+\/(?:reference_equity|peak_equity)$/,
  /^\/retail_experience\/\d+\/stocks\/\d+\/(?:entry_reference_price|peak_price_since_entry|last_buy_price)$/,
  /^\/retail_experience\/\d+\/feedback\/stocks\/\d+\/last_own_observation\/price$/,
  /^\/belief_books\/\d+\/experience\/(?:reference_equity|peak_equity)$/,
  /^\/belief_books\/\d+\/experience\/stocks\/\d+\/(?:entry_reference_price|peak_price_since_entry|last_buy_price)$/,
  /^\/belief_books\/\d+\/experience\/feedback\/stocks\/\d+\/institutional_fees_paid$/,
  /^\/belief_books\/\d+\/experience\/feedback\/stocks\/\d+\/last_own_observation\/price$/,
  /^\/price_memories\/\d+\/stocks\/\d+\/(?:first_observed_price|last_observed_price|observed_high|observed_low)$/,
]

function pointer(path) {
  return '/' + path.map(part => String(part).replaceAll('~', '~0').replaceAll('/', '~1')).join('/')
}

function compare(baseline, current, name) {
  const before = tokens(baseline)
  const after = tokens(current)
  assert.equal(before.length, after.length, `${name} token 数量改变`)
  const replacements = []
  const paths = new Map()
  for (let index = 0; index < before.length; index += 1) {
    const oldToken = before[index]
    const newToken = after[index]
    if (oldToken.text === newToken.text) continue
    assert.ok(oldToken.path && newToken.path, `${name} 结构或键改变`)
    assert.deepEqual(oldToken.path, newToken.path, `${name} path 改变`)
    assert.match(oldToken.text, /^(?:0|-[1-9]\d*|[1-9]\d*)$/)
    assert.equal(newToken.text, JSON.stringify(oldToken.text), `${name} 非精确分字符串变化`)
    const typedPath = pointer(oldToken.path)
    assert.ok(approvedPatterns.some(pattern => pattern.test(typedPath)), `${name} 未核验类型 path：${typedPath}`)
    paths.set(typedPath, (paths.get(typedPath) ?? 0) + 1)
    replacements.push({ start: oldToken.start, end: oldToken.end, text: newToken.text })
  }
  let rebuilt = ''
  let cursor = 0
  for (const replacement of replacements) {
    rebuilt += baseline.slice(cursor, replacement.start) + replacement.text
    cursor = replacement.end
  }
  rebuilt += baseline.slice(cursor)
  assert.equal(rebuilt, current, `${name} 许可分 token 加引号之后原字节不等`)
  return { count: replacements.length, paths }
}

function digest(chunks) {
  let high = 0xcbf29ce4
  let low = 0x84222325
  const sha = createHash('sha256')
  for (const chunk of chunks) {
    sha.update(chunk)
    for (const byte of chunk) {
      const operand = (low ^ byte) >>> 0
      const product = operand * 435
      high = (high * 435 + Math.floor(product / 0x100000000) + operand * 256) >>> 0
      low = product >>> 0
    }
  }
  const fnv = (BigInt(high) << 32n) | BigInt(low)
  return { fnv: fnv.toString(), sha256: sha.digest('hex') }
}

let names = stepFixture
  ? Array.from({ length: 20 }, (_, tick) => `step-auction-${fixture}-tick-${String(tick + 1).padStart(2, '0')}.json`)
  : ['events', 'mid', 'end'].map(name => `replay-${fixture === 'perturbed' ? '104375189074106' : '104372539623683'}-${name}.json`)
if (stepFixture && partition !== 'digest') names = names.slice(Number(partition) * 5, (Number(partition) + 1) * 5)
const baselines = []
const currents = []
const paths = new Map()
let changedTokens = 0
for (const name of names) {
  const baseline = readFileSync(join(baselineDirectory, name))
  const current = readFileSync(join(currentDirectory, name))
  if (partition !== 'digest') {
    const result = compare(baseline.toString('utf8'), current.toString('utf8'), name)
    changedTokens += result.count
    for (const [path, count] of result.paths) paths.set(path, (paths.get(path) ?? 0) + count)
    if (!stepFixture) console.log(JSON.stringify({ name, baseline: digest([baseline]), current: digest([current]), changedTokens: result.count }))
  }
  baselines.push(baseline)
  currents.push(current)
}
if (stepFixture) console.log(JSON.stringify({ fixture, partition, baseline: digest(baselines), current: digest(currents), changedTokens }))
console.log(JSON.stringify({ paths: Object.fromEntries([...paths].sort()), changedTokens }))
