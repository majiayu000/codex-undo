# Validation record — 2026-10-07

This is a beta candidate. M1–M3 have implemented behavior with local evidence;
M0 desktop acceptance and parts of M4 distribution/filesystem acceptance remain
open. Production publication and merge have not been performed.

## Real official client evidence (M0)

[Sanitized captures](../tests/fixtures/cli-0.160.0.jsonl) contain **30 actual inputs**
from official `codex-cli 0.160.0`, recorded only in an isolated scratch project:

| Event | Captured inputs |
|---|---:|
| SessionStart | 4 |
| UserPromptSubmit | 4 |
| PreToolUse | 9 |
| PostToolUse | 7 |
| Stop | 3 |
| SubagentStart | 1 |
| SubagentStop | 1 |
| Interrupt | 1 |

Session/turn/tool identifiers are mapped consistently to anonymous identifiers;
workspace/transcript paths are replaced; scratch tool outputs are removed. The
payload structure is retained. These were not constructed from a schema.
Schemas from official OpenAI source commit
`0e1520605f67969b58e53762be4d675c036a981d` supplement the captures. **29 inputs**
validate against those generated schemas; that source has no generated Interrupt
input schema, so its single capture remains runtime evidence, not a schema pass.
The original pin omitted the real CLI `source=fork` value. The current official
schema includes it; the pin was updated instead of rewriting the capture.
Actual pinned raw GitHub source retrieval was checked.

Verified runtime findings:

- Canonical shell name is `Bash`; apply_patch carries patch text in
  `tool_input.command`. Paths can be absolute.
- Parallel calls produce overlapping Pre/Post lifetimes; serial journal order is
  lock acquisition order, not proof of tool execution order.
- A denied shell attempt emitted PreToolUse without PostToolUse. Outstanding
  pre records are reported at Stop/Interrupt and reconciled there.
- Child agents share the parent session_id, carry their own turn_id and agent_id,
  and have SubagentStart/SubagentStop. Child edits join the current parent turn.
- A real `codex exec fork` emitted SessionStart source=fork, with no parent session
  or parent turn identity. No invented ancestry record is stored.
- SIGINT sent while a shell tool was active emitted Interrupt.
  [Actual signal result](interrupt.json). Official Interrupt timeout maximum is
  3 seconds; the installer uses 3 rather than the spec's blanket 10 seconds.
- Source/unknown fields are tolerated; outputs never issue blocking or
  continuation decisions.

The CLI recorder used invocation-scoped hook-trust bypass only after reviewing
its scratch-only code. No user global hook definitions, remem/vibeguard entries,
trust records, approval policies, or auth files were changed. The scratch recorder registration and private raw probe log were removed after
sanitized captures were extracted.

