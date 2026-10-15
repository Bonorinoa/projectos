//! M3 local Git broker. No network, hooks, tests, commits, or canonical writes.
//! Applying a reviewed patch remains held until graph-owned recovery integration.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io::Read, path::{Component, Path, PathBuf}, process::{Command, Stdio}, sync::{Arc, atomic::{AtomicBool, Ordering}}, thread, time::{Duration, Instant}};
use uuid::Uuid;

type Result<T> = std::result::Result<T, String>;
const LIMIT: usize = 16 * 1024 * 1024;
const FILE_LIMIT: usize = 1000;
const DEADLINE: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Serialize)]
pub struct RepositoryInfo { pub root: String, pub head: String, pub branch: String, pub dirty_files: Vec<String> }
#[derive(Clone, Debug, Serialize)]
pub struct WorktreeInfo { pub id: String, pub path: String, pub base_head: String }
#[derive(Clone, Debug, Serialize)]
pub struct ChangedFile { pub path: String, pub status: String, pub before_hash: Option<String>, pub after_hash: Option<String>, pub diff: String }
#[derive(Clone, Debug, Serialize)]
pub struct PatchReview { pub id: String, pub worktree_id: String, pub base_head: String, pub content_hash: String, pub files: Vec<ChangedFile>, pub capability_notes: Vec<String>, pub can_apply: bool }
/// Owned bytes after explicit confirmation and revalidation; never writes canonical files.
#[derive(Debug)]
pub struct ReviewedFile { pub path: String, pub before: Option<Vec<u8>>, pub after: Option<Vec<u8>> }
#[derive(Debug)]
pub struct ReviewedPatch { pub base_head: String, pub content_hash: String, pub files: Vec<ReviewedFile> }
#[derive(Clone)]
struct Blob { bytes: Vec<u8>, executable: bool }
struct Stage { info: WorktreeInfo, base: BTreeMap<String, Blob> }
pub struct SoftwareBroker { root: PathBuf, staging_parent: PathBuf, stages: BTreeMap<String, Stage>, reviews: BTreeMap<String, PatchReview> }

