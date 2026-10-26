// 实际组合 Web parser 双向全档见证；不删除字段或改变生产对象。
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {pathToFileURL} from 'node:url';
const [repo,input,output] = process.argv.slice(2);
const {parseSaveSlot} = await import(pathToFileURL(repo+'/apps/web/src/save/save-schema.ts').href);
const bytes = await fs.readFile(input);
const original = JSON.parse(bytes.toString('utf8'));
const parsed = parseSaveSlot(original);
assert.deepStrictEqual(parsed,original);
const encoded = JSON.stringify(parsed);
const reparsed = parseSaveSlot(JSON.parse(encoded));
assert.deepStrictEqual(reparsed,parsed);
assert(parsed.plans.owner_sequences.length>=2);
await fs.writeFile(output,encoded);
console.log(JSON.stringify({input_bytes:bytes.length,input_sha256:createHash('sha256').update(bytes).digest('hex'),output_bytes:Buffer.byteLength(encoded),output_sha256:createHash('sha256').update(encoded).digest('hex'),full_object_deep_equal:true,stringify_reparse_deep_equal:true,owner_count:parsed.plans.owner_sequences.length,plan_count:parsed.plans.plans.length}));
