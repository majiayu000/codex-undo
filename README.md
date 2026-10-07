# codex-undo

[简体中文](README.zh-CN.md)

**Codex deleted an uncommitted file? Restore a recorded checkpoint without changing your Git state.**

A Rust command for official Codex hooks. It previews file changes and conflicts,
creates a safety snapshot before writing, and lets you redo a restore.
**v0.1 beta candidate:** real CLI tests passed; desktop validation and
100k performance acceptance remain open. See [the evidence](docs/VALIDATION.md).

![30-second real CLI deletion and restore demo](docs/demo.gif)

From a cloned source checkout (Rust 1.97+):

```sh
cargo install --locked --path .
codex-undo install
```

Restart/resume the Codex CLI, open **`/hooks`**, review and trust the new definitions.
After the first turn, run `codex-undo status` to verify it recorded a checkpoint.
The installer preserves other hook registrations and makes an exact original-file
backup. The command does not change Codex trust or approval settings.

GitHub Releases, crates.io and the Homebrew tap are **not yet published**. The
release workflow prepares four platform archives and a checksum file as a draft.
[The HEAD formula](dist/codex-undo.rb) is prepared for tap review after the repository
exists; it is not a claim that `brew install` currently works.

## Use it

Run in the project root with Codex and its agents idle:

```sh
codex-undo list
codex-undo diff 7
codex-undo undo             # preview and confirmation
codex-undo rewind 5         # before recorded local turn 5
codex-undo redo             # restore the last operation's safety snapshot
codex-undo status
codex-undo gc               # remove unreachable objects, keep all journal history
```

Unexpected current content is a conflict. The default refuses the entire
operation before writing anything. Review the preview, then use `--force` to
explicitly overwrite conflicts, or `--yes` to skip confirmation. These are
separate flags; `--yes` does not override a conflict. Every executed restore has
its own durable safety snapshot, including forced operations.

For multiple sessions in one project, pass `--session ID`; `status --all` lists
IDs. Inside a Codex shell that exposes `CODEX_THREAD_ID`, that value selects the
session. This tool's turn numbers are **local checkpoint numbers**, not verified
indices into the client's conversation menu. The terminal `!` shortcut remains
unverified; using another terminal works in the tested CLI scenarios.

File restore does not change conversation. The next `UserPromptSubmit` hook adds
a reminder to reread affected files. Use Codex's own conversation fork separately.

## What gets recorded

- Visible files in the read-only Git index. No Git subprocess, commit, stash,
  reset, index write, branch change or Git configuration change occurs.
- Paths explicitly declared by `apply_patch` (add/update/delete/move). They can
  include workspace-external paths; links and Git metadata are protected.
- The 100 most recently modified other visible files, each at most 16 MiB.
  Dependency/build directories are skipped. Capped coverage is reported.
- Session tracking only grows. A new path's first explicit observation adds its
  before-tool state to that turn's baseline. Earlier checkpoints without an
  observation for it leave it untouched.

Hidden files are excluded until explicitly edited. Create `.codexundoignore`
using gitignore patterns for paths that should never be recorded, restored or
deleted. The ignore policy at **operation start** applies to both undo and redo.
Outside the workspace, only basename matching applies (for example `*.key`);
workspace-relative directory patterns do not describe external directories.
Snapshots and short prompt excerpts are private local data; exclude sensitive
files before using the recorder.

Structured pre-tool checkpoints refresh declared paths; opaque shell/MCP tools
and turn boundaries inspect the tracked set. Stop/Interrupt and post-tool
checkpoints provide comparison evidence. A hook exits successfully on recording
failure, writes a coverage warning when possible, and never emits a deny/block
or a continuation decision. An OS kill/timeout can prevent the hook from writing
any warning; `status` cannot detect turns whose hooks never ran at all.

## Safe restore and storage

The tool restores **only recorded regular single-link files**. An absent entry is
positive evidence permitting deletion; an omitted entry never permits deletion.
A restore verifies source blobs, writes its safety snapshot and recovery intent,
then replaces individual files with synced temporary files and atomic renames.
It preserves rwx permissions and binary bytes. A failure between files leaves the
recovery record intact; run `redo` to return to the safety snapshot. There is no
multi-file filesystem transaction. Stop all writers before restoring.

