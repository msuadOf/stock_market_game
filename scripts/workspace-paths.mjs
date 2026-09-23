import { constants } from "node:fs";
import { access, lstat, mkdir, readFile, realpath } from "node:fs/promises";
import path from "node:path";

async function statRequired(target, context) {
  try {
    return await lstat(target);
  } catch (error) {
    throw new Error(`${context} is unavailable at ${target}: ${error.message}`);
  }
}

export async function resolveWorkspaceRoot(sourceRoot) {
  const resolvedSource = await realpath(path.resolve(sourceRoot));
  const gitMarker = path.join(resolvedSource, ".git");
  const markerStat = await statRequired(gitMarker, "Git metadata");
  if (markerStat.isSymbolicLink()) throw new Error(`Git metadata must not be a symbolic link: ${gitMarker}`);
  if (markerStat.isDirectory()) return resolvedSource;
  if (!markerStat.isFile()) throw new Error(`Git metadata must be a directory or worktree pointer: ${gitMarker}`);

  const match = /^gitdir:\s*(.+)\s*$/i.exec(await readFile(gitMarker, "utf8"));
  if (!match) throw new Error(`linked worktree metadata is malformed: ${gitMarker}`);
  const gitDir = await realpath(path.resolve(resolvedSource, match[1]));
  const commonDirPath = path.join(gitDir, "commondir");
  let commonDir = gitDir;
  try {
    const configured = (await readFile(commonDirPath, "utf8")).trim();
    if (!configured) throw new Error("commondir is empty");
    commonDir = await realpath(path.resolve(gitDir, configured));
  } catch (error) {
    if (error?.code !== "ENOENT") throw new Error(`linked worktree common directory is invalid: ${error.message}`);
  }
  if (path.basename(commonDir) !== ".git") throw new Error(`Git common directory must end in .git: ${commonDir}`);
  return realpath(path.dirname(commonDir));
}

async function ensureDirectoryChain(root, target) {
  const relative = path.relative(root, target);
  if (!relative || relative.startsWith("..") || path.isAbsolute(relative)) throw new Error(`workspace path must be a strict child of ${root}: ${target}`);
  let current = root;
  for (const part of relative.split(path.sep)) {
    current = path.join(current, part);
    let stat;
    try {
      stat = await lstat(current);
    } catch (error) {
      if (error?.code !== "ENOENT") throw error;
      await mkdir(current);
      stat = await lstat(current);
    }
    if (stat.isSymbolicLink()) throw new Error(`workspace path must not contain a symbolic link: ${current}`);
    if (!stat.isDirectory()) throw new Error(`workspace path component is not a directory: ${current}`);
  }
  const canonical = await realpath(target);
  const canonicalRelative = path.relative(root, canonical);
  if (!canonicalRelative || canonicalRelative.startsWith("..") || path.isAbsolute(canonicalRelative)) throw new Error(`workspace path escaped its root: ${canonical}`);
  await access(canonical, constants.W_OK);
  return canonical;
}

export async function prepareWorkspacePaths({ sourceRoot, scope, workspaceRoot }) {
  if (typeof scope !== "string" || !/^[a-z0-9][a-z0-9-]*$/.test(scope)) throw new Error(`workspace path scope is invalid: ${scope}`);
  const resolvedWorkspaceRoot = workspaceRoot === undefined
    ? await resolveWorkspaceRoot(sourceRoot)
    : await realpath(path.resolve(workspaceRoot));
  const cargoTargetDir = await ensureDirectoryChain(resolvedWorkspaceRoot, path.join(resolvedWorkspaceRoot, ".tmp", "build-cache", scope));
  const processTmpDir = await ensureDirectoryChain(resolvedWorkspaceRoot, path.join(resolvedWorkspaceRoot, ".tmp", "process-tmp", scope));
  return { workspaceRoot: resolvedWorkspaceRoot, cargoTargetDir, processTmpDir };
}

export async function validateWorkspaceOutputPath(workspaceRoot, outputPath) {
  const resolvedWorkspaceRoot = await realpath(path.resolve(workspaceRoot));
  const tmpRoot = path.join(resolvedWorkspaceRoot, ".tmp");
  const tmpStat = await statRequired(tmpRoot, "workspace temporary root");
  if (tmpStat.isSymbolicLink() || !tmpStat.isDirectory()) throw new Error(`workspace .tmp must be a real directory: ${tmpRoot}`);
  const canonicalTmpRoot = await realpath(tmpRoot);
  const candidate = path.resolve(outputPath);
  const relative = path.relative(canonicalTmpRoot, candidate);
  if (!relative || relative.startsWith("..") || path.isAbsolute(relative)) {
    throw new Error(`validation output must be a strict child of workspace .tmp: ${candidate}`);
  }
  let current = canonicalTmpRoot;
  for (const part of relative.split(path.sep)) {
    current = path.join(current, part);
    let stat;
    try {
      stat = await lstat(current);
    } catch (error) {
      if (error?.code === "ENOENT") break;
      throw error;
    }
    if (stat.isSymbolicLink()) throw new Error(`validation output path must not contain a symbolic link: ${current}`);
    if (!stat.isDirectory()) throw new Error(`validation output path component is not a directory: ${current}`);
    const canonical = await realpath(current);
    const canonicalRelative = path.relative(canonicalTmpRoot, canonical);
    if (!canonicalRelative || canonicalRelative.startsWith("..") || path.isAbsolute(canonicalRelative)) {
      throw new Error(`validation output path escaped workspace .tmp: ${canonical}`);
    }
  }
  return candidate;
}
