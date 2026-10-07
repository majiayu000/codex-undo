# Launch drafts — author reviews and posts; nothing sent

Do not publish until the acceptance gaps in VALIDATION.md are resolved and a
reviewed GitHub Release exists. Replace the repository link with the live release
only after confirming installation from a clean environment.

## GitHub issue #9203

I built a small hook-based file-checkpoint tool for official Codex. In a real CLI
test it restored an untracked note after a shell deletion, and redo returned to
the deleted state. It keeps snapshots outside your Git repository and previews
conflicts before writing. It is a beta: snapshots are partial, files without an
explicit recorded baseline are left alone, and it does not restore conversation.
Desktop lifecycle and large-workspace latency still need validation. Code and
validation record: https://github.com/majiayu000/codex-undo

## Rewind request issue (verify exact issue before posting)

This experiment supplies the file half of rewind through official Codex hooks.
You select a local recorded turn with `codex-undo rewind N`; the command creates a
safety snapshot first and can redo the restore. Conversation branching remains
Codex's own UI. It cannot promise full tool coverage or infer files it never
observed. This is not an official `/rewind` implementation. Validation and source:
https://github.com/majiayu000/codex-undo

## X

Codex deleted an untracked note. A recorded local checkpoint brought it back;
redo returned to the deleted state. codex-undo uses official Codex hooks and
independent storage. Beta; partial coverage and explicit conflict previews.
[Attach the 30-second real deletion demo after release acceptance.]

## Show HN

Title: Show HN: File checkpoints and safe restore through official Codex hooks

Why not Git? Untracked deletions are exactly the scenario Git cannot restore
without having captured the contents first. This tool reads the index to choose
files but stores blobs and manifests separately. It never commits or stashes.
The main tradeoff is incomplete hook/file coverage; positive absence evidence is
required before any deletion. The current beta's measured limits are documented.

## Thank-you to codex-rewind authors (draft only)

Thank you for documenting the correctness traps around untracked paths, ignored
files, and redo safety. Those ideas informed an independently implemented
hook-based experiment for official Codex. The README links and credits your
project; no source code was copied. No message has been sent.
