import { createHash } from "node:crypto";
import { cp, lstat, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createArchive, rejectSymlinkParents, validateTree } from "./package-distributions.mjs";
import { verifyWebReleaseWasm } from "./check-web-release-wasm.mjs";

export async function packageStaticWeb({ input, output, sha }, { archive = createArchive, root = process.cwd() } = {}) {
  if (typeof sha !== "string" || !/^[a-f0-9]{40}$/.test(sha)) throw new Error("static Web requires a valid source SHA");
  if (input !== "apps/web/dist-pages" || output !== "target/distributions/web") throw new Error("static Web packaging requires apps/web/dist-pages -> target/distributions/web");
  input = path.resolve(root, input);
  output = path.resolve(root, output);
  const license = path.resolve(root, "LICENSE");
  for (const filename of [input, output, license]) await rejectSymlinkParents(filename);
  try {
    await lstat(output);
    throw new Error("static Web output already exists; refusing overwrite");
  } catch (error) { if (error.code !== "ENOENT") throw error; }
  await validateTree(input);
  await verifyWebReleaseWasm(input);
  const html = await readFile(path.join(input, "index.html"), "utf8");
  if (!html.includes("<script")) throw new Error("static Web is missing its HTML entry script");
  await readFile(path.join(input, "pages-isolation.js"));
  await writeFile(path.join(input, "build-info.json"), `${JSON.stringify({ commit: sha, product: "web", engine: "browser-wasm" })}\n`, { flag: "wx" });
  await cp(license, path.join(input, "LICENSE"), { errorOnExist: true, force: false });
  await mkdir(output, { recursive: true });
  if ((await readdir(output)).length !== 0) throw new Error("static Web output already contains files; refusing overwrite");
  const entries = await readdir(input);
  const files = [];
  for (const format of ["zip", "tar.gz"]) {
    const name = `web-static.${format}`;
    await archive({ source: input, entries, output: path.join(output, name), format });
    const bytes = await readFile(path.join(output, name));
    files.push({ name, bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") });
  }
  const manifest = { schema: "distribution-manifest-v1", product: "web", target: "static", commit: sha, files };
  await writeFile(path.join(output, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`, { flag: "wx" });
  return manifest;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const argv = process.argv.slice(2);
  if (argv.length !== 6 || argv[0] !== "--input" || argv[2] !== "--output" || argv[4] !== "--sha") throw new Error("usage: package-static-web.mjs --input apps/web/dist-pages --output target/distributions/web --sha SHA");
  packageStaticWeb({ input: argv[1], output: argv[3], sha: argv[5] }).catch((error) => { console.error(`Static Web packaging failed: ${error.stack}`); process.exitCode = 1; });
}
