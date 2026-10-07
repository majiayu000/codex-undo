# v0.1.0-beta.1 release draft

Codex deleted a file you had never committed? Restore recorded files with a
local checkpoint command, using official Codex hooks and independent storage.

Includes turn checkpoints, apply_patch path capture, conflict previews,
explicit absence records, safety snapshots and redo, symmetric ignore policy,
private content-addressed storage, install/uninstall, integrity status, and gc.

This is a **beta candidate, not approved for production publication**.
Actual desktop app lifecycle, terminal `!` shortcut, conversation menu ordering,
btrfs and complete tool coverage are not claimed. macOS performance targets are
met for the 10k warm fixture (incremental p95 85.21 ms, full p95 196.60 ms);
100k acceptance remains open. Final cold 10k startup is 1.34 seconds
(previous-source run: 2.98 seconds). Consult docs/VALIDATION.md before trusting large workspaces.

Install a downloaded executable on PATH, run `codex-undo install`, then open
Codex `/hooks` to review and trust the definitions. Run status after the first
turn. Restore with all agents idle. The binary does not run Git or modify its
index, branches, commits or configuration. Existing hook registrations survive.

File restore does not restore conversation. Next user prompt receives a reminder
to reread affected files. Use the client's own conversation fork independently.

Acknowledgment: the correct handling of absent/untracked files and ignored paths
was informed by codex-rewind's public design discussion; no source code copied.
