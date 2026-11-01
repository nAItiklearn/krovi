//! The Windows side of Voice v3: enumerate the closed folder / app sets, open and launch them,
//! and query the Windows Search index. Everything is a local COM / Win32 call - Shell namespace
//! enumeration, `ShellExecuteW`, and the OLE DB provider of the Windows Search service
//! (`Search.CollatorDSO`, an in-process local index). There is no network code in this file.
//!
//! * Folders: the Known Folders (Desktop, Documents, Downloads, Pictures, Music, Videos), the
//!   "Krovi downloads" folder (`Downloads\Krovi`), and the folders in Quick Access (pinned and
//!   frequent), enumerated from the Shell namespace `shell:::{679f85cb-...}`.
//! * Apps: the Start-menu app list (the `shell:AppsFolder` namespace, `FOLDERID_AppsFolder`),
//!   plus any Start-menu `.lnk` not already in it.
//! * Search: `SELECT ... FROM SystemIndex WHERE scope='file:' AND System.FileName LIKE ...`.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use windows::core::{Interface, GUID, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::Com::{
    CLSIDFromProgID, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Search::{
    IAccessor, ICommand, ICommandText, IDBCreateCommand, IDBCreateSession, IDBInitialize, IRowset,
    DBACCESSOR_ROWDATA, DBBINDING, DBMEMOWNER_PROVIDEROWNED, DBPARAMIO_NOTPARAM, DBPART_STATUS,
    DBPART_VALUE, DBTYPE_BYREF, DBTYPE_WSTR, HACCESSOR,
};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads,
    FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos, IEnumShellItems, IShellItem,
    SHCreateItemFromParsingName, SHGetKnownFolderItem, SHGetKnownFolderPath, ShellExecuteW,
    KF_FLAG_DEFAULT, SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::actions::Host;
use crate::catalog::{Catalog, Entry};
use crate::types::{ResultItem, ResultKind};

/// Give up on a Shell enumeration after this long (a hung network drive must not hang Krovi).
const SCAN_TIMEOUT: Duration = Duration::from_secs(10);
/// Typical index queries take 0.4-1.3 s; the indexer can stall for a few seconds right after many
/// files change, so allow generously before reporting "not available".
const SEARCH_TIMEOUT: Duration = Duration::from_secs(15);

const APPS_PREFIX: &str = "shell:AppsFolder\\";

// ---------------------------------------------------------------------------------------------
// plumbing
// ---------------------------------------------------------------------------------------------

/// Run `f` on a fresh COM apartment thread with a deadline.
fn on_sta<T: Send + 'static>(
    name: &str,
    timeout: Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            // SAFETY: COM is initialised for this thread's lifetime and released before it ends.
            let inited = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
            let value = f();
            if inited {
                // SAFETY: matches the successful CoInitializeEx above.
                unsafe { CoUninitialize() };
            }
            let _ = tx.send(value);
        })
        .ok()?;
    rx.recv_timeout(timeout).ok()
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Take ownership of a CoTaskMem string from the shell.
unsafe fn take_pwstr(p: PWSTR) -> String {
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as *const c_void));
    s
}

// ---------------------------------------------------------------------------------------------
// folders
// ---------------------------------------------------------------------------------------------

unsafe fn known_folder(id: &GUID) -> Option<PathBuf> {
    let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
    let s = take_pwstr(p);
    (!s.is_empty()).then(|| PathBuf::from(s))
}

/// Desktop, Documents, Downloads, Pictures, Music, Videos and "Krovi downloads".
pub fn known_folders() -> Vec<Entry> {
    // SAFETY: SHGetKnownFolderPath needs no COM apartment; strings are freed by `take_pwstr`.
    unsafe {
        let mut out = Vec::new();
        for (name, id) in [
            ("Desktop", &FOLDERID_Desktop),
            ("Documents", &FOLDERID_Documents),
            ("Downloads", &FOLDERID_Downloads),
            ("Pictures", &FOLDERID_Pictures),
            ("Music", &FOLDERID_Music),
            ("Videos", &FOLDERID_Videos),
        ] {
            if let Some(p) = known_folder(id) {
                if name == "Downloads" {
                    // listed right after Downloads so "krovi downloads" keeps its own entry
                    out.push(Entry::new(name, p.to_string_lossy().into_owned()));
                    out.push(Entry::new(
                        "Krovi downloads",
                        p.join("Krovi").to_string_lossy().into_owned(),
                    ));
                } else {
                    out.push(Entry::new(name, p.to_string_lossy().into_owned()));
                }
            }
        }
        out
    }
}

