//! The fixed list of places Clean My OS knows about, and where they are on this PC.
//!
//! Nothing here deletes anything. [`categories`] turns an [`Env`] (the handful of Windows
//! folders it needs) into [`CategorySpec`]s the scanner can walk. The environment is injected so
//! tests can point everything at scratch directories.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::fsx::{inside_or_same, normal_depth, same_path};
use crate::model::CategoryInfo;

pub const USER_TEMP: &str = "user-temp";
pub const WINDOWS_TEMP: &str = "windows-temp";
pub const BROWSER_CACHE: &str = "browser-cache";
pub const THUMBNAILS: &str = "thumbnails";
pub const CRASH_REPORTS: &str = "crash-reports";
pub const INSTALLERS: &str = "installers";
pub const RECYCLE_BIN: &str = "recycle-bin";

/// Files changed more recently than this in the temp folders may still be in use.
pub const TEMP_MIN_AGE: Duration = Duration::from_secs(24 * 3600);
/// Downloaded installers are only offered once they are this old.
pub const INSTALLER_MIN_AGE: Duration = Duration::from_secs(30 * 24 * 3600);

pub const INFOS: [CategoryInfo; 7] = [
    CategoryInfo {
        id: USER_TEMP,
        title: "Temporary files (your account)",
        description: "Leftovers that apps and installers drop in your Temp folder. Anything changed in the last 24 hours is left alone because an app may still be using it.",
        risk_note: "Low risk. Apps recreate what they need. Files that are in use cannot be moved and are reported, not forced.",
        default_selected: true,
        badge: None,
        read_only: false,
    },
    CategoryInfo {
        id: WINDOWS_TEMP,
        title: "Windows temporary files",
        description: "Temporary files from Windows and system installers (the Temp folder inside the Windows folder). Anything changed in the last 24 hours is left alone.",
        risk_note: "Low risk. Windows may not let Krovi remove everything here without administrator rights. Those items are skipped. Krovi never asks for elevation.",
        default_selected: true,
        badge: None,
        read_only: false,
    },
    CategoryInfo {
        id: BROWSER_CACHE,
        title: "Browser caches",
        description: "Saved copies of pages, scripts and images that Chrome, Edge, Brave and Firefox keep so sites load faster. Only the cache folders are listed. Cookies, history and logins are never touched.",
        risk_note: "Low risk. Close the browser first, or the cache it is using cannot be moved. Sites load a little slower once while the cache fills up again.",
        default_selected: true,
        badge: None,
        read_only: false,
    },
    CategoryInfo {
        id: THUMBNAILS,
        title: "Thumbnail cache",
        description: "Small preview pictures File Explorer saves for images, videos and documents.",
        risk_note: "Low risk. Windows rebuilds them the next time you open a folder. Files Explorer is using right now cannot be moved and are reported.",
        default_selected: true,
        badge: None,
        read_only: false,
    },
    CategoryInfo {
        id: CRASH_REPORTS,
        title: "Crash dumps and error reports",
        description: "Memory dumps and reports that Windows and apps save after something crashes.",
        risk_note: "Low risk unless you are debugging a crash and still need them. Some system report folders need administrator rights and are skipped.",
        default_selected: true,
        badge: None,
        read_only: false,
    },
    CategoryInfo {
        id: INSTALLERS,
        title: "Old installers in Downloads",
        description: "Setup programs (.exe and .msi) sitting directly in your Downloads folder that have not changed in 30 days.",
        risk_note: "These are your own files, not system leftovers. Look at each one before you tick it: you may want an installer again. Not selected by default.",
        default_selected: false,
        badge: Some("Your files - review first"),
        read_only: false,
    },
    CategoryInfo {
        id: RECYCLE_BIN,
        title: "Recycle Bin",
        description: "What is in the Recycle Bin right now. Krovi only shows its size.",
        risk_note: "Krovi does not empty the Recycle Bin, because that cannot be undone. Empty it yourself from the desktop when you are sure. Everything Krovi cleans is moved here, so disk space only comes back once you empty it.",
        default_selected: false,
        badge: None,
        read_only: true,
    },
];

pub fn category_infos() -> Vec<CategoryInfo> {
    INFOS.to_vec()
}

fn info_for(id: &str) -> CategoryInfo {
    INFOS
        .iter()
        .find(|i| i.id == id)
        .cloned()
        .expect("category id is in INFOS")
}

/// Which names inside a folder belong to a category (lower-case; matching ignores case).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameFilter {
    pub prefix: Option<&'static str>,
    /// Extensions without the dot. Empty = any.
    pub extensions: &'static [&'static str],
}