fn digest(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn io(e: std::io::Error) -> String { e.to_string() }
fn text(bytes: Vec<u8>) -> Result<String> { String::from_utf8(bytes).map_err(|_| "Non-UTF-8 Git output is unsupported".into()) }
fn safe_path(name: &str) -> Result<()> {
    if name.is_empty() || Path::new(name).components().count()>64 || name.contains('\\') || name.chars().any(char::is_control) || Path::new(name).components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err("Unsafe repository path".into());
    }
    for part in Path::new(name).components() {
        let lower = part.as_os_str().to_string_lossy().to_ascii_lowercase();
        if lower == ".git" || lower == ".projectos" || lower == ".gitmodules" || lower == ".gitattributes" { return Err("Repository metadata, submodules, and attributes are unsupported patch paths".into()); }
    }
    Ok(())
}
/// Reject links at every existing component rather than canonicalizing through them.
fn no_links(path: &Path) -> Result<()> {
    let mut at = PathBuf::new();
    for c in path.components() {
        at.push(c.as_os_str());
        if let Ok(meta) = fs::symlink_metadata(&at) {
            if meta.file_type().is_symlink() { return Err("Symlinks are unsupported".into()); }
        }
    }
    Ok(())
}
fn walk(root: &Path, omit_metadata: bool) -> Result<BTreeMap<String, Blob>> {
    fn visit(root: &Path, dir: &Path, out: &mut BTreeMap<String, Blob>, total: &mut usize, count: &mut usize, omit: bool) -> Result<()> {
        for entry in fs::read_dir(dir).map_err(io)? {
            let entry = entry.map_err(io)?;
            let path = entry.path();
            let name = path.strip_prefix(root).map_err(|e| e.to_string())?.to_str().ok_or("Non-UTF-8 filename")?.to_string();
            if omit && (name == ".git" || name == ".projectos") { continue; }
            *count += 1;
            if *count > FILE_LIMIT * 10 { return Err("Repository traversal limit exceeded".into()); }
            let meta = fs::symlink_metadata(&path).map_err(io)?;
            if meta.file_type().is_symlink() { return Err("Symlinks are unsupported".into()); }
            if meta.is_dir() { safe_path(&name)?; visit(root, &path, out, total, count, omit)?; }
            else if meta.is_file() {
                safe_path(&name)?;
                *total = total.checked_add(meta.len() as usize).ok_or("Size overflow")?;
                if *total > LIMIT || out.len() >= FILE_LIMIT { return Err("Repository file/byte limit exceeded".into()); }
                let bytes = fs::read(&path).map_err(io)?;
                if bytes.len() as u64 != meta.len() { return Err("File changed during read".into()); }
                #[cfg(unix)] let executable = { use std::os::unix::fs::PermissionsExt; meta.permissions().mode() & 0o111 != 0 };
                #[cfg(not(unix))] let executable = false;
                out.insert(name, Blob { bytes, executable });
            } else { return Err("Special files are unsupported".into()); }
        }
        Ok(())
    }
    no_links(root)?;
    let mut out = BTreeMap::new();
    visit(root, root, &mut out, &mut 0, &mut 0, omit_metadata)?;
    Ok(out)
}
/// Read config as inert text before Git can parse any local includes or helpers.
fn check_metadata(root: &Path) -> Result<()> {
    no_links(root)?;
    let git = root.join(".git");
    if !fs::symlink_metadata(&git).map_err(io)?.is_dir() { return Err("Select the canonical repository root; bare/nested/worktree repositories are unsupported".into()); }
    fn check(dir: &Path, count: &mut usize) -> Result<()> {
        for e in fs::read_dir(dir).map_err(io)? {
            let path = e.map_err(io)?.path();
            *count += 1;
            if *count > 50000 || path.components().count()>128 { return Err("Git metadata traversal limit exceeded".into()); }
            let m = fs::symlink_metadata(&path).map_err(io)?;
            if m.file_type().is_symlink() || (!m.is_dir() && !m.is_file()) { return Err("Unsafe Git metadata".into()); }
            if m.is_dir() { check(&path, count)?; }
        }
        Ok(())
    }
    check(&git, &mut 0)?;
    for forbidden in ["objects/info/alternates", "objects/info/http-alternates", "info/grafts", "info/attributes", "config.worktree", "shallow"] {
        if git.join(forbidden).exists() { return Err(format!("Unsupported Git metadata: {forbidden}")); }
    }
    let config_path = git.join("config");
    if fs::metadata(&config_path).map_err(io)?.len() > 65536 { return Err("Git config exceeds limit".into()); }
    let cfg = fs::read_to_string(config_path).map_err(io)?;
    let mut section = String::new();
    for line in cfg.lines() {
        let lower = line.trim().to_ascii_lowercase();
        if lower.is_empty() || lower.starts_with('#') || lower.starts_with(';') { continue; }
        if lower.contains('\\') { return Err("Escaped/multiline Git config is unsupported".into()); }
        if lower.starts_with('[') {
            section = lower.trim_start_matches('[').split([' ',']','"']).next().unwrap_or("").to_string();
            if !matches!(section.as_str(),"core"|"user"|"remote"|"branch") { return Err("Unsupported Git config section".into()); }
        } else {
            let (key,value) = lower.split_once('=').ok_or("Unsupported Git config syntax")?;
            let key = key.trim(); let value = value.trim();
            let allowed = match section.as_str() {
                "core" => matches!(key,"repositoryformatversion"|"filemode"|"bare"|"logallrefupdates"|"ignorecase"|"precomposeunicode"),
                "user" => matches!(key,"name"|"email"),
                "remote" => matches!(key,"url"|"fetch"|"pushurl"),
                "branch" => matches!(key,"remote"|"merge"),
                _ => false,
            };
            if !allowed || (key=="bare" && value!="false") || (key=="repositoryformatversion" && value!="0") { return Err("Unsupported Git config value".into()); }
        }
    }
    Ok(())
}

/// Scan only Git-visible paths, so ignored build/dependency directories are outside the capability.
fn canonical_files(root: &Path) -> Result<()> {
    let listing = text(git(root,&["ls-files","--cached","--others","--exclude-standard","-z"],false)?)?;
    let mut count=0; let mut total=0usize;
    for name in listing.split('\0').filter(|s|!s.is_empty()) {
        if name.split('/').any(|p|p.eq_ignore_ascii_case(".projectos")) { continue; }
        safe_path(name)?; let path=root.join(name); no_links(&path)?;
        if let Ok(meta)=fs::symlink_metadata(&path) {
            if !meta.is_file() { return Err("Submodule/special file unsupported".into()); }
            count+=1; total=total.checked_add(meta.len() as usize).ok_or("Size overflow")?;
            if count>FILE_LIMIT || total>LIMIT { return Err("Repository file/byte limit exceeded".into()); }
        }
    }
    Ok(())
}

