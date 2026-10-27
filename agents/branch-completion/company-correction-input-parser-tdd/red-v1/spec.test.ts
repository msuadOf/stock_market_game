import assert from "node:assert/strict";
import test from "node:test";
import { parseCompanyReportCorrection, parseCompanyReportCorrectionJson, parseReportCorrectionStatus, parseReportCorrectionStatusJson } from "./report-corrections.ts";
const fixture = {
  "company": 3,
  "entries": [
    {
      "cash_flow": "Operating",
      "date": "2030-06-03",
      "kind": "CashRevenue",
      "lines": [
        {
          "account": "1002",
          "amount": "10.00",
          "side": "Debit"
        },
        {
          "account": "6001",
          "amount": "10.00",
          "side": "Credit"
        }
      ],
      "source": 900000001
    }
  ],
  "operation_id": "correction-2030-06-a",
  "reason": "补记前期已经收现收入",
  "supersedes": 9
};
const amountProbe: { input: string; accepted: boolean }[] = [
  {
    "accepted": true,
    "input": "  +10.0 ",
    "normalized": "10.00"
  },
  {
    "accepted": true,
    "input": "0010",
    "normalized": "10.00"
  },
  {
    "accepted": true,
    "input": "-0",
    "normalized": "0.00"
  },
  {
    "accepted": true,
    "input": "+0.00",
    "normalized": "0.00"
  },
  {
    "accepted": true,
    "input": "10.00",
    "normalized": "10.00"
  },
  {
    "accepted": false,
    "error": "decimal parse failed: input \"10.\": dangling decimal point",
    "input": "10."
  },
  {
    "accepted": false,
    "error": "decimal parse failed: input \".5\": integer part must be all digits",
    "input": ".5"
  },
  {
    "accepted": false,
    "error": "decimal parse failed: input \"1.234\": too many fractional digits: 3",
    "input": "1.234"
  },
  {
    "accepted": true,
    "input": "1701411834604692317316873037158841057.27",
    "normalized": "1701411834604692317316873037158841057.27"
  },
  {
    "accepted": true,
    "input": "-1701411834604692317316873037158841057.27",
    "normalized": "-1701411834604692317316873037158841057.27"
  },
  {
    "accepted": false,
    "error": "decimal parse failed: input \"-1701411834604692317316873037158841057.28\": value out of i128 range",
    "input": "-1701411834604692317316873037158841057.28"
  }
];
const kinds = ["SimpleRevenueAccrual", "SimpleFixedExpenseAccrual", "SimpleVariableExpenseAccrual", "CompanyDividendDeclaration", "CompanyDividendPayment", "CompanyStockDistributionCredit", "CompanyRightsOfferingSettlement", "CompanyRepurchaseCancellation", "CompanyShareReDenomination", "OpeningBalance", "LoanDisbursement", "LoanRepayment", "CashRevenue", "CreditSale", "ReceivableCollection", "CashExpense", "InterestAccrual", "InterestPayment", "TaxAccrual", "TaxPayment", "Depreciation", "CustomerDeposit", "CustomerWithdrawal", "LoanIssued", "LoanPrincipalCollected", "LoanInterestAccrued", "LoanInterestCollected", "DepositInterestAccrued", "DepositInterestPaid", "FeeAndCommissionEarned", "CreditImpairment", "LoanWriteOff", "WriteOffRecovery", "LandAcquisition", "DevelopmentCostIncurred", "PresaleCollection", "RealEstateDelivery", "FinalPaymentCollected", "BorrowingCostCapitalized", "DevelopmentImpairment", "InsurancePremiumAccrued", "InsurancePremiumCollected", "InsuranceServiceRevenue", "InsuranceFinance", "InsuranceLossComponent", "InsuranceClaimIncurred", "InsuranceClaimPaid"];
function request() { return structuredClone(fixture); }
function completed() { return { request: request(), publications: [9], committed_at: { date: "2030-06-03", second_of_day: 64800 } }; }
function status() { return { pending: [request()], completed: { op: completed() } }; }
function positive() { assert.deepEqual(parseCompanyReportCorrection(request()), request()); }
function check(name: string, body: () => void) { test(name, { timeout: 10000 }, () => { positive(); body(); }); }
check("真实Rust request与status对象/raw输入无损通过且不修改输入", () => {
 const a=request(), before=structuredClone(a); assert.deepEqual(parseCompanyReportCorrectionJson(JSON.stringify(a)),a); assert.deepEqual(a,before);
 const s=status(); assert.deepEqual(JSON.parse(JSON.stringify(parseReportCorrectionStatus(s))),s); assert.deepEqual(JSON.parse(JSON.stringify(parseReportCorrectionStatusJson(JSON.stringify(s)))),s);
});
check("request每个字段required并拒绝未知字段", () => {
 for(const key of Object.keys(fixture)){ const x:Record<string,unknown>={...request()}; delete x[key]; assert.throws(()=>parseCompanyReportCorrection(x),new RegExp(key)); assert.throws(()=>parseCompanyReportCorrection({...request(),[key]:null}),new RegExp(key)); }
 for(const key of ["epoch","account","approved_on","outcome"]) assert.throws(()=>parseCompanyReportCorrection({...request(),[key]:0}),new RegExp(key));
});
check("所有嵌套层精确字段并拒绝非对象", () => {
 for(const key of Object.keys(fixture.entries[0])){ const a=request(); delete (a.entries[0] as Record<string,unknown>)[key]; assert.throws(()=>parseCompanyReportCorrection(a),new RegExp(key)); }
 for(const key of Object.keys(fixture.entries[0].lines[0])){ const a=request(); delete (a.entries[0].lines[0] as Record<string,unknown>)[key]; assert.throws(()=>parseCompanyReportCorrection(a),new RegExp(key)); }
 const a=request(); Object.assign(a.entries[0],{extra:0}); assert.throws(()=>parseCompanyReportCorrection(a),/extra/);
 const b=request(); Object.assign(b.entries[0].lines[0],{extra:0}); assert.throws(()=>parseCompanyReportCorrection(b),/extra/);
 for(const value of [null,[],"text",1]) assert.throws(()=>parseCompanyReportCorrection(value));
});
check("u32身份端点与source安全整数端点", () => {
 for(const field of ["company","supersedes"]){ for(const value of [0,4294967295]) assert.equal((parseCompanyReportCorrection({...request(),[field]:value}) as Record<string,unknown>)[field],value); for(const value of [-0,-1,4294967296,1.5,"0",NaN,Infinity]) assert.throws(()=>parseCompanyReportCorrection({...request(),[field]:value}),new RegExp(field)); }
 for(const value of [0,Number.MAX_SAFE_INTEGER]){const a=request();a.entries[0].source=value;assert.equal(parseCompanyReportCorrection(a).entries[0].source,value);}
 for(const value of [-0,-1,Number.MAX_SAFE_INTEGER+1,1.5,"1"]){const a=request();(a.entries[0] as Record<string,unknown>).source=value;assert.throws(()=>parseCompanyReportCorrection(a),/source/);}
});
check("真实Rust AccountingAmount输入probe逐项一致并原样保持", () => {
 for(const item of amountProbe){const a=request();a.entries[0].lines[0].amount=item.input;if(item.accepted)assert.equal(parseCompanyReportCorrection(a).entries[0].lines[0].amount,item.input);else assert.throws(()=>parseCompanyReportCorrection(a),/amount/);}
 for(const amount of ["\u0085+01.2\u0085","-0","+0.00"]){const a=request();a.entries[0].lines[0].amount=amount;assert.equal(parseCompanyReportCorrection(a).entries[0].lines[0].amount,amount);}
 for(const amount of ["\ufeff1","1e2",".1","1.","1.234",1]){const a=request();(a.entries[0].lines[0] as Record<string,unknown>).amount=amount;assert.throws(()=>parseCompanyReportCorrection(a),/amount/);}
});
check("全部47项BusinessKind与现金流借贷enum闭包", () => {
 assert.equal(kinds.length,47);for(const kind of kinds){const a=request();a.entries[0].kind=kind;assert.equal(parseCompanyReportCorrection(a).entries[0].kind,kind);}
 for(const cash of ["Operating","Investing","Financing","NonCash"]){const a=request();a.entries[0].cash_flow=cash;assert.equal(parseCompanyReportCorrection(a).entries[0].cash_flow,cash);}
 for(const side of ["Debit","Credit"]){const a=request();a.entries[0].lines[0].side=side;assert.equal(parseCompanyReportCorrection(a).entries[0].lines[0].side,side);}
 for(const key of ["kind","cash_flow"]){const a=request();(a.entries[0] as Record<string,unknown>)[key]="Unknown";assert.throws(()=>parseCompanyReportCorrection(a),new RegExp(key));}
 const a=request();a.entries[0].lines[0].side="Unknown";assert.throws(()=>parseCompanyReportCorrection(a),/side/);
});
check("日期实际窗口公历与完成秒边界", () => {
 for(const date of ["1900-01-01","2000-02-29","2199-12-31"]){const a=request();a.entries[0].date=date;assert.equal(parseCompanyReportCorrection(a).entries[0].date,date);}
 for(const date of ["1899-12-31","2200-01-01","1900-02-29","2030-02-29","2030-6-03","2030-04-31"]){const a=request();a.entries[0].date=date;assert.throws(()=>parseCompanyReportCorrection(a),/date/);}
 for(const value of [0,86399]){const s=status();s.completed.op.committed_at.second_of_day=value;assert.equal(parseReportCorrectionStatus(s).completed.op.committed_at.second_of_day,value);}
 for(const value of [-0,-1,86400,1.5]){const s=status();s.completed.op.committed_at.second_of_day=value;assert.throws(()=>parseReportCorrectionStatus(s),/second_of_day/);}
});
check("status和completed每层required/未知字段", () => {
 for(const key of ["pending","completed"]){const s:Record<string,unknown>={...status()};delete s[key];assert.throws(()=>parseReportCorrectionStatus(s),new RegExp(key));}
 assert.throws(()=>parseReportCorrectionStatus({...status(),extra:1}),/extra/);
 for(const key of ["request","publications","committed_at"]){const s=status();delete (s.completed.op as Record<string,unknown>)[key];assert.throws(()=>parseReportCorrectionStatus(s),new RegExp(key));}
 const s=status();Object.assign(s.completed.op.committed_at,{extra:1});assert.throws(()=>parseReportCorrectionStatus(s),/extra/);
 const x=status();x.completed.op.publications=[-0];assert.throws(()=>parseReportCorrectionStatus(x),/publications/);
});
check("结构通过不冒充更正业务成功", () => {
 const a=request();a.entries=[];a.reason="";a.operation_id="";assert.deepEqual(parseCompanyReportCorrection(a),a);
 const s={pending:[],completed:{op:{...completed(),publications:[]}}};assert.deepEqual(JSON.parse(JSON.stringify(parseReportCorrectionStatus(s))),s);
});
check("Unicode scalar字符串和completed动态key原样保留", () => {
 for(const value of ["\ud800\udc00","\u0000","__proto__","constructor"]){const a=request();a.reason=value;a.operation_id=value;a.entries[0].lines[0].account=value;assert.deepEqual(parseCompanyReportCorrection(a),a);const s={pending:[],completed:Object.fromEntries([[value,completed()]])};const out=parseReportCorrectionStatusJson(JSON.stringify(s));assert.equal(Object.hasOwn(out.completed,value),true);}
 for(const value of ["\ud800","\udc00"]){for(const field of ["reason","operation_id","account"]){const a=request();if(field==="account")a.entries[0].lines[0].account=value;else(a as Record<string,unknown>)[field]=value;assert.throws(()=>parseCompanyReportCorrection(a));assert.throws(()=>parseCompanyReportCorrectionJson(JSON.stringify(a)));}assert.throws(()=>parseReportCorrectionStatusJson(JSON.stringify({pending:[],completed:{[value]:completed()}})));}
});
check("raw数字在舍入前拒绝unsafe/小数/指数/负零", () => {
 const raw=JSON.stringify(request());for(const token of ["9007199254740993","1.00000000000000001","1e0","-0","01","+1"]){assert.throws(()=>parseCompanyReportCorrectionJson(raw.replace('"source":900000001','"source":'+token)));}
 for(const field of ["company","supersedes"]){assert.throws(()=>parseCompanyReportCorrectionJson(raw.replace(new RegExp('"'+field+'":\\d+'),'"'+field+'":-0')));}
 const s=JSON.stringify(status());assert.throws(()=>parseReportCorrectionStatusJson(s.replace('"publications":[9]','"publications":[-0]')));assert.throws(()=>parseReportCorrectionStatusJson(s.replace('"second_of_day":64800','"second_of_day":-0')));
});
check("raw解码后重复键包含转义key必须在覆盖前拒绝", () => {
 const raw=JSON.stringify(request());assert.throws(()=>parseCompanyReportCorrectionJson(raw.replace('"company":3','"company":3,"company":4')));assert.throws(()=>parseCompanyReportCorrectionJson(raw.replace('"company":3','"company":3,"\\u0063ompany":4')));
 const c=JSON.stringify(completed());assert.throws(()=>parseReportCorrectionStatusJson('{"pending":[],"completed":{"op":'+c+',"\\u006fp":'+c+'}}'));
});
check("raw标准JSON语法及字符串中的数字和标点", () => {
 const a=request();a.reason='1e9 {"x":1,"x":2} \\ "';assert.deepEqual(parseCompanyReportCorrectionJson(JSON.stringify(a)),a);
 for(const raw of ["", "null", "[]", "{} trailing", JSON.stringify(request())+"x", '{"company":NaN}', '{"company":/*x*/3}', '{"company":3,}'])assert.throws(()=>parseCompanyReportCorrectionJson(raw));
});