**Not verified:** actual desktop app hook execution, terminal `!` frontend
shortcut, double-Esc ordering, or desktop plugin loading. The bundled CLI version
`0.160.1` is inventory only. A fresh desktop project/plugin hook needs review and
trust through `/hooks`; see [official hook trust documentation](https://learn.chatgpt.com/docs/hooks).
The saved desktop project available here is the shared `tool` parent. Installing
its project hook would affect unrelated parallel chats, so it was not done.
A private isolated scratch project and minimal recorder were prepared for human
review/trust via `/hooks`, with a bounded deletion/apply_patch/parallel/child probe.
It has not run; the exact local directory and commands are in the coordinator
handoff, outside this published source. Required desktop action: open that
project, review/trust only its eight recorder definitions, and send the bounded
prompt in a fresh Desktop chat. No fake desktop
source flag or UI classification is used as evidence.

## Real file restoration (M2–M3)

[Ten-turn result](real-cli.json): **10 actual official Codex turns**, each with
recorded start/end evidence. First turn shell-deleted untracked `notes.txt` and
created/edited files via apply_patch. `undo --yes` restored the note and baseline;
`redo --yes` restored the deletion and resulting edits. Turn ten undo restored
version nine, and redo restored version ten. Model runtime/allowance cost is
separate from checkpoint latency. These tests are opt-in and excluded from CI.

[One-turn repeat](real-final.json) and [accepted-source repeat](real-accepted.json)
also passed. The 30-second
[demo](demo.gif) renders [actual CLI output](demo-transcript.json), compressing
model wait time; it does not depict a simulated restore or a desktop client.

## Storage and correctness (M1–M3)

macOS arm64 full run: **28 tests passed** (2 journal/recovery, 5 command-entry,
21 correctness including 64 randomized operation sequences). Coverage includes:

- Exact content/mode restoration, binary bytes and monotonic tracking.
- Positive absence permits deletion; omitted/unobserved files are preserved.
- Symmetric ignore policy frozen at operation start, including restoring the
  ignore file itself; ignored external `*.key` paths remain untouched.
- Default conflict rejection before all writes; force has a safety snapshot;
  edits during confirmation are refused even under force.
- Rewind idempotence, redo round-trip, untouched unknown files and Git metadata.
- Symlinks, symlink parent paths, hard links and Git internals are rejected.
- Interrupted journal tail replay and completed-record corruption rejection.
- Partial restore recovery; gc preserves all recovery/history references.
- Two installs plus uninstall preserve foreign hook entries and exact backups.
- Malformed hook input exits 0, emits `{}`, and records a visible generic gap.
- External ignore regression: the ignore crate panicked on an absolute path
  outside its matcher root. Fixed with workspace-relative matching inside and
  basename matching outside. Library and actual binary stdin regressions cover
  ordinary external files, `*.key`, and repository subdirectory/index siblings.

[Actual process-kill result](crash.json): SIGKILL during blob capture recovered
on the next invocation. SIGKILL after the first of 50 file restores left a durable
safety record; redo recovered **all 50 files**. Subsequent gc/integrity succeeded.
Deterministic unit fixtures separately cover torn journal append and partial
recovery. Not every microsecond-sized manifest/journal write boundary was hit by
SIGKILL; do not describe those deterministic fixtures as real process kills.

Linux runs use local Docker `rust:1.97-bookworm` with explicit architecture.
Final native arm64 and emulated x64 runs each passed 28 tests
(2 recovery, 5 CLI entry, 21 correctness including 64 randomized sequences). Docker overlayfs and
bind mounts are not proof of ext4/btrfs-specific behavior. The coordinator
also probed isolated 1 GiB ext4/btrfs images: both formatting commands succeeded,
but both loop mounts exited 32 (`No such file or directory`); `/dev/loop*`
was absent. No benchmark ran on either filesystem. The temporary probe container
was removed; no host devices, mounts or kernel settings changed. This is an
unavailable test environment, not a claim that codex-undo fails on those filesystems.
Native Linux, APFS and cross-architecture executables are reported separately
from remote CI.

## Performance (M4)

Subprocess wall-clock timings include hook startup, journal replay, checkpoint and
sync. Fixtures are Git-tracked, and benchmark setup alone uses Git commands.
Before/after `.git` file contents are compared byte-for-byte. Binary SHA-256 is
recorded for the latest measurements. p95 uses nearest rank
`ceil(0.95 * n)` (the 19th sorted observation in a 20-sample run).
Earlier script output incorrectly labeled the maximum as p95; evidence JSON
retains that value as `original_reported_max_ms`, alongside corrected p95,
maximum and all actual samples. No timing samples were removed.

| macOS arm64 fixture | Samples | Incremental p95 | Full p95 | Result |
|---|---:|---:|---:|---|
| 10k, initial | 20 | 2532.34 ms | 2789.75 ms | Failed |
| 10k, directory/cache-write optimization | 20 | 1467.01 ms | 1773.91 ms | Failed |
| 10k, targeted checkpoint | 20 | 863.86 ms | 1689.25 ms | Failed |
| 10k, lazy cache + focused baseline | 20 | **113.41 ms** | **384.25 ms** | Warm targets passed |
| 100k, earlier version | 20 | 15349.17 ms | 16312.66 ms | Failed |
| 100k, lazy cache | 20 | 1311.43 ms | 8204.11 ms | Failed |
| 10k, final parent-check reuse | 20 | **85.21 ms** | **196.60 ms** | Warm targets passed |
| 100k, final parent-check reuse | 20 | 747.15 ms | 2410.94 ms | Full target exceeded; slow warning |

The final 10k incremental test names one existing file with a context-only patch;
it is not an empty-path benchmark. Full maximum is **639.97 ms** (one observation;
19 others below 200 ms), and incremental maximum is **95.56 ms**. Cold full scan
is **1336.42 ms**; previous-source cold scan was **2975 ms**. Cold timing remains
separate from the warm steady-state target. Final 100k cold scan is **7390.22 ms**,
full maximum **2560.17 ms**. The 100k full target remains exceeded;
the implemented >2-second warning advises narrowing `.codexundoignore`. No claim
of 100k performance acceptance. Read-only Git metadata checks passed.
Linux ext4/btrfs timing remains separate acceptance. File content is copied
and hashed; reflink optimization is not implemented.

Evidence: [initial 10k](benchmark-10000.json),
[second 10k](benchmark-10000-optimized.json),
[latest 10k](benchmark-10000-lazy.json),
[targeted 10k](benchmark-10000-targeted.json),
[earlier 100k](benchmark-100000.json), [lazy-cache 100k](benchmark-100000-lazy.json),
[final 10k](benchmark-10000-final.json), [final 100k](benchmark-100000-final.json).

## Distribution and publication (M4)

- MIT license, English/Chinese README, real transcript GIF, release notes and
  author-only issue/social/thank-you drafts prepared.
- `cargo fmt`, `cargo clippy --locked --all-targets -- -D warnings`, Rust tests,
  schema capture check and `cargo package --locked --allow-dirty` completed.
  Final package verification rebuilt the packaged crate; private evidence,
  temporary installs and the Python venv are explicitly excluded from Cargo
  packaging. Fresh local installation used a private prefix; isolated hooks
  install/uninstall passed without touching shared/global definitions.
- Final-source macOS/Linux x64/arm64 release builds completed. All four
  executables passed actual `--version` runs; macOS x64 used Rosetta, Linux x64
  used Docker emulation. Linux GNU builds are dynamically linked and were tested
  in Debian Bookworm containers. [Binary/archive evidence](builds.json) records
  real `file` output and SHA-256 for each platform. Local archives contain the
  executable, MIT license and both READMEs; all four checksum checks passed.
  The release workflow builds all four archives, SHA256SUMS and a **draft**
  prerelease; it never publishes automatically.
- Homebrew HEAD formula Ruby syntax passed. Tap installation is not claimed
  before an accessible source repository and reviewed formula exist.
- crates.io publish, Homebrew tap merge, production GitHub Release, social posts
  and issue replies have **not** occurred.
- GitHub plugin has valid majiayu000 write access to existing repositories, but
  `majiayu000/codex-undo` returned 404 and the available tools do not expose
  repository creation. CLI credential was invalid. No account was switched.
  Repository creation is awaiting the coordinator's authorized path.
- CI definitions are prepared. **No remote CI run is claimed** until repository,
  branch, workflow permissions and an actual completed run exist.

## Final status

Local implementation, platform checks, crash recovery, source installation,
package verification and four-architecture distribution preparation are complete.
M0 Desktop review/trust and actual runtime evidence are still required. M4 100k
full-checkpoint latency exceeds 2 seconds, ext4/btrfs timing lacks a test
environment, and remote repository/PR/CI/publication remain unavailable.
Production publication and merge remain for review. No social/issue message sent.
