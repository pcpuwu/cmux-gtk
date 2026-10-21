//! Locate Ghostty's resources directory (themes, shell integration, terminfo).
//!
//! libghostty only finds resources through `GHOSTTY_RESOURCES_DIR` or by climbing from the
//! executable to a `share/terminfo` sibling, which never matches a cmux install. Without them,
//! named themes fail to load, shell integration (cwd and git-branch reporting) is never
//! injected and TERM falls back to xterm-256color. Resolve a directory before Ghostty starts.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

const ENV_NAME: &str = "GHOSTTY_RESOURCES_DIR";

/// Export `GHOSTTY_RESOURCES_DIR` when unset and a complete resources directory is found.
/// Call before any thread starts; returns the directory Ghostty will use, if any.
pub fn configure() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok();
    let dev = option_env!("CARGO_MANIFEST_DIR")
        .map(|root| Path::new(root).join("ghostty/zig-out/share/ghostty"));
    let resolved = resolve(
        &|name| std::env::var_os(name),
        exe.as_deref(),
        dev.as_deref(),
    );
    match &resolved {
        Some(Resolved::Inherited(dir)) => {
            crate::diagnostics::event(format_args!(
                "ghostty resources inherited={}",
                dir.display()
            ));
        }
        Some(Resolved::Found(dir)) => {
            std::env::set_var(ENV_NAME, dir);
            crate::diagnostics::event(format_args!("ghostty resources found={}", dir.display()));
        }
        None => {
            crate::diagnostics::event(format_args!("ghostty resources missing"));
            eprintln!(
                "cmux: Ghostty resources not found; named themes and shell integration are \
                 unavailable. Install them under ~/.local/share/cmux/ghostty or set {ENV_NAME}."
            );
        }
    }
    resolved.map(Resolved::into_path)
}

/// Print Ghostty's configuration diagnostics (for example an unknown theme name), which
/// libghostty otherwise keeps to itself.
///
/// # Safety
/// `config` must be a live, finalized Ghostty configuration.
pub unsafe fn report_config_diagnostics(config: crate::ghostty::ffi::ghostty_config_t) {
    use crate::ghostty::ffi;
    let count = ffi::ghostty_config_diagnostics_count(config);
    for index in 0..count {
        let diagnostic = ffi::ghostty_config_get_diagnostic(config, index);
        if diagnostic.message.is_null() {
            continue;
        }
        let message = std::ffi::CStr::from_ptr(diagnostic.message).to_string_lossy();
        crate::diagnostics::event(format_args!("ghostty config diagnostic: {message}"));
        eprintln!("cmux: ghostty config: {message}");
    }
}

/// Where the resources directory came from.
#[derive(Debug, PartialEq, Eq)]
pub enum Resolved {
    /// The caller already exported a non-empty `GHOSTTY_RESOURCES_DIR`; it is left untouched.
    Inherited(PathBuf),
    /// A complete resources directory was discovered on disk.
    Found(PathBuf),
}

impl Resolved {
    fn into_path(self) -> PathBuf {
        match self {
            Self::Inherited(path) | Self::Found(path) => path,
        }
    }
}

/// Pure resolver: `env` reads variables, `exe` is the running binary, `dev` the build-tree copy.
pub fn resolve(
    env: &dyn Fn(&str) -> Option<OsString>,
    exe: Option<&Path>,
    dev: Option<&Path>,
) -> Option<Resolved> {
    if let Some(dir) = env(ENV_NAME).filter(|value| !value.is_empty()) {
        return Some(Resolved::Inherited(PathBuf::from(dir)));
    }
    candidates(env, exe, dev)
        .into_iter()
        .find(|dir| is_complete(dir))
        .map(Resolved::Found)
}

/// Candidate directories in priority order: next to the binary, the user data dir, then the
/// system data dirs (cmux's own copy before a system Ghostty install), then the build tree.
fn candidates(
    env: &dyn Fn(&str) -> Option<OsString>,
    exe: Option<&Path>,
    dev: Option<&Path>,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(bin_dir) = exe.and_then(Path::parent) {
        // Release archive layout: <dir>/cmux-app and <dir>/share/cmux/ghostty.
        out.push(bin_dir.join("share/cmux/ghostty"));
        // Prefix layout: <prefix>/bin/cmux-app and <prefix>/share/cmux/ghostty.
        if let Some(prefix) = bin_dir.parent() {
            out.push(prefix.join("share/cmux/ghostty"));
        }
    }
    let data_home = env("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            env("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local/share"))
        });
    if let Some(data_home) = data_home {
        out.push(data_home.join("cmux/ghostty"));
    }
    let data_dirs = env("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| OsString::from("/usr/local/share:/usr/share"));
    let data_dirs: Vec<PathBuf> = std::env::split_paths(&data_dirs)
        .filter(|path| path.is_absolute())
        .collect();
    out.extend(data_dirs.iter().map(|dir| dir.join("cmux/ghostty")));
    out.extend(data_dirs.iter().map(|dir| dir.join("ghostty")));
    if let Some(dev) = dev {
        out.push(dev.to_path_buf());
    }
    out
}

