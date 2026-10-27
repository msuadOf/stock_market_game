import assert from "node:assert/strict";
import test from "node:test";
import { parseGroups } from "./groups.ts";

test("GroupStructure的CompanyId为独立u32，两个真实母子持股结构无损", {timeout:10000}, () => {
 const groups=[{root:0,holdings:[{company:1,parent_held_shares:80000}]},{root:2,holdings:[{company:3,parent_held_shares:80000}]}];
 assert.deepEqual(parseGroups(groups),groups);
 for(const value of ["0",-0,-1,4294967296,1.5,NaN]){assert.throws(()=>parseGroups([{root:value,holdings:[]}]));assert.throws(()=>parseGroups([{root:0,holdings:[{company:value,parent_held_shares:80000}]}]));}
 assert.throws(()=>parseGroups([{root:0,holdings:[{company:0,parent_held_shares:80000}]}]),/重复/);
});
