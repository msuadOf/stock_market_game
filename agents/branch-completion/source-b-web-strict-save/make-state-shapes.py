from pathlib import Path
import json,re,runpy
scope=runpy.run_path(str(Path(__file__).parent/'extract-rust-fields.py'));table=json.loads((Path(__file__).parent/'rust-field-table.json').read_text())['types'];split=scope['split_top'];unattr=scope['unattr']
special={'CivilDate':'date','CivilInstant':'instant','Money':'money','AccountingAmount':'amount','CompanyId':'u32','StockId':'u32','PublicationId':'u32','AccountId':'account','StockCode':'string','LedgerAccountId':'string','BusinessEventId':'u64number','AccountingPeriod':'period','IndustryId':'string','CompanySpec':'companySpec','IssuerRegistry':'issuerMap','OperatingRng':'rng','Books':'books','ClosingEngine':'closing','CompanyOperations':'operations','GroupStructure':'group','SimpleConfig':'simpleConfig','SimpleFinanceConfig':'simpleFinanceConfig','IncomeTaxPosition':'incomeTax','CompanyKind':'companyKind'}
def typ(t,attr=''):
 t=re.sub(r'(?:[a-z_]\w*::)+','',t.strip());t=re.sub(r'\s+','',t)
 if 'nonnegative_decimal' in attr:return 'nonnegativeI128'
 if 'canonical_i128_decimal' in attr:return 'canonicalI128'
 if 'canonical_u64_decimal' in attr:return 'canonicalU64'
 if 'u64_decimal' in attr:return 'decimalU64'
 if 'js_safe_u64' in attr:return 'u64number'
 if t.startswith('(')and t.endswith(')'):return ['tuple',[typ(x)for x in split(t[1:-1])]]
 m=re.match(r'^(\w+)<(.*)>$',t)
 if m:
  args=split(m[2]);base=m[1]
  if base in ['Option','RequiredOption']:return ['nullable',typ(args[0])]
  if base in ['Vec','BTreeSet','VecDeque']:return ['array',typ(args[0])]
  if base in ['BTreeMap','HashMap']:return ['map',typ(args[0]),typ(args[1])]
  if base in ['Arc','Box']:return typ(args[0])
  raise RuntimeError(t)
 if t in special:return special[t]
 if t in ['String','str']:return 'string'
 if t=='bool':return 'boolean'
 if t in ['u8','u16','u32','i8','i16','i32','i64','i128','u64','usize']:return 'u64number'if t in ['u64','usize'] else t
 return ['ref',t]
def fields(parts):
 result={}
 for part in parts:
  text=unattr(part);m=re.match(r'(?:pub(?:\([^)]*\))?\s+)?(\w+)\s*:\s*(.*)$',text,re.S)
  if not m:raise RuntimeError(part)
  result[m[1]]=typ(m[2],part[:part.find(text)]if text in part else part)
 return result
rules={};todo=['CompanySystem','SimpleFundamentals','SessionCorporateActions']
while todo:
 name=todo.pop(0)
 if name in rules or name in special:continue
 entry=table[name]
 if name=='CompanyImplementation':rule=['object',{'mode':['literal','Simple'],'state':['ref','SimpleFundamentals']}]
 elif entry['kind']=='struct':rule=['object',{f['name']:typ(f['type'],f['attributes'])for f in entry['fields']if 'serde(skip)'not in f['attributes']}]
 else:
  variants={}
  for f in entry['fields']:
   v=f['attributes'];m=re.match(r'^(\w+)(.*)$',v,re.S);tail=m[2].strip()
   if not tail:variants[m[1]]=None
   elif tail.startswith('('):variants[m[1]]=typ(tail[1:-1])if len(split(tail[1:-1]))==1 else ['tuple',[typ(x)for x in split(tail[1:-1])]]
   elif tail.startswith('{'):variants[m[1]]=['object',fields(split(tail[1:-1]))]
   else:raise RuntimeError(v)
  rule=['enum',variants]
 if name=='CalendarExchange':rule=['enum',{'sse':None,'szse':None}]
 rules[name]=rule
 def refs(x):
  if isinstance(x,list):
   if x[0]=='ref':todo.append(x[1])
   else:
    for child in x:refs(child)
  elif isinstance(x,dict):
   for child in x.values():refs(child)
 refs(rule)
# CompanySystem跳过cache；CompanyImplementation使用真实adjacent tag；其余是当前Rust字段/serdecodec投影。
output=Path.cwd()/'apps/web/src/save/schema/company/state-shapes.ts';output.write_text('''// 当前 Rust 公司状态 wire 字段表；来源与逐文件 SHA 保存在 source-b-web-strict-save/rust-field-table.json。
// 仅描述结构与 codec，不复制会计、权益、审批或日结业务算法。
export type StateRule = string | readonly [string, ...unknown[]];
export const STATE_SHAPES: Readonly<Record<string, StateRule>> = '''+json.dumps(rules,ensure_ascii=False,indent=2)+';\n')
print('production shapes',len(rules),'lines',len(output.read_text().splitlines()))

def ts_type(rule):
 if isinstance(rule,str):
  if rule in ['string','date','instantString','money','amount','canonicalU64','decimalU64','canonicalI128','nonnegativeI128','account','period']:return 'string'
  if rule=='boolean':return 'boolean'
  if rule in ['u8','u16','u32','u64number','i8','i16','i32','i64','i128']:return 'number'
  return {'companySpec':'ReturnType<typeof parseCompanySpec>','issuerMap':'Record<string, ReturnType<typeof parseCompanySpec>>','rng':'{ state: string }','books':'ReturnType<typeof parseBooks>','closing':'ReturnType<typeof parseClosingRegistry>','operations':'ReturnType<typeof parseCompanyOperations>','group':'ReturnType<typeof parseGroups>[number]','simpleConfig':'SimpleConfig','simpleFinanceConfig':'SimpleFinanceConfig','incomeTax':'ReturnType<typeof parseIncomeTaxPosition>','companyKind':'CompanyKind'}[rule]
 kind=rule[0]
 if kind=='ref':return rule[1]+'Wire'
 if kind=='literal':return json.dumps(rule[1])
 if kind=='array':return '('+ts_type(rule[1])+')[]'
 if kind=='tuple':return '['+', '.join(ts_type(x)for x in rule[1])+']'
 if kind=='nullable':return '('+ts_type(rule[1])+' | null)'
 if kind=='map':return 'Record<string, '+ts_type(rule[2])+'>'
 if kind=='object':return '{ '+'; '.join(json.dumps(k)+': '+ts_type(v)for k,v in rule[1].items())+' }'
 if kind=='enum':return ' | '.join(json.dumps(k)if v is None else '{ '+json.dumps(k)+': '+ts_type(v)+' }'for k,v in rule[1].items())
 raise RuntimeError(rule)
imports='''import type { parseCompanySpec } from "./accounting/spec.ts";
import type { parseBooks } from "./accounting/books.ts";
import type { parseClosingRegistry } from "./closing.ts";
import type { parseIncomeTaxPosition } from "./books/industrial.ts";
import type { SimpleConfig } from "../../../types/generated/SimpleConfig.ts";
import type { SimpleFinanceConfig } from "../../../types/generated/SimpleFinanceConfig.ts";
import type { CompanyKind } from "./accounting/spec.ts";
'''
with output.open('a')as file:file.write('\n'+imports+'\n'+'\n'.join('export type '+name+'Wire = '+ts_type(rule)+';'for name,rule in rules.items())+'\n')
