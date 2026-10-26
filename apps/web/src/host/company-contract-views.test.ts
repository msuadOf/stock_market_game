import assert from 'node:assert/strict';
import test from 'node:test';
import { parseCompanyCapabilities, parseOwnerRightsOfferings, parsePeriodChangeExplanation } from './company-contract-views.ts';

const kinds = ['CashDividend', 'StockDistribution', 'RightsOffering', 'IssuerRepurchase', 'ShareSplit'];
const capabilities = () => ({ revenue: true, net_income: true, equity: true, cash_flow: false, full_financial_statements: false, cash_settlement: false, unsupported_reason: '实际投资者结算尚未接线', par_value_per_share: {Available: {cents: '100'}}, issued_shares: '18446744073709551615', registered_capital: {Available: {amount_yuan: '12345678901234567890.12'}}, distributable_profit: {Unavailable: {reason: '尚无可分配利润快照'}}, active_plans: [{kind: 'RightsOffering', identity: 'r1', stage: 'Entitled', key_dates: [{label: '登记日', date: '2030-02-28'}]}], action_readiness: kinds.map(kind => ({kind, ready: false, blockers: ['结算未接线']})), owner_rights: [{event_id:'r1', stock:'600101', stage:'Entitled', payment_window:'Open', entitled_shares:null, open_subscription_remaining:'0'}] });
const rights = () => ({event_id:'r1', stock:'600101', issuer: 4294967295, stage:'Entitled', payment_window:'Open', price_per_share:'100', payment_start_on:'2030-03-01', payment_deadline_on:'2030-03-04', ex_rights_on:'2030-03-05', settlement_on:'2030-03-06', owner_entitlement:{rights_shares:'9007199254740993',lock_until:null}, open_subscription_remaining_shares:null, queued_subscription:{requested_shares:'3',submitted_on:'2030-03-02'}, settled_subscription:{requested_shares:'10',paid_shares:'8',paid_amount:'800',waived_shares:'2'}});
const period = () => ({previous:{revenue:'9007199254740993.01',fixed_expense:'0.00',variable_expense:'-1.00'},cycle:'Quarterly',environment_change_bp:-2147483648,demand_contribution_bp:2147483647,revenue_segments:[{annual_growth_bp:-10000,months:3}],fixed_expense_segments:[],variable_expense_segments:[],revenue_noise_bp:0,fixed_expense_noise_bp:0,variable_expense_noise_bp:0,restart_revenue:null,restart_source:null});