/// Enumerate the children of a Shell namespace item as (display name, parsing name, file path).
unsafe fn enum_children(item: &IShellItem) -> Vec<IShellItem> {
    let mut out = Vec::new();
    let Ok(en) = item.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) else {
        return out;
    };
    loop {
        let mut slot: [Option<IShellItem>; 1] = [None];
        let mut fetched = 0u32;
        if en.Next(&mut slot, Some(&mut fetched)).is_err() || fetched == 0 {
            break;
        }
        match slot[0].take() {
            Some(child) => out.push(child),
            None => break,
        }
        if out.len() > 2000 {
            break;
        }
    }
    out
}

/// The folders pinned to, or frequently used in, Explorer's Quick Access.
pub fn quick_access_folders() -> Vec<Entry> {
    on_sta("krovi-voice-quickaccess", SCAN_TIMEOUT, || {
        // SAFETY: COM is initialised on this thread by `on_sta`.
        unsafe {
            let wide_name = wide("shell:::{679f85cb-0220-4080-b29b-5540cc05aab6}");
            let Ok(root) =
                SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(wide_name.as_ptr()), None)
            else {
                return Vec::new();
            };
            let mut out = Vec::new();
            for child in enum_children(&root) {
                let Ok(path) = child
                    .GetDisplayName(SIGDN_FILESYSPATH)
                    .map(|p| take_pwstr(p))
                else {
                    continue;
                };
                if !Path::new(&path).is_dir() {
                    continue;
                }
                let name = child
                    .GetDisplayName(SIGDN_NORMALDISPLAY)
                    .map(|p| take_pwstr(p))
                    .unwrap_or_default();
                let name = if name.is_empty() {
                    Path::new(&path)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                } else {
                    name
                };
                out.push(Entry::new(name, path));
            }
            out
        }
    })
    .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// apps
// ---------------------------------------------------------------------------------------------

/// The Start-menu apps as the shell lists them (`shell:AppsFolder`).
pub fn apps_folder_apps() -> Vec<Entry> {
    on_sta("krovi-voice-apps", SCAN_TIMEOUT, || {
        // SAFETY: COM is initialised on this thread by `on_sta`.
        unsafe {
            let Ok(root) = SHGetKnownFolderItem::<_, IShellItem>(
                &FOLDERID_AppsFolder,
                KF_FLAG_DEFAULT,
                HANDLE::default(),
            ) else {
                return Vec::new();
            };
            let mut out = Vec::new();
            for child in enum_children(&root) {
                let name = child
                    .GetDisplayName(SIGDN_NORMALDISPLAY)
                    .map(|p| take_pwstr(p))
                    .unwrap_or_default();
                let parsing = child
                    .GetDisplayName(SIGDN_PARENTRELATIVEPARSING)
                    .map(|p| take_pwstr(p))
                    .unwrap_or_default();
                if name.is_empty() || parsing.is_empty() || !is_app_target(&parsing) {
                    continue;
                }
                out.push(Entry::new(name, format!("{APPS_PREFIX}{parsing}")));
            }
            out
        }
    })
    .unwrap_or_default()
}

/// The Start-menu app list also contains web links ("Git FAQs"), documents ("Release Notes") and
/// the like. They are not apps: launching one would open a browser or a document viewer, so they
/// never enter the catalog (and nothing "open ..." says can start a web page).
fn is_app_target(parsing_name: &str) -> bool {
    let p = parsing_name.to_ascii_lowercase();
    const WEB: [&str; 4] = ["http://", "https://", "ftp://", "ftps://"];
    const DOCS: [&str; 10] = [
        ".html", ".htm", ".url", ".txt", ".pdf", ".chm", ".md", ".rtf", ".doc", ".docx",
    ];
    !WEB.iter().any(|w| p.contains(w)) && !DOCS.iter().any(|d| p.ends_with(d))
}

fn collect_lnks(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for item in rd.flatten() {
        let p = item.path();
        if p.is_dir() {
            if depth > 0 {
                collect_lnks(&p, depth - 1, out);
            }
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
            out.push(p);
        }
    }
}

