import assert from "node:assert/strict";
import test from "node:test";
import { publicCompanies } from "../components/company/company-catalog.ts";
import { codeByCompanyId, codeOfCompany, companyIdByCode, companyIdOf } from "./company-ids.ts";

const options = { concurrency: true, timeout: 10000 };

test("公司目录表按代码升序对齐引擎 CompanyId 分配且翻译闭环", options, () => {
  // 引擎 CompanyCatalog::from_metas 按代码升序分配 0..n（packages/engine/src/company/spec.rs）；
  // 前端目录与默认公司集（engine defaults.rs）一致时，排序取下标必须逐一对齐。
  const sortedCodes = [...new Set(publicCompanies.map((company) => company.id))].sort();
  assert.deepEqual(sortedCodes, ["C-000812", "C-002156", "C-300260", "C-600101", "C-600610", "C-TEST-BANK", "C-TEST-IND", "C-TEST-INS", "C-TEST-RE"]);
  for (const [index, code] of sortedCodes.entries()) {
    assert.equal(companyIdOf(code), index);
    assert.equal(codeOfCompany(index), code);
  }
  assert.equal(companyIdByCode.size, sortedCodes.length);
  assert.equal(codeByCompanyId.size, sortedCodes.length);
});

test("未知公司代码或数值 id 显式拒绝，不静默回退", options, () => {
  assert.throws(() => companyIdOf("C-999999"), /未知公司代码/);
  assert.throws(() => codeOfCompany(sortedCodesLength()), /未知公司 CompanyId/);
});

function sortedCodesLength(): number {
  return new Set(publicCompanies.map((company) => company.id)).size;
}
