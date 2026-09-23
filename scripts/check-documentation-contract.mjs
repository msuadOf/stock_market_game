import fs from "node:fs";
import path from "node:path";

const root = path.resolve(process.argv[2] ?? ".");
const documents = ["README.md", "docs/diagnostics.md", "docs/causal-diagnostics.md", "docs/trading-rules.md"];
const required = [
  ["README.md", (text) => /8\s*MiB/.test(text) && /8,388,608/.test(text) && /远程加载请求体上限/.test(text) && /部署和传输边界/.test(text), "README names the remote body gate and consequence"],
  ["README.md", (text) => /真实 Wry\/Tauri/.test(text) && /(?:已解码 PNG|未形成像素有效截图)/.test(text) && /Xvfb.*X11 compatibility fallback/s.test(text), "README distinguishes native launch, pixels, and Xvfb"],
  ["docs/diagnostics.md", (text) => /MAX_LOAD_BODY_BYTES\s*=\s*8\s*\*\s*1024\s*\*\s*1024/.test(text) && /server_body_fit=false/.test(text) && /部署\/?传输限制/.test(text), "diagnostics records a coherent body-gate result"],
  ["docs/diagnostics.md", (text) => /C06/.test(text) && /缺少历史数据校准与独立留出验证/.test(text) && /incomplete\/unsupported/.test(text), "diagnostics names incomplete calibration and source limits"],
  ["docs/trading-rules.md", (text) => /primary-source/.test(text) && /incomplete\/unsupported/.test(text), "trading rules names primary-source limits"],
  ["docs/causal-diagnostics.md", (text) => /shared CivilInstant observation clock includes the\s+lunch interval/.test(text) && /5401-second civil jump/.test(text), "causal docs use repaired CivilInstant lunch wording"],
];

const qualification = /(?:不是|不得|没有|未|不能|不保证|不声明|不代表|不支持|不兼容|\bnot\b|\bno\b|\bwithout\b|\bunsupported\b|\bincomplete\b|\bcannot\b|\bdoesn't\b|\bdoes not\b)/iu;
const staleClaimChecks = [
  [/\b(?:weston|compositor|wayland)\b/i, /\b(?:universally|universal(?:ly)?|all|every|each|any|across)\b/i, /(?:support|compatib|work|behavio)/i, "universal/all compositor compatibility claim"],
  [/(?:合成器|wayland)/i, /(?:所有|全部|任意|任何|各类|通用|每个|各个)/u, /(?:支持|兼容|正常工作|行为)/u, "Chinese universal compositor compatibility claim"],
  [/(?:weston|wayland)/i, /(?:image|screenshot|pixel|capture|visual|render)/i, /(?:green|pass(?:ed|es)?|success(?:ful|fully)?|valid|correct|works?)/i, "generic Weston/Wayland image success claim"],
  [/(?:weston|wayland)/i, /(?:图片|图像|截图|像素|捕获|视觉|渲染)/u, /(?:green|通过|成功|有效|正确|正常工作)/iu, "Chinese generic Weston/Wayland image success claim"],
  [/(?:c06|historical calibration|holdout validation|calibration and holdout|calibration holdout)/i, /(?:complete|completed|finished|approved|done|finalized|resolved|validated)/i, /(?:^|$)/, "C06 English completion claim"],
  [/(?:c06|历史数据校准|留出验证)/iu, /(?:通过|完成|已完成|批准|结束|最终|解决)/u, /(?:^|$)/, "C06 Chinese completion claim"],
  [/(?:observation|decision|acquisition)\s+(?:timeline|clock)|(?:timeline|clock)[^.!?\n]{0,30}(?:break|lunch)/i, /(?:skips?|omits?|without|excludes?|misses?|leaves out)/i, /(?:midday break|lunch|noon(?: break)?)/i, "stale English midday-break omission"],
  [/(?:观察|决策|采集)[^。\n]*(?:时间线|时钟|clock)|(?:时间线|时钟)[^。\n]{0,30}(?:午休|中午休息)/u, /(?:跳过|省略|不含|不计|遗漏|缺少)/u, /(?:午休|中午休息|midday break|lunch)/iu, "stale Chinese midday-break omission"],
];

const bodyGateParagraph = (paragraph) =>
  /MAX_LOAD_BODY_BYTES\s*=\s*8\s*\*\s*1024\s*\*\s*1024/i.test(paragraph) &&
  /8\s*MiB/i.test(paragraph) &&
  /8,388,608/.test(paragraph) &&
  /8192\s*KiB/i.test(paragraph) &&
  /MAX_LOAD_BODY_BYTES\s*=\s*8\s*\*\s*1024\s*\*\s*1024/i.test(paragraph) &&
  /(?:server|remote|body|load|transport|请求体|远程|部署|传输)/i.test(paragraph) &&
  /(?:fit|limit|gate|failure|false|限制|门禁|失败|边界)/i.test(paragraph);

const contents = new Map(documents.map((document) => {
  const file = path.join(root, document);
  if (!fs.existsSync(file)) throw new Error(`missing documentation file: ${document}`);
  return [document, fs.readFileSync(file, "utf8")];
}));

const failures = [];
for (const [document, predicate, description] of required) {
  if (!predicate(contents.get(document))) failures.push(`${description}: ${document}`);
}
for (const [document, content] of contents) {
  const paragraphs = content.split(/\n\s*\n/u);
  const claimUnits = content.split(/(?<=[.!?。！？])\s*/u).map((unit) => unit.normalize("NFKC").replace(/[\u200b\uFEFF]/g, " "));
  if (document === "README.md" && !paragraphs.some(bodyGateParagraph)) failures.push(`README body-gate coherence: ${document}`);
  if (document === "docs/diagnostics.md" && !paragraphs.some(bodyGateParagraph)) failures.push(`diagnostics body-gate coherence: ${document}`);
  for (const [subject, assertion, outcome, description] of staleClaimChecks) {
    if (claimUnits.some((unit) => subject.test(unit) && assertion.test(unit) && outcome.test(unit) && !qualification.test(unit))) {
      failures.push(`${description}: ${document}`);
    }
  }
  if (paragraphs.some((paragraph) => /(?:MAX_LOAD_BODY_BYTES|server_body_fit|8\s*MiB|8,388,608|8192\s*KiB)/i.test(paragraph) && !bodyGateParagraph(paragraph))) {
    failures.push(`detached or incoherent body-limit token: ${document}`);
  }
}
if (failures.length > 0) {
  console.error(["Documentation contract failed:", ...failures.map((failure) => `- ${failure}`)].join("\n"));
  process.exitCode = 1;
} else {
    console.log(`Documentation contract verified: ${documents.length} documents, ${required.length} semantic requirements, ${staleClaimChecks.length} semantic stale-claim guards`);
}
