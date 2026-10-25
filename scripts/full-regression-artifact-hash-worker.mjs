import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { parentPort } from "node:worker_threads";

parentPort.on("message", async ({ executable }) => {
  try {
    const hash = createHash("sha256");
    let bytes = 0;
    // 固定 chunk，内存随并发数增长，不随 binary 总大小增长。
    for await (const chunk of createReadStream(executable, { highWaterMark: 65536 })) {
      hash.update(chunk);
      bytes += chunk.length;
    }
    parentPort.postMessage({ bytes, sha256: hash.digest("hex") });
  } catch (error) {
    parentPort.postMessage({ error: { message: error.message, code: error.code } });
  }
});