impl NameFilter {
    pub fn matches(&self, name: &OsStr) -> bool {
        let n = name.to_string_lossy().to_lowercase();
        if let Some(p) = self.prefix {
            if !n.starts_with(p) {
                return false;
            }
        }
        if self.extensions.is_empty() {
            return true;
        }
        match n.rsplit_once('.') {
            Some((_, ext)) => self.extensions.contains(&ext),
            None => false,
        }
    }
}

/// How a root folder is cleaned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    /// What is inside the folder: files, and whole sub-folders when everything in them is fine.
    /// The folder itself is never touched.
    Tree,
    /// The folder itself is the single item (a browser cache folder). All or nothing.
    Whole,
    /// Only files directly in the folder whose names match. Sub-folders are ignored.
    Files(NameFilter),
}

#[derive(Clone, Debug)]
pub struct Root {
    pub path: PathBuf,
    pub shape: Shape,
    /// Friendly name for the item a `Whole` root produces.
    pub label: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Walk the roots and list what could be moved.
    Files,
    /// Report only the Recycle Bin's size.
    RecycleBin,
}

#[derive(Clone, Debug)]
pub struct CategorySpec {
    pub info: CategoryInfo,
    pub kind: Kind,
    pub roots: Vec<Root>,
    /// Only things not modified for at least this long are offered.
    pub min_age: Option<Duration>,
}

/// The few Windows folders the categories are built from.
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub temp: Option<PathBuf>,
    pub system_root: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub program_data: Option<PathBuf>,
    pub user_profile: Option<PathBuf>,
    pub downloads: Option<PathBuf>,
}

impl Env {
    /// Reads the real environment. Downloads comes from the shell's known-folder lookup when
    /// available (it can be moved off the profile), else `<profile>\Downloads`.
    pub fn from_os() -> Env {
        let var = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let user_profile = var("USERPROFILE");
        let downloads = crate::system::downloads_dir()
            .or_else(|| user_profile.as_ref().map(|p| p.join("Downloads")));
        Env {
            temp: var("TEMP").or_else(|| var("TMP")),
            system_root: var("SystemRoot").or_else(|| var("windir")),
            local_app_data: var("LOCALAPPDATA"),
            program_data: var("ProgramData"),
            user_profile,
            downloads,
        }
    }
}

/// Folders that must never be a cleaning root or sit inside one.
fn guard_dirs(env: &Env) -> Vec<PathBuf> {
    [
        &env.user_profile,
        &env.system_root,
        &env.local_app_data,
        &env.program_data,
        &env.downloads,
    ]
    .into_iter()
    .flatten()
    .cloned()
    .collect()
}

/// A root is refused when it is relative, a drive root, or is (or contains) the profile, the
/// Windows folder, AppData, ProgramData or Downloads. This stops a strange `TEMP` setting from
/// turning "clean temp" into "clean everything".
pub fn acceptable_root(root: &Root, guards: &[PathBuf]) -> bool {
    if !root.path.is_absolute() || normal_depth(&root.path) == 0 {
        return false;
    }
    match root.shape {
        // Only files with matching names are ever touched, so the folder may be anything.
        Shape::Files(_) => true,
        Shape::Tree | Shape::Whole => !guards.iter().any(|g| inside_or_same(g, &root.path)),
    }
}

fn tree(path: PathBuf) -> Root {
    Root {
        path,
        shape: Shape::Tree,
        label: None,
    }
}

/// Real directories directly inside `dir`, sorted by name. Links are not directories here.
fn child_dirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    out.sort();
    out
}

fn browser_roots(local: &Path) -> Vec<Root> {
    let mut roots = Vec::new();
    let chromium: [(&str, [&str; 3]); 3] = [
        ("Chrome", ["Google", "Chrome", "User Data"]),
        ("Edge", ["Microsoft", "Edge", "User Data"]),
        ("Brave", ["BraveSoftware", "Brave-Browser", "User Data"]),
    ];
    // Only the cache folders themselves. Everything else in a profile (cookies, history,
    // logins, bookmarks, extensions) is never named here.
    let cache_dirs: [(&[&str], &str); 3] = [
        (&["Cache", "Cache_Data"], "Cache"),
        (&["Code Cache"], "Code Cache"),
        (&["GPUCache"], "GPU Cache"),
    ];
    for (browser, parts) in chromium {
        let user_data = parts.iter().fold(local.to_path_buf(), |p, s| p.join(s));
        for (profile, dir) in child_dirs(&user_data) {
            if profile != "Default" && !profile.starts_with("Profile ") {
                continue;
            }
            for (rel, what) in cache_dirs {
                let path = rel.iter().fold(dir.clone(), |p, s| p.join(s));
                if fs::symlink_metadata(&path).is_ok() {
                    roots.push(Root {
                        path,
                        shape: Shape::Whole,
                        label: Some(format!("{browser} / {profile} / {what}")),
                    });
                }
            }
        }
    }
    let firefox = local.join("Mozilla").join("Firefox").join("Profiles");
    for (profile, dir) in child_dirs(&firefox) {
        let path = dir.join("cache2");
        if fs::symlink_metadata(&path).is_ok() {
            roots.push(Root {
                path,
                shape: Shape::Whole,
                label: Some(format!("Firefox / {profile} / Cache")),
            });
        }
    }
    roots
}

