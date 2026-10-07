//! Independent local checkpoints. Correctness ideas credited in docs/DESIGN.md.
#![cfg(unix)]
use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
pub fn default_data_dir() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("CODEX_UNDO_DATA_DIR") {
        return Ok(p.into());
    }
    if let Some(p) = std::env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(p).join("codex-undo"));
    }
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is unset")?);
    Ok(if cfg!(target_os = "macos") {
        home.join("Library/Application Support/codex-undo")
    } else {
        home.join(".local/share/codex-undo")
    })
}
pub fn absolute(p: &Path, cwd: &Path) -> Result<PathBuf> {
    let input = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    };
    let mut out = PathBuf::new();
    for c in input.components() {
        match c {
            Component::ParentDir => {
                ensure!(out.pop(), "path escapes root");
            }
            Component::CurDir => {}
            c => out.push(c.as_os_str()),
        }
    }
    ensure!(out.is_absolute(), "absolute path required");
    Ok(out)
}
fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    ensure!(
        fs::symlink_metadata(path)?.is_dir(),
        "store directory must be a real directory"
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
/// Named-temp-file rename and directory fsync. Old destination survives failures.
pub fn atomic(path: &Path, data: &[u8], mode: u32) -> Result<()> {
    let parent = path.parent().context("path has no parent")?;
    fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(data)?;
    tmp.as_file()
        .set_permissions(fs::Permissions::from_mode(mode))?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum Entry {
    Absent,
    File { blob: String, mode: u32, size: u64 },
    Skipped { reason: String },
}
pub type Manifest = BTreeMap<PathBuf, Entry>;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Turn {
    pub id: String,
    pub number: usize,
    pub time: u64,
    pub prompt: String,
    pub start: String,
    pub end: Option<String>,
    pub interrupted: bool,
    pub finished: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Restore {
    pub safety: String,
    pub target: String,
    pub applied: String,
    pub time: u64,
    pub complete: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Session {
    pub id: String,
    pub cwd: PathBuf,
    pub source: String,
    pub turns: Vec<Turn>,
    pub tracked: BTreeSet<PathBuf>,
    pub gaps: Vec<String>,
    pub latest: Option<String>,
    pub restore: Option<Restore>,
    pub notice: Option<String>,
    pub active_tools: BTreeSet<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "event")]
enum Record {
    Session {
        id: String,
        cwd: PathBuf,
        source: String,
    },
    Turn {
        turn: Turn,
        paths: Vec<PathBuf>,
    },
    Checkpoint {
        number: usize,
        hash: String,
        paths: Vec<PathBuf>,
        stage: String,
    },
    Gap {
        message: String,
    },
    Tool {
        id: String,
        active: bool,
    },
    Restore {
        restore: Restore,
        notice: String,
    },
    Restored,
    NoticeClear,
}
#[derive(Serialize, Deserialize)]
struct Line {
    record: Record,
    checksum: String,
}
impl Session {
    fn apply(&mut self, r: Record) {
        match r {
            Record::Session { id, cwd, source } => {
                self.id = id;
                self.cwd = cwd;
                self.source = source;
            }
            Record::Turn { turn, paths } => {
                self.latest = Some(turn.start.clone());
                self.turns.push(turn);
                self.tracked.extend(paths);
            }
            Record::Checkpoint {
                number,
                hash,
                paths,
                stage,
            } => {
                if stage != "start" {
                    self.latest = Some(hash.clone());
                }
                self.tracked.extend(paths);
                if let Some(t) = self.turns.iter_mut().find(|t| t.number == number) {
                    if stage == "start" {
                        t.start = hash;
                    } else if stage != "pre" {
                        t.end = Some(hash);
                        t.interrupted = stage == "interrupt";
                        t.finished = matches!(stage.as_str(), "stop" | "interrupt");
                    }
                }
            }
            Record::Gap { message } => self.gaps.push(message),
            Record::Tool { id, active } => {
                if active {
                    self.active_tools.insert(id);
                } else {
                    self.active_tools.remove(&id);
                }
            }
            Record::Restore { restore, notice } => {
                self.restore = Some(restore);
                self.notice = Some(notice);
            }
            Record::Restored => {
                if let Some(r) = &mut self.restore {
                    r.complete = true;
                    self.latest = Some(r.applied.clone());
                }
            }
            Record::NoticeClear => self.notice = None,
        }
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Stat {
    dev: u64,
    ino: u64,
    size: u64,
    mtime: i64,
    mtime_ns: i64,
    ctime: i64,
    ctime_ns: i64,
    mode: u32,
}
impl Stat {
    fn new(m: &fs::Metadata) -> Self {
        Self {
            dev: m.dev(),
            ino: m.ino(),
            size: m.len(),
            mtime: m.mtime(),
            mtime_ns: m.mtime_nsec(),
            ctime: m.ctime(),
            ctime_ns: m.ctime_nsec(),
            mode: m.mode() & 0o777,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Cached {
    stat: Stat,
    entry: Entry,
    sampled: u64,
}
pub struct Store {
    pub root: PathBuf,
    _lock: File,
    cache: BTreeMap<PathBuf, Cached>,
    cache_dirty: bool,
    cache_loaded: bool,
}
impl Store {
    pub fn open(root: PathBuf) -> Result<Self> {
        private_dir(&root)?;
        let root = root.canonicalize()?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc_no_follow())
            .open(root.join("lock"))?;
        let start = Instant::now();
        loop {
            match lock.try_lock_exclusive() {
                Ok(()) => break,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        && start.elapsed() < Duration::from_secs(2) =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(e) => {
                    return Err(e).context("checkpoint store busy; retry while Codex is idle");
                }
            }
        }
        for name in ["blobs", "manifests", "sessions", "statcache"] {
            private_dir(&root.join(name))?;
        }
        Ok(Self {
            root,
            _lock: lock,
            cache: BTreeMap::new(),
            cache_dirty: false,
            cache_loaded: false,
        })
    }
    fn object_path(&self, kind: &str, id: &str) -> Result<PathBuf> {
        ensure!(
            id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid object hash"
        );
        Ok(if kind == "blobs" {
            self.root.join(kind).join(&id[..2]).join(id)
        } else {
            self.root.join(kind).join(id)
        })
    }
    fn object(&self, kind: &str, data: &[u8]) -> Result<String> {
        let id = hash(data);
        let p = self.object_path(kind, &id)?;
        if p.exists() {
            ensure!(
                hash(&fs::read(&p)?) == id,
                "corrupt existing {kind} object {id}"
            );
        } else {
            private_dir(p.parent().context("object parent")?)?;
            atomic(&p, data, 0o600)?;
        }
        Ok(id)
    }
    pub fn manifest(&self, id: &str) -> Result<Manifest> {
        let b = fs::read(self.object_path("manifests", id)?)?;
        ensure!(hash(&b) == id, "corrupt manifest {id}");
        Ok(serde_json::from_slice(&b)?)
    }
    pub fn save_manifest(&self, m: &Manifest) -> Result<String> {
        self.object("manifests", &serde_json::to_vec(m)?)
    }
    fn logpath(&self, id: &str) -> PathBuf {
        self.root
            .join("sessions")
            .join(format!("{}.jsonl", hash(id.as_bytes())))
    }
    pub fn load(&self, id: &str) -> Result<Session> {
        self.read_log(&self.logpath(id))
    }
    fn read_log(&self, p: &Path) -> Result<Session> {
        let mut s = Session::default();
        let bytes = match fs::read(p) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(s),
            Err(e) => return Err(e.into()),
        };
        // A killed append can leave an incomplete tail. Completed records are checksummed.
        for line in bytes.split_inclusive(|b| *b == b'\n') {
            if line.last() != Some(&b'\n') {
                s.gaps
                    .push("incomplete journal tail; previous durable records retained".into());
                break;
            }
            let l: Line = serde_json::from_slice(line)
                .context("corrupt completed journal record; do not restore")?;
            ensure!(
                hash(&serde_json::to_vec(&l.record)?) == l.checksum,
                "journal checksum mismatch"
            );
            s.apply(l.record);
        }
        Ok(s)
    }
    fn append(&self, s: &mut Session, r: Record) -> Result<()> {
        let checksum = hash(&serde_json::to_vec(&r)?);
        let mut b = serde_json::to_vec(&Line {
            record: r.clone(),
            checksum,
        })?;
        b.push(b'\n');
        let p = self.logpath(&s.id);
        let mut f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .append(true)
            .mode(0o600)
            .custom_flags(libc_no_follow())
            .open(&p)?;
        let mut old = Vec::new();
        f.read_to_end(&mut old)?;
        if !old.is_empty() && old.last() != Some(&b'\n') {
            let end = old.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
            f.set_len(end as u64)?;
        }
        f.write_all(&b)?;
        f.sync_all()?;
        File::open(p.parent().context("journal parent")?)?.sync_all()?;
        s.apply(r);
        Ok(())
    }
    pub fn gap(&self, s: &mut Session, message: String) -> Result<()> {
        if !s.gaps.contains(&message) {
            self.append(s, Record::Gap { message })?;
        }
        Ok(())
    }
    pub fn sessions(&self) -> Result<Vec<Session>> {
        let mut out = Vec::new();
        for e in fs::read_dir(self.root.join("sessions"))? {
            let p = e?.path();
            if p.extension().is_some_and(|x| x == "jsonl") {
                out.push(self.read_log(&p)?);
            }
        }
        Ok(out)
    }
    pub fn select(&self, id: Option<&str>, cwd: &Path) -> Result<Session> {
        if let Some(id) = id {
            let s = self.load(id)?;
            ensure!(!s.id.is_empty(), "session not recorded");
            return Ok(s);
        }
        let mut sessions = self
            .sessions()?
            .into_iter()
            .filter(|s| s.cwd == cwd)
            .collect::<Vec<_>>();
        ensure!(
            sessions.len() == 1,
            "found {} sessions for this workspace; pass --session ID (status --all lists IDs)",
            sessions.len()
        );
        Ok(sessions.remove(0))
    }
    pub fn safe_path(&self, p: &Path) -> Result<()> {
        self.safe_path_checked(p, &mut BTreeSet::new())
    }
    fn safe_path_checked(&self, p: &Path, directories: &mut BTreeSet<PathBuf>) -> Result<()> {
        self.safe_path_through(p, directories, true)
    }
    fn safe_path_through(
        &self,
        p: &Path,
        directories: &mut BTreeSet<PathBuf>,
        leaf: bool,
    ) -> Result<()> {
        ensure!(
            p.is_absolute() && !p.starts_with(&self.root),
            "refusing protected store path"
        );
        ensure!(
            !p.components()
                .any(|c| c.as_os_str() == ".git" || matches!(c, Component::ParentDir)),
            "refusing Git metadata or non-normalized path"
        );
        if !leaf
            && p.parent()
                .is_some_and(|parent| directories.contains(parent))
        {
            return Ok(());
        }
        let mut cur = PathBuf::new();
        for c in p.components() {
            cur.push(c);
            if cur == p && !leaf {
                break;
            }
            if cur != p && directories.contains(&cur) {
                continue;
            }
            match fs::symlink_metadata(&cur) {
                Ok(m) => {
                    ensure!(!m.file_type().is_symlink(), "symbolic link path skipped");
                    if cur != p && m.is_dir() {
                        directories.insert(cur.clone());
                    }
                    if cur == p && m.is_file() {
                        ensure!(m.nlink() == 1, "hard-linked file skipped");
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
    pub fn capture(&mut self, p: &Path, cache: bool) -> Result<Entry> {
        self.capture_checked(p, cache, &mut BTreeSet::new())
    }
    fn capture_checked(
        &mut self,
        p: &Path,
        cache: bool,
        directories: &mut BTreeSet<PathBuf>,
    ) -> Result<Entry> {
        if cache && !self.cache_loaded {
            // Structured incremental checkpoints need no global stat-cache parse.
            // Cache is disposable; malformed entries cause complete rehashing.
            let mut cached: BTreeMap<PathBuf, Cached> =
                fs::read(self.root.join("statcache/files.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or_default();
            cached.extend(std::mem::take(&mut self.cache));
            self.cache = cached;
            self.cache_loaded = true;
        }
        if let Err(e) = self.safe_path_through(p, directories, false) {
            return Ok(Entry::Skipped {
                reason: e.to_string(),
            });
        }
        let m = match fs::symlink_metadata(p) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Entry::Absent),
            Err(e) => return Err(e).with_context(|| format!("inspect {}", p.display())),
        };
        if !m.is_file() || m.nlink() != 1 {
            return Ok(Entry::Skipped {
                reason: "not a regular single-link file".into(),
            });
        }
        let stat = Stat::new(&m);
        if cache
            && let Some(c) = self.cache.get(p)
            && c.stat == stat
            && timestamp().saturating_sub(c.sampled) > 2
            && timestamp().saturating_sub(stat.ctime.max(0) as u64) > 2
        {
            return Ok(c.entry.clone());
        }
        let mut f = OpenOptions::new()
            .read(true)
            .custom_flags(libc_no_follow())
            .open(p)?;
        ensure!(
            Stat::new(&f.metadata()?) == stat,
            "file changed before snapshot: {}",
            p.display()
        );
        let mut b = Vec::new();
        f.read_to_end(&mut b)?;
        ensure!(
            Stat::new(&f.metadata()?) == stat && Stat::new(&fs::symlink_metadata(p)?) == stat,
            "file changed during snapshot: {}",
            p.display()
        );
        let blob = self.object("blobs", &b)?;
        let entry = Entry::File {
            blob,
            mode: stat.mode,
            size: stat.size,
        };
        self.cache_dirty = true;
        self.cache.insert(
            p.to_path_buf(),
            Cached {
                stat,
                entry: entry.clone(),
                sampled: timestamp(),
            },
        );
        Ok(entry)
    }
    pub fn snapshot(
        &mut self,
        paths: &BTreeSet<PathBuf>,
        policy: &Policy,
        cache: bool,
    ) -> Result<String> {
        let mut m = Manifest::new();
        // Directory checks are shared only within this one checkpoint. Leaves are
        // always inspected; restores do not use this optimization.
        let mut directories = BTreeSet::new();
        for p in paths {
            if !policy.ignored(p) {
                m.insert(p.clone(), self.capture_checked(p, cache, &mut directories)?);
            }
        }
        let id = self.save_manifest(&m)?;
        if self.cache_dirty {
            atomic(
                &self.root.join("statcache/files.json"),
                &serde_json::to_vec(&self.cache)?,
                0o600,
            )?;
            self.cache_dirty = false;
        }
        Ok(id)
    }
    fn checkpoint(
        &mut self,
        s: &mut Session,
        number: usize,
        stage: &str,
        discover: bool,
    ) -> Result<String> {
        let policy = Policy::new(&s.cwd)?;
        let mut paths = s.tracked.clone();
        if discover {
            let (new, gaps) = discover_paths(&s.cwd, &policy)?;
            paths.extend(new);
            for g in gaps {
                self.gap(s, g)?;
            }
        }
        let added = paths.difference(&s.tracked).cloned().collect();
        let id = self.snapshot(&paths, &policy, true)?;
        for (p, e) in self.manifest(&id)? {
            if let Entry::Skipped { reason } = e {
                self.gap(s, format!("skipped {}: {reason}", p.display()))?;
            }
        }
        self.append(
            s,
            Record::Checkpoint {
                number,
                hash: id.clone(),
                paths: added,
                stage: stage.into(),
            },
        )?;
        Ok(id)
    }
    pub fn hook(&mut self, v: &serde_json::Value) -> Result<serde_json::Value> {
        let started = Instant::now();
        let id = v["session_id"]
            .as_str()
            .context("hook session_id missing")?;
        let cwd = PathBuf::from(v["cwd"].as_str().context("hook cwd missing")?).canonicalize()?;
        let event = v["hook_event_name"]
            .as_str()
            .context("hook_event_name missing")?;
        let mut s = self.load(id)?;
        if s.id.is_empty() {
            s.id = id.into();
            self.append(
                &mut s,
                Record::Session {
                    id: id.into(),
                    cwd: cwd.clone(),
                    source: v["source"].as_str().unwrap_or("hook").into(),
                },
            )?;
        }
        ensure!(
            s.cwd == cwd,
            "session workspace changed; select original workspace"
        );
        let turn = v["turn_id"].as_str().unwrap_or("");
        let child = v["agent_id"].is_string();
        if event == "SessionStart" {
            self.gap(&mut s,"coverage limited to recorded hook events; fork ancestry unavailable from SessionStart payload".into())?;
            return Ok(serde_json::json!({}));
        }
        let mut output = serde_json::json!({});
        if event == "UserPromptSubmit" && !child {
            if let Some(last) = s.turns.last()
                && !last.finished
            {
                self.gap(
                    &mut s,
                    "previous turn lacks Stop/Interrupt checkpoint".into(),
                )?;
            }
            let policy = Policy::new(&cwd)?;
            let (new, gaps) = discover_paths(&cwd, &policy)?;
            for g in gaps {
                self.gap(&mut s, g)?;
            }
            let mut paths = s.tracked.clone();
            paths.extend(new);
            let start = self.snapshot(&paths, &policy, true)?;
            let number = s.turns.len() + 1;
            let prompt = v["prompt"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(200)
                .collect();
            let t = Turn {
                id: turn.into(),
                number,
                time: timestamp(),
                prompt,
                start,
                end: None,
                interrupted: false,
                finished: false,
            };
            let added = paths.difference(&s.tracked).cloned().collect();
            self.append(
                &mut s,
                Record::Turn {
                    turn: t,
                    paths: added,
                },
            )?;
            if let Some(notice) = s.notice.clone() {
                output = serde_json::json!({"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":notice}});
                self.append(&mut s, Record::NoticeClear)?;
            }
        } else if matches!(
            event,
            "PreToolUse" | "PostToolUse" | "Stop" | "Interrupt" | "SubagentStop"
        ) {
            if s.turns.is_empty() {
                self.gap(
                    &mut s,
                    "tool/stop observed without UserPromptSubmit; no turn-start coverage".into(),
                )?;
                return Ok(output);
            }
            let number = if child {
                s.turns.last().context("parent turn")?.number
            } else {
                s.turns
                    .iter()
                    .find(|t| t.id == turn)
                    .context("unrecognized turn_id; no checkpoint attribution")?
                    .number
            };
            if event == "PreToolUse" {
                let paths = tool_paths(v, &cwd)?;
                let policy = Policy::new(&cwd)?;
                let mut start: Option<Manifest> = None;
                let mut added = Vec::new();
                for p in &paths {
                    if policy.ignored(p) {
                        continue;
                    }
                    if !s.tracked.contains(p) {
                        if start.is_none() {
                            start = Some(self.manifest(&s.turns[number - 1].start)?);
                        }
                        start
                            .as_mut()
                            .context("baseline missing")?
                            .entry(p.clone())
                            .or_insert(self.capture(p, false)?);
                        added.push(p.clone());
                    }
                }
                if !added.is_empty() {
                    let hash = self.save_manifest(start.as_ref().context("baseline missing")?)?;
                    self.append(
                        &mut s,
                        Record::Checkpoint {
                            number,
                            hash,
                            paths: added,
                            stage: "start".into(),
                        },
                    )?;
                }
                if matches!(
                    v["tool_name"].as_str(),
                    Some("apply_patch" | "Edit" | "Write")
                ) {
                    // Structured pre-tool checkpoints refresh only declared paths.
                    // Full boundaries and opaque tools still inspect the tracked set.
                    let hash = if paths.is_empty() {
                        s.latest.clone().context("latest checkpoint missing")?
                    } else {
                        let mut manifest = self
                            .manifest(s.latest.as_deref().context("latest checkpoint missing")?)?;
                        for p in &paths {
                            if !policy.ignored(p) {
                                manifest.insert(p.clone(), self.capture(p, false)?);
                            }
                        }
                        self.save_manifest(&manifest)?
                    };
                    self.append(
                        &mut s,
                        Record::Checkpoint {
                            number,
                            hash,
                            paths: Vec::new(),
                            stage: "pre".into(),
                        },
                    )?;
                } else {
                    self.checkpoint(&mut s, number, "pre", false)?;
                }
                if let Some(tool) = v["tool_use_id"].as_str() {
                    self.append(
                        &mut s,
                        Record::Tool {
                            id: tool.into(),
                            active: true,
                        },
                    )?;
                }
                if v["tool_name"] != "apply_patch" {
                    self.gap(&mut s,"shell/MCP/opaque tool: workspace-external edits and uncaptured new-file absence cannot be inferred".into())?;
                }
            } else {
                self.checkpoint(
                    &mut s,
                    number,
                    if event == "Interrupt" {
                        "interrupt"
                    } else if event == "Stop" {
                        "stop"
                    } else {
                        "end"
                    },
                    true,
                )?;
                if let Some(tool) = v["tool_use_id"].as_str() {
                    self.append(
                        &mut s,
                        Record::Tool {
                            id: tool.into(),
                            active: false,
                        },
                    )?;
                }
                if matches!(event, "Stop" | "Interrupt") {
                    for id in s.active_tools.clone() {
                        self.gap(&mut s,"PreToolUse without PostToolUse: denied/failed tool or interruption; intermediate checkpoint coverage unavailable".into())?;
                        self.append(&mut s, Record::Tool { id, active: false })?;
                    }
                }
                if event == "Interrupt" {
                    self.gap(
                        &mut s,
                        "interrupted turn; verify tool completion before restore".into(),
                    )?;
                }
            }
        }
        if started.elapsed() > Duration::from_secs(2) {
            self.gap(
                &mut s,
                "slow checkpoint (>2 seconds); narrow .codexundoignore to reduce hook timeouts"
                    .into(),
            )?;
        }
        Ok(output)
    }
    pub fn plan(&mut self, s: &Session, target: &str) -> Result<Vec<Change>> {
        ensure!(
            s.active_tools.is_empty(),
            "tools still active or missing PostToolUse; wait for completion and inspect status"
        );
        let policy = Policy::new(&s.cwd)?;
        let m = self.manifest(target)?;
        let expected = match &s.latest {
            Some(id) => self.manifest(id)?,
            None => Manifest::new(),
        };
        let mut out = Vec::new();
        for (p, to) in m {
            if policy.ignored(&p) || !s.tracked.contains(&p) || matches!(to, Entry::Skipped { .. })
            {
                continue;
            }
            self.safe_path(&p)?;
            let from = self.capture(&p, false)?;
            ensure!(
                !matches!(from, Entry::Skipped { .. }),
                "unsafe restore path: {}",
                p.display()
            );
            if from != to {
                let mut conflict = expected.get(&p) != Some(&from);
                if let Some(r) = &s.restore
                    && !r.complete
                {
                    let safety = self.manifest(&r.safety)?;
                    let applied = self.manifest(&r.applied)?;
                    if safety.get(&p) == Some(&from) || applied.get(&p) == Some(&from) {
                        conflict = false;
                    }
                }
                out.push(Change {
                    path: p,
                    from,
                    to,
                    conflict,
                });
            }
        }
        Ok(out)
    }
    pub fn restore(
        &mut self,
        s: &mut Session,
        target: &str,
        force: bool,
        preview: &[Change],
    ) -> Result<usize> {
        // Recompute after confirmation; an external edit during preview must not be overwritten.
        let changes = self.plan(s, target)?;
        ensure!(
            changes == preview,
            "workspace changed after preview; run again"
        );
        ensure!(
            force || !changes.iter().any(|c| c.conflict),
            "conflicts present; inspect and use --force to override"
        );
        if changes.is_empty() {
            return Ok(0);
        }
        let policy = Policy::new(&s.cwd)?;
        let mut paths = s.tracked.clone();
        paths.extend(self.manifest(target)?.into_keys());
        let safety = self.snapshot(&paths, &policy, false)?;
        // Validate every source blob before any mutation.
        for c in &changes {
            if let Entry::File { blob, .. } = &c.to {
                self.blob(blob)?;
            }
        }
        let mut applied = self.manifest(&safety)?;
        for c in &changes {
            applied.insert(c.path.clone(), c.to.clone());
        }
        let applied = self.save_manifest(&applied)?;
        let notice="The user restored workspace files with codex-undo. Earlier file changes may no longer be on disk. Re-read affected files before continuing. Conversation history was not restored.".into();
        self.append(
            s,
            Record::Restore {
                restore: Restore {
                    safety,
                    target: target.into(),
                    applied,
                    time: timestamp(),
                    complete: false,
                },
                notice,
            },
        )?;
        for c in &changes {
            self.safe_path(&c.path)?;
            ensure!(
                self.capture(&c.path, false)? == c.from,
                "concurrent file edit; restore stopped; redo safety snapshot retained"
            );
            match &c.to {
                Entry::File { blob, mode, .. } => atomic(&c.path, &self.blob(blob)?, *mode)?,
                Entry::Absent => {
                    fs::remove_file(&c.path)?;
                    File::open(c.path.parent().context("file parent")?)?.sync_all()?;
                }
                Entry::Skipped { .. } => unreachable!("plan filters skipped files"),
            }
            self.cache.remove(&c.path);
        }
        self.append(s, Record::Restored)?;
        Ok(changes.len())
    }
    pub fn blob(&self, id: &str) -> Result<Vec<u8>> {
        let b = fs::read(self.object_path("blobs", id)?)?;
        ensure!(hash(&b) == id, "corrupt blob {id}");
        Ok(b)
    }
    pub fn bytes(&self) -> Result<u64> {
        let mut bytes = 0;
        for e in walkdir::WalkDir::new(&self.root) {
            let e = e?;
            if e.file_type().is_file() {
                bytes += e.metadata()?.len();
            }
        }
        Ok(bytes)
    }
    pub fn gc(&mut self) -> Result<(usize, usize)> {
        let mut manifests = BTreeSet::new();
        // Retain historical restore records too: interrupted operations remain recoverable.
        for e in fs::read_dir(self.root.join("sessions"))? {
            let p = e?.path();
            if p.extension().is_none_or(|x| x != "jsonl") {
                continue;
            }
            self.read_log(&p)?;
            let bytes = fs::read(p)?;
            for l in bytes.split_inclusive(|b| *b == b'\n') {
                if l.last() != Some(&b'\n') {
                    continue;
                }
                let l: Line = serde_json::from_slice(l)?;
                match l.record {
                    Record::Turn { turn, .. } => {
                        manifests.insert(turn.start);
                        if let Some(e) = turn.end {
                            manifests.insert(e);
                        }
                    }
                    Record::Checkpoint { hash, .. } => {
                        manifests.insert(hash);
                    }
                    Record::Restore { restore, .. } => {
                        manifests.insert(restore.safety);
                        manifests.insert(restore.target);
                        manifests.insert(restore.applied);
                    }
                    _ => {}
                }
            }
        }
        let mut blobs = BTreeSet::new();
        for id in &manifests {
            for e in self.manifest(id)?.into_values() {
                if let Entry::File { blob, .. } = e {
                    self.blob(&blob)?;
                    blobs.insert(blob);
                }
            }
        }
        let mut counts = (0, 0);
        for (kind, live) in [("manifests", &manifests), ("blobs", &blobs)] {
            for e in walkdir::WalkDir::new(self.root.join(kind)) {
                let e = e?;
                if e.file_type().is_file()
                    && !live.contains(e.file_name().to_str().context("invalid object name")?)
                {
                    fs::remove_file(e.path())?;
                    if kind == "blobs" {
                        counts.0 += 1;
                    } else {
                        counts.1 += 1;
                    }
                }
            }
        }
        // Cached blobs may have been orphaned by interrupted checkpoints.
        self.cache.clear();
        self.cache_dirty = false;
        atomic(&self.root.join("statcache/files.json"), b"{}", 0o600)?;
        Ok(counts)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub path: PathBuf,
    pub from: Entry,
    pub to: Entry,
    pub conflict: bool,
}
#[derive(Clone)]
pub struct Policy {
    cwd: PathBuf,
    matcher: Gitignore,
}
impl Policy {
    pub fn new(cwd: &Path) -> Result<Self> {
        let mut builder = GitignoreBuilder::new(cwd);
        let p = cwd.join(".codexundoignore");
        if p.exists()
            && let Some(e) = builder.add(p)
        {
            return Err(e.into());
        }
        Ok(Self {
            cwd: cwd.into(),
            matcher: builder.build()?,
        })
    }
    pub fn ignored(&self, p: &Path) -> bool {
        if self.matcher.is_empty() {
            return false;
        }
        // External explicit paths use root-relative matching when inside the project;
        // outside paths can still match basename rules such as *.key.
        let candidate = match p.strip_prefix(&self.cwd) {
            Ok(relative) => relative,
            Err(_) => Path::new(p.file_name().unwrap_or_default()),
        };
        self.matcher
            .matched_path_or_any_parents(candidate, false)
            .is_ignore()
    }
}
fn libc_no_follow() -> i32 {
    #[cfg(target_os = "macos")]
    {
        0x100
    }
    #[cfg(not(target_os = "macos"))]
    {
        0x20000
    }
}
pub fn tool_paths(v: &serde_json::Value, cwd: &Path) -> Result<BTreeSet<PathBuf>> {
    let mut out = BTreeSet::new();
    if v["tool_name"] == "apply_patch" {
        let patch = v["tool_input"]["command"]
            .as_str()
            .context("apply_patch command missing")?;
        for line in patch.lines() {
            for prefix in [
                "*** Add File: ",
                "*** Update File: ",
                "*** Delete File: ",
                "*** Move to: ",
            ] {
                if let Some(p) = line.strip_prefix(prefix) {
                    out.insert(absolute(Path::new(p), cwd)?);
                }
            }
        }
    } else if matches!(v["tool_name"].as_str(), Some("Edit" | "Write"))
        && let Some(p) = v["tool_input"]["file_path"].as_str()
    {
        out.insert(absolute(Path::new(p), cwd)?);
    }
    Ok(out)
}
pub fn discover_paths(cwd: &Path, policy: &Policy) -> Result<(BTreeSet<PathBuf>, Vec<String>)> {
    let mut paths = BTreeSet::new();
    let mut gaps = Vec::new();
    // Read the nearest repository's index; worktree gitfiles resolve without Git commands.
    if let Some(root) = cwd.ancestors().find(|p| p.join(".git").exists()) {
        let dot = root.join(".git");
        let gitdir = if dot.is_file() {
            let b = fs::read_to_string(&dot)?;
            absolute(
                Path::new(
                    b.trim()
                        .strip_prefix("gitdir: ")
                        .context("invalid .git file")?,
                ),
                root,
            )?
        } else {
            dot
        };
        if gitdir.join("index").exists() {
            let config = fs::read_to_string(gitdir.join("config")).unwrap_or_default();
            ensure!(
                !config.contains("objectformat = sha256"),
                "SHA-256 Git index not supported yet"
            );
            let index = gix_index::File::at(
                gitdir.join("index"),
                gix_hash::Kind::Sha1,
                false,
                Default::default(),
            )
            .context("read Git index")?;
            ensure!(!index.is_sparse(), "sparse Git index coverage unavailable");
            for entry in index.entries() {
                let p = PathBuf::from(std::ffi::OsString::from_vec(entry.path(&index).to_vec()));
                let full = absolute(&p, root)?;
                if !p
                    .components()
                    .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
                    && !policy.ignored(&full)
                {
                    paths.insert(full);
                }
            }
        }
    }
    let mut recent = Vec::new();
    let mut omitted = 0;
    let iter = walkdir::WalkDir::new(cwd)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let n = e.file_name().to_string_lossy();
            !n.starts_with('.')
                && !matches!(
                    n.as_ref(),
                    "node_modules" | "target" | "vendor" | "dist" | "build" | "__pycache__"
                )
                && !policy.ignored(e.path())
        });
    for e in iter {
        let e = e?;
        if !e.file_type().is_file() || paths.contains(e.path()) {
            continue;
        }
        let m = e.metadata()?;
        if m.len() > 16 * 1024 * 1024 {
            omitted += 1;
            continue;
        }
        recent.push((m.mtime(), m.mtime_nsec(), e.into_path()));
    }
    recent.sort_unstable_by(|a, b| b.cmp(a));
    if recent.len() > 100 {
        omitted += recent.len() - 100;
    }
    paths.extend(recent.into_iter().take(100).map(|(_, _, p)| p));
    if omitted > 0 {
        gaps.push(format!("recent-file coverage limit: {omitted} untracked candidates excluded (100 files / 16 MiB each)"));
    }
    Ok((paths, gaps))
}
use std::os::unix::ffi::OsStringExt;

#[cfg(test)]
mod crash_tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> (tempfile::TempDir, Store, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("workspace");
        fs::create_dir(&cwd).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        let store = Store::open(tmp.path().join("store")).unwrap();
        (tmp, store, cwd)
    }
    #[test]
    fn incomplete_log_tail_is_recoverable_completed_corruption_is_rejected() {
        let (_tmp, mut store, cwd) = fixture();
        store.hook(&json!({"session_id":"s","cwd":cwd,"turn_id":"t","hook_event_name":"UserPromptSubmit","prompt":"test"})).unwrap();
        let p = store.logpath("s");
        let mut f = OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b"{partial").unwrap();
        f.sync_all().unwrap();
        let mut s = store.load("s").unwrap();
        assert_eq!(s.turns.len(), 1);
        assert!(!s.gaps.is_empty());
        store.gap(&mut s, "recovered torn append".into()).unwrap();
        assert!(
            store
                .load("s")
                .unwrap()
                .gaps
                .contains(&"recovered torn append".into())
        );
        let mut f = OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b"corrupt complete line\n").unwrap();
        f.sync_all().unwrap();
        assert!(store.load("s").is_err());
        assert!(store.gc().is_err());
    }
    #[test]
    fn partial_restore_can_redo_and_orphan_objects_are_collected() {
        let (_tmp, mut store, cwd) = fixture();
        fs::write(cwd.join("a"), b"before").unwrap();
        fs::write(cwd.join("b"), b"before").unwrap();
        store.hook(&json!({"session_id":"s","cwd":cwd,"turn_id":"t","hook_event_name":"UserPromptSubmit","prompt":"test"})).unwrap();
        fs::write(cwd.join("a"), b"after").unwrap();
        fs::write(cwd.join("b"), b"after").unwrap();
        store
            .hook(&json!({"session_id":"s","cwd":cwd,"turn_id":"t","hook_event_name":"Stop"}))
            .unwrap();
        let mut s = store.load("s").unwrap();
        let target = s.turns[0].start.clone();
        let safety = s.latest.clone().unwrap();
        let mut applied = store.manifest(&safety).unwrap();
        let target_manifest = store.manifest(&target).unwrap();
        applied.extend(target_manifest);
        let applied = store.save_manifest(&applied).unwrap();
        store
            .append(
                &mut s,
                Record::Restore {
                    restore: Restore {
                        safety: safety.clone(),
                        target,
                        applied,
                        time: timestamp(),
                        complete: false,
                    },
                    notice: "restore interrupted".into(),
                },
            )
            .unwrap();
        // Model a process death after the durable intent and only the first write.
        fs::write(cwd.join("a"), b"before").unwrap();
        let mut s = store.load("s").unwrap();
        let plan = store.plan(&s, &safety).unwrap();
        assert!(!plan.iter().any(|c| c.conflict));
        store.restore(&mut s, &safety, false, &plan).unwrap();
        assert_eq!(fs::read(cwd.join("a")).unwrap(), b"after");
        assert_eq!(fs::read(cwd.join("b")).unwrap(), b"after");
        store.object("blobs", b"unreferenced").unwrap();
        assert!(store.gc().unwrap().0 >= 1);
    }
}
