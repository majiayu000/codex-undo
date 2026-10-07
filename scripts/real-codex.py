#!/usr/bin/env python3
"""Opt-in billed real official Codex test, never run by CI. No global config edits."""
import argparse,json,os,pathlib,subprocess,tempfile,time
p=argparse.ArgumentParser();p.add_argument('binary');p.add_argument('--codex',default='codex');p.add_argument('--turns',type=int,default=10);p.add_argument('--output',required=True);p.add_argument('--demo',help='write actual command-output transcript for a 30-second demo');a=p.parse_args()
binary=str(pathlib.Path(a.binary).resolve());results=[];demo=[]
with tempfile.TemporaryDirectory(prefix='codex-undo-live-') as tmp:
 root=pathlib.Path(tmp).resolve();cwd=root/'workspace';cwd.mkdir();(cwd/'.codex').mkdir();data=root/'store'
 (cwd/'notes.txt').write_text('untracked note before Codex\n');(cwd/'version.txt').write_text('0\n')
 env=os.environ.copy();env['CODEX_UNDO_DATA_DIR']=str(data);env.pop('CODEX_THREAD_ID',None)
 subprocess.run([binary,'install','--hooks-file',str(cwd/'.codex/hooks.json')],env=env,check=True,capture_output=True,text=True)
 session=None
 def cli(command):return subprocess.run([binary,'--data-dir',str(data),'--session',session,*command],cwd=cwd,env=env,check=True,capture_output=True,text=True).stdout
 for n in range(1,a.turns+1):
  prompt=('In this isolated test, delete only notes.txt using shell, then use apply_patch to set version.txt to 1 and create new.txt containing new. Do not inspect other directories. End with DONE.' if n==1 else f'Use apply_patch to set version.txt to exactly {n} followed by newline. Do not inspect other directories. End with DONE.')
  cmd=[a.codex,'exec','--skip-git-repo-check','--dangerously-bypass-hook-trust','--json']
  if session:cmd+=['resume',session,prompt]
  else:cmd+=['-C',str(cwd),'-s','workspace-write',prompt]
  start=time.monotonic();r=subprocess.run(cmd,cwd=cwd,env=env,text=True,capture_output=True,timeout=240)
  if r.returncode:raise RuntimeError(f'Codex invocation {n} failed; stderr retained only locally: {r.returncode}')
  events=[json.loads(l) for l in r.stdout.splitlines() if l.startswith('{')]
  for event in events:
   if event.get('type')=='thread.started':session=event['thread_id']
  if not session:raise RuntimeError('No real thread id returned')
  assert (cwd/'version.txt').read_text()==f'{n}\n'
  assert 'Latest checkpoint objects verified' in cli(['status'])
  results.append({'turn':n,'seconds':round(time.monotonic()-start,2),'version':n,'recorded':True})
  if n==1:
   assert not (cwd/'notes.txt').exists()
   shell=[e['item']['command'] for e in events if e.get('type')=='item.completed' and e.get('item',{}).get('type')=='command_execution']
   demo.append({'command':"codex exec 'Delete notes.txt using shell'",'output':'Real Codex tool: '+str(shell)+'\nnotes.txt is absent (verified on disk).'})
   undo=cli(['undo','--yes']);demo.append({'command':'codex-undo undo --yes','output':undo});assert (cwd/'notes.txt').read_text()=='untracked note before Codex\n';assert not (cwd/'new.txt').exists();assert (cwd/'version.txt').read_text()=='0\n'
   demo.append({'command':'cat notes.txt','output':(cwd/'notes.txt').read_text()})
   redo=cli(['redo','--yes']);demo.append({'command':'codex-undo redo --yes','output':redo});assert not (cwd/'notes.txt').exists();assert (cwd/'new.txt').read_text()=='new\n';assert (cwd/'version.txt').read_text()=='1\n'
 if a.turns>=2:
  cli(['undo','--yes']);assert (cwd/'version.txt').read_text()==f'{a.turns-1}\n';cli(['redo','--yes']);assert (cwd/'version.txt').read_text()==f'{a.turns}\n'
 result={'client':subprocess.check_output([a.codex,'--version'],text=True).strip(),'actual_codex_turns':a.turns,'deletion_undo_redo':True,'last_turn_undo_redo':True,'turns':results,'status':cli(['status']),'list':cli(['list'])}
 # Strip scratch paths/session IDs in publishable evidence.
 encoded=json.dumps(result,indent=2).replace(str(root),'<scratch>').replace(session,'<recorded-session>')
 pathlib.Path(a.output).write_text(encoded+'\n')
 if a.demo:pathlib.Path(a.demo).write_text(json.dumps(demo,indent=2).replace(str(root),'<scratch>').replace(session,'<recorded-session>')+'\n')
 subprocess.run([binary,'uninstall','--hooks-file',str(cwd/'.codex/hooks.json')],env=env,check=True,capture_output=True,text=True)
 print(f'PASS: {a.turns} real Codex turns; deletion, undo and redo verified')
