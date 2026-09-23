function check(id, label, passed, detail) {
  return { id, label, passed, detail };
}

export function formatBrowserException(details) {
  return details?.exception?.description ?? details?.text ?? "未知异常";
}

/** Analyze authoritative DOM diagnostics rather than guessing from screenshot pixels. */
export function analyzeChartProgress(before, after) {
  const gameAdvanced = after.game.tick > before.game.tick || after.game.day > before.game.day;
  const intradayAdvanced = after.intraday.signature !== before.intraday.signature
    || after.intraday.latestMinute !== before.intraday.latestMinute
    || after.intraday.count !== before.intraday.count;
  const klineAdvanced = after.kline.signature !== before.kline.signature
    || after.kline.count !== before.kline.count;
  const checks = [
    check("clock", "游戏时钟", gameAdvanced, gameAdvanced
      ? `tick ${before.game.tick} → ${after.game.tick}（第 ${before.game.day + 1} → ${after.game.day + 1} 日）`
      : `未推进：tick 仍为 ${after.game.tick}`),
    check("intraday", "分时图", intradayAdvanced, intradayAdvanced
      ? `点数 ${before.intraday.count} → ${after.intraday.count}，最新分钟 ${before.intraday.latestMinute} → ${after.intraday.latestMinute}`
      : `未推进：${after.intraday.signature}`),
    check("kline", "K 线", klineAdvanced, klineAdvanced
      ? `K 线数 ${before.kline.count} → ${after.kline.count}，末根签名已变化`
      : `未推进：${after.kline.signature}`),
  ];
  return { passed: checks.every((item) => item.passed), checks };
}

function htmlEscape(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function jsonBlock(value) {
  return htmlEscape(JSON.stringify(value, null, 2));
}

export function buildHtmlReport(report) {
  const status = report.result.passed ? "PASS" : "FAIL";
  const checks = report.result.checks.map((item) => `
    <tr><td>${htmlEscape(item.label)}</td><td class="${item.passed ? "pass" : "fail"}">${item.passed ? "PASS" : "FAIL"}</td><td>${htmlEscape(item.detail)}</td></tr>`).join("");
  const metrics = Object.entries(report.performance).map(([name, value]) => `
    <tr><td>${htmlEscape(name)}</td><td>${htmlEscape(typeof value === "number" ? Number(value.toFixed(3)) : value)}</td></tr>`).join("");
  const screenshots = report.screenshots.map((path) => `
    <figure><img src="${htmlEscape(path)}" alt="${htmlEscape(path)}"><figcaption>${htmlEscape(path)}</figcaption></figure>`).join("");
  return `<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>行情 UI 性能回归 ${status}</title><style>
body{font:14px/1.55 system-ui,sans-serif;max-width:1100px;margin:auto;padding:24px;color:#172033;background:#f4f6f8}h1,h2{margin:.3em 0}.hero,section{background:#fff;border:1px solid #dfe4ea;border-radius:12px;padding:18px;margin:14px 0}.badge{display:inline-block;padding:5px 10px;border-radius:999px;font-weight:800}.pass{color:#087a38}.fail{color:#c62828}.badge.pass{background:#dff7e8}.badge.fail{background:#ffe2e2}table{border-collapse:collapse;width:100%}td,th{border-bottom:1px solid #e7ebef;padding:8px;text-align:left}figure{display:inline-block;width:min(46%,390px);vertical-align:top;margin:10px}img{width:100%;border:1px solid #ccd3da;border-radius:8px}pre{overflow:auto;background:#101722;color:#dce7f3;padding:12px;border-radius:8px}</style></head><body>
<div class="hero"><span class="badge ${report.result.passed ? "pass" : "fail"}">${status}</span><h1>行情 UI 最快档性能回归</h1><p>${htmlEscape(report.generatedAt)} · ${htmlEscape(report.url)} · 每阶段 ${report.durationMs} ms</p></div>
<section><h2>推进检查</h2><table><thead><tr><th>对象</th><th>结果</th><th>证据</th></tr></thead><tbody>${checks}</tbody></table></section>
<section><h2>性能数据</h2><table><tbody>${metrics}</tbody></table></section>
<section><h2>截图</h2>${screenshots}</section>
<section><h2>原始诊断</h2><pre>${jsonBlock({ before: report.before, after: report.after })}</pre></section>
</body></html>`;
}
