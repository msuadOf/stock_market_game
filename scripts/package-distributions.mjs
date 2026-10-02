#!/usr/bin/env node
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { constants, createReadStream } from "node:fs";
import { chmod, copyFile, lstat, mkdir, mkdtemp, readdir, readFile, readlink, realpath, rm, rmdir, symlink, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const products = new Set(["desktop", "server", "webui-server"]);
const usage = "usage: node scripts/package-distributions.mjs <desktop|server|webui-server> --input target/build-artifacts/NAME --output target/distributions/NAME --target <native rust triple>";

export function parseArgs(argv) {
  if (!products.has(argv[0])) throw new Error(`${usage}; unknown product: ${argv[0]}`);
  const options = { product: argv[0] };
  for (let index = 1; index < argv.length; index += 2) {
    const flag = argv[index];
    if (!["--input", "--output", "--target"].includes(flag)) throw new Error(`${usage}; unknown option: ${flag}`);
    const key = flag.slice(2);
    if (Object.hasOwn(options, key)) throw new Error(`${usage}; duplicate option: ${flag}`);
    const value = argv[index + 1];
    if (typeof value !== "string" || value.length === 0 || value.startsWith("--")) throw new Error(`${usage}; ${flag} requires a value`);
    options[key] = value;
  }
  for (const key of ["input", "output", "target"]) {
    if (!Object.hasOwn(options, key)) throw new Error(`${usage}; --${key} is required`);
  }
  return options;
}

export function validateTarget(target, { platform = process.platform, arch = process.arch } = {}) {
  const match = /^(x86_64|aarch64|i686|armv7|riscv64gc|powerpc64le|s390x)-(pc-windows-(?:msvc|gnu|gnullvm)|apple-darwin|unknown-linux-(?:gnu|musl)(?:eabihf)?)$/.exec(target);
  if (match === null) throw new Error(`unsupported native Rust target: ${target}`);
  const targetPlatform = match[2].startsWith("pc-windows-") ? "win32" : match[2] === "apple-darwin" ? "darwin" : "linux";
  const targetArch = { x86_64: "x64", aarch64: "arm64", i686: "ia32", armv7: "arm", riscv64gc: "riscv64", powerpc64le: "ppc64", s390x: "s390x" }[match[1]];
  if (targetPlatform !== platform || targetArch !== arch) {
    throw new Error(`host mismatch: target ${target} requires ${targetPlatform}/${targetArch}, actual host is ${platform}/${arch}; package on that native host (no compilation is performed)`);
  }
  return targetPlatform;
}

async function statIfPresent(filename) {
  try {
    return await lstat(filename);
  } catch (error) {
    if (error.code === "ENOENT") return undefined;
    throw error;
  }
}

function artifactPath(root, value, category) {
  if (typeof value !== "string" || value.length === 0) throw new Error(`${category} path is required`);
  const base = path.join(root, "target", category);
  const directory = path.resolve(root, value);
  if (path.dirname(directory) !== base || !/^[A-Za-z0-9][A-Za-z0-9_-]*$/.test(path.basename(directory))) {
    throw new Error(`${category} path must be a direct child: target/${category}/NAME; got ${value}`);
  }
  return directory;
}

export async function rejectSymlinkParents(filename) {
  const absolute = path.resolve(filename);
  const paths = [];
  for (let current = absolute; ; current = path.dirname(current)) {
    paths.push(current);
    if (path.dirname(current) === current) break;
  }
  for (const current of paths.reverse()) {
    const info = await statIfPresent(current);
    if (info?.isSymbolicLink()) throw new Error(`symlink path or parent is not allowed: ${current}`);
    if (info !== undefined && current !== absolute && !info.isDirectory()) throw new Error(`parent is not a directory: ${current}`);
  }
}

function within(root, filename) {
  const relative = path.relative(root, filename);
  return relative === "" || (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative));
}

async function requireDirectory(directory) {
  const info = await lstat(directory);
  if (info.isSymbolicLink()) throw new Error(`symlink directory is not allowed: ${directory}`);
  if (!info.isDirectory()) throw new Error(`expected a directory: ${directory}`);
  return info;
}