/// A usable directory holds themes or shell integration and has the sibling terminfo database
/// Ghostty points TERMINFO at; without it TERM=xterm-ghostty would break remote programs.
fn is_complete(dir: &Path) -> bool {
    let has_payload = dir.join("themes").is_dir() || dir.join("shell-integration").is_dir();
    let Some(parent) = dir.parent() else {
        return false;
    };
    let terminfo = parent.join("terminfo");
    has_payload
        && (terminfo.join("x/xterm-ghostty").is_file() || terminfo.join("g/ghostty").is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::current_dir()
                .unwrap()
                .join("target/test-ghostty-resources")
                .join(format!("{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        /// Create `<share>/<name>/themes` and, when `terminfo`, `<share>/terminfo/x/xterm-ghostty`.
        fn install(&self, share: &str, name: &str, terminfo: bool) -> PathBuf {
            let share = self.root.join(share);
            let dir = share.join(name);
            std::fs::create_dir_all(dir.join("themes")).unwrap();
            if terminfo {
                std::fs::create_dir_all(share.join("terminfo/x")).unwrap();
                std::fs::write(share.join("terminfo/x/xterm-ghostty"), b"").unwrap();
            }
            dir
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn env_of(pairs: &[(&str, &Path)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.as_os_str().to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn inherited_variable_wins_and_empty_is_ignored() {
        let fx = Fixture::new("inherited");
        let explicit = fx.root.join("explicit");
        let env = env_of(&[(ENV_NAME, &explicit)]);
        assert_eq!(
            resolve(&env, None, None),
            Some(Resolved::Inherited(explicit))
        );
        let empty = env_of(&[(ENV_NAME, Path::new("")), ("XDG_DATA_DIRS", &fx.root)]);
        assert_eq!(resolve(&empty, None, None), None);
    }

    #[test]
    fn release_archive_layout_next_to_binary() {
        let fx = Fixture::new("archive");
        let dir = fx.install("bundle/share/cmux", "ghostty", true);
        let exe = fx.root.join("bundle/cmux-app");
        let env = env_of(&[("XDG_DATA_DIRS", &fx.root.join("none"))]);
        assert_eq!(resolve(&env, Some(&exe), None), Some(Resolved::Found(dir)));
    }

    #[test]
    fn prefix_layout_beats_user_data_dir() {
        let fx = Fixture::new("prefix");
        let prefix_dir = fx.install("usr/share/cmux", "ghostty", true);
        fx.install("home/share/cmux", "ghostty", true);
        let exe = fx.root.join("usr/bin/cmux-app");
        let env = env_of(&[
            ("XDG_DATA_HOME", &fx.root.join("home/share")),
            ("XDG_DATA_DIRS", &fx.root.join("none")),
        ]);
        assert_eq!(
            resolve(&env, Some(&exe), None),
            Some(Resolved::Found(prefix_dir))
        );
    }

    #[test]
    fn user_data_dir_from_home_when_xdg_unset() {
        let fx = Fixture::new("home");
        let dir = fx.install("home/.local/share/cmux", "ghostty", true);
        let env = env_of(&[
            ("HOME", &fx.root.join("home")),
            ("XDG_DATA_DIRS", &fx.root.join("none")),
        ]);
        let exe = fx.root.join("bin/cmux-app");
        assert_eq!(resolve(&env, Some(&exe), None), Some(Resolved::Found(dir)));
    }

    #[test]
    fn system_ghostty_install_is_used_after_cmux_copies() {
        let fx = Fixture::new("system");
        let system = fx.install("sys", "ghostty", true);
        let env = env_of(&[("XDG_DATA_DIRS", &fx.root.join("sys"))]);
        assert_eq!(resolve(&env, None, None), Some(Resolved::Found(system)));
        let cmux_copy = fx.install("sys/cmux", "ghostty", true);
        assert_eq!(resolve(&env, None, None), Some(Resolved::Found(cmux_copy)));
    }

    #[test]
    fn directory_without_terminfo_is_skipped_for_build_tree() {
        let fx = Fixture::new("terminfo");
        fx.install("user/cmux", "ghostty", false);
        let dev = fx.install("dev/share", "ghostty", true);
        let env = env_of(&[
            ("XDG_DATA_HOME", &fx.root.join("user")),
            ("XDG_DATA_DIRS", &fx.root.join("none")),
        ]);
        assert_eq!(resolve(&env, None, Some(&dev)), Some(Resolved::Found(dev)));
        assert_eq!(resolve(&env, None, None), None);
    }

    #[test]
    fn relative_xdg_paths_are_ignored() {
        let env = env_of(&[
            ("XDG_DATA_HOME", Path::new("relative/share")),
            ("XDG_DATA_DIRS", Path::new("also/relative")),
        ]);
        assert!(candidates(&env, None, None).is_empty());
    }
}
