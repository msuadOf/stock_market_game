from pathlib import Path
import re,json,hashlib
src=Path('/data1/baiyifan/workplace/stock_market_game/.worktree/company-source-exclusive/packages/engine/src')
def split_top(s):
 out=[];start=0;stack=[];string=False;escaped=False
 for i,c in enumerate(s):
  if string:
   if escaped:escaped=False
   elif c=='\\':escaped=True
   elif c=='"':string=False
   continue
  if c=='"':string=True;continue
  if c in '({[<':stack.append(c)
  elif c in ')}]>':
   if stack:stack.pop()
  elif c==',' and not stack:out.append(s[start:i].strip());start=i+1
 if s[start:].strip():out.append(s[start:].strip())
 return out
def unattr(s):
 while '#[' in s:
  a=s.index('#[');i=a+2;depth=1;quote=False;escape=False
  while i<len(s)and depth:
   c=s[i]
   if quote:
    if escape:escape=False
    elif c=='\\':escape=True
    elif c=='"':quote=False
   elif c=='"':quote=True
   elif c=='[':depth+=1
   elif c==']':depth-=1
   i+=1
  s=s[:a]+s[i:]
 return s.strip()
index={}
for file in sorted(src.rglob('*.rs')):
 if '/tests' in str(file)or file.name.endswith('_tests.rs'):continue
 s=re.sub(r'//[^\n]*','',file.read_text())
 for m in re.finditer(r'\b(struct|enum)\s+(\w+)\s*\{',s):
  a=m.end();i=a;d=1
  while d and i<len(s):
   if s[i]=='{':d+=1
   elif s[i]=='}':d-=1
   i+=1
  body=s[a:i-1];parts=split_top(body);fields=[]
  for part in parts:
   text=unattr(part)
   match=re.match(r'(?:pub(?:\([^)]*\))?\s+)?(\w+)\s*:\s*(.*)$',text,re.S)
   if match:fields.append({'name':match[1],'type':re.sub(r'\s+',' ',match[2]).strip(),'attributes':part[:part.find(text)]if text in part else part})
   else:fields.append({'variant':re.sub(r'\s+',' ',text).strip(),'attributes':part})
  key=m[2];entry={'kind':m[1],'file':str(file.relative_to(src)),'line':s[:m.start()].count('\n')+1,'fields':fields,'sourceSha256':hashlib.sha256(file.read_bytes()).hexdigest()}
  index.setdefault(key,[]).append(entry)
roots=['SavedSessionCompanies','CompanySystem','CompanyImplementation','SimpleFundamentals','SimpleCompanyState','PeriodGenerationState','SimpleFinanceState','SessionCorporateActions','ShareSettlementBinding']
seen={};todo=roots.copy();unresolved=set();primitive={'String','str','Vec','Option','BTreeMap','BTreeSet','HashMap','bool','u8','u16','u32','u64','i8','i16','i32','i64','i128','usize','f64','Arc','Self','Box','RequiredOption'}
special={'CivilDate','CivilInstant','Money','AccountingAmount','CompanyId','AccountId','StockId','StockCode','LedgerAccountId','BusinessEventId','AccountingPeriod','IndustryId','IssuerRegistry','CompanyOperations','ClosingEngine','GroupStructure','Books','SimpleConfig','SimpleFinanceConfig','CompanyKind','OperatingRng'}
while todo:
 name=todo.pop(0)
 if name in seen or name in primitive or name in special:continue
 if name not in index:unresolved.add(name);continue
 candidates=index[name];chosen=candidates[0]
 if len(candidates)>1:
  nonraw=[x for x in candidates if 'tests'not in x['file']]
  if nonraw:chosen=nonraw[0]
 seen[name]=chosen
 for field in chosen['fields']:
  if '#[serde(skip)]'in field.get('attributes',''):continue
  reference=field.get('type',field.get('variant',''))
  if 'variant' in field:reference=re.sub(r'^\w+','',reference)
  for token in re.findall(r'\b[A-Z]\w*\b',reference):
   if token not in primitive and token not in special and token!=name:todo.append(token)
out=Path(__file__).parent/'rust-field-table.json';out.write_text(json.dumps({'roots':roots,'specialExistingParsers':sorted(special),'types':seen,'unresolved':sorted(unresolved)},ensure_ascii=False,indent=2)+'\n')
print('types',len(seen),'unresolved',sorted(unresolved))
for name,v in seen.items():print(name,v['file'],[(x.get('name',x.get('variant')),x.get('type'))for x in v['fields']])
