use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand};
use codex_undo::*;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Local file checkpoints for official Codex. Restore while agents are idle."
)]
struct Cli {
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    session: Option<String>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Hook {
        event: String,
    },
    List,
    Diff {
        turn: usize,
    },
    Undo {
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        force: bool,
    },
    Rewind {
        turn: usize,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        force: bool,
    },
    Redo {
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        force: bool,
    },
    Status {
        #[arg(long)]
        all: bool,
    },
    Gc,
    Install {
        #[arg(long)]
        hooks_file: Option<PathBuf>,
    },
    Uninstall {
        #[arg(long)]
        hooks_file: Option<PathBuf>,
    },
}
fn hooks_path(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p);
    }
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or(PathBuf::from(std::env::var_os("HOME").context("HOME unset")?).join(".codex"));
    Ok(home.join("hooks.json"))
}
fn install(path: PathBuf, remove: bool) -> Result<()> {
    use fs2::FileExt;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    ensure!(
        !fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()),
        "refusing symlink hooks file"
    );
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path.with_extension("codex-undo.lock"))?;
    lock.lock_exclusive()?;
    let old = match fs::read(&path) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let mut doc: serde_json::Value = match &old {
        Some(b) => serde_json::from_slice(b).context("invalid hooks JSON; left untouched")?,
        None => serde_json::json!({"hooks":{}}),
    };
    ensure!(doc.is_object(), "hooks config must be an object");
    if doc.get("hooks").is_none() {
        doc["hooks"] = serde_json::json!({});
    }
    let hooks = doc["hooks"]
        .as_object_mut()
        .context("hooks must be object")?;
    for groups in hooks.values_mut() {
        let groups = groups.as_array_mut().context("hook event must be array")?;
        groups.retain_mut(|group| {
            let Some(handlers) = group["hooks"].as_array_mut() else {
                return true;
            };
            let before = handlers.len();
            handlers.retain(|h| h["statusMessage"] != "codex-undo: recording checkpoint");
            before == handlers.len() || !handlers.is_empty()
        });
    }
    if !remove {
        let exe = std::env::current_exe()?.canonicalize()?;
        let quoted = format!("'{}'", exe.display().to_string().replace('\'', "'\\''"));
        for event in [
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "Stop",
            "Interrupt",
            "SubagentStop",
        ] {
            let groups = hooks
                .entry(event)
                .or_insert_with(|| serde_json::json!([]))
                .as_array_mut()
                .context("event must be array")?;
            groups.push(serde_json::json!({"hooks":[{"type":"command","command":format!("{quoted} hook {event}"),"timeout":if event=="Interrupt" {3}else {10},"statusMessage":"codex-undo: recording checkpoint"}]}));
        }
    }
    if let Some(old) = old {
        let backup = path.with_extension(format!(
            "codex-undo-backup-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        ensure!(!backup.exists(), "backup collision; retry in one second");
        atomic(&backup, &old, 0o600)?;
        println!("Original configuration backed up: {}", backup.display());
    }
    atomic(&path, &serde_json::to_vec_pretty(&doc)?, 0o600)?;
    println!(
        "{} hooks in {}",
        if remove {
            "Removed codex-undo"
        } else {
            "Installed codex-undo"
        },
        path.display()
    );
    if !remove {
        println!(
            "Restart/resume Codex, open /hooks and review/trust these hooks. Run codex-undo status after a turn to verify recording."
        );
    }
    Ok(())
}
fn show(changes: &[Change]) {
    for c in changes {
        println!(
            "{} {}{}",
            if matches!(c.to, Entry::Absent) {
                "DELETE"
            } else if matches!(c.from, Entry::Absent) {
                "CREATE"
            } else {
                "RESTORE"
            },
            c.path.display(),
            if c.conflict {
                " [CONFLICT: unexpected current content]"
            } else {
                ""
            }
        );
    }
    println!(
        "{} file(s); missing target entries are left untouched.",
        changes.len()
    );
}
fn execute(mut store: Store, mut s: Session, target: String, yes: bool, force: bool) -> Result<()> {
    let preview = store.plan(&s, &target)?;
    show(&preview);
    ensure!(
        force || !preview.iter().any(|c| c.conflict),
        "conflicts; no files changed. Review then pass --force"
    );
    if !preview.is_empty() && !yes {
        print!("Apply these changes after a safety snapshot? [y/N] ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !matches!(input.trim(), "y" | "Y" | "yes") {
            println!("Cancelled; no files changed.");
            return Ok(());
        }
    }
    let n = store.restore(&mut s, &target, force, &preview)?;
    println!(
        "Restored {n} file(s). Use redo to return to the safety snapshot. Conversation unchanged; use Codex's conversation fork UI separately. Turn numbers here are local checkpoint numbers, not menu indices."
    );
    Ok(())
}
fn run(cli: Cli) -> Result<()> {
    if let Command::Install { hooks_file } = cli.command {
        return install(hooks_path(hooks_file)?, false);
    }
    if let Command::Uninstall { hooks_file } = cli.command {
        return install(hooks_path(hooks_file)?, true);
    }
    let root = cli.data_dir.unwrap_or(default_data_dir()?);
    if let Command::Hook { event } = cli.command {
        let mut input = String::new();
        io::stdin()
            .take(16 * 1024 * 1024)
            .read_to_string(&mut input)?;
        let v: serde_json::Value = serde_json::from_str(&input)?;
        ensure!(
            v["hook_event_name"] == event,
            "hook event argument/input mismatch"
        );
        let mut store = Store::open(root)?;
        let result = store.hook(&v);
        if let Err(e) = &result
            && let Some(id) = v["session_id"].as_str()
        {
            let mut s = store.load(id)?;
            if !s.id.is_empty() {
                store.gap(&mut s, format!("{event} failed: {e:#}"))?;
            }
        }
        println!("{}", result?);
        return Ok(());
    }
    let mut store = Store::open(root)?;
    if let Ok(failures) = fs::read_to_string(store.root.join("hook-failures.log")) {
        eprintln!(
            "GAP: {} hook failure(s) in {}",
            failures.lines().count(),
            store.root.join("hook-failures.log").display()
        );
    }
    match cli.command {
        Command::Gc => {
            let (b, m) = store.gc()?;
            println!(
                "Removed {b} unreachable blobs and {m} manifests. All journal history and recovery snapshots retained."
            );
            return Ok(());
        }
        Command::Status { all: true } => {
            for s in store.sessions()? {
                println!(
                    "{} {} turns={} gaps={}",
                    s.id,
                    s.cwd.display(),
                    s.turns.len(),
                    s.gaps.len()
                );
            }
            println!("Store: {} ({} bytes)", store.root.display(), store.bytes()?);
            return Ok(());
        }
        _ => {}
    }
    let cwd = std::env::current_dir()?.canonicalize()?;
    let session = cli
        .session
        .or_else(|| std::env::var("CODEX_THREAD_ID").ok());
    let s = store.select(session.as_deref(), &cwd)?;
    match cli.command {
        Command::List => {
            println!("Session {} {} gaps={}", s.id, s.cwd.display(), s.gaps.len());
            for t in s.turns.iter().rev() {
                let start = store.manifest(&t.start)?;
                let end = t.end.as_deref().map(|id| store.manifest(id)).transpose()?;
                let count = end.as_ref().map(|end| {
                    start
                        .keys()
                        .chain(end.keys())
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .filter(|p| start.get(*p) != end.get(*p))
                        .count()
                });
                println!(
                    "{} {} {} files={}{}",
                    t.number,
                    t.time,
                    t.prompt
                        .chars()
                        .take(60)
                        .collect::<String>()
                        .replace('\n', " "),
                    count.map_or("?".into(), |n| n.to_string()),
                    if t.interrupted {
                        " [interrupted]"
                    } else if !t.finished {
                        " [unfinished]"
                    } else {
                        ""
                    }
                );
            }
        }
        Command::Status { .. } => {
            println!(
                "Session {} | {} turns | {} bytes",
                s.id,
                s.turns.len(),
                store.bytes()?
            );
            println!("Coverage is partial; status cannot detect hooks that never ran.");
            for gap in &s.gaps {
                println!("GAP: {gap}");
            }
            for t in &s.turns {
                if !t.finished {
                    println!("GAP: turn {} lacks final checkpoint", t.number);
                }
            }
            println!("Outstanding tools: {}", s.active_tools.len());
            if let Some(r) = s.restore {
                println!(
                    "Last restore complete={} safety={}; redo available",
                    r.complete, r.safety
                );
            }
            if let Some(latest) = s.latest {
                for e in store.manifest(&latest)?.values() {
                    if let Entry::File { blob, .. } = e {
                        store.blob(blob)?;
                    }
                }
                println!("Latest checkpoint objects verified.");
            }
        }
        Command::Diff { turn } => {
            let t = s
                .turns
                .iter()
                .find(|t| t.number == turn)
                .context("turn not found")?;
            let a = store.manifest(&t.start)?;
            let b = store.manifest(t.end.as_deref().context("turn has no end checkpoint")?)?;
            for p in a
                .keys()
                .chain(b.keys())
                .collect::<std::collections::BTreeSet<_>>()
            {
                if a.get(p) != b.get(p) {
                    println!("{}: {:?} -> {:?}", p.display(), a.get(p), b.get(p));
                    let old = match a.get(p) {
                        Some(Entry::File { blob, .. }) => Some(store.blob(blob)?),
                        Some(Entry::Absent) => Some(Vec::new()),
                        _ => None,
                    };
                    let new = match b.get(p) {
                        Some(Entry::File { blob, .. }) => Some(store.blob(blob)?),
                        Some(Entry::Absent) => Some(Vec::new()),
                        _ => None,
                    };
                    if let (Some(old), Some(new)) = (old, new) {
                        if let (Ok(old), Ok(new)) =
                            (std::str::from_utf8(&old), std::str::from_utf8(&new))
                        {
                            if !old.contains('\0') && !new.contains('\0') {
                                let label = p.display().to_string();
                                print!(
                                    "{}",
                                    similar::TextDiff::from_lines(old, new)
                                        .unified_diff()
                                        .context_radius(3)
                                        .header(&label, &label)
                                );
                            } else {
                                println!("Binary contents differ.");
                            }
                        } else {
                            println!("Binary contents differ.");
                        }
                    }
                }
            }
        }
        Command::Undo { yes, force } => {
            let target = s.turns.last().context("no turns recorded")?.start.clone();
            execute(store, s, target, yes, force)?;
        }
        Command::Rewind { turn, yes, force } => {
            let target = s
                .turns
                .iter()
                .find(|t| t.number == turn)
                .context("turn not found")?
                .start
                .clone();
            execute(store, s, target, yes, force)?;
        }
        Command::Redo { yes, force } => {
            let target = s
                .restore
                .as_ref()
                .context("no restore safety snapshot")?
                .safety
                .clone();
            execute(store, s, target, yes, force)?;
        }
        _ => bail!("unsupported command"),
    }
    Ok(())
}
fn main() {
    let cli = Cli::parse();
    let hook = matches!(cli.command, Command::Hook { .. });
    let root = cli.data_dir.clone().or_else(|| default_data_dir().ok());
    if let Err(e) = run(cli) {
        if hook {
            // Hook failures must never become a Codex blocking decision. Persist a
            // generic warning without input/tool arguments or credential-bearing output.
            if let Some(root) = root {
                let p = root.join("hook-failures.log");
                if fs::create_dir_all(&root).is_ok() {
                    use std::os::unix::fs::OpenOptionsExt;
                    if let Ok(mut f) = fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .mode(0o600)
                        .open(p)
                    {
                        let _ = writeln!(
                            f,
                            "{} checkpoint hook failed; inspect session gaps and rerun status",
                            timestamp()
                        );
                        let _ = f.sync_all();
                    }
                }
            }
            eprintln!(
                "codex-undo: checkpoint failed; coverage gap recorded (hook permits tool execution)"
            );
            println!("{{}}");
        } else {
            eprintln!("codex-undo: {e:#}");
            std::process::exit(1);
        }
    }
}