async function requireFile(filename, { executable = false } = {}) {
  const info = await lstat(filename);
  if (info.isSymbolicLink()) throw new Error(`symlink file is not allowed: ${filename}`);
  if (!info.isFile()) throw new Error(`expected a regular file: ${filename}`);
  if (info.size === 0) throw new Error(`empty distribution file: ${filename}`);
  if (executable && process.platform !== "win32" && (info.mode & 0o111) === 0) throw new Error(`distribution binary is not executable: ${filename}`);
  return info;
}

export async function validateTree(directory, { allowSymlinks = false } = {}) {
  await requireDirectory(directory);
  let files = 0;
  async function visit(current) {
    for (const name of (await readdir(current)).sort()) {
      const filename = path.join(current, name);
      const info = await lstat(filename);
      if (info.isSymbolicLink()) {
        if (!allowSymlinks) throw new Error(`symlink is not allowed: ${filename}`);
        const target = await readlink(filename);
        if (path.isAbsolute(target) || path.win32.isAbsolute(target) || !within(directory, path.resolve(current, target))) {
          throw new Error(`absolute or external symlink is not allowed: ${filename} -> ${target}`);
        }
        let resolved;
        try {
          resolved = await realpath(filename);
        } catch (error) {
          throw new Error(`cannot resolve symlink ${filename} -> ${target}: ${error.message}`, { cause: error });
        }
        if (!within(directory, resolved)) throw new Error(`external symlink is not allowed: ${filename} -> ${resolved}`);
      } else if (info.isDirectory()) {
        await visit(filename);
      } else {
        await requireFile(filename);
        files += 1;
      }
    }
  }
  await visit(directory);
  if (files === 0) throw new Error(`empty distribution directory: ${directory}`);
}

async function copyRegular(source, destination) {
  const info = await requireFile(source);
  await copyFile(source, destination, constants.COPYFILE_EXCL);
  await chmod(destination, info.mode & 0o777);
}

async function copyTree(source, destination) {
  const info = await requireDirectory(source);
  await mkdir(destination);
  for (const name of (await readdir(source)).sort()) {
    const sourceEntry = path.join(source, name);
    const destinationEntry = path.join(destination, name);
    const entryInfo = await lstat(sourceEntry);
    if (entryInfo.isSymbolicLink()) await symlink(await readlink(sourceEntry), destinationEntry);
    else if (entryInfo.isDirectory()) await copyTree(sourceEntry, destinationEntry);
    else await copyRegular(sourceEntry, destinationEntry);
  }
  await chmod(destination, info.mode & 0o777);
}

export function runArchiveCommand(command, args, { cwd }) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd, stdio: ["ignore", "pipe", "pipe"], windowsHide: true });
    let detail = "";
    for (const stream of [child.stdout, child.stderr]) stream.on("data", (chunk) => { detail = (detail + chunk.toString()).slice(-16384); });
    child.on("error", (error) => reject(new Error(`cannot start archive tool ${command} in ${cwd}: ${error.message}; install the system archive tool`, { cause: error })));
    child.on("close", (code, signal) => {
      if (code !== 0) reject(new Error(`archive tool ${command} failed in ${cwd} (${signal === null ? `exit ${code}` : `signal ${signal}`}): ${detail.trim()}`));
      else resolve();
    });
  });
}

export async function createArchive({ source, entries, output, format }, { run = runArchiveCommand, platform = process.platform } = {}) {
  if (!["zip", "tar.gz"].includes(format)) throw new Error(`unsupported archive format: ${format}`);
  if (!Array.isArray(entries) || entries.length === 0 || entries.some((name) => typeof name !== "string" || name.length === 0 || name === "." || name === ".." || name.startsWith("-") || /[/\\:*?\[\]\x00-\x1f\x7f]/u.test(name))) {
    throw new Error("archive entries must be explicit top-level artifact names");
  }
  await rejectSymlinkParents(source);
  await rejectSymlinkParents(output);
  if (await statIfPresent(output) !== undefined) throw new Error(`archive output already exists: ${output}`);
  for (const entry of entries) {
    const filename = path.join(source, entry);
    const info = await lstat(filename);
    if (info.isDirectory()) await validateTree(filename, { allowSymlinks: process.platform !== "win32" && entry.endsWith(".app") });
    else await requireFile(filename);
  }
  if (format === "tar.gz") {
    await run("tar", ["-czf", output, "--", ...entries], { cwd: source });
  } else if (platform === "darwin") {
    await run("tar", ["--format", "zip", "--options", "zip:hdrcharset=UTF-8", "-cf", output, "--", ...entries], { cwd: source });
  } else if (platform === "win32") {
    const quote = (value) => `'${value.replaceAll("'", "''")}'`;
    const command = `$ErrorActionPreference = 'Stop'; Compress-Archive -LiteralPath @(${entries.map(quote).join(",")}) -DestinationPath ${quote(output)} -CompressionLevel Optimal`;
    await run("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", command], { cwd: source });
  } else {
    await run("zip", ["-q", "-r", "-y", "-UN=UTF8", output, "--", ...entries], { cwd: source });
  }
  await requireFile(output);
}