/// Only the broker's fixed argv calls reach this function. Ambient Git/PATH is discarded.
fn git(root: &Path, args: &[&str], allow_difference: bool) -> Result<Vec<u8>> {
    let mut cmd = Command::new("/usr/bin/git");
    cmd.env_clear().env("PATH", "/usr/bin:/bin").env("HOME", "/nonexistent").env("LANG", "C").env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0").env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_ATTR_NOSYSTEM", "1").env("GIT_NO_LAZY_FETCH", "1")
        .args(["--no-pager", "-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=false", "-c", "core.attributesFile=/dev/null", "-c", "credential.helper=", "-c", "protocol.allow=never", "-c", "core.quotePath=false", "-C"])
        .arg(root).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(io)?;
    let exceeded = Arc::new(AtomicBool::new(false));
    fn reader(mut pipe: impl Read + Send + 'static, exceeded: Arc<AtomicBool>) -> thread::JoinHandle<Vec<u8>> {
        thread::spawn(move || { let mut out = Vec::new(); let mut buf = [0; 8192]; loop { match pipe.read(&mut buf) { Ok(0) | Err(_) => break, Ok(n) => { if out.len() + n > LIMIT { exceeded.store(true, Ordering::Relaxed); break; } out.extend_from_slice(&buf[..n]); } } } out })
    }
    let stdout = reader(child.stdout.take().ok_or("Missing stdout")?, exceeded.clone());
    let stderr = reader(child.stderr.take().ok_or("Missing stderr")?, exceeded.clone());
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if exceeded.load(Ordering::Relaxed) || start.elapsed() > DEADLINE {
            timed_out = true; let _ = child.kill(); break child.wait().map_err(io)?;
        }
        if let Some(status) = child.try_wait().map_err(io)? { break status; }
        thread::sleep(Duration::from_millis(5));
    };
    let out = stdout.join().map_err(|_| "Git stdout reader failed")?;
    let _err = stderr.join().map_err(|_| "Git stderr reader failed")?;
    if timed_out || exceeded.load(Ordering::Relaxed) { return Err("Git command exceeded output/time limit".into()); }
    if !status.success() && !(allow_difference && status.code() == Some(1)) { return Err(format!("Local Git command failed (status {})", status.code().unwrap_or(-1))); }
    Ok(out)
}
fn head(root: &Path) -> Result<String> {
    let h = text(git(root, &["rev-parse", "--verify", "HEAD^{commit}"], false)?)?.trim().to_string();
    if !matches!(h.len(),40|64) || !h.bytes().all(|c| c.is_ascii_hexdigit()) { return Err("Invalid HEAD".into()); }
    Ok(h)
}
fn tree(root: &Path, base: &str) -> Result<BTreeMap<String, Blob>> {
    let listing = text(git(root, &["ls-tree", "-rz", "--full-tree", base], false)?)?;
    let mut out = BTreeMap::new(); let mut total: usize = 0;
    for entry in listing.split('\0').filter(|s| !s.is_empty()) {
        let (meta, path) = entry.split_once('\t').ok_or("Malformed tree")?;
        if path.split('/').any(|p| p.eq_ignore_ascii_case(".projectos")) { continue; }
        safe_path(path)?;
        let parts: Vec<_> = meta.split(' ').collect();
        if parts.len()!=3 || !matches!(parts[0],"100644"|"100755") || parts[1]!="blob" { return Err("Symlink/submodule/special Git tree entry unsupported".into()); }
        let oid = parts[2];
        if !matches!(oid.len(),40|64) || !oid.bytes().all(|c| c.is_ascii_hexdigit()) { return Err("Invalid blob id".into()); }
        let bytes = git(root, &["cat-file", "blob", oid], false)?;
        total = total.checked_add(bytes.len()).ok_or("Size overflow")?;
        if total > LIMIT || out.len() >= FILE_LIMIT { return Err("Tree file/byte limit exceeded".into()); }
        out.insert(path.to_string(), Blob{bytes, executable:parts[0]=="100755"});
    }
    Ok(out)
}

