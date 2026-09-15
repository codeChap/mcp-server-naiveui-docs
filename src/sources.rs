use std::path::{Component, Path, PathBuf};

use anyhow::{Result, bail};

/// Default Naive UI git tag. Matches StackChap `naive-ui.iife.js` 2.40.4.
/// Override with `NAIVE_UI_MCP_REV` (tag `v2.45.3`, SHA, or HEAD/main).
pub const NAIVE_UI_PINNED_REV: &str = "v2.40.4";

#[allow(dead_code)]
pub const REMOTE_GIT: &str = "https://github.com/tusen-ai/naive-ui.git";
pub const REMOTE_ID: &str = "naive-ui";

/// jsDelivr listing (sync last resort). Per-file CDN, not a tarball.
#[allow(dead_code)]
pub const JSDELIVR_LIST_TEMPLATE: &str =
    "https://data.jsdelivr.com/v1/packages/gh/tusen-ai/naive-ui@{rev}";
/// jsDelivr file URL. Never a `.tar.gz` — that path is HTTP 400.
#[allow(dead_code)]
pub const JSDELIVR_FILE_TEMPLATE: &str =
    "https://cdn.jsdelivr.net/gh/tusen-ai/naive-ui@{rev}/{path}";

/// GitHub archive fallback (sync only). Tag vs SHA use different paths.
#[allow(dead_code)]
pub fn archive_url(rev: &str) -> Option<String> {
    if looks_like_sha(rev) {
        Some(format!(
            "https://github.com/tusen-ai/naive-ui/archive/{rev}.tar.gz"
        ))
    } else if is_unpinned(rev) {
        None
    } else {
        Some(format!(
            "https://github.com/tusen-ai/naive-ui/archive/refs/tags/{rev}.tar.gz"
        ))
    }
}

#[allow(dead_code)]
fn looks_like_sha(rev: &str) -> bool {
    let n = rev.len();
    (7..=40).contains(&n) && rev.bytes().all(|b| b.is_ascii_hexdigit())
}

#[allow(dead_code)]
fn is_unpinned(rev: &str) -> bool {
    matches!(
        rev.to_ascii_lowercase().as_str(),
        "head" | "main" | "master"
    )
}

/// Pin for checkout. `None` means follow the default branch (unpinned).
pub fn resolve_rev(env: Option<&str>) -> Option<String> {
    match env {
        Some(v) if v.trim().is_empty() => Some(NAIVE_UI_PINNED_REV.to_string()),
        Some(v)
            if v.eq_ignore_ascii_case("head")
                || v.eq_ignore_ascii_case("main")
                || v.eq_ignore_ascii_case("master") =>
        {
            None
        }
        Some(v) => Some(v.trim().to_string()),
        None => Some(NAIVE_UI_PINNED_REV.to_string()),
    }
}

pub fn cache_dir() -> Result<PathBuf> {
    finalize_cache_path(raw_cache_dir()?)
}

fn raw_cache_dir() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("NAIVE_UI_MCP_CACHE")
        && !p.is_empty()
    {
        return Ok(PathBuf::from(p));
    }
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME")
        && !xdg.is_empty()
    {
        return Ok(PathBuf::from(xdg).join("mcp-server-naive-ui"));
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        return Ok(PathBuf::from(home)
            .join(".cache")
            .join("mcp-server-naive-ui"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA")
        && !local.is_empty()
    {
        return Ok(PathBuf::from(local).join("mcp-server-naive-ui"));
    }
    bail!("set NAIVE_UI_MCP_CACHE or HOME (refusing world-writable /tmp as a cache)");
}

fn finalize_cache_path(path: PathBuf) -> Result<PathBuf> {
    let path = normalize_lexically(&path);
    if path.as_os_str().is_empty() {
        bail!("cache path is empty; set NAIVE_UI_MCP_CACHE to a private directory");
    }
    let path = match std::path::absolute(&path) {
        Ok(p) => p,
        Err(e) => bail!("cannot resolve cache path {}: {e}", path.display()),
    };
    let path = if path.exists() {
        path.canonicalize().unwrap_or(path)
    } else {
        path
    };
    if is_forbidden_tmp(&path) {
        bail!(
            "cache path {} is forbidden (under /tmp, /var/tmp, or /dev/shm); set NAIVE_UI_MCP_CACHE to a private directory",
            path.display()
        );
    }
    let parent = path.parent();
    if is_world_writable(&path) || parent.is_some_and(is_world_writable) {
        bail!(
            "cache path {} is world-writable (or its parent is); set NAIVE_UI_MCP_CACHE to a private directory",
            path.display()
        );
    }
    Ok(path)
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir) | Some(Component::Prefix(_)) => {}
                _ => out.push(c),
            },
            _ => out.push(c),
        }
    }
    out
}

