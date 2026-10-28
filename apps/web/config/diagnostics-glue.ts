import { lstat, readFile } from "node:fs/promises";
import { isAbsolute, join, resolve } from "node:path";
import ts from "typescript";
import type { Plugin } from "vite";

export const ENGINE_ENV_BROWSER_URL = "/src/host/engine-clock-env.ts";
export const DIAGNOSTICS_GLUE_URL = "/wasm-diagnostics-pkg/web_wasm.js";

function explicitPackageDirectory(value: string): string {
  if (value.length === 0 || value.trim() !== value || value.includes("\0") || !isAbsolute(value) || resolve(value) !== value) {
    throw new Error("STOCK_GAME_DIAGNOSTICS_PKG 必须是无首尾空白、无 traversal 的规范绝对目录");
  }
  return value;
}

async function packageGlue(directory: string): Promise<string> {
  const directoryStat = await lstat(directory);
  if (directoryStat.isSymbolicLink() || !directoryStat.isDirectory()) {
    throw new Error("diagnostics package 目录不能是符号链接或普通文件");
  }
  const filename = join(directory, "web_wasm.js");
  const glueStat = await lstat(filename);
  if (glueStat.isSymbolicLink() || !glueStat.isFile() || glueStat.size === 0) {
    throw new Error("diagnostics glue 必须是非空普通文件，不能是符号链接");
  }
  return filename;
}

function rewriteEnvironmentImport(source: string): string {
  const parsed = ts.createSourceFile("web_wasm.js", source, ts.ScriptTarget.Latest, false, ts.ScriptKind.JS);
  const specifiers = parsed.statements.flatMap((statement) => ts.isImportDeclaration(statement)
    && ts.isStringLiteral(statement.moduleSpecifier) && statement.moduleSpecifier.text === "env"
    ? [statement.moduleSpecifier] : []);
  if (specifiers.length === 0) throw new Error("diagnostics glue 缺少真实静态 bare env import，无法接入产品 singleton");
  // 仅替换 AST 中完整静态 moduleSpecifier 的内容；注释、字符串和相似模块名保持原字节。
  for (const specifier of specifiers.reverse()) {
    source = source.slice(0, specifier.getStart(parsed) + 1) + ENGINE_ENV_BROWSER_URL + source.slice(specifier.getEnd() - 1);
  }
  return source;
}

/** @vite-ignore 的诊断入口绕过 import analysis；仅在 DEV 为它接回产品唯一 env 模块。 */
export function diagnosticsGluePlugin(options: { readonly packageDirectory?: string } = {}): Plugin {
  return {
    name: "diagnostics-glue-env",
    apply: "serve",
    async configureServer(server) {
      if (server.config.env.DEV !== true) return;
      const directory = options.packageDirectory === undefined
        ? join(server.config.root, "wasm-diagnostics-pkg")
        : explicitPackageDirectory(options.packageDirectory);
      // 显式配置应在启动时失败；默认目录按请求读取，不要求普通 DEV 启动准备诊断包。
      if (options.packageDirectory !== undefined) await packageGlue(directory);
      server.middlewares.use(async (request, response, next) => {
        if ((request.method !== "GET" && request.method !== "HEAD")
          || request.url?.split("?")[0] !== DIAGNOSTICS_GLUE_URL) {
          next();
          return;
        }
        try {
          const filename = await packageGlue(directory);
          const body = rewriteEnvironmentImport(await readFile(filename, "utf8"));
          response.setHeader("Content-Type", "text/javascript; charset=utf-8");
          response.setHeader("Cache-Control", "no-store");
          response.end(request.method === "HEAD" ? "" : body);
        } catch (cause) {
          const detail = cause instanceof Error ? cause.message : "非 Error 异常，原始详情保留在 cause";
          next(new Error(`diagnostics glue 分发失败：${detail}`, { cause }));
        }
      });
    },
  };
}
