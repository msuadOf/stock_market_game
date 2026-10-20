/**
 * 公司数值身份（CompanyId，ADR 0041）与字符串代码的唯一前端翻译表。
 *
 * 引擎侧 `IssuerRegistry`（packages/engine/src/company/identity.rs）经
 * `CompanyCatalog::from_metas`（packages/engine/src/company/spec.rs）按代码字符串
 * 升序分配 `CompanyId = 0..n`；Rust `String` 的 `Ord` 是字节序，JavaScript
 * `Array#sort()` 默认按 UTF-16 码点序比较，对 ASCII 前缀 `C-` 的公司代码两者
 * 完全一致，因此本表用 `publicCompanies` 代码去重后排序取下标，与引擎的 id
 * 分配逐一对齐（与 `config/stock-ids.ts` 同式）。
 *
 * 简化边界（与 STOCK_LIST 同款）：公司目录当前不从 wire 下发，本表锚定
 * `publicCompanies` 静态目录 = 引擎 `default_companies` 全集；引擎默认公司集
 * 变更时需同步本表（测试钉住代码集与排序）。
 *
 * 未知代码 / 未知 id 一律显式抛错（铁律二），绝不静默回退。
 */
import { publicCompanies } from "../components/company/company-catalog.ts";
import type { CompanyId } from "../types/generated/CompanyId.ts";

const sortedCodes = [...new Set(publicCompanies.map((company) => company.id))].sort();

/** 公司代码串 → 数值 CompanyId。 */
export const companyIdByCode = new Map<string, CompanyId>(sortedCodes.map((code, index) => [code, index]));

/** 数值 CompanyId → 公司代码串。 */
export const codeByCompanyId = new Map<CompanyId, string>(sortedCodes.map((code, index) => [index, code]));

/** 公司代码串转数值 CompanyId；未知代码显式抛错。 */
export function companyIdOf(code: string): CompanyId {
  const id = companyIdByCode.get(code);
  if (id === undefined) {
    throw new Error(`未知公司代码：${code}（不在 publicCompanies 目录内）`);
  }
  return id;
}

/** 数值 CompanyId 转公司代码串；未知或非整数 id 显式抛错。 */
export function codeOfCompany(id: CompanyId): string {
  const code = codeByCompanyId.get(id);
  if (code === undefined) {
    throw new Error(`未知公司 CompanyId：${id}（超出 publicCompanies 目录范围）`);
  }
  return code;
}
