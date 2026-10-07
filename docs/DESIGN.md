# v0.1 decision record (2026-10-07)

Required: official Codex hooks, local macOS/Linux, file restore without writing
Git, untracked deletion recovery, explicit coverage gaps, preview/conflicts/redo.

Build a standalone Rust command. Adapt the *ideas* of monotonic tracking,
positive absence evidence, symmetric ignore rules, and safety snapshots from
[codex-rewind](https://github.com/extracurricular-ai/codex-rewind); no source copied.
Reject adopting its fork because replacing official Codex conflicts with the requirement.
Reject Aider commits and Gemini/Cline shadow Git because this design needs
explicit per-path absence and no Git mutation. Claude Code/Cursor checkpoints
are tied to their clients. Source evidence: official
[Claude checkpoint docs](https://code.claude.com/docs/en/checkpointing),
[Gemini checkpoint docs](https://geminicli.com/docs/cli/checkpointing/),
[Codex hooks](https://learn.chatgpt.com/docs/hooks).

Single binary; blob and manifest hashes; append-only per-session journal;
stat cache is an optimization only. Restore writes a durable recovery record
before the first file mutation. Interrupted restore is recoverable with redo.
No daemon, SDK, database, or shared infrastructure. gix-index reads the index,
never calls Git. Current workspace ignore policy applies both directions.

Risks: hooks miss some tool paths; external writers don't share our lock; no
filesystem transaction spanning multiple files. Require idle agents for restore,
check content again before each write, keep safety snapshot on any failure.
No promise of user-vs-agent attribution: conflicts mean unexpected current content.
Validate real hooks separately from fixture tests. Desktop/escape menu/!
behavior remains unclaimed until observed. Files discovered after a target
checkpoint remain untouched if that target has no explicit record for them.