/// All categories with their folders resolved for `env`, in display order. Unsafe roots (see
/// [`acceptable_root`]) are dropped; folders that do not exist are kept (the scan reports
/// "nothing found").
pub fn categories(env: &Env) -> Vec<CategorySpec> {
    let guards = guard_dirs(env);
    let keep = |roots: Vec<Root>| -> Vec<Root> {
        roots
            .into_iter()
            .filter(|r| acceptable_root(r, &guards))
            .collect()
    };
    let spec = |id: &str, roots: Vec<Root>, min_age: Option<Duration>| CategorySpec {
        info: info_for(id),
        kind: Kind::Files,
        roots: keep(roots),
        min_age,
    };

    let user_temp = env.temp.clone().map(tree);
    let mut win_temp = env.system_root.as_ref().map(|r| tree(r.join("Temp")));
    // Running as SYSTEM, both point at the same folder: list it once.
    if let (Some(u), Some(w)) = (&user_temp, &win_temp) {
        if same_path(&u.path, &w.path) {
            win_temp = None;
        }
    }

    let local = env.local_app_data.as_deref();
    let thumbs = local.map(|l| Root {
        path: l.join("Microsoft").join("Windows").join("Explorer"),
        shape: Shape::Files(NameFilter {
            prefix: Some("thumbcache_"),
            extensions: &["db"],
        }),
        label: None,
    });
    let mut crash = Vec::new();
    if let Some(l) = local {
        crash.push(tree(l.join("CrashDumps")));
    }
    if let Some(pd) = &env.program_data {
        let wer = pd.join("Microsoft").join("Windows").join("WER");
        crash.push(tree(wer.join("ReportArchive")));
        crash.push(tree(wer.join("ReportQueue")));
    }
    let installers = env.downloads.clone().map(|d| Root {
        path: d,
        shape: Shape::Files(NameFilter {
            prefix: None,
            extensions: &["exe", "msi"],
        }),
        label: None,
    });

    vec![
        spec(
            USER_TEMP,
            user_temp.into_iter().collect(),
            Some(TEMP_MIN_AGE),
        ),
        spec(
            WINDOWS_TEMP,
            win_temp.into_iter().collect(),
            Some(TEMP_MIN_AGE),
        ),
        spec(
            BROWSER_CACHE,
            local.map(browser_roots).unwrap_or_default(),
            None,
        ),
        spec(THUMBNAILS, thumbs.into_iter().collect(), None),
        spec(CRASH_REPORTS, crash, None),
        spec(
            INSTALLERS,
            installers.into_iter().collect(),
            Some(INSTALLER_MIN_AGE),
        ),
        CategorySpec {
            info: info_for(RECYCLE_BIN),
            kind: Kind::RecycleBin,
            roots: Vec::new(),
            min_age: None,
        },
    ]
}