test('能力面精确保留股数、不可用原因、五类readiness与实际settlement位', {timeout:10000}, () => {
  assert.deepEqual(parseCompanyCapabilities(capabilities()), capabilities());
  assert.deepEqual(parseCompanyCapabilities({...capabilities(),cash_settlement:true,unsupported_reason:''}), {...capabilities(),cash_settlement:true,unsupported_reason:''});
});
test('能力面拒绝重复readiness、缺多键、空原因、未知enum及金额越界', {timeout:10000}, () => {
  const c=capabilities();
  const cases:unknown[]=[{...c,action_readiness:[...c.action_readiness.slice(0,4),c.action_readiness[0]]},{...c,action_readiness:c.action_readiness.slice(0,4)},{...c,extra:1},{...c,issued_shares:'18446744073709551616'},{...c,issued_shares:'01'},{...c,issued_shares:'0'},{...c,unsupported_reason:''},{...c,registered_capital:{Available:{amount_yuan:'1701411834604692317316873037158841057.28'}}},{...c,par_value_per_share:{Available:{cents:'9223372036854775808'}}},{...c,distributable_profit:{Unavailable:{reason:' '}}},{...c,active_plans:[{...c.active_plans[0],stage:'Unknown'}]},{...c,active_plans:[{...c.active_plans[0],key_dates:[{label:'日期',date:'2030-02-30'}]}]}];
  const missing={...c} as Record<string,unknown>; delete missing.owner_rights; cases.push(missing);
  for(const value of cases) assert.throws(()=>parseCompanyCapabilities(value));
});
test('配股query使用数值CompanyId、字符串StockCode与显式nullable', {timeout:10000}, () => {
  assert.deepEqual(parseOwnerRightsOfferings([rights()]), [rights()]);
  for(const patch of [{issuer:'C-1'},{issuer:4294967296},{stock:0},{price_per_share:'0'},{price_per_share:'-1'},{owner_entitlement:{rights_shares:'0',lock_until:null}},{queued_subscription:{requested_shares:'0',submitted_on:'2030-03-02'}},{settled_subscription:{requested_shares:'10',paid_shares:'8',paid_amount:'800',waived_shares:'3'}},{payment_start_on:'1899-12-31'},{settlement_on:'2200-01-01'},{owner_entitlement:undefined},{extra:true}]) assert.throws(()=>parseOwnerRightsOfferings([{...rights(),...patch}]));
  assert.deepEqual(parseOwnerRightsOfferings([{...rights(),owner_entitlement:null,queued_subscription:null,settled_subscription:null,open_subscription_remaining_shares:'0'}]),[{...rights(),owner_entitlement:null,queued_subscription:null,settled_subscription:null,open_subscription_remaining_shares:'0'}]);
});
test('期间解释遵守i32/u8/会计i128值域并保留null与大额', {timeout:10000}, () => {
  assert.deepEqual(parsePeriodChangeExplanation(period()),period());
  for(const patch of [{environment_change_bp:2147483648},{demand_contribution_bp:-2147483649},{cycle:'Weekly'},{restart_revenue:undefined},{restart_source:undefined},{previous:{...period().previous,revenue:'-0.00'}},{revenue_segments:[{annual_growth_bp:-10001,months:3}]},{revenue_segments:[{annual_growth_bp:1,months:256}]},{revenue_segments:[{annual_growth_bp:1,months:0}]},{previous:{...period().previous,revenue:'1701411834604692317316873037158841057.28'}}]) assert.throws(()=>parsePeriodChangeExplanation({...period(),...patch}));
});

test('金额保持元和分边界，嵌套契约缺键多键与非规范数字明确拒绝', { timeout: 10000 }, () => {
  const amount = { Available: { accumulated_after_loss_yuan: '-1701411834604692317316873037158841057.28', statutory_reserve_yuan: '0.00', available_for_distribution_yuan: '1701411834604692317316873037158841057.27', reserve_basis_year: -2147483648 } };
  const valid = { ...capabilities(), par_value_per_share: { Available: { cents: '9223372036854775807' } }, distributable_profit: amount };
  assert.deepEqual(parseCompanyCapabilities(valid), valid);
  for (const patch of [
    { par_value_per_share: { Available: { cents: '01' } } },
    { par_value_per_share: { Available: { cents: '0' } } },
    { registered_capital: { Available: { amount_yuan: '01.00' } } },
    { registered_capital: { Available: { amount_yuan: '-1701411834604692317316873037158841057.29' } } },
    { distributable_profit: { Available: { ...amount.Available, reserve_basis_year: 2147483648 } } },
    { distributable_profit: { Available: { ...amount.Available, extra: true } } },
    { action_readiness: kinds.map(kind => ({ kind, ready: true, blockers: [] })) .map((row, i) => i === 0 ? { ...row, extra: true } : row) },
    { owner_rights: [{ ...capabilities().owner_rights[0], entitled_shares: '01' }] },
    { owner_rights: [{ ...capabilities().owner_rights[0], stock: 0 }] },
  ]) assert.throws(() => parseCompanyCapabilities({ ...valid, ...patch }));
  assert.throws(() => parseCompanyCapabilities({ ...valid, distributable_profit: { Available: { accumulated_after_loss_yuan: '0.00' } } }), /distributable_profit/);
  assert.throws(() => parseOwnerRightsOfferings([{ ...rights(), owner_entitlement: { rights_shares: '1', lock_until: '2200-01-01' } }]), /lock_until/);
  assert.throws(() => parsePeriodChangeExplanation({ ...period(), revenue_segments: [{ annual_growth_bp: 0, months: 1, extra: 0 }] }), /revenue_segments/);
});
