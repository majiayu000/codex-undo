#!/usr/bin/env python3
"""SIGKILL real processes during blob capture and a multi-file restore."""
import argparse,json,os,pathlib,subprocess,tempfile,time
p=argparse.ArgumentParser();p.add_argument('binary');p.add_argument('--output',required=True);a=p.parse_args();binary=str(pathlib.Path(a.binary).resolve())
def invoke(data,cwd,args,value=None):
 env=os.environ.copy();env.pop('CODEX_THREAD_ID',None)
 return subprocess.run([binary,'--data-dir',str(data),*args],cwd=cwd,env=env,input=None if value is None else json.dumps(value),text=True,capture_output=True,check=True)
def hook(data,cwd,event):return invoke(data,cwd,['hook',event],dict(session_id='crash',cwd=str(cwd),turn_id='t',hook_event_name=event,prompt='crash fixture'))
result={}
with tempfile.TemporaryDirectory(prefix='codex-undo-crash-') as tmp:
 root=pathlib.Path(tmp).resolve();cwd=root/'workspace';cwd.mkdir();data=root/'store'
 before=b'a'*512*1024;after=b'b'*512*1024
 for i in range(50):(cwd/f'f{i:03}').write_bytes(before)
 value=dict(session_id='crash',cwd=str(cwd),turn_id='t',hook_event_name='UserPromptSubmit',prompt='crash fixture')
 env=os.environ.copy();env.pop('CODEX_THREAD_ID',None)
 proc=subprocess.Popen([binary,'--data-dir',str(data),'hook','UserPromptSubmit'],cwd=cwd,env=env,stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,text=True)
 proc.stdin.write(json.dumps(value));proc.stdin.close()
 deadline=time.monotonic()+60;killed=False
 while time.monotonic()<deadline and proc.poll() is None:
  if any((data/'blobs').glob('*/*')) and not any((data/'manifests').glob('*')):
   proc.kill();proc.wait();killed=True;break
  time.sleep(.001)
 assert killed,'missed blob-write kill window; do not claim crash check passed'
 hook(data,cwd,'UserPromptSubmit');result['sigkill_during_blob_capture']=True
 for i in range(50):(cwd/f'f{i:03}').write_bytes(after)
 hook(data,cwd,'Stop')
 proc=subprocess.Popen([binary,'--data-dir',str(data),'--session','crash','undo','--yes'],cwd=cwd,env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 deadline=time.monotonic()+60;killed=False
 while time.monotonic()<deadline and proc.poll() is None:
  if (cwd/'f000').read_bytes()==before and (cwd/'f049').read_bytes()==after:
   proc.kill();proc.wait();killed=True;break
  time.sleep(.001)
 assert killed,'missed partial-restore kill window; do not claim crash check passed'
 invoke(data,cwd,['--session','crash','redo','--yes'])
 assert all((cwd/f'f{i:03}').read_bytes()==after for i in range(50))
 result['sigkill_during_restore_redo_all_50_files']=True
 invoke(data,cwd,['gc']);invoke(data,cwd,['--session','crash','status'])
 result['post_crash_gc_and_integrity']=True
pathlib.Path(a.output).write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