/// The specs for the requested category ids, in display order. Unknown ids are ignored.
pub fn select(all: Vec<CategorySpec>, ids: &[String]) -> Vec<CategorySpec> {
    all.into_iter()
        .filter(|c| ids.iter().any(|i| i == c.info.id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;
    use std::collections::HashSet;
    use std::ffi::OsString;

    fn fake_env(t: &TempDir) -> Env {
        Env {
            temp: Some(t.mkdir("profile/AppData/Local/Temp")),
            system_root: Some(t.mkdir("Windows")),
            local_app_data: Some(t.mkdir("profile/AppData/Local")),
            program_data: Some(t.mkdir("ProgramData")),
            user_profile: Some(t.mkdir("profile")),
            downloads: Some(t.mkdir("profile/Downloads")),
        }
    }

    fn by_id<'a>(v: &'a [CategorySpec], id: &str) -> &'a CategorySpec {
        v.iter().find(|c| c.info.id == id).unwrap()
    }

    #[test]
    fn there_are_seven_unique_categories_with_the_right_defaults() {
        let infos = category_infos();
        assert_eq!(infos.len(), 7);
        let ids: HashSet<_> = infos.iter().map(|i| i.id).collect();
        assert_eq!(ids.len(), 7);
        for i in &infos {
            assert!(
                !i.description.is_empty() && !i.risk_note.is_empty(),
                "{}",
                i.id
            );
        }
        let get = |id| infos.iter().find(|i| i.id == id).unwrap();
        assert!(get(USER_TEMP).default_selected);
        assert!(get(WINDOWS_TEMP).default_selected);
        assert!(get(BROWSER_CACHE).default_selected);
        assert!(get(THUMBNAILS).default_selected);
        assert!(get(CRASH_REPORTS).default_selected);
        assert!(
            !get(INSTALLERS).default_selected,
            "installers are the user's files"
        );
        assert!(get(INSTALLERS).badge.unwrap().contains("review"));
        assert!(get(RECYCLE_BIN).read_only && !get(RECYCLE_BIN).default_selected);
        assert!(get(RECYCLE_BIN).risk_note.contains("does not empty"));
        assert!(get(BROWSER_CACHE).risk_note.contains("Close the browser"));
    }

    #[test]
    fn name_filter_matches_prefix_and_extension_ignoring_case() {
        let thumb = NameFilter {
            prefix: Some("thumbcache_"),
            extensions: &["db"],
        };
        assert!(thumb.matches(OsStr::new("thumbcache_256.db")));
        assert!(thumb.matches(OsStr::new("THUMBCACHE_idx.DB")));
        assert!(!thumb.matches(OsStr::new("iconcache_256.db")));
        assert!(!thumb.matches(OsStr::new("thumbcache_256.txt")));
        assert!(!thumb.matches(OsStr::new("thumbcache_256")));
        let exe = NameFilter {
            prefix: None,
            extensions: &["exe", "msi"],
        };
        assert!(exe.matches(&OsString::from("Setup.EXE")));
        assert!(exe.matches(OsStr::new("a.b.msi")));
        assert!(!exe.matches(OsStr::new("readme.txt")));
        assert!(!exe.matches(OsStr::new("exe")));
    }

    #[test]
    fn roots_follow_the_spec() {
        let t = TempDir::new("cat-roots");
        let env = fake_env(&t);
        let cats = categories(&env);
        assert_eq!(cats.len(), 7);

        let u = by_id(&cats, USER_TEMP);
        assert_eq!(u.roots.len(), 1);
        assert_eq!(u.roots[0].path, env.temp.clone().unwrap());
        assert_eq!(u.roots[0].shape, Shape::Tree);
        assert_eq!(u.min_age, Some(TEMP_MIN_AGE));

        let w = by_id(&cats, WINDOWS_TEMP);
        assert_eq!(
            w.roots[0].path,
            env.system_root.clone().unwrap().join("Temp")
        );
        assert_eq!(w.min_age, Some(TEMP_MIN_AGE));

        let th = by_id(&cats, THUMBNAILS);
        assert!(th.roots[0].path.ends_with("Explorer"));
        assert!(matches!(th.roots[0].shape, Shape::Files(_)));

        let crash = by_id(&cats, CRASH_REPORTS);
        let names: Vec<_> = crash
            .roots
            .iter()
            .map(|r| r.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["CrashDumps", "ReportArchive", "ReportQueue"]);

        let inst = by_id(&cats, INSTALLERS);
        assert_eq!(inst.roots[0].path, env.downloads.clone().unwrap());
        assert_eq!(inst.min_age, Some(INSTALLER_MIN_AGE));
        assert_eq!(INSTALLER_MIN_AGE, Duration::from_secs(30 * 86_400));

        let bin = by_id(&cats, RECYCLE_BIN);
        assert_eq!(bin.kind, Kind::RecycleBin);
        assert!(bin.roots.is_empty());
    }

    #[test]
    fn browser_caches_are_only_the_cache_folders() {
        let t = TempDir::new("cat-browsers");
        let env = fake_env(&t);
        let local = env.local_app_data.clone().unwrap();
        let chrome = local.join("Google").join("Chrome").join("User Data");
        for rel in [
            "Default/Cache/Cache_Data",
            "Default/Code Cache",
            "Default/GPUCache",
            "Profile 2/Cache/Cache_Data",
            "Guest Profile/Cache/Cache_Data",
            "System Profile/GPUCache",
            "Default/Network", // cookies live here
            "Default/Local Storage",
        ] {
            fs::create_dir_all(chrome.join(rel)).unwrap();
        }
        fs::write(chrome.join("Default").join("Login Data"), b"secret").unwrap();
        fs::write(chrome.join("Default").join("History"), b"history").unwrap();
        let edge = local.join("Microsoft").join("Edge").join("User Data");
        fs::create_dir_all(edge.join("Default").join("Cache").join("Cache_Data")).unwrap();
        let brave = local
            .join("BraveSoftware")
            .join("Brave-Browser")
            .join("User Data");
        fs::create_dir_all(brave.join("Default").join("GPUCache")).unwrap();
        let ff = local.join("Mozilla").join("Firefox").join("Profiles");
        fs::create_dir_all(ff.join("ab12cd.default-release").join("cache2")).unwrap();
        fs::create_dir_all(ff.join("ab12cd.default-release").join("storage")).unwrap();
        fs::create_dir_all(ff.join("no-cache-here")).unwrap();

        let cats = categories(&env);
        let b = by_id(&cats, BROWSER_CACHE);
        let got: Vec<String> = b.roots.iter().map(|r| r.label.clone().unwrap()).collect();
        assert_eq!(
            got,
            [
                "Chrome / Default / Cache",
                "Chrome / Default / Code Cache",
                "Chrome / Default / GPU Cache",
                "Chrome / Profile 2 / Cache",
                "Edge / Default / Cache",
                "Brave / Default / GPU Cache",
                "Firefox / ab12cd.default-release / Cache",
            ]
        );
        assert!(b.roots.iter().all(|r| r.shape == Shape::Whole));
        // never the places that hold secrets
        for r in &b.roots {
            let s = r.path.to_string_lossy().to_lowercase();
            for bad in [
                "network",
                "login",
                "history",
                "cookies",
                "local storage",
                "storage",
            ] {
                assert!(!s.contains(bad), "{s} must not be a cache root");
            }
        }
        assert_eq!(b.min_age, None);
    }

    #[test]
    fn same_user_and_windows_temp_is_listed_once() {
        let t = TempDir::new("cat-same");
        let mut env = fake_env(&t);
        let win = env.system_root.clone().unwrap().join("Temp");
        fs::create_dir_all(&win).unwrap();
        env.temp = Some(win);
        let cats = categories(&env);
        assert_eq!(by_id(&cats, USER_TEMP).roots.len(), 1);
        assert!(by_id(&cats, WINDOWS_TEMP).roots.is_empty());
    }

    #[test]
    fn dangerous_roots_are_refused() {
        let t = TempDir::new("cat-guard");
        let base = fake_env(&t);

        // TEMP pointing at the whole profile, a drive root, the Windows folder, or a relative path
        for bad in [
            base.user_profile.clone().unwrap(),
            t.path().parent().unwrap().to_path_buf(), // an ancestor of the profile
            PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" }),
            base.system_root.clone().unwrap(),
            base.local_app_data.clone().unwrap(),
            PathBuf::from("relative/temp"),
        ] {
            let mut env = base.clone();
            env.temp = Some(bad.clone());
            let cats = categories(&env);
            assert!(
                by_id(&cats, USER_TEMP).roots.is_empty(),
                "{bad:?} must not be accepted as a temp root"
            );
        }

        // a normal temp inside the profile is fine
        let cats = categories(&base);
        assert_eq!(by_id(&cats, USER_TEMP).roots.len(), 1);

        // Downloads may be a filter root (only matching files are touched) but never a Tree.
        let guards = guard_dirs(&base);
        let downloads = base.downloads.clone().unwrap();
        let filter = Root {
            path: downloads.clone(),
            shape: Shape::Files(NameFilter {
                prefix: None,
                extensions: &["exe"],
            }),
            label: None,
        };
        assert!(acceptable_root(&filter, &guards));
        assert!(!acceptable_root(&tree(downloads), &guards));
    }

    #[test]
    fn select_keeps_display_order_and_ignores_unknown_ids() {
        let t = TempDir::new("cat-select");
        let cats = categories(&fake_env(&t));
        let ids = vec![
            INSTALLERS.to_string(),
            USER_TEMP.to_string(),
            "nonsense".to_string(),
        ];
        let got: Vec<_> = select(cats, &ids).iter().map(|c| c.info.id).collect();
        assert_eq!(got, [USER_TEMP, INSTALLERS]);
    }
}
