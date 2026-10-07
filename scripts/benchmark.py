#!/usr/bin/env python3
"""Real filesystem timings; Git is used only to prepare a tracked fixture."""
import argparse,json,os,pathlib,statistics,subprocess,tempfile,time,hashlib,platform,math
p=argparse.ArgumentParser();p.add_argument('binary');p.add_argument('--files',type=int,default=10000);p.add_argument('--samples',type=int,default=20);a=p.parse_args()
binary=str(pathlib.Path(a.binary).resolve())
with tempfile.TemporaryDirectory(prefix='codex-undo-bench-') as tmp:
 root=pathlib.Path(tmp).resolve();cwd=root/'repo';cwd.mkdir();data=root/'store'
 for i in range(a.files):
  d=cwd/f'd{i//1000}';d.mkdir(exist_ok=True);(d/f'f{i:06}.txt').write_text('benchmark\n')
 subprocess.run(['git','init','-q',str(cwd)],check=True)
 subprocess.run(['git','-C',str(cwd),'add','.'],check=True)
 git_before={str(f.relative_to(cwd)):f.read_bytes() for f in (cwd/'.git').rglob('*') if f.is_file()}
 env=os.environ.copy();env.pop('CODEX_THREAD_ID',None)
 def hook(event,turn,**extra):
  v=dict(session_id='benchmark',cwd=str(cwd),hook_event_name=event,turn_id=str(turn),prompt='benchmark');v.update(extra)
  start=time.perf_counter();r=subprocess.run([binary,'--data-dir',str(data),'hook',event],input=json.dumps(v),text=True,capture_output=True,env=env,check=True)
  if r.stderr:raise RuntimeError(r.stderr)
  return (time.perf_counter()-start)*1000
 cold=hook('UserPromptSubmit',1);hook('Stop',1);time.sleep(3)
 full=[];incremental=[]
 for i in range(a.samples):
  full.append(hook('UserPromptSubmit',i+2))
  incremental.append(hook('PreToolUse',i+2,tool_name='apply_patch',tool_input={'command':'*** Begin Patch\n*** Update File: d0/f000000.txt\n@@\n benchmark\n*** End Patch'}))
  hook('Stop',i+2)
 git_after={str(f.relative_to(cwd)):f.read_bytes() for f in (cwd/'.git').rglob('*') if f.is_file()}
 assert git_before==git_after,'Git metadata changed'
 def stats(v):return {'median_ms':round(statistics.median(v),2),'p95_ms':round(sorted(v)[max(0,math.ceil(len(v)*.95)-1)],2),'max_ms':round(max(v),2),'samples_ms':[round(x,2) for x in v]}
 print(json.dumps({'platform':platform.platform(),'binary_sha256':hashlib.sha256(pathlib.Path(binary).read_bytes()).hexdigest(),'files':a.files,'samples':a.samples,'p95_method':'nearest-rank ceil(0.95*n)','cold_full_ms':round(cold,2),'incremental':stats(incremental),'full':stats(full),'git_metadata_unchanged':True},indent=2))