/// Start-menu shortcuts (`.lnk`) of this user and of all users.
pub fn start_menu_apps() -> Vec<Entry> {
    let mut roots = Vec::new();
    for var in ["APPDATA", "ProgramData"] {
        if let Some(base) = std::env::var_os(var) {
            roots.push(
                PathBuf::from(base)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Start Menu")
                    .join("Programs"),
            );
        }
    }
    let mut links = Vec::new();
    for r in &roots {
        collect_lnks(r, 3, &mut links);
    }
    links
        .into_iter()
        .filter_map(|p| {
            let name = p.file_stem()?.to_string_lossy().into_owned();
            Some(Entry::new(name, p.to_string_lossy().into_owned()))
        })
        .collect()
}

/// Everything "open ..." / "what's in ..." may name right now. Takes a fraction of a second.
pub fn scan_catalog() -> Catalog {
    let mut folders = known_folders();
    folders.extend(quick_access_folders());
    let mut apps = apps_folder_apps();
    apps.extend(start_menu_apps()); // dedup keeps the AppsFolder entry when both exist
    Catalog::new(folders, apps)
}

// ---------------------------------------------------------------------------------------------
// opening and launching
// ---------------------------------------------------------------------------------------------

/// Open a path or a `shell:` target the way a double-click would. Runs on its own COM apartment
/// thread (ShellExecute wants an STA; the speech thread is an MTA).
fn shell_open(target: &str) -> Result<(), String> {
    let target = target.to_string();
    on_sta("krovi-voice-open", Duration::from_secs(8), move || {
        shell_open_here(&target)
    })
    .unwrap_or_else(|| Err("Windows did not respond in time".into()))
}