impl SoftwareBroker {
    pub fn new(canonical_root: &Path, staging_parent: &Path) -> Result<Self> {
        no_links(canonical_root)?; no_links(staging_parent)?;
        let root = fs::canonicalize(canonical_root).map_err(io)?;
        let parent = fs::canonicalize(staging_parent).map_err(io)?;
        if parent.starts_with(&root) || root.starts_with(&parent) { return Err("Worktree staging must be outside the canonical project".into()); }
        let broker = Self{root, staging_parent:parent, stages:BTreeMap::new(), reviews:BTreeMap::new()};
        broker.inspect()?; Ok(broker)
    }
    pub fn inspect(&self) -> Result<RepositoryInfo> {
        check_metadata(&self.root)?;
        canonical_files(&self.root)?;
        let h = head(&self.root)?;
        tree(&self.root,&h)?;
        let branch = text(git(&self.root,&["rev-parse","--abbrev-ref","HEAD"],false)?)?.trim().to_string();
        let status = text(git(&self.root,&["status","--porcelain=v1","-z","--untracked-files=all","--", ".", ":(exclude).projectos"],false)?)?;
        // Keep Git's status fields intact; NUL-separated rename paths remain separate entries.
        let dirty_files = status.split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect();
        Ok(RepositoryInfo{root:self.root.to_string_lossy().into_owned(),head:h,branch,dirty_files})
    }
    pub fn review_binding(&self, review_id: &str) -> Result<PatchReview> {
        self.reviews.get(review_id).cloned().ok_or("Unknown review; capture a fresh patch review".into())
    }
    pub fn create_worktree(&mut self, confirmed: bool) -> Result<WorktreeInfo> {
        if !confirmed { return Err("Explicit worktree creation confirmation required".into()); }
        let repo = self.inspect()?;
        let base = tree(&self.root,&repo.head)?;
        no_links(&self.staging_parent)?;
        let id = Uuid::new_v4().to_string();
        let path = self.staging_parent.join(format!("projectos-worktree-{id}"));
        let p = path.to_str().ok_or("Non-UTF-8 staging path")?;
        git(&self.root,&["worktree","add","--detach","--no-checkout",p,&repo.head],false)?;
        // Populate inert bytes ourselves: Git checkout/smudge filters never execute.
        let populate = (|| -> Result<()> {
            for (name,blob) in &base {
                let target = path.join(name);
                fs::create_dir_all(target.parent().ok_or("Missing parent")?).map_err(io)?;
                fs::write(&target,&blob.bytes).map_err(io)?;
                #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(target,fs::Permissions::from_mode(if blob.executable {0o755} else {0o644})).map_err(io)?; }
            }
            // Initialize only index metadata, with no checkout or hooks.
            git(&path,&["read-tree",&repo.head],false)?;
            Ok(())
        })();
        if let Err(e) = populate { let _ = git(&self.root,&["worktree","remove","--force",p],false); return Err(e); }
        let info = WorktreeInfo{id:id.clone(),path:p.into(),base_head:repo.head};
        self.stages.insert(id,Stage{info:info.clone(),base}); Ok(info)
    }
    fn stage_files(&self, id:&str) -> Result<(&Stage,BTreeMap<String,Blob>)> {
        check_metadata(&self.root)?;
        let stage = self.stages.get(id).ok_or("Unknown worktree; reopen/create a new staging run")?;
        let path = Path::new(&stage.info.path);
        no_links(path)?;
        let pointer = path.join(".git");
        no_links(&pointer)?;
        let expected = self.root.join(".git/worktrees").join(path.file_name().ok_or("Missing worktree name")?);
        let actual = fs::read_to_string(&pointer).map_err(io)?;
        if actual.trim() != format!("gitdir: {}", expected.display()) { return Err("Worktree identity changed".into()); }
        if fs::symlink_metadata(path.join(".projectos")).is_ok() { return Err("An execution worktree cannot have canonical .projectos state".into()); }
        Ok((stage,walk(path,true)?))
    }
    pub fn review(&mut self, worktree_id: &str) -> Result<PatchReview> {
        let (stage,after) = self.stage_files(worktree_id)?;
        let mut files = Vec::new(); let mut identity = Vec::new();
        let mut names: Vec<_> = stage.base.keys().chain(after.keys()).cloned().collect(); names.sort(); names.dedup();
        for name in names {
            let before = stage.base.get(&name); let next = after.get(&name);
            if before.map(|b|(&b.bytes,b.executable)) == next.map(|b|(&b.bytes,b.executable)) { continue; }
            if before.map(|b|b.executable) != next.map(|b|b.executable) && before.is_some() && next.is_some() { return Err("Executable mode changes are unsupported".into()); }
            let old = before.map(|b|b.bytes.as_slice()).unwrap_or_default(); let new = next.map(|b|b.bytes.as_slice()).unwrap_or_default();
            if old.contains(&0) || new.contains(&0) || std::str::from_utf8(old).is_err() || std::str::from_utf8(new).is_err() { return Err("Binary/non-UTF-8 patch review is unsupported".into()); }
            let before_hash = before.map(|b|digest(&b.bytes)); let after_hash = next.map(|b|digest(&b.bytes));
            identity.extend_from_slice(&(name.len() as u64).to_le_bytes()); identity.extend_from_slice(name.as_bytes());
            for blob in [before,next] { identity.push(u8::from(blob.is_some())); if let Some(b)=blob { identity.push(u8::from(b.executable)); identity.extend_from_slice(&(b.bytes.len() as u64).to_le_bytes()); identity.extend_from_slice(&b.bytes); } }
            // An inert complete unified hunk is deliberately simple; all changed bytes are visible.
            let old_lines = std::str::from_utf8(old).map_err(|e|e.to_string())?.split_inclusive('\n').collect::<Vec<_>>();
            let new_lines = std::str::from_utf8(new).map_err(|e|e.to_string())?.split_inclusive('\n').collect::<Vec<_>>();
            let quote = |p:&str| format!("\"{}\"",p.replace('"',"\\\""));
            let a = quote(&format!("a/{name}")); let b = quote(&format!("b/{name}"));
            let mut diff = format!("diff --git {a} {b}\n");
            if before.is_none() { diff.push_str(if next.is_some_and(|b|b.executable) {"new file mode 100755\n"}else{"new file mode 100644\n"}); }
            if next.is_none() { diff.push_str(if before.is_some_and(|b|b.executable) {"deleted file mode 100755\n"}else{"deleted file mode 100644\n"}); }
            if !old_lines.is_empty() || !new_lines.is_empty() {
                diff.push_str(&format!("--- {}\n+++ {}\n@@ -{},{} +{},{} @@\n",if before.is_some(){a}else{"/dev/null".into()},if next.is_some(){b}else{"/dev/null".into()},if old_lines.is_empty(){0}else{1},old_lines.len(),if new_lines.is_empty(){0}else{1},new_lines.len()));
            }
            for (prefix,lines) in [('-',old_lines),('+',new_lines)] { for line in lines { diff.push(prefix); diff.push_str(line); if !line.ends_with('\n') { diff.push_str("\n\\ No newline at end of file\n"); } } }
            files.push(ChangedFile{path:name,status:if before.is_none(){"added"}else if next.is_none(){"deleted"}else{"modified"}.into(),before_hash,after_hash,diff});
        }
        if files.iter().map(|f|f.diff.len()).sum::<usize>()>LIMIT { return Err("Patch output limit exceeded".into()); }
        identity.extend_from_slice(stage.info.base_head.as_bytes());
        let result = PatchReview{id:Uuid::new_v4().to_string(),worktree_id:worktree_id.into(),base_head:stage.info.base_head.clone(),content_hash:digest(&identity),files,capability_notes:vec!["Local inert file review only; no tests, hooks, network, commit, or push.".into(),"Applying changes awaits graph-owned multi-file recovery integration.".into(),"Worker execution and computer use remain held by Gate C.".into()],can_apply:false};
        self.reviews.insert(result.id.clone(),result.clone()); Ok(result)
    }
    /// Return exactly the reviewed patch after revalidating its base and bytes.
    pub fn export_review(&mut self, review_id:&str, expected_hash:&str) -> Result<String> {
        let reviewed = self.reviews.get(review_id).cloned().ok_or("Unknown review")?;
        self.validate_review(review_id,expected_hash,true)?;
        Ok(reviewed.files.iter().map(|f|f.diff.as_str()).collect::<Vec<_>>().join(""))
    }
    pub fn validate_review(&mut self, review_id:&str, expected_hash:&str, confirmed:bool) -> Result<ReviewedPatch> {
        if !confirmed { return Err("Explicit reviewed confirmation required".into()); }
        let reviewed = self.reviews.get(review_id).cloned().ok_or("Unknown review")?;
        if reviewed.content_hash != expected_hash { return Err("Review hash does not match confirmation".into()); }
        self.inspect()?;
        if head(&self.root)? != reviewed.base_head { return Err("Canonical HEAD changed; review again".into()); }
        let current = self.review(&reviewed.worktree_id)?;
        if current.content_hash != reviewed.content_hash { return Err("Worktree content changed; review again".into()); }
        let (stage,after) = self.stage_files(&reviewed.worktree_id)?;
        let mut files = Vec::new();
        for file in &reviewed.files {
            safe_path(&file.path)?;
            let target = self.root.join(&file.path); no_links(&target)?;
            let before = stage.base.get(&file.path).map(|b|b.bytes.clone());
            let actual = match fs::read(&target) { Ok(b)=>Some(b), Err(e) if e.kind()==std::io::ErrorKind::NotFound=>None,Err(e)=>return Err(io(e)) };
            if actual != before { return Err("Canonical content changed; reviewed patch is stale".into()); }
            let next = after.get(&file.path).map(|b|b.bytes.clone());
            if before.as_ref().map(|b|digest(b)) != file.before_hash || next.as_ref().map(|b|digest(b)) != file.after_hash { return Err("Patch changed during validation".into()); }
            files.push(ReviewedFile{path:file.path.clone(),before,after:next});
        }
        Ok(ReviewedPatch{base_head:reviewed.base_head,content_hash:reviewed.content_hash,files})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir,SoftwareBroker) {
        let dir=tempfile::tempdir().unwrap();
        let root=dir.path().join("repo"); let stages=dir.path().join("stages");
        fs::create_dir(&root).unwrap(); fs::create_dir(&stages).unwrap();
        let raw=|args:&[&str]| { let out=Command::new("/usr/bin/git").env_clear().env("PATH","/usr/bin:/bin").env("HOME","/nonexistent").env("GIT_CONFIG_NOSYSTEM","1").env("GIT_CONFIG_GLOBAL","/dev/null").args(["-C",root.to_str().unwrap()]).args(args).output().unwrap(); assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr)); };
        raw(&["init","-q"]); raw(&["config","user.name","Fixture"]); raw(&["config","user.email","fixture@example.invalid"]);
        fs::write(root.join("draft.txt"),"before\n").unwrap(); raw(&["add","draft.txt"]); raw(&["commit","-qm","fixture"]);
        let root=fs::canonicalize(root).unwrap(); let stages=fs::canonicalize(stages).unwrap();
        let broker=SoftwareBroker::new(&root,&stages).unwrap(); (dir,broker)
    }
    #[test]
    fn isolated_stage_and_export_bind_exact_bytes_without_canonical_writes() {
        let (_dir,mut broker)=fixture();
        assert!(broker.create_worktree(false).is_err());
        let info=broker.create_worktree(true).unwrap();
        assert!(!Path::new(&info.path).join(".projectos").exists());
        fs::write(Path::new(&info.path).join("draft.txt"),"after\n").unwrap();
        let r=broker.review(&info.id).unwrap(); assert_eq!(r.files.len(),1); assert!(!r.can_apply);
        assert!(broker.validate_review(&r.id,&r.content_hash,false).is_err());
        assert!(broker.export_review(&r.id,"wrong").is_err());
        let patch=broker.export_review(&r.id,&r.content_hash).unwrap(); assert!(patch.contains("-before\n+after\n"));
        assert_eq!(fs::read_to_string(broker.root.join("draft.txt")).unwrap(),"before\n");
        // Export must be a real patch, including files with no trailing newline.
        fs::write(Path::new(&info.path).join("new.txt"),"new text").unwrap();
        let r=broker.review(&info.id).unwrap(); let patch=broker.export_review(&r.id,&r.content_hash).unwrap();
        let patch_path=broker.staging_parent.join("review.patch"); fs::write(&patch_path,patch).unwrap();
        git(&broker.root,&["apply","--check",patch_path.to_str().unwrap()],false).unwrap();
    }
    #[test]
    #[cfg(unix)]
    fn empty_and_executable_patch_export_preserves_bytes_and_modes() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir,mut broker)=fixture();
        fs::write(broker.root.join("delete-empty.txt"),b"").unwrap();
        git(&broker.root,&["add","delete-empty.txt"],false).unwrap(); git(&broker.root,&["commit","-qm","empty"],false).unwrap();
        let info=broker.create_worktree(true).unwrap(); let stage=Path::new(&info.path);
        fs::remove_file(stage.join("delete-empty.txt")).unwrap(); fs::write(stage.join("empty space.txt"),b"").unwrap();
        fs::write(stage.join("run.sh"),"#!/bin/sh\necho inert\n").unwrap(); fs::set_permissions(stage.join("run.sh"),fs::Permissions::from_mode(0o755)).unwrap();
        let r=broker.review(&info.id).unwrap(); let patch=broker.export_review(&r.id,&r.content_hash).unwrap();
        assert!(patch.contains("new file mode 100755"));
        let patch_file=broker.staging_parent.join("exact.patch"); fs::write(&patch_file,patch).unwrap();
        git(&broker.root,&["apply",patch_file.to_str().unwrap()],false).unwrap();
        assert_eq!(fs::read(broker.root.join("empty space.txt")).unwrap(),b"");
        assert!(!broker.root.join("delete-empty.txt").exists());
        assert_ne!(fs::metadata(broker.root.join("run.sh")).unwrap().permissions().mode()&0o111,0);
    }
    #[test]
    fn stale_worktree_and_canonical_files_refuse_export() {
        let (_dir,mut broker)=fixture(); let info=broker.create_worktree(true).unwrap();
        let path=Path::new(&info.path).join("draft.txt"); fs::write(&path,"after\n").unwrap(); let r=broker.review(&info.id).unwrap();
        fs::write(&path,"changed again\n").unwrap(); assert!(broker.export_review(&r.id,&r.content_hash).unwrap_err().contains("Worktree content changed"));
        let r=broker.review(&info.id).unwrap(); fs::write(broker.root.join("draft.txt"),"canonical changed\n").unwrap();
        assert!(broker.export_review(&r.id,&r.content_hash).unwrap_err().contains("Canonical content changed"));
    }
    #[test]
    fn local_hooks_filters_includes_fsmonitor_and_external_diff_fail_closed() {
        for extra in ["[core]\n hooksPath = /tmp/execute\n","[core]\n fsmonitor = malicious-command\n","[filter \"evil\"]\n smudge = touch /tmp/execute\n","[include]\n path = /tmp/evil\n","[diff \"evil\"]\n textconv = malicious\n","[core]\n worktree = /tmp/outside\n","[extensions]\n partialClone = origin\n"] {
            let (_dir,broker)=fixture(); let config=broker.root.join(".git/config"); let mut bytes=fs::read_to_string(&config).unwrap(); bytes.push_str(extra); fs::write(config,bytes).unwrap(); assert!(broker.inspect().is_err(),"Accepted {extra}");
        }
    }
    #[test]
    fn ambient_git_environment_is_discarded() {
        let (_dir,broker)=fixture();
        let previous_dir=std::env::var_os("GIT_DIR");let previous_tree=std::env::var_os("GIT_WORK_TREE");
        unsafe { std::env::set_var("GIT_DIR","/definitely/not/projectos");std::env::set_var("GIT_WORK_TREE","/definitely/not/projectos"); }
        let inspected=broker.inspect();
        unsafe {
            match previous_dir {Some(value)=>std::env::set_var("GIT_DIR",value),None=>std::env::remove_var("GIT_DIR")}
            match previous_tree {Some(value)=>std::env::set_var("GIT_WORK_TREE",value),None=>std::env::remove_var("GIT_WORK_TREE")}
        }
        assert!(inspected.is_ok());
    }
    #[test]
    fn hooks_are_never_executed_during_worktree_creation() {
        let (_dir,mut broker)=fixture(); let marker=broker.staging_parent.join("hook-executed"); let hook=broker.root.join(".git/hooks/post-checkout");
        fs::write(&hook,format!("#!/bin/sh\ntouch '{}'\n",marker.display())).unwrap();
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&hook,fs::Permissions::from_mode(0o755)).unwrap(); }
        broker.create_worktree(true).unwrap(); assert!(!marker.exists());
    }
    #[test]
    fn unsupported_attributes_binary_and_capability_escape_are_refused() {
        let (_dir,mut broker)=fixture(); let info=broker.create_worktree(true).unwrap();
        let staged=Path::new(&info.path); fs::write(staged.join(".gitattributes"),"* filter=evil").unwrap(); assert!(broker.review(&info.id).is_err());
        fs::remove_file(staged.join(".gitattributes")).unwrap(); fs::write(staged.join("binary"),[0,1,2]).unwrap(); assert!(broker.review(&info.id).is_err());
        fs::remove_file(staged.join("binary")).unwrap(); fs::create_dir(staged.join(".projectos")).unwrap(); assert!(broker.review(&info.id).is_err());
        assert!(SoftwareBroker::new(&broker.root,&broker.root).is_err());
        assert!(SoftwareBroker::new(&broker.root.join(".git"),&broker.staging_parent).is_err());
    }
    #[test]
    fn nested_repositories_and_control_character_filenames_are_refused() {
        let (_dir,mut broker)=fixture();let info=broker.create_worktree(true).unwrap();let staged=Path::new(&info.path);
        fs::create_dir_all(staged.join("nested/.git")).unwrap();fs::write(staged.join("nested/.git/config"),"[core]\n").unwrap();
        assert!(broker.review(&info.id).is_err());
        fs::remove_dir_all(staged.join("nested")).unwrap();
        fs::write(staged.join("line\nbreak.txt"),"inert").unwrap();
        assert!(broker.review(&info.id).is_err());
    }
    #[test]
    #[cfg(unix)]
    fn symlinks_and_changed_worktree_identity_are_refused() {
        use std::os::unix::fs::symlink;
        let (_dir,mut broker)=fixture(); let info=broker.create_worktree(true).unwrap();
        let staged=Path::new(&info.path); symlink(broker.root.join("draft.txt"),staged.join("escape")).unwrap(); assert!(broker.review(&info.id).is_err());
        fs::remove_file(staged.join("escape")).unwrap(); fs::write(staged.join(".git"),"gitdir: /tmp/outside\n").unwrap(); assert!(broker.review(&info.id).is_err());
    }
    #[test]
    fn dropped_session_retains_dirty_stage_and_canonical_bytes() {
        let (_dir,mut broker)=fixture(); let info=broker.create_worktree(true).unwrap(); let path=Path::new(&info.path).join("draft.txt"); fs::write(&path,"unsaved staged work\n").unwrap(); drop(broker); assert_eq!(fs::read_to_string(path).unwrap(),"unsaved staged work\n");
    }
    #[test]
    fn tracked_projectos_metadata_is_not_copied_into_execution_stage() {
        let (_dir,mut broker)=fixture(); fs::create_dir(broker.root.join(".projectos")).unwrap(); fs::write(broker.root.join(".projectos/project.toml"),"private graph state").unwrap();
        git(&broker.root,&["add","--force",".projectos/project.toml"],false).unwrap(); git(&broker.root,&["commit","-qm","metadata fixture"],false).unwrap();
        let info=broker.create_worktree(true).unwrap(); assert!(!Path::new(&info.path).join(".projectos").exists());
        fs::write(Path::new(&info.path).join("draft.txt"),"reviewable\n").unwrap();
        let review=broker.review(&info.id).unwrap();assert_eq!(review.files.len(),1);
        assert!(!broker.export_review(&review.id,&review.content_hash).unwrap().contains(".projectos"));
    }

    #[test]
    fn stale_head_oversized_changes_and_unusual_names_are_bounded() {
        let (_dir,mut broker)=fixture();let info=broker.create_worktree(true).unwrap();let stage=Path::new(&info.path);
        fs::write(stage.join("space \"quote\" café.txt"),"literal text\n").unwrap();
        let reviewed=broker.review(&info.id).unwrap();
        let patch=broker.export_review(&reviewed.id,&reviewed.content_hash).unwrap();
        let patch_path=broker.staging_parent.join("unusual.patch");fs::write(&patch_path,patch).unwrap();
        git(&broker.root,&["apply","--check",patch_path.to_str().unwrap()],false).unwrap();
        fs::write(stage.join("oversized"),vec![b'x';LIMIT+1]).unwrap();assert!(broker.review(&info.id).is_err());
        fs::remove_file(stage.join("oversized")).unwrap();
        git(&broker.root,&["-c","user.name=Fixture","-c","user.email=fixture@example.invalid","commit","--allow-empty","-qm","new head"],false).unwrap();
        assert!(broker.export_review(&reviewed.id,&reviewed.content_hash).unwrap_err().contains("HEAD changed"));
    }

    #[test]
    #[cfg(unix)]
    fn symlinked_git_metadata_is_refused_before_git_runs() {
        use std::os::unix::fs::symlink;
        let (_dir,broker)=fixture();let config=broker.root.join(".git/config");let outside=broker.staging_parent.join("config");
        fs::rename(&config,&outside).unwrap();symlink(&outside,&config).unwrap();
        assert!(broker.inspect().unwrap_err().contains("Unsafe Git metadata"));
    }
}
