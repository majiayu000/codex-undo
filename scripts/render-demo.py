#!/usr/bin/env python3
"""Render actual real-codex.py transcript; no invented tool results."""
import argparse,json,pathlib,textwrap
from PIL import Image,ImageDraw,ImageFont
p=argparse.ArgumentParser();p.add_argument('transcript');p.add_argument('output');a=p.parse_args()
rows=json.loads(pathlib.Path(a.transcript).read_text())
font_path='/System/Library/Fonts/Menlo.ttc'
font=ImageFont.truetype(font_path,18);titlefont=ImageFont.truetype(font_path,23)
frames=[]
for n,row in enumerate(rows):
 image=Image.new('RGB',(1160,680),'#10151e');d=ImageDraw.Draw(image)
 d.text((30,22),'codex-undo: an untracked note comes back',font=titlefont,fill='#f4f7fb')
 d.text((30,62),'Real CLI transcript | model wait compressed | beta candidate',font=font,fill='#a2afc4')
 command=row['command']
 if n==0:command='Real Codex task: delete notes.txt with shell (excerpt)'
 y=116
 for line in textwrap.wrap(command,98):d.text((30,y),('$ ' if n else '')+line,font=font,fill='#73e4b3');y+=27
 y+=12
 for raw in row['output'].splitlines():
  for line in textwrap.wrap(raw,98,replace_whitespace=False) or ['']:
   d.text((30,y),line,font=font,fill='#d9e1ed');y+=25
 d.text((30,632),f'{n+1}/4 | File restore uses a safety snapshot; conversation is separate.',font=font,fill='#a2afc4')
 frames.append(image)
frames[0].save(a.output,save_all=True,append_images=frames[1:],duration=[6000,9000,6000,9000],loop=0,optimize=False)
print(a.output)