export async function desktopInputs(input, platform) {
  if (!["win32", "linux", "darwin"].includes(platform)) throw new Error(`unsupported desktop platform: ${platform}`);
  const bundle = path.join(input, "bundle");
  await requireDirectory(bundle);
  const formats = platform === "win32" ? [["msi", ".msi"], ["nsis", ".exe"]] : platform === "linux" ? [["deb", ".deb"], ["rpm", ".rpm"], ["appimage", ".AppImage"]] : [["dmg", ".dmg"]];
  const installers = [];
  for (const [folder, extension] of formats) {
    const directory = path.join(bundle, folder);
    if (await statIfPresent(directory) === undefined) throw new Error(`missing required ${folder} installer directory: ${directory}`);
    await requireDirectory(directory);
    let found = false;
    for (const name of (await readdir(directory)).sort()) {
      if (name.endsWith(extension)) {
        await requireFile(path.join(directory, name));
        installers.push(path.join(directory, name));
        found = true;
      }
    }
    if (!found) throw new Error(`missing required ${folder} installation file (${extension}) in ${directory}; publish every promised native bundle before packaging`);
  }
  const names = installers.map((filename) => path.basename(filename));
  if (new Set(names).size !== names.length) throw new Error(`duplicate installer names in ${bundle}`);
  if (platform !== "darwin") {
    const portable = path.join(input, "portable");
    await requireDirectory(portable);
    await requireFile(path.join(portable, platform === "win32" ? "stock-market-game.exe" : "stock-market-game.AppImage"), { executable: true });
    await requireFile(path.join(portable, "LICENSE"));
    if (!(await readFile(path.join(portable, "LICENSE"))).equals(await readFile(path.join(input, "LICENSE")))) throw new Error(`portable/LICENSE differs from input LICENSE: ${portable}`);
    return { installers, apps: [] };
  }
  const apps = [];
  const macos = path.join(bundle, "macos");
  if (await statIfPresent(macos) !== undefined) {
    await requireDirectory(macos);
    for (const name of (await readdir(macos)).sort()) {
      if (!name.endsWith(".app")) continue;
      const app = path.join(macos, name);
      await validateTree(app, { allowSymlinks: true });
      const executables = path.join(app, "Contents/MacOS");
      await requireDirectory(executables);
      const binaries = await readdir(executables);
      if (binaries.length === 0) throw new Error(`empty mac app executable directory: ${executables}`);
      for (const binary of binaries) await requireFile(path.join(executables, binary), { executable: true });
      apps.push(app);
    }
  }
  if (apps.length === 0) throw new Error(`missing required mac app bundle in ${macos}; publish both app and dmg before packaging`);
  return { installers, apps };
}

async function fileDigest(filename) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(filename)) hash.update(chunk);
  return hash.digest("hex");
}

async function publish(staged, output) {
  await rejectSymlinkParents(output);
  try {
    await mkdir(output);
  } catch (error) {
    if (error.code === "EEXIST") throw new Error(`output already exists; refusing to overwrite user files: ${output}`, { cause: error });
    throw error;
  }
  const published = [];
  try {
    for (const name of (await readdir(staged)).sort()) {
      const source = path.join(staged, name);
      const info = await requireFile(source);
      const destination = path.join(output, name);
      await copyFile(source, destination, constants.COPYFILE_EXCL);
      published.push(destination);
      await chmod(destination, info.mode & 0o777);
    }
  } catch (error) {
    try {
      for (const filename of published) await rm(filename);
      await rmdir(output);
    } catch (cleanupError) {
      throw new AggregateError([error, cleanupError], `publication failed: ${error.message}; rollback failed: ${cleanupError.message}; inspect ${output}`);
    }
    throw error;
  }
}

