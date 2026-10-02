import { appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

export function isReleaseTag(tag) {
  return typeof tag === "string" && tag === tag.trim() && /^(?:v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)|test-[A-Za-z0-9][A-Za-z0-9_-]*)$/.test(tag);
}

export function parseReleaseTag(tag) {
  if (!isReleaseTag(tag)) {
    throw new Error(`invalid release tag: ${JSON.stringify(tag)}; expected vMAJOR.MINOR.PATCH or test-SUFFIX`);
  }
  return { tag, prerelease: tag.startsWith("test-") };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { tag, prerelease } = parseReleaseTag(process.env.RELEASE_TAG);
  if (!/^[a-f0-9]{40}$/.test(process.env.GITHUB_SHA)) throw new Error("invalid release commit SHA");
  appendFileSync(process.env.GITHUB_OUTPUT, `tag=${tag}\nprerelease=${prerelease}\nsha=${process.env.GITHUB_SHA}\n`);
}
