import {readFileSync,writeFileSync} from "node:fs";
import {createHash} from "node:crypto";
import {parseSaveJson} from "../../../../apps/web/src/save/save-schema.ts";
const names=["generated-company-state-registry.json","manual-company-state-registry.json","legacy-company-state.json","generated-custom-monthly-nonempty.json","manual-custom-monthly-nonempty.json","generated-quarterly-nonempty.json","manual-quarterly-nonempty.json","nonempty-legacy-group.json"];
const sha=value=>createHash("sha256").update(value).digest("hex");
const receipts=names.map(name=>{const input=readFileSync(new URL(`../../../../apps/web/src/save/fixtures/source-b/${name}`,import.meta.url));const output=JSON.stringify(parseSaveJson(input.toString("utf8")));writeFileSync(new URL(name,import.meta.url),output);return {name,inputSha256:sha(input),webOutputSha256:sha(output),webOutputBytes:Buffer.byteLength(output)};});
writeFileSync(new URL("receipt.json",import.meta.url),JSON.stringify({sourceRust:"04cae",scope:"Web parseSaveJson→JSON.stringify，等待04cae真实Rust reader回读",receipts},null,2)+"\n");