export async function packageDistributions(options, { root = process.cwd(), run = runArchiveCommand } = {}) {
  if (!products.has(options.product)) throw new Error(`unsupported product: ${options.product}`);
  const platform = validateTarget(options.target);
  const requestedRoot = path.resolve(root);
  const requestedInput = artifactPath(requestedRoot, options.input, "build-artifacts");
  const requestedOutput = artifactPath(requestedRoot, options.output, "distributions");
  root = await realpath(requestedRoot);
  const input = path.resolve(root, path.relative(requestedRoot, requestedInput));
  const output = path.resolve(root, path.relative(requestedRoot, requestedOutput));
  await rejectSymlinkParents(input);
  await rejectSymlinkParents(output);
  await requireDirectory(input);
  if (await statIfPresent(output) !== undefined) throw new Error(`output already exists; refusing to overwrite user files: ${output}`);
  await requireFile(path.join(input, "LICENSE"));
  let desktop;
  const binary = platform === "win32" ? "server.exe" : "server";
  if (options.product === "desktop") desktop = await desktopInputs(input, platform);
  else {
    await requireFile(path.join(input, binary), { executable: true });
    if (options.product === "webui-server") {
      await validateTree(path.join(input, "webui"));
      await requireFile(path.join(input, "webui/index.html"));
    }
  }
  await mkdir(path.dirname(output), { recursive: true });
  const staging = await mkdtemp(path.join(path.dirname(output), `.${path.basename(output)}-packaging-`));
  try {
    const payload = path.join(staging, "payload");
    const distributions = path.join(staging, "distributions");
    await mkdir(payload);
    await mkdir(distributions);
    const stem = `${options.product}-${options.target}`;
    const archive = (source, entries, suffix, format) => createArchive({ source, entries, output: path.join(distributions, `${stem}${suffix}.${format}`), format }, { run });
    if (options.product === "desktop") {
      await copyRegular(path.join(input, "LICENSE"), path.join(distributions, "LICENSE"));
      for (const installer of desktop.installers) await copyRegular(installer, path.join(distributions, path.basename(installer)));
      if (platform !== "darwin") {
        const portable = platform === "win32" ? "stock-market-game.exe" : "stock-market-game.AppImage";
        for (const name of [portable, "LICENSE"]) await copyRegular(path.join(input, "portable", name), path.join(payload, name));
        await archive(payload, [portable, "LICENSE"], "-portable", "zip");
      } else if (desktop.apps.length > 0) {
        for (const app of desktop.apps) await copyTree(app, path.join(payload, path.basename(app)));
        await copyRegular(path.join(input, "LICENSE"), path.join(payload, "LICENSE"));
        const entries = [...desktop.apps.map((app) => path.basename(app)), "LICENSE"];
        for (const format of ["zip", "tar.gz"]) await archive(payload, entries, "-app", format);
      }
    } else {
      for (const name of [binary, "LICENSE"]) await copyRegular(path.join(input, name), path.join(payload, name));
      const entries = [binary, "LICENSE"];
      if (options.product === "webui-server") {
        await copyTree(path.join(input, "webui"), path.join(payload, "webui"));
        entries.push("webui");
      }
      for (const format of ["zip", "tar.gz"]) await archive(payload, entries, "", format);
    }
    const files = [];
    for (const name of (await readdir(distributions)).sort()) {
      const filename = path.join(distributions, name);
      const info = await requireFile(filename);
      files.push({ name, bytes: info.size, sha256: await fileDigest(filename) });
    }
    const manifest = { schema: "distribution-manifest-v1", product: options.product, target: options.target, files };
    await writeFile(path.join(distributions, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`, { flag: "wx" });
    await publish(distributions, output);
    return manifest;
  } finally {
    await rm(staging, { recursive: true, force: true });
  }
}

export async function main(argv) {
  if (argv.length === 1 && ["--help", "-h"].includes(argv[0])) {
    console.log(`${usage}\nPackages existing publisher artifacts only; run from the workspace root. No build or compilation.\nOutput must not exist. Use scripts/run-long-validation.mjs 300000 -- node scripts/package-distributions.mjs ... for the external process-tree deadline.`);
    return;
  }
  const options = parseArgs(argv);
  const manifest = await packageDistributions(options);
  console.log(`Published ${manifest.files.length} distributions and manifest.json to ${options.output}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(`Distribution packaging failed: ${error.stack}; verify publisher input and system zip/tar (Windows: PowerShell and bsdtar), then report the command and full error.`);
    process.exitCode = 1;
  });
}
