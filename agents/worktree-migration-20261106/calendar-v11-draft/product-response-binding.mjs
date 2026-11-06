// 被动记录产品 TS 的真实响应；只读源码与响应，不改路由、消息或产品行为。
import { createHash } from "node:crypto";
import { readFileSync, realpathSync, writeFileSync } from "node:fs";
import path from "node:path";

export function productSourcePath(root, url) {
  const pathname = decodeURIComponent(String(url).replace(/^[a-z][a-z\d+.-]*:\/\/[^/]+/i, "").split(/[?#]/)[0]);
  if (pathname.split("/").some(part => part === "." || part === "..")) throw new Error("产品响应路径禁止越界段");
  let file;
  if (pathname.startsWith("/src/")) file = path.join(root, "apps/web", pathname);
  else if (pathname.startsWith("/apps/web/src/")) file = path.join(root, pathname);
  else if (pathname.startsWith("/@fs/") && pathname.includes("/apps/web/src/")) file = path.resolve("/" + pathname.slice("/@fs/".length));
  else return undefined;
  if (!file.startsWith(path.join(root, "apps/web/src") + path.sep)) throw new Error("产品响应指向旧树或其他源码根");
  return file;
}

const sha = bytes => createHash("sha256").update(bytes).digest("hex");
export function createProductResponseBinding(root, output, label) {
  const expected = JSON.parse(readFileSync(path.join(output, "source-before.json"), "utf8"));
  const records = [];
  return {
    records,
    capture(response) {
      let file;
      try { file = productSourcePath(root, response.url()); }
      catch (reason) { return Promise.resolve({ status: "rejected", reason, context: { productResponse: response.url() } }); }
      if (file === undefined) return undefined;
      return (async () => {
        if (realpathSync(file) !== file) throw new Error("产品响应源码 realpath 未绑定本人文件");
        const relative = path.relative(root, file), source = readFileSync(file);
        if (!Object.hasOwn(expected, relative) || expected[relative].bytes !== source.length || expected[relative].sha256 !== sha(source)) throw new Error("产品响应源码不等本批 source-before");
        const finished = await response.finished();
        if (finished !== null || response.status() !== 200) throw new Error("产品 TS 响应未真实成功完成");
        const body = await response.body();
        records.push({ url: response.url(), status: response.status(), finishedWithoutError: true,
          productRoot: root, sourceRelative: relative, sourceBytes: source.length, sourceSha256: sha(source),
          transformedResponseBytes: body.length, transformedResponseSha256: sha(body),
          scope: "产品原始源码 SHA 绑定本批快照；响应 SHA 为 Vite 变换后的真实 browser body，二者不宣称相等" });
        return { status: "fulfilled" };
      })().catch(reason => ({ status: "rejected", reason, context: { productResponse: response.url(), source: file } }));
    },
    write() {
      writeFileSync(path.join(output, `${label}-product-source-responses.json`), JSON.stringify({ productRoot: root, records }, null, 2), { flag: "wx" });
    },
    verify(required) {
      for (const relative of required) if (!records.some(row => row.sourceRelative === relative)) throw new Error("缺少本候选产品真实响应：" + relative);
    },
  };
}