/// Show a file selected in a new/reused Explorer window (never runs the file).
pub fn reveal(path: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    if !Path::new(path).exists() {
        return Err("that file no longer exists".into());
    }
    std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{path}\""))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn shell_open_here(target: &str) -> Result<(), String> {
    let t = wide(target);
    // SAFETY: `t` is a valid NUL-terminated wide string for the duration of the call.
    let h = unsafe {
        ShellExecuteW(
            HWND::default(),
            windows::core::w!("open"),
            PCWSTR(t.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute reports success with a value greater than 32.
    if (h.0 as isize) > 32 {
        Ok(())
    } else {
        Err(format!("Windows could not open it (code {})", h.0 as isize))
    }
}

/// The real [`Host`]: Explorer, the Start menu and the Windows Search index.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinHost;

impl Host for WinHost {
    fn open_folder(&self, path: &str) -> Result<(), String> {
        if !Path::new(path).is_dir() {
            return Err("that folder does not exist".into());
        }
        shell_open(path)
    }

    fn launch_app(&self, target: &str) -> Result<(), String> {
        // Only what the catalog hands out: an AppsFolder entry or a Start-menu shortcut.
        let ok = target.starts_with(APPS_PREFIX)
            || (target.to_ascii_lowercase().ends_with(".lnk") && Path::new(target).is_file());
        if !ok {
            return Err("not an installed Start-menu app".into());
        }
        shell_open(target)
    }

    fn search_files(&self, query: &str, limit: usize) -> Result<Vec<ResultItem>, String> {
        search_index(query, limit)
    }
}

// ---------------------------------------------------------------------------------------------
// Windows Search (OLE DB provider Search.CollatorDSO)
// ---------------------------------------------------------------------------------------------

/// DBGUID_DEFAULT: the provider's default command dialect (Windows Search SQL).
const DBGUID_DEFAULT: GUID = GUID::from_u128(0xC8B521FB_5CF3_11CE_ADE5_00AA0044773D);
const DB_S_ENDOFROWSET_OR_NO_ROWS: i32 = 0;

/// What one row looks like once the accessor has copied it: for each of the two columns, a status
/// word and a pointer to provider-owned UTF-16 text.
#[repr(C)]
struct Row {
    path_status: u32,
    path: *const u16,
    name_status: u32,
    name: *const u16,
}

/// DBSTATUS_S_OK
const DBSTATUS_OK: u32 = 0;

fn sql_for(query: &str, limit: usize) -> Option<String> {
    // Only the search term is interpolated, with quotes doubled; % and control characters are
    // dropped (so it cannot widen the LIKE pattern or break the statement).
    let term: String = query
        .chars()
        .filter(|c| !c.is_control() && *c != '%' && *c != '\u{0}')
        .collect::<String>()
        .trim()
        .replace('\'', "''");
    if term.is_empty() {
        return None;
    }
    Some(format!(
        "SELECT TOP {limit} System.ItemPathDisplay, System.ItemName FROM SystemIndex \
         WHERE scope='file:' AND System.FileName LIKE '%{term}%' \
         AND NOT System.ItemPathDisplay LIKE '%\\AppData\\%' \
         ORDER BY System.DateModified DESC"
    ))
}

/// Query the Windows Search index for file or folder names containing `query`. Local only: the
/// provider talks to the Windows Search service on this PC.
pub fn search_index(query: &str, limit: usize) -> Result<Vec<ResultItem>, String> {
    let sql = sql_for(query, limit.clamp(1, 100)).ok_or("there is nothing to search for")?;
    on_sta("krovi-voice-search", SEARCH_TIMEOUT, move || {
        // SAFETY: COM is initialised on this thread by `on_sta`; every raw pointer below comes
        // from the provider and is only read while the rows holding it are alive.
        unsafe { run_query(&sql) }
    })
    .unwrap_or_else(|| Err("Windows Search did not answer in time".into()))
}

unsafe fn run_query(sql: &str) -> Result<Vec<ResultItem>, String> {
    let err = |what: &str, e: windows::core::Error| format!("{what}: {e}");
    let clsid = CLSIDFromProgID(windows::core::w!("Search.CollatorDSO"))
        .map_err(|e| err("the Windows Search provider is not installed", e))?;
    let init: IDBInitialize = CoCreateInstance(&clsid, None, CLSCTX_ALL)
        .map_err(|e| err("could not reach Windows Search", e))?;
    init.Initialize()
        .map_err(|e| err("Windows Search is not running", e))?;
    let sessions: IDBCreateSession = init.cast().map_err(|e| err("Windows Search", e))?;
    let create: IDBCreateCommand = sessions
        .CreateSession(None, &IDBCreateCommand::IID)
        .and_then(|u| u.cast())
        .map_err(|e| err("could not open a Windows Search session", e))?;
    let command: ICommandText = create
        .CreateCommand(None, &ICommandText::IID)
        .and_then(|u| u.cast())
        .map_err(|e| err("could not create a search command", e))?;
    let text = wide(sql);
    command
        .SetCommandText(&DBGUID_DEFAULT, PCWSTR(text.as_ptr()))
        .map_err(|e| err("could not set the search text", e))?;

    let mut affected = 0isize;
    let mut rowset: Option<windows::core::IUnknown> = None;
    command
        .cast::<ICommand>()
        .map_err(|e| err("search command", e))?
        .Execute(
            None,
            &IRowset::IID,
            None,
            Some(&mut affected),
            Some(&mut rowset),
        )
        .map_err(|e| err("the search failed", e))?;
    let rowset: IRowset = rowset
        .ok_or_else(|| "the search returned no rows object".to_string())?
        .cast()
        .map_err(|e| err("search rows", e))?;

    // Bind the two columns as provider-owned UTF-16 strings.
    let accessor: IAccessor = rowset.cast().map_err(|e| err("search accessor", e))?;
    let mut bindings: [DBBINDING; 2] = std::mem::zeroed();
    for (i, (value, status)) in [
        (
            std::mem::offset_of!(Row, path),
            std::mem::offset_of!(Row, path_status),
        ),
        (
            std::mem::offset_of!(Row, name),
            std::mem::offset_of!(Row, name_status),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let b = &mut bindings[i];
        b.iOrdinal = i + 1;
        b.obValue = value;
        b.obStatus = status;
        b.dwPart = (DBPART_VALUE.0 | DBPART_STATUS.0) as u32;
        b.dwMemOwner = DBMEMOWNER_PROVIDEROWNED.0 as u32;
        b.eParamIO = DBPARAMIO_NOTPARAM.0 as u32;
        b.wType = (DBTYPE_WSTR.0 | DBTYPE_BYREF.0) as u16;
    }
    let mut haccessor = HACCESSOR::default();
    accessor
        .CreateAccessor(
            DBACCESSOR_ROWDATA.0 as u32,
            2,
            bindings.as_ptr(),
            std::mem::size_of::<Row>(),
            &mut haccessor,
            None,
        )
        .map_err(|e| err("could not bind the search columns", e))?;

    let mut results = Vec::new();
    'fetch: loop {
        // The provider allocates the row-handle array (all-NULL slice in) and returns its address
        // in the first element.
        let mut got = 0usize;
        let mut slots = [std::ptr::null_mut::<usize>(); 50];
        if rowset.GetNextRows(0, 0, &mut got, &mut slots).is_err() {
            break;
        }
        let rows = slots[0];
        if got == 0 || rows.is_null() {
            break;
        }
        let handles = std::slice::from_raw_parts(rows, got);
        for &hrow in handles {
            let mut row: Row = std::mem::zeroed();
            if rowset
                .GetData(hrow, haccessor, &mut row as *mut Row as *mut c_void)
                .is_ok()
                && row.path_status == DBSTATUS_OK
                && !row.path.is_null()
            {
                let path = PCWSTR(row.path).to_string().unwrap_or_default();
                let name = if row.name_status == DBSTATUS_OK && !row.name.is_null() {
                    PCWSTR(row.name).to_string().unwrap_or_default()
                } else {
                    String::new()
                };
                let name = if name.is_empty() {
                    Path::new(&path)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                } else {
                    name
                };
                if !path.is_empty() {
                    let kind = if Path::new(&path).is_dir() {
                        ResultKind::Folder
                    } else {
                        ResultKind::File
                    };
                    let detail = std::fs::metadata(&path).ok().and_then(|m| {
                        let when = m
                            .modified()
                            .ok()
                            .and_then(crate::civil::describe_system_time);
                        match (m.is_dir(), when) {
                            (true, Some(w)) => Some(format!("folder, {w}")),
                            (true, None) => None,
                            (false, Some(w)) => {
                                Some(format!("{}, {w}", crate::listing::human_size(m.len())))
                            }
                            (false, None) => Some(crate::listing::human_size(m.len())),
                        }
                    });
                    results.push(ResultItem {
                        kind,
                        name,
                        path,
                        detail,
                    });
                }
            }
            if results.len() >= 100 {
                let _ = rowset.ReleaseRows(
                    got,
                    rows,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
                CoTaskMemFree(Some(rows as *const c_void));
                break 'fetch;
            }
        }
        let _ = rowset.ReleaseRows(
            got,
            rows,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        CoTaskMemFree(Some(rows as *const c_void));
    }
    let _ = accessor.ReleaseAccessor(haccessor, None);
    let _ = DB_S_ENDOFROWSET_OR_NO_ROWS;
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_search_statement_escapes_and_bounds_the_term() {
        let sql = sql_for("it's 100% fine", 20).unwrap();
        assert!(sql.contains("LIKE '%it''s 100 fine%'"), "{sql}");
        assert!(sql.starts_with("SELECT TOP 20 "));
        assert!(sql.contains("scope='file:'"));
        assert!(sql_for("   ", 20).is_none());
        assert!(sql_for("%%%", 20).is_none());
        let evil = sql_for("x' OR 1=1 --", 5).unwrap();
        assert!(evil.contains("'%x'' OR 1=1 --%'"), "{evil}");
    }

    #[test]
    fn known_folders_exist_on_this_machine() {
        let k = known_folders();
        let names: Vec<&str> = k.iter().map(|e| e.name.as_str()).collect();
        for want in ["Desktop", "Documents", "Downloads", "Krovi downloads"] {
            assert!(names.contains(&want), "{names:?}");
        }
    }

    #[test]
    fn web_links_and_documents_are_not_apps() {
        assert!(is_app_target("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App"));
        assert!(is_app_target(
            r"{6D809377-6AF0-444B-8957-A3773F02200E}\7-Zip\7zFM.exe"
        ));
        assert!(is_app_target("steam://rungameid/1815780"));
        assert!(!is_app_target("https://gitforwindows.org/faq"));
        assert!(!is_app_target("http://www.kmplayer.com/forums"));
        assert!(!is_app_target(
            r"{6D809377-6AF0-444B-8957-A3773F02200E}\Git\ReleaseNotes.html"
        ));
        assert!(!is_app_target(r"C:\x\readme.TXT"));
    }

    #[test]
    fn launching_only_accepts_catalog_style_targets() {
        let host = WinHost;
        assert!(host.launch_app(r"C:\Windows\System32\cmd.exe").is_err());
        assert!(host.launch_app("https://example.com").is_err());
        assert!(host.launch_app(r"C:\nope\x.lnk").is_err());
        assert!(host.open_folder(r"Z:\definitely\not\here").is_err());
    }
}
