// A 的 response取证：创建任务时立即处理拒绝，原错误仍留给最终失败汇合。
import { lstatSync, readFileSync } from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
export function errorRecord(error, seen = new Set()) {
  if (!(error instanceof Error)) return { message: String(error) };
  if (seen.has(error)) return { circular: true };
  seen.add(error);
  return { name: error.name, message: error.message, stack: error.stack,
    ...(error.cause === undefined ? {} : { cause: errorRecord(error.cause, seen) }) };
}
export function captureResponse(response, served, wasmEvidence, metadata = {}, glueEvidence) {
  const url = response.url(), status = response.status();
  return (async () => {
    if (new URL(url).pathname === WASM_PATH) {
      if (wasmEvidence === undefined) throw new Error("WASM缺少固定server-response-buffer取证");
      served.push({ ...metadata, ...await wasmEvidence.capture(response) });
    } else if (glueEvidence !== undefined && new URL(url).pathname === "/wasm-diagnostics-pkg/web_wasm.js") {
      served.push({ ...metadata, ...await glueEvidence.capture(response) });
    } else {
      const bytes = await response.body();
      served.push({ ...metadata, url, status, evidenceSource: "browser-response-body", bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") });
    }
  })().then(() => ({ status: "fulfilled" }), (reason) => ({ status: "rejected", reason, context: { url, status, error: errorRecord(reason) } }));
}

const WASM_PATH = "/wasm-diagnostics-pkg/web_wasm_bg.wasm";
const ID_HEADER = "x-w1-wasm-response-id";
function requireEvidence(condition, message) { if (!condition) throw new Error(message); }
export function createWasmResponseEvidence(artifact, expected) {
  const records = [], errors = [];
  return {
    records, errors,
    handle(request, response) {
      if (new URL(request.url, "http://localhost").pathname !== WASM_PATH) return false;
      const record = { requestId: `w1-${records.length + 1}`, url: new URL(request.url, `http://${request.headers.host}`).href,
        method: request.method, status: 200, finished: false, matched: false };
      records.push(record);
      response.on("finish", () => { record.finished = true; });
      response.on("close", () => { if (!record.finished) { record.aborted = true; errors.push(new Error(`WASM响应中止：${record.requestId}`)); } });
      response.on("error", (error) => { errors.push(error); record.error = errorRecord(error); });
      try {
        requireEvidence(request.method === "GET", "WASM只允许完整GET");
        requireEvidence(request.headers.range === undefined, "WASM不允许Range代替完整GET");
        const file = path.join(artifact, "web_wasm_bg.wasm"), stat = lstatSync(file);
        requireEvidence(stat.isFile() && !stat.isSymbolicLink(), "WASM必须为非link普通文件");
        const buffer = readFileSync(file);
        record.bytes = buffer.length; record.sha256 = createHash("sha256").update(buffer).digest("hex");
        requireEvidence(record.bytes === expected.bytes && record.sha256 === expected.sha256, "WASM实际发送Buffer与artifact seal不符");
        response.statusCode = 200;
        response.setHeader("Content-Type", "application/wasm");
        response.setHeader("Content-Length", String(buffer.length));
        response.setHeader("Cache-Control", "no-store");
        response.setHeader("Cross-Origin-Opener-Policy", "same-origin");
        response.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
        response.setHeader(ID_HEADER, record.requestId);
        // hash对应这次res.end的同一个Buffer，不能称为独立browser body hash。
        response.end(buffer);
      } catch (error) {
        errors.push(error); record.error = errorRecord(error); record.status = 500;
        response.statusCode = 500; response.end("WASM完整GET取证失败");
      }
      return true;
    },
    async capture(response) {
      const headers = response.headers(), requestId = headers[ID_HEADER];
      const record = records.find((item) => item.requestId === requestId);
      requireEvidence(record !== undefined, "WASM browser响应缺少唯一server映射");
      requireEvidence(!record.matched && record.error === undefined, "WASM重复映射或server响应失败");
      requireEvidence(response.url() === record.url && response.request().method() === "GET" && record.method === "GET" && response.status() === 200 && record.status === 200, "WASM URL/method/status错配");
      requireEvidence(headers["content-length"] === String(record.bytes) && headers["content-type"] === "application/wasm", "WASM响应Length/type错配");
      const finishedError = await response.finished();
      requireEvidence(finishedError === null, `WASM browser请求未正常完成：${String(finishedError)}`);
      requireEvidence(record.finished && !record.aborted, "WASM server响应中止或尚未完成");
      requireEvidence(record.bytes === expected.bytes && record.sha256 === expected.sha256, "WASM server Buffer seal漂移");
      // await期间另一response不能复用同一ID。
      requireEvidence(!record.matched, "WASM重复browser映射"); record.matched = true;
      return { url: record.url, status: 200, evidenceSource: "server-response-buffer", directBrowserWasmBodySha256Proven: false,
        serverResponseBuffer: { ...record }, browserMetadata: { url: response.url(), method: "GET", status: 200, requestId,
          contentLength: headers["content-length"], contentType: headers["content-type"], finishedWithoutError: true } };
    },
    verify() {
      const missing = records.filter((record) => !record.finished || record.aborted || !record.matched);
      if (records.length === 0 || errors.length || missing.length) throw new AggregateError(errors, `WASM响应缺少、未完成或缺少唯一映射：${JSON.stringify(missing)}`);
    },
  };
}

export function createGlueResponseEvidence() {
  const records = [], errors = [], idHeader = "x-w1-glue-response-id";
  function fail(record, error) { record.error = errorRecord(error); errors.push(error); }
  return {
    records, errors,
    observe(request, response) {
      if (new URL(request.url, "http://localhost").pathname !== "/wasm-diagnostics-pkg/web_wasm.js") return;
      const record = { requestId: `g-${records.length + 1}`, url: new URL(request.url, `http://${request.headers.host}`).href,
        method: request.method, bytes: 0, finished: false, matched: false };
      const hash = createHash("sha256"); records.push(record);
      response.setHeader(idHeader, record.requestId);
      if (request.method !== "GET" || request.headers.range !== undefined) fail(record, new Error("glue只接受无Range的完整GET"));
      for (const method of ["write", "end"]) {
        const original = response[method];
        response[method] = function (...args) {
          // 仅观察原调用的实际chunk，不改this、参数、encoding、回调、返回值或原异常。
          try {
            const chunk = args[0];
            if (typeof chunk === "string" || chunk instanceof Uint8Array) {
              const bytes = typeof chunk === "string" ? Buffer.from(chunk, typeof args[1] === "string" ? args[1] : "utf8") : Buffer.from(chunk);
              hash.update(bytes); record.bytes += bytes.length;
            }
          } catch (error) { fail(record, error); }
          try { return Reflect.apply(original, this, args); }
          catch (error) { fail(record, error); throw error; }
        };
      }
      response.on("finish", () => {
        try {
          record.sha256 = hash.digest("hex"); record.status = response.statusCode;
          record.contentType = String(response.getHeader("Content-Type"));
          record.declaredContentLength = response.getHeader("Content-Length"); record.finished = true;
          requireEvidence(response.getHeader(idHeader) === record.requestId, "glue响应ID被替换");
          if (record.declaredContentLength !== undefined) requireEvidence(String(record.declaredContentLength) === String(record.bytes), "glue声明Length与发送字节错配");
          requireEvidence(response.getHeader("Content-Encoding") === undefined || response.getHeader("Content-Encoding") === "identity", "glue观察不接受压缩后字节冒充实际发送body");
        } catch (error) { fail(record, error); }
      });
      response.on("close", () => { if (!record.finished) { record.aborted = true; fail(record, new Error("glue响应中止")); } });
      response.on("error", (error) => fail(record, error));
    },
    async capture(response) {
      const headers = response.headers(), requestId = headers[idHeader];
      const record = records.find((item) => item.requestId === requestId);
      requireEvidence(record !== undefined && !record.matched, "glue缺少或重复唯一ID映射");
      requireEvidence(record.url === response.url() && record.method === "GET" && response.request().method() === "GET" && response.status() === 200, "glue URL/GET/status错配");
      const finishedError = await response.finished();
      requireEvidence(finishedError === null, `glue browser请求未完成：${String(finishedError)}`);
      requireEvidence(record.finished && !record.aborted && record.error === undefined && record.status === 200, "glue server发送未完成或失败");
      if (headers["content-length"] !== undefined) requireEvidence(headers["content-length"] === String(record.bytes), "glue已有Length与实际字节错配");
      if (record.declaredContentLength !== undefined) requireEvidence(String(record.declaredContentLength) === headers["content-length"], "glue声明Length与browser headers错配");
      requireEvidence(headers["content-type"] === record.contentType && /^(?:text|application)\/javascript(?:;|$)/.test(record.contentType), "glue Content-Type错配");
      requireEvidence(!record.matched, "glue重复browser响应映射"); record.matched = true;
      return { url: record.url, status: record.status, evidenceSource: "server-response-observer", directBrowserGlueBodySha256Proven: false,
        serverResponseObserver: { ...record }, browserMetadata: { requestId, url: response.url(), method: "GET", status: 200,
          contentLength: headers["content-length"] === undefined ? null : headers["content-length"], contentType: headers["content-type"], finishedWithoutError: true } };
    },
    verify() {
      const missing = records.filter((record) => !record.finished || record.aborted || !record.matched);
      if (!records.length || errors.length || missing.length) throw new AggregateError(errors, `glue缺少、未完成或未唯一映射：${JSON.stringify(missing)}`);
    },
  };
}