Symbolic links, linked parent paths, hard links, directories and special files
are recorded as skipped and are not restored. New empty directories may remain
after undo. Files outside the target manifest stay untouched. Default conflicts
identify content/mode that differs from recorded expectations, not the identity
of whoever edited it.

Storage is independent of `~/.codex` and `.git`:

```text
$XDG_DATA_HOME/codex-undo/                    # Linux default: ~/.local/share/codex-undo
~/Library/Application Support/codex-undo/    # macOS without XDG_DATA_HOME
  blobs/<prefix>/<blake3>
  manifests/<blake3>
  sessions/<hashed-session-id>.jsonl
  statcache/files.json
  lock
```

Use `--data-dir PATH` or `CODEX_UNDO_DATA_DIR` for an isolated store. The advisory
process lock covers hooks and commands using that store. Content and manifest
hashes are checked; a torn final journal append preserves earlier complete
records. The stat cache compares size, inode, device, nanosecond mtime/ctime and
permissions, and distrusts recent fingerprints. Recovery reads full contents.
`gc` validates references before deleting unreachable objects, retains safety
snapshots and history, and clears stat-cache references to collected blobs.

## Coverage limits

| Scenario | v0.1 evidence / boundary |
|---|---|
| Official CLI 0.160.0 | Ten real turns, untracked deletion undo/redo and last-turn undo/redo passed |
| CLI child agents | Real payloads share parent session_id and have separate turn_id + agent_id; assigned to current parent turn |
| CLI fork | Real source=fork observed; payload gives no parent identity, so ancestry is reported unavailable |
| CLI interrupt | Real SIGINT during shell execution produced Interrupt; 3-second hook timeout |
| Desktop app | **Not verified**; bundled CLI version and schemas do not establish desktop support |
| Conversation / `!` UI | Not verified; no menu ordering claim |
| External shell/MCP edits | Unobserved; explicit apply_patch paths can be recorded |
| Bypassed hooks / new opaque files | Partial coverage; no absence inference, so unknown paths are preserved |
| Concurrent sessions | Use separate worktrees; no merge of independent histories |
| Split/sparse/SHA-256 Git index | Unsupported or partial; index errors become visible coverage gaps |
| macOS / Linux | Local tests; filesystem-specific details in validation record |
| Remote, cloud, Windows | Outside v0.1 scope |

## Comparison

| Tool | Client integration | File history |
|---|---|---|
| codex-undo | Official Codex hooks; CLI verified, desktop pending | Independent blobs/manifests; explicit safety snapshot and conflicts |
| [codex-rewind](https://github.com/extracurricular-ai/codex-rewind) | Modified Codex CLI (`codexr`) | Integrated file + conversation rewind/redo; stronger client integration |
| [Claude Code checkpoints](https://code.claude.com/docs/en/checkpointing) | Built into Claude Code | Built-in edit checkpoints; shell edits have documented limits |
| Codex conversation fork | Official client UI | Does not provide this tool's independent file history |

We credit codex-rewind's public correctness discussion for monotonic tracking,
positive absence evidence, symmetric ignores and redo safety. The implementation
is independent; no source copied. This project is not affiliated with OpenAI.

## Development and distribution review

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo package --locked
python -m pip install jsonschema==4.26.0
python scripts/check-protocol.py
python scripts/crash-check.py target/release/codex-undo --output /tmp/crash.json
python scripts/benchmark.py target/release/codex-undo --files 10000
```

Real-client tests use the account's Codex allowance and are opt-in, excluded from
CI. Run `python scripts/real-codex.py --help` in a local authenticated environment.
The weekly protocol workflow checks current official schemas against a pinned
source commit; schema conformance supplements real captures and never replaces
runtime validation. [Release notes](docs/RELEASE.md) and
[launch drafts](docs/LAUNCH-DRAFTS.md) remain for author review.

## Uninstall

```sh
codex-undo uninstall
cargo uninstall codex-undo
```

Other hooks, Codex conversations and Git are preserved. Snapshot data and exact
configuration backups remain for recovery. Remove that data manually only when
you no longer need its history. No global hooks are installed by building or
running tests. MIT licensed.
