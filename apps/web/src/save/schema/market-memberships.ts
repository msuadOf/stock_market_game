import { accountId, exact, money, record, SaveSchemaError } from "./primitives.ts"
import type { MarketMembershipState } from "../../types/generated/MarketMembershipState.ts"
import type { SaveSnapshot } from "./save-snapshot.ts"
import type { parseSetup } from "./market.ts"

export function parseMarketMemberships(value: unknown): MarketMembershipState {
  const path = "market_memberships"
  const parsed = record(value, path)
  exact(parsed, ["members"], path)
  const members = Object.fromEntries(Object.entries(record(parsed.members, `${path}.members`)).map(([subject, value]) => {
    const memberPath = `${path}.members.${subject}`
    if (subject.length === 0 || Array.from(subject).some((character) => {
      const code = character.codePointAt(0)!
      return code <= 0x1f || (code >= 0x7f && code <= 0x9f)
    })) throw new SaveSchemaError(memberPath, "认证主体必须非空且不含控制字符")
    const member = record(value, memberPath)
    exact(member, ["account_id", "admission_funding"], memberPath)
    const fundingPath = `${memberPath}.admission_funding`
    const funding = record(member.admission_funding, fundingPath)
    exact(funding, ["external_cash"], fundingPath)
    const external_cash = money(funding.external_cash, `${fundingPath}.external_cash`)
    if (BigInt(external_cash) < 0n) throw new SaveSchemaError(`${fundingPath}.external_cash`, "入场资金不能为负数")
    return [subject, { account_id: accountId(member.account_id, `${memberPath}.account_id`), admission_funding: { external_cash } }] as const
  }))
  return { members }
}

export function validateMembershipAccounts(memberships: MarketMembershipState, setup: ReturnType<typeof parseSetup>, snapshot: SaveSnapshot): void {
  const npcCount = BigInt(setup.npcs.retail_count) + BigInt(setup.npcs.inst_count) + BigInt(setup.npcs.hot_count)
  const members = new Set<string>()
  for (const [subject, member] of Object.entries(memberships.members)) {
    const account = member.account_id
    const id = BigInt(account)
    if (members.has(account) || !Object.hasOwn(snapshot.accounts, account) || (id > 0n && id <= npcCount)) throw new SaveSchemaError(`market_memberships.members.${subject}.account_id`, "成员账户必须唯一、存在且不与 NPC 账户重叠")
    members.add(account)
  }
  if (!members.has("0")) throw new SaveSchemaError("market_memberships.members", "初始账户 0 必须具有经济成员关系")
  const accounts = Object.keys(snapshot.accounts)
  if (BigInt(accounts.length) !== BigInt(members.size) + npcCount || accounts.some((account) => !members.has(account) && (BigInt(account) < 1n || BigInt(account) > npcCount))) throw new SaveSchemaError("snapshot.accounts", "账户集合必须恰好为 NPC 与市场成员账户")
}
