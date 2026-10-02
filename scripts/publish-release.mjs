import { createHash } from "node:crypto";
import { constants, createReadStream } from "node:fs";
import { copyFile, lstat, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseReleaseTag } from "./release-policy.mjs";
import { runBoundedCommand } from "./run-with-deadline.mjs";

function requireSha(sha) {
  if (typeof sha !== "string" || !/^[a-f0-9]{40}$/.test(sha)) throw new Error("invalid release source SHA");
}

export async function collectReleaseAssets(input, output, sha) {
  requireSha(sha);
  const directories = (await readdir(input)).sort();
  if (directories.length !== 10) throw new Error("release requires exactly ten distributions: three Desktop, three Server, three WebUI Server and one static Web");
  const distributions = [];
  const artifacts = [];
  const usedNames = new Set(["release-source.json"]);
  const platforms = new Set();
  for (const directory of directories) {
    const parent = path.join(input, directory);
    const info = await lstat(parent);
    if (!info.isDirectory() || info.isSymbolicLink()) throw new Error(`invalid distribution directory: ${directory}`);
    const manifestFile = path.join(parent, "manifest.json");
    if (!(await lstat(manifestFile)).isFile() || (await lstat(manifestFile)).isSymbolicLink()) throw new Error(`invalid manifest: ${directory}`);
    const manifest = JSON.parse(await readFile(manifestFile, "utf8"));
    const targetMatch = /^(?:x86_64|aarch64)-(unknown-linux-gnu|pc-windows-msvc|apple-darwin)$/.exec(manifest.target);
    const web = manifest.product === "web" && manifest.target === "static";
    if (web && manifest.commit !== sha) throw new Error(`static Web manifest source SHA differs from release source: ${directory}`);
    if (manifest.schema !== "distribution-manifest-v1" || (!web && (!["desktop", "server", "webui-server"].includes(manifest.product) || !targetMatch))
      || directory !== `stock-market-game-${manifest.product}-${manifest.target}-unsigned`
      || !Array.isArray(manifest.files) || manifest.files.length === 0) throw new Error(`invalid distribution manifest: ${directory}`);
    const identity = web ? "web/static" : `${manifest.product}/${targetMatch[1]}`;
    if (platforms.has(identity)) throw new Error(`duplicate product/platform: ${identity}`);
    platforms.add(identity);
    const actual = (await readdir(parent)).sort();
    const expected = ["manifest.json", ...manifest.files.map((file) => file.name)].sort();
    if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error(`missing or extra files in ${directory}`);
    for (const file of manifest.files) {
      if (typeof file.name !== "string" || file.name === "." || file.name === ".." || /[/\\\x00-\x1f\x7f]/.test(file.name)
        || !Number.isSafeInteger(file.bytes) || file.bytes <= 0 || typeof file.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(file.sha256)) throw new Error(`invalid asset record: ${directory}`);
      const source = path.join(parent, file.name);
      const stat = await lstat(source);
      if (!stat.isFile() || stat.isSymbolicLink() || stat.size !== file.bytes) throw new Error(`invalid asset bytes: ${source}`);
      const hash = createHash("sha256");
      for await (const chunk of createReadStream(source)) hash.update(chunk);
      const digest = hash.digest("hex");
      if (digest !== file.sha256) throw new Error(`asset digest mismatch: ${source}`);
      if (file.name === "LICENSE") continue;
      if (usedNames.has(file.name)) throw new Error(`release asset name collision: ${file.name}`);
      usedNames.add(file.name);
      artifacts.push({ source, name: file.name });
    }
    const name = `${manifest.product}-${manifest.target}-manifest.json`;
    if (usedNames.has(name)) throw new Error(`manifest name collision: ${name}`);
    usedNames.add(name);
    artifacts.push({ source: manifestFile, name });
    distributions.push(manifest);
  }
  await mkdir(output);
  const assets = [];
  for (const { source, name } of artifacts) {
    const destination = path.join(output, name);
    await copyFile(source, destination, constants.COPYFILE_EXCL);
    assets.push(destination);
  }
  const provenance = path.join(output, "release-source.json");
  await writeFile(provenance, `${JSON.stringify({ schema: "release-source-v1", commit: sha, unsigned: true, distributions }, null, 2)}\n`, { flag: "wx" });
  assets.push(provenance);
  return assets;
}

async function gh(args) {
  const result = await runBoundedCommand({ command: "gh", args, timeoutMs: 299000, captureOutput: true });
  return result.stdout.trim();
}

export async function main(env = process.env, invoke = gh, collect = collectReleaseAssets) {
  const { tag, prerelease } = parseReleaseTag(env.RELEASE_TAG);
  requireSha(env.RELEASE_SHA);
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(env.GITHUB_REPOSITORY)) throw new Error("invalid release repository");
  const resolved = await invoke(["api", `repos/${env.GITHUB_REPOSITORY}/commits/${tag}`, "--jq", ".sha"]);
  if (resolved !== env.RELEASE_SHA) throw new Error("tag commit changed since build; refusing to publish stale artifacts");
  const assets = await collect("target/release-input", "target/release-assets", env.RELEASE_SHA);
  await invoke(["release", "create", tag, ...assets, "--repo", env.GITHUB_REPOSITORY, "--verify-tag", "--draft", "--title", tag,
    "--notes", `Source commit: ${env.RELEASE_SHA}\nUnsigned Windows/Linux/macOS packages and static Web. No publisher signing or notarization. Product version remains the version recorded in source.`,
    ...(prerelease ? ["--prerelease"] : [])]);
  const draft = JSON.parse(await invoke(["release", "view", tag, "--repo", env.GITHUB_REPOSITORY, "--json", "databaseId,isDraft,tagName"]));
  if (!Number.isSafeInteger(draft?.databaseId) || draft.databaseId <= 0 || draft.isDraft !== true || draft.tagName !== tag) {
    throw new Error("uploaded Release draft identity differs; refusing publication");
  }
  const uploaded = JSON.parse(await invoke(["api", `repos/${env.GITHUB_REPOSITORY}/releases/${draft.databaseId}`]));
  if (uploaded?.id !== draft.databaseId || uploaded.draft !== true || uploaded.tag_name !== tag) {
    throw new Error("uploaded Release draft identity changed; refusing publication");
  }
  if (!Array.isArray(uploaded.assets) || uploaded.assets.length !== assets.length
    || new Set(uploaded.assets.map((asset) => asset?.name)).size !== assets.length) {
    throw new Error("uploaded Release asset inventory differs from verified local assets; leaving draft");
  }
  for (const filename of assets) {
    const asset = uploaded.assets.find((entry) => entry?.name === path.basename(filename));
    const hash = createHash("sha256");
    for await (const chunk of createReadStream(filename)) hash.update(chunk);
    if (!asset || asset.size !== (await lstat(filename)).size || asset.digest !== `sha256:${hash.digest("hex")}`) {
      throw new Error(`uploaded Release asset name, size or digest differs: ${path.basename(filename)}; leaving draft`);
    }
  }
  await invoke(["release", "edit", tag, "--repo", env.GITHUB_REPOSITORY, "--draft=false", ...(prerelease ? ["--latest=false"] : [])]);
  console.log(`Published ${tag} from ${env.RELEASE_SHA}: ${assets.length} verified assets.`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((error) => { console.error(`Release publication failed: ${error.stack}; an interrupted upload may leave a draft; inspect it before retrying.`); process.exitCode = 1; });
}
