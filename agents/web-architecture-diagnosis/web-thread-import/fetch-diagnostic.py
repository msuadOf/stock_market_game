import concurrent.futures,hashlib,json,os,pathlib,sys,urllib.request
h=pathlib.Path(sys.argv[1])/'registry'; idx=h/'index/index.crates.io-1949cf8c6b5b557f/.cache'; cache=h/'cache/index.crates.io-1949cf8c6b5b557f'
items={'js-sys':'0.3.75','web-sys':'0.3.75','wasm-bindgen':'0.2.98','wasm-bindgen-macro':'0.2.98','wasm-bindgen-macro-support':'0.2.98','wasm-bindgen-shared':'0.2.98','wasm-bindgen-backend':'0.2.98','wasm-bindgen-futures':'0.4.48','pin-project':'1.1.10','pin-project-internal':'1.1.10','atomic-waker':'1.1.2'}
if len(sys.argv) > 2:
 items=json.loads(sys.argv[2])
def download(item):
 name,version=item;rel=name[:2]+'/'+name[2:4]+'/'+name;parts=(idx/rel).read_bytes().split(b'\0'); entries=[json.loads(x) for x in parts if x.startswith(b'{')];entry=next(x for x in entries if x['vers']==version); data=urllib.request.urlopen(f'https://static.crates.io/crates/{name}/{name}-{version}.crate',timeout=8).read(); assert hashlib.sha256(data).hexdigest()==entry['cksum'], name; (cache/f'{name}-{version}.crate').write_bytes(data); return name+' '+version+' checksum OK'
failed=False
with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
 for f in concurrent.futures.as_completed([pool.submit(download,x) for x in items.items()]):
  try: print(f.result(),flush=True)
  except Exception as e:
   failed=True
   print('ERROR',repr(e),flush=True)
if failed: sys.exit(1)
