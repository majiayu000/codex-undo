#!/usr/bin/env python3
"""Compare pinned upstream schemas and validate real, sanitized input captures."""
import argparse, json, pathlib, subprocess, urllib.request

parser = argparse.ArgumentParser()
parser.add_argument('--upstream', help='local openai/codex checkout; otherwise fetch pinned raw source')
parser.add_argument('--check-current', action='store_true', help='fail if upstream main differs from pinned schemas')
args = parser.parse_args()
root = pathlib.Path(__file__).resolve().parents[1]
commit = '0e1520605f67969b58e53762be4d675c036a981d'
events = {'SessionStart':'session-start','UserPromptSubmit':'user-prompt-submit','PreToolUse':'pre-tool-use','PostToolUse':'post-tool-use','Stop':'stop','SubagentStart':'subagent-start','SubagentStop':'subagent-stop'}
# jsonschema is used only by this developer verification script.
import jsonschema
schemas = {}
for event, stem in events.items():
    relative = f'codex-rs/hooks/schema/generated/{stem}.command.input.schema.json'
    if args.upstream:
        data = (pathlib.Path(args.upstream)/relative).read_bytes()
    else:
        with urllib.request.urlopen(f'https://raw.githubusercontent.com/openai/codex/{commit}/{relative}', timeout=30) as response:
            data = response.read()
    schemas[event] = json.loads(data)
    if args.check_current:
        with urllib.request.urlopen(f'https://raw.githubusercontent.com/openai/codex/main/{relative}', timeout=30) as response:
            current = json.load(response)
        if current != schemas[event]:
            raise SystemExit(f'Schema drift: {event}; review and update fixtures before claiming support')
count=0
for fixture in sorted((root/'tests/fixtures').glob('*.jsonl')):
    for line in fixture.read_text().splitlines():
        value=json.loads(line)
        event=value['hook_event_name']
        if event not in schemas:
            print(f'{event}: no pinned schema; capture retained as evidence only')
            continue
        jsonschema.validate(value,schemas[event]);count+=1
print(f'Validated {count} captured hook inputs against OpenAI source {commit}')