fn is_forbidden_tmp(path: &Path) -> bool {
    ["/tmp", "/var/tmp", "/dev/shm"]
        .into_iter()
        .any(|root| path.starts_with(root))
}

#[cfg(unix)]
fn is_world_writable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o002 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_world_writable(_path: &Path) -> bool {
    false
}

/// Relative path under a clone (demo file names). Rejects traversal.
#[allow(dead_code)]
pub fn is_safe_rel(rel: &str) -> bool {
    if rel.is_empty() || rel.contains('\0') {
        return false;
    }
    if rel.starts_with('/') || rel.contains('\\') {
        return false;
    }
    if rel.contains("..") {
        return false;
    }
    true
}

#[allow(dead_code)]
pub fn is_safe_source_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && !id.contains("..")
        && !id.contains('/')
        && !id.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn set_var(key: &str, val: Option<&str>) {
        // SAFETY: callers must hold ENV_LOCK.
        match val {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
    }

    struct EnvRestore {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvRestore {
        fn set(key: &'static str, val: &str) -> Self {
            let prev = std::env::var(key).ok();
            set_var(key, Some(val));
            Self { key, prev }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            set_var(self.key, self.prev.as_deref());
        }
    }

    struct CwdRestore(PathBuf);

    impl CwdRestore {
        fn chdir(path: &Path) -> Self {
            let prev = std::env::current_dir().expect("cwd");
            std::env::set_current_dir(path).expect("chdir");
            Self(prev)
        }
    }

    impl Drop for CwdRestore {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    #[test]
    fn naive_ui_mcp_cache_tmp_bails() {
        let _g = ENV_LOCK.lock().expect("env lock");
        let _env = EnvRestore::set("NAIVE_UI_MCP_CACHE", "/tmp");
        let err = cache_dir().expect_err("NAIVE_UI_MCP_CACHE=/tmp must fail");
        let msg = format!("{err:#}").to_lowercase();
        assert!(
            msg.contains("tmp") || msg.contains("forbidden") || msg.contains("world-writable"),
            "{msg}"
        );
    }

    #[test]
    fn naive_ui_mcp_cache_tmp_naive_ui_bails() {
        let _g = ENV_LOCK.lock().expect("env lock");
        let _env = EnvRestore::set("NAIVE_UI_MCP_CACHE", "/tmp/naive-ui");
        let err = cache_dir().expect_err("NAIVE_UI_MCP_CACHE=/tmp/naive-ui must fail");
        let msg = format!("{err:#}").to_lowercase();
        assert!(
            msg.contains("tmp") || msg.contains("forbidden") || msg.contains("world-writable"),
            "{msg}"
        );
    }

    #[test]
    fn relative_cache_under_tmp_cwd_bails() {
        let _g = ENV_LOCK.lock().expect("env lock");
        let _cwd = CwdRestore::chdir(Path::new("/tmp"));
        for rel in [".", "./.", "foo/..", "naive-ui-rel"] {
            let _env = EnvRestore::set("NAIVE_UI_MCP_CACHE", rel);
            cache_dir().expect_err(&format!("NAIVE_UI_MCP_CACHE={rel} with cwd /tmp must fail"));
        }
    }

    #[test]
    fn finalize_rejects_tmp_even_without_env() {
        assert!(finalize_cache_path(PathBuf::from("/tmp")).is_err());
        assert!(finalize_cache_path(PathBuf::from("/tmp/naive-ui")).is_err());
        assert!(finalize_cache_path(PathBuf::from("/var/tmp/x")).is_err());
        assert!(finalize_cache_path(PathBuf::from("/dev/shm/x")).is_err());
        assert!(finalize_cache_path(PathBuf::from("/tmp/../tmp")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn world_writable_parent_bails() {
        use std::os::unix::fs::PermissionsExt;

        let _g = ENV_LOCK.lock().expect("env lock");
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("ww-cache-parent");
        std::fs::create_dir_all(&parent).expect("mkdir ww parent");
        let orig = std::fs::metadata(&parent)
            .expect("stat ww parent")
            .permissions();
        struct ModeRestore(PathBuf, std::fs::Permissions);
        impl Drop for ModeRestore {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, self.1.clone());
            }
        }
        let _mode = ModeRestore(parent.clone(), orig);
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o777))
            .expect("chmod 0777");
        let cache = parent.join("cache");
        let _env = EnvRestore::set(
            "NAIVE_UI_MCP_CACHE",
            cache.to_str().expect("utf-8 cache path"),
        );
        let err = cache_dir().expect_err("world-writable parent must fail");
        let msg = format!("{err:#}").to_lowercase();
        assert!(msg.contains("world-writable"), "{msg}");
    }

    #[test]
    fn is_safe_rel_rejects_traversal() {
        assert!(!is_safe_rel("../x"));
        assert!(!is_safe_rel("/abs"));
        assert!(!is_safe_rel(r"a\b"));
        assert!(!is_safe_rel(""));
        assert!(!is_safe_rel("a\0b"));
        assert!(is_safe_rel("basic.demo.vue"));
        assert!(is_safe_rel("enUS/basic.demo.vue"));
    }

    #[test]
    fn resolve_rev_matrix() {
        assert_eq!(resolve_rev(None).as_deref(), Some(NAIVE_UI_PINNED_REV));
        assert_eq!(resolve_rev(Some("")).as_deref(), Some(NAIVE_UI_PINNED_REV));
        assert_eq!(
            resolve_rev(Some("   ")).as_deref(),
            Some(NAIVE_UI_PINNED_REV)
        );
        assert_eq!(resolve_rev(Some("HEAD")), None);
        assert_eq!(resolve_rev(Some("head")), None);
        assert_eq!(resolve_rev(Some("main")), None);
        assert_eq!(resolve_rev(Some("master")), None);
        assert_eq!(resolve_rev(Some("Master")), None);
        assert_eq!(resolve_rev(Some("v2.45.3")).as_deref(), Some("v2.45.3"));
        assert_eq!(resolve_rev(Some("  abcdef1  ")).as_deref(), Some("abcdef1"));
        assert_eq!(
            resolve_rev(Some("3bdde87071fed9e2cd2073b6571521b8240c0060")).as_deref(),
            Some("3bdde87071fed9e2cd2073b6571521b8240c0060")
        );
    }

    #[test]
    fn archive_url_tag_vs_sha_vs_head() {
        assert_eq!(
            archive_url("v2.40.4").as_deref(),
            Some("https://github.com/tusen-ai/naive-ui/archive/refs/tags/v2.40.4.tar.gz")
        );
        assert_eq!(
            archive_url("v2.45.3").as_deref(),
            Some("https://github.com/tusen-ai/naive-ui/archive/refs/tags/v2.45.3.tar.gz")
        );
        assert_eq!(
            archive_url("abcdef1").as_deref(),
            Some("https://github.com/tusen-ai/naive-ui/archive/abcdef1.tar.gz")
        );
        assert_eq!(
            archive_url("3bdde87071fed9e2cd2073b6571521b8240c0060").as_deref(),
            Some(
                "https://github.com/tusen-ai/naive-ui/archive/3bdde87071fed9e2cd2073b6571521b8240c0060.tar.gz"
            )
        );
        assert_eq!(archive_url("HEAD"), None);
        assert_eq!(archive_url("main"), None);
        assert_eq!(archive_url("master"), None);
        assert_eq!(archive_url("Master"), None);
        // 6 hex chars is not a SHA; treat as a tag.
        assert_eq!(
            archive_url("abcdef").as_deref(),
            Some("https://github.com/tusen-ai/naive-ui/archive/refs/tags/abcdef.tar.gz")
        );
    }

    #[test]
    fn is_safe_source_id_rejects_traversal() {
        assert!(!is_safe_source_id("../x"));
        assert!(!is_safe_source_id("a/b"));
        assert!(!is_safe_source_id("-evil"));
        assert!(is_safe_source_id("naive-ui"));
        assert!(is_safe_source_id("book"));
    }

    #[test]
    fn jsdelivr_is_per_file_not_tarball() {
        assert!(!JSDELIVR_FILE_TEMPLATE.contains(".tar.gz"));
        assert!(!JSDELIVR_LIST_TEMPLATE.contains(".tar.gz"));
        assert!(REMOTE_GIT.starts_with("https://"));
        assert_eq!(REMOTE_ID, "naive-ui");
    }
}
