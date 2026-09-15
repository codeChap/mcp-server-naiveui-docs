//! Clone / archive / jsDelivr ingest. `fetch_into_empty_dir` is the only populate path.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::sources::{
    JSDELIVR_FILE_TEMPLATE, JSDELIVR_LIST_TEMPLATE, REMOTE_GIT, REMOTE_ID, archive_url, clone_dir,
    is_safe_rel, looks_like_sha, resolve_rev,
};

const GIT_TIMEOUT: Duration = Duration::from_secs(120);
const LOCK_TIMEOUT: Duration = Duration::from_secs(180);
const JSDELIVR_OVERALL: Duration = Duration::from_secs(180);
const JSDELIVR_FILE_TIMEOUT: u64 = 30;
const JSDELIVR_WORKERS: usize = 8;

struct DirLock(PathBuf);

impl Drop for DirLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

fn acquire_lock(cache: &Path) -> Result<DirLock> {
    std::fs::create_dir_all(cache)?;
    let p = cache.join(".sync.lock");
    let start = Instant::now();
    loop {
        match std::fs::create_dir(&p) {
            Ok(()) => return Ok(DirLock(p)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if start.elapsed() > LOCK_TIMEOUT {
                    bail!("timed out waiting for cache lock {}", p.display());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e).context("create cache lock dir"),
        }
    }
}

pub fn same_git_rev(a: &str, b: &str) -> bool {
    let n = a.len().min(b.len());
    n >= 7 && a[..n].eq_ignore_ascii_case(&b[..n])
}

pub fn git_rev(dir: &Path) -> Result<String> {
    let out = git_run(Some(dir), ["rev-parse", "HEAD"])?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn read_mcp_origin(dir: &Path) -> Option<(String, String)> {
    let text = std::fs::read_to_string(dir.join(".mcp-origin")).ok()?;
    let mut lines = text.lines().map(str::trim).filter(|s| !s.is_empty());
    let origin = lines.next()?.to_string();
    let rev = lines.next()?.to_string();
    Some((origin, rev))
}

fn write_mcp_origin(dir: &Path, origin: &str, rev: &str) -> Result<()> {
    std::fs::write(dir.join(".mcp-origin"), format!("{origin}\n{rev}\n"))
        .with_context(|| format!("write {}", dir.join(".mcp-origin").display()))
}

fn checkout_rev() -> String {
    resolve_rev(std::env::var("NAIVE_UI_MCP_REV").ok().as_deref())
        .unwrap_or_else(|| "HEAD".to_string())
}

fn is_unpinned_rev(rev: &str) -> bool {
    crate::sources::is_unpinned(rev)
}

fn rev_matches(recorded: &str, want: &str) -> bool {
    recorded == want || same_git_rev(recorded, want)
}

pub fn ensure_sources(cache: &Path, force: bool) -> Result<String> {
    let _lock = acquire_lock(cache)?;
    std::fs::create_dir_all(cache.join("src"))?;
    let dest = clone_dir(cache);
    let rev = checkout_rev();

    if dest.join(".git").exists() && !force {
        let msg = checkout_existing(&dest, &rev)?;
        return Ok(format!("[{REMOTE_ID}] {msg}"));
    }

    if dest.exists()
        && !force
        && let Some((origin, recorded)) = read_mcp_origin(&dest)
        && rev_matches(&recorded, &rev)
    {
        return Ok(format!("[{REMOTE_ID}] already at {rev} ({origin})"));
    }

    if dest.exists() {
        std::fs::remove_dir_all(&dest).with_context(|| format!("rm -rf {}", dest.display()))?;
    }
    let msg = fetch_into_empty_dir(&dest, &rev)?;
    Ok(format!("[{REMOTE_ID}] {msg}"))
}

fn checkout_existing(dir: &Path, rev: &str) -> Result<String> {
    if is_unpinned_rev(rev) {
        let msg = pull(dir)?;
        write_mcp_origin(dir, "git", rev)?;
        return Ok(msg);
    }
    if let Ok(current) = git_rev(dir)
        && same_git_rev(&current, rev)
    {
        write_mcp_origin(dir, "git", rev)?;
        return Ok(format!("already at {current}"));
    }
    fetch_rev(dir, rev)?;
    git_run(Some(dir), ["checkout", "--detach", "FETCH_HEAD"])?;
    write_mcp_origin(dir, "git", rev)?;
    let got = git_rev(dir).unwrap_or_default();
    Ok(format!("checked out {got}"))
}

/// Cold start and wipe. `dest` must not be a leftover tree (caller `rm -rf`s first).
pub fn fetch_into_empty_dir(dest: &Path, rev: &str) -> Result<String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    match git_populate(dest, rev) {
        Ok(msg) => {
            write_mcp_origin(dest, "git", rev)?;
            return Ok(msg);
        }
        Err(e) => {
            tracing::warn!("git populate failed: {e:#}");
            let _ = std::fs::remove_dir_all(dest);
            if is_unpinned_rev(rev) {
                bail!("git failed for unpinned {rev} (no archive/jsDelivr): {e:#}");
            }
        }
    }

    match archive_populate(dest, rev) {
        Ok(msg) => {
            write_mcp_origin(dest, "archive", rev)?;
            return Ok(msg);
        }
        Err(e) => {
            tracing::warn!("archive populate failed: {e:#}");
            let _ = std::fs::remove_dir_all(dest);
        }
    }

    if looks_like_sha(rev) && rev.len() != 40 {
        bail!("jsdelivr needs a tag or full SHA, not {rev}");
    }
    match jsdelivr_populate(dest, rev) {
        Ok(msg) => {
            write_mcp_origin(dest, "jsdelivr", rev)?;
            Ok(msg)
        }
        Err(e) => bail!("jsdelivr populate failed: {e:#}"),
    }
}

fn git_populate(dest: &Path, rev: &str) -> Result<String> {
    if is_unpinned_rev(rev) {
        clone_full(REMOTE_GIT, dest)?;
        return Ok("cloned (unpinned)".into());
    }
    if looks_like_sha(rev) {
        std::fs::create_dir_all(dest)?;
        git_run(Some(dest), ["init"])?;
        git_run(Some(dest), ["remote", "add", "origin", REMOTE_GIT])?;
        fetch_rev(dest, rev)?;
        git_run(Some(dest), ["checkout", "--detach", "FETCH_HEAD"])?;
        return Ok(format!("cloned at {rev}"));
    }
    let mut cmd = git_cmd();
    cmd.args(["clone", "--depth", "1", "--branch", rev, "--", REMOTE_GIT]);
    cmd.arg(dest.as_os_str());
    run_cmd(cmd, "git clone")?;
    Ok(format!("cloned at {rev}"))
}

fn clone_full(url: &str, dir: &Path) -> Result<()> {
    let mut cmd = git_cmd();
    cmd.args(["clone", "--depth", "1", "--"]);
    cmd.arg(url);
    cmd.arg(dir.as_os_str());
    let _ = run_cmd(cmd, "git clone")?;
    Ok(())
}

fn fetch_rev(dir: &Path, rev: &str) -> Result<()> {
    match git_run(Some(dir), ["fetch", "--depth", "1", "origin", rev]) {
        Ok(_) => Ok(()),
        Err(shallow) => match git_run(Some(dir), ["fetch", "origin", rev]) {
            Ok(_) => Ok(()),
            Err(full) => bail!("git fetch {rev} failed: {shallow:#}; fallback: {full:#}"),
        },
    }
}

fn pull(dir: &Path) -> Result<String> {
    let out = git_run(Some(dir), ["pull", "--ff-only"])?;
    Ok(format!(
        "pulled {}",
        String::from_utf8_lossy(&out.stdout).trim()
    ))
}

fn sibling_temp(dest: &Path) -> PathBuf {
    let pid = std::process::id();
    match dest.parent() {
        Some(p) => p.join(format!(".tmp-naive-ui-{pid}")),
        None => PathBuf::from(format!(".tmp-naive-ui-{pid}")),
    }
}

fn archive_populate(dest: &Path, rev: &str) -> Result<String> {
    let url = archive_url(rev).ok_or_else(|| anyhow::anyhow!("no archive URL for rev {rev}"))?;
    let tmp = sibling_temp(dest);
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).context("create archive temp dir")?;
    let tarball = tmp.join("archive.tar.gz");
    let result = (|| {
        curl_download(&url, &tarball, 120)
            .context("curl archive (need curl; git clone already failed)")?;
        tar_extract(&tarball, &tmp)?;
        let extracted = unique_naive_ui_root(&tmp)?;
        if dest.exists() {
            std::fs::remove_dir_all(dest)?;
        }
        std::fs::rename(&extracted, dest)
            .with_context(|| format!("rename {} -> {}", extracted.display(), dest.display()))?;
        Ok::<_, anyhow::Error>(())
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    Ok(format!("archive {rev}"))
}

fn tar_extract(tarball: &Path, dest: &Path) -> Result<()> {
    let mut cmd = Command::new("tar");
    cmd.args(["-xzf"]);
    cmd.arg(tarball);
    cmd.arg("-C");
    cmd.arg(dest);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    run_cmd(cmd, "tar")
        .context("archive extract needs `tar` on PATH (and `git` for the primary path)")?;
    Ok(())
}

fn unique_naive_ui_root(tmp: &Path) -> Result<PathBuf> {
    let mut roots = Vec::new();
    for ent in std::fs::read_dir(tmp).context("read archive temp")? {
        let ent = ent?;
        if !ent.file_type()?.is_dir() {
            continue;
        }
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("naive-ui-") {
            roots.push(ent.path());
        }
    }
    match roots.len() {
        1 => Ok(roots.remove(0)),
        n => bail!("expected exactly one naive-ui-* extract root, got {n}"),
    }
}

fn curl_download(url: &str, dest: &Path, timeout_secs: u64) -> Result<()> {
    let dest_s = dest
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-utf8 curl dest"))?;
    let mut cmd = Command::new("curl");
    cmd.args([
        "-fsSL",
        "--max-time",
        &timeout_secs.to_string(),
        "--proto",
        "=https",
        "-o",
        dest_s,
        "--",
        url,
    ]);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    run_cmd_timeout(
        cmd,
        "curl",
        Duration::from_secs(timeout_secs.saturating_add(5)),
    )?;
    Ok(())
}

fn curl_http_file(url: &str, dest: &Path, timeout_secs: u64) -> Result<u16> {
    let dest_s = dest
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-utf8 curl dest"))?;
    let mut cmd = Command::new("curl");
    cmd.args([
        "-sS",
        "-L",
        "--max-time",
        &timeout_secs.to_string(),
        "--proto",
        "=https",
        "-o",
        dest_s,
        "-w",
        "%{http_code}",
        "--",
        url,
    ]);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let out = run_cmd_timeout(
        cmd,
        "curl",
        Duration::from_secs(timeout_secs.saturating_add(5)),
    )?;
    let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
    code.parse::<u16>()
        .with_context(|| format!("curl http code {code:?}"))
}

#[derive(Debug, Deserialize)]
struct JsdelivrListing {
    files: Vec<JsdelivrNode>,
}

#[derive(Debug, Deserialize)]
struct JsdelivrNode {
    #[serde(rename = "type")]
    type_: String,
    name: String,
    #[serde(default)]
    files: Vec<JsdelivrNode>,
}

fn flatten(nodes: &[JsdelivrNode], prefix: &str, out: &mut Vec<String>) {
    for n in nodes {
        let path = if prefix.is_empty() {
            n.name.clone()
        } else {
            format!("{prefix}/{}", n.name)
        };
        match n.type_.as_str() {
            "file" => out.push(path),
            "directory" => flatten(&n.files, &path, out),
            _ => {}
        }
    }
}

fn jsdelivr_keep(path: &str) -> bool {
    if !is_safe_rel(path) {
        return false;
    }
    let p = path.replace('\\', "/");
    if p == "src/components.ts" || p == "src/index.ts" {
        return true;
    }
    if let Some(rest) = p.strip_prefix("src/")
        && let Some(name) = rest.strip_suffix("/index.ts")
        && !name.is_empty()
        && !name.contains('/')
    {
        return true;
    }
    if let Some(rest) = p.strip_prefix("src/") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 4 && parts[1] == "demos" && parts[2] == "enUS" {
            return true;
        }
    }
    if p.starts_with("src/composables/") {
        return true;
    }
    if p.starts_with("src/") && p.ends_with(".cssr.ts") {
        return true;
    }
    if p.starts_with("src/_styles/common/") {
        return true;
    }
    if let Some(rest) = p.strip_prefix("demo/pages/docs/") {
        let parts: Vec<&str> = rest.split('/').collect();
        if let Some(i) = parts.iter().position(|s| *s == "enUS")
            && i + 1 < parts.len()
        {
            return true;
        }
    }
    false
}

fn jsdelivr_populate(dest: &Path, rev: &str) -> Result<String> {
    let deadline = Instant::now() + JSDELIVR_OVERALL;
    let tmp = sibling_temp(dest);
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).context("create jsdelivr temp dir")?;
    let result = (|| {
        let listing_path = tmp.join("listing.json");
        let remain = deadline.saturating_duration_since(Instant::now());
        let t = remain.as_secs().clamp(1, 30);
        let list_url = JSDELIVR_LIST_TEMPLATE.replace("{rev}", rev);
        curl_download(&list_url, &listing_path, t).context("jsdelivr listing")?;
        let text = std::fs::read_to_string(&listing_path)?;
        let listing: JsdelivrListing = serde_json::from_str(&text).context(
            "jsdelivr listing schema (files missing or not an array of {type,name,files?})",
        )?;
        let mut paths = Vec::new();
        flatten(&listing.files, "", &mut paths);
        let keep: Vec<String> = paths.into_iter().filter(|p| jsdelivr_keep(p)).collect();
        if keep.is_empty() {
            bail!("jsdelivr listing matched 0 allowlisted paths");
        }
        let tree = tmp.join("tree");
        std::fs::create_dir_all(&tree)?;
        let n = fetch_jsdelivr_files(&tree, rev, &keep, deadline)?;
        if n == 0 {
            bail!("jsdelivr wrote 0 files");
        }
        if dest.exists() {
            std::fs::remove_dir_all(dest)?;
        }
        std::fs::rename(&tree, dest)
            .with_context(|| format!("rename {} -> {}", tree.display(), dest.display()))?;
        Ok(format!("jsdelivr {rev} ({n} files)"))
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn fetch_jsdelivr_files(
    dest: &Path,
    rev: &str,
    paths: &[String],
    deadline: Instant,
) -> Result<usize> {
    let (tx, rx) = mpsc::channel::<String>();
    for p in paths {
        if Instant::now() >= deadline {
            break;
        }
        let _ = tx.send(p.clone());
    }
    drop(tx);
    let rx = Mutex::new(rx);
    let written = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..JSDELIVR_WORKERS {
            scope.spawn(|| {
                loop {
                    if Instant::now() >= deadline {
                        break;
                    }
                    let path = {
                        let guard = rx.lock().unwrap_or_else(|p| p.into_inner());
                        match guard.recv() {
                            Ok(p) => p,
                            Err(_) => break,
                        }
                    };
                    match fetch_one_jsdelivr(dest, rev, &path) {
                        Ok(true) => {
                            written.fetch_add(1, Ordering::Relaxed);
                        }
                        Ok(false) => {}
                        Err(e) => tracing::debug!(path = path.as_str(), "jsdelivr file: {e:#}"),
                    }
                }
            });
        }
    });
    Ok(written.load(Ordering::Relaxed))
}

fn fetch_one_jsdelivr(dest: &Path, rev: &str, rel: &str) -> Result<bool> {
    if !is_safe_rel(rel) {
        return Ok(false);
    }
    let url = JSDELIVR_FILE_TEMPLATE
        .replace("{rev}", rev)
        .replace("{path}", rel);
    if url.contains(".tar.gz") {
        bail!("refusing jsdelivr tarball URL");
    }
    let dest_file = dest.join(rel);
    if let Some(parent) = dest_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let code = curl_http_file(&url, &dest_file, JSDELIVR_FILE_TIMEOUT)?;
    if code == 200 {
        return Ok(true);
    }
    if code == 404 {
        let _ = std::fs::remove_file(&dest_file);
        tracing::debug!(rel, "jsdelivr 404 skip");
        return Ok(false);
    }
    if (500..600).contains(&code) {
        tracing::debug!(rel, code, "jsdelivr 5xx retry");
        let code2 = curl_http_file(&url, &dest_file, JSDELIVR_FILE_TIMEOUT)?;
        if code2 == 200 {
            return Ok(true);
        }
    }
    let _ = std::fs::remove_file(&dest_file);
    Ok(false)
}

fn git_cmd() -> Command {
    let mut c = Command::new("git");
    c.env("GIT_TERMINAL_PROMPT", "0");
    c.stdin(Stdio::null());
    c.stdout(Stdio::piped());
    c.stderr(Stdio::piped());
    c
}

fn git_run(
    dir: Option<&Path>,
    args: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> Result<Output> {
    let mut cmd = git_cmd();
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    cmd.args(args);
    run_cmd(cmd, "git")
}

fn run_cmd(cmd: Command, label: &str) -> Result<Output> {
    run_cmd_timeout(cmd, label, GIT_TIMEOUT)
}

fn run_cmd_timeout(mut cmd: Command, label: &str, timeout: Duration) -> Result<Output> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(cmd.output());
    });
    let out = match rx.recv_timeout(timeout) {
        Ok(r) => r.with_context(|| format!("{label} spawn"))?,
        Err(_) => bail!("{label} timed out after {}s", timeout.as_secs()),
    };
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!(
            "{label} failed: {}",
            err.trim().chars().take(800).collect::<String>()
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_git_rev_accepts_prefix() {
        let full = "d9ad6aff67e47de43abb270d22de75dd950f1b48";
        assert!(same_git_rev(full, full));
        assert!(same_git_rev(full, "d9ad6af"));
        assert!(same_git_rev("d9ad6af", full));
        assert!(!same_git_rev(full, "6e2fae619c45"));
        assert!(!same_git_rev("abc", "abcdef1"));
        assert!(!same_git_rev("", full));
    }

    #[test]
    fn flatten_three_levels_to_demo_entry() {
        let listing = r#"{
          "type": "gh",
          "name": "tusen-ai/naive-ui",
          "version": "v2.40.4",
          "files": [
            {
              "type": "directory",
              "name": "src",
              "files": [
                {
                  "type": "directory",
                  "name": "button",
                  "files": [
                    {
                      "type": "directory",
                      "name": "demos",
                      "files": [
                        {
                          "type": "directory",
                          "name": "enUS",
                          "files": [
                            { "type": "file", "name": "index.demo-entry.md" }
                          ]
                        }
                      ]
                    }
                  ]
                }
              ]
            },
            { "type": "file", "name": "package.json" }
          ]
        }"#;
        let parsed: JsdelivrListing = serde_json::from_str(listing).unwrap();
        let mut out = Vec::new();
        flatten(&parsed.files, "", &mut out);
        assert!(
            out.contains(&"src/button/demos/enUS/index.demo-entry.md".to_string()),
            "{out:?}"
        );
        assert!(out.contains(&"package.json".to_string()));
        assert!(jsdelivr_keep("src/button/demos/enUS/index.demo-entry.md"));
        assert!(!jsdelivr_keep("package.json"));
        assert!(!jsdelivr_keep("../etc/passwd"));
        assert!(!JSDELIVR_FILE_TEMPLATE.contains(".tar.gz"));
    }

    #[test]
    fn jsdelivr_keep_allowlist() {
        assert!(jsdelivr_keep("src/button/index.ts"));
        assert!(jsdelivr_keep("src/index.ts"));
        assert!(jsdelivr_keep("src/components.ts"));
        assert!(jsdelivr_keep("src/button/src/styles/index.cssr.ts"));
        assert!(jsdelivr_keep("src/_styles/common/light.ts"));
        assert!(jsdelivr_keep("src/composables/use-theme-vars.ts"));
        assert!(jsdelivr_keep(
            "demo/pages/docs/customize-theme/enUS/index.md"
        ));
        assert!(!jsdelivr_keep("src/button/demos/zhCN/index.demo-entry.md"));
        assert!(!jsdelivr_keep("README.md"));
    }

    #[test]
    #[ignore]
    fn net_fetch_into_empty_dir_optional() {
        if std::env::var("NAIVE_UI_MCP_NET").ok().as_deref() != Some("1") {
            return;
        }
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("net-fetch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("naive-ui");
        let result = fetch_into_empty_dir(&dest, "v2.40.4");
        let origin = read_mcp_origin(&dest);
        let _ = std::fs::remove_dir_all(&dir);
        result.expect("net fetch_into_empty_dir");
        let (kind, rev) = origin.expect(".mcp-origin");
        assert!(
            matches!(kind.as_str(), "git" | "archive" | "jsdelivr"),
            "{kind}"
        );
        assert_eq!(rev, "v2.40.4");
    }
}
