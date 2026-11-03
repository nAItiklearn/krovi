//! The Windows-only pieces, behind the `native` feature: the real Recycle Bin call, the Recycle
//! Bin's size, and the Downloads folder lookup. Everything else in the crate is plain Rust and is
//! tested with a fake [`Recycler`].

use std::collections::HashMap;
use std::ffi::{c_void, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{BOOL, ERROR_SUCCESS, HANDLE, HWND};
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Registry::{
    RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Downloads, SHFileOperationW, SHGetKnownFolderPath, SHQueryRecycleBinW, FOF_ALLOWUNDO,
    FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, KF_FLAG_DEFAULT, SHFILEOPSTRUCTW,
    SHQUERYRBINFO,
};

use crate::delete::{recycle_verdict, shell_error_text, BinFacts, RecycleError, Recycler};
use crate::fsx::has_wildcard;
use crate::model::RecycleBinSize;

/// `DE_OPCANCELLED`: what `fAnyOperationsAborted` means.
const OP_CANCELLED: i32 = 0x75;

/// The path list `SHFileOperationW` wants: each path NUL-terminated, then one more NUL.
/// Refuses empty names, wildcards and embedded NULs (the shell would expand or truncate them).
pub fn pack_paths(paths: &[PathBuf]) -> Result<Vec<u16>, String> {
    let mut out: Vec<u16> = Vec::new();
    for p in paths {
        if has_wildcard(p) {
            return Err("A path contains a wildcard or NUL character.".into());
        }
        let wide: Vec<u16> = p.as_os_str().encode_wide().collect();
        if wide.is_empty() {
            return Err("A path is empty.".into());
        }
        out.extend(wide);
        out.push(0);
    }
    if out.is_empty() {
        return Err("There is nothing to move.".into());
    }
    out.push(0);
    Ok(out)
}

fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// A DWORD from the registry, or `None` if the key or value is missing.
fn reg_dword(root: HKEY, subkey: &str, value: &str) -> Option<u32> {
    let (key, name) = (HSTRING::from(subkey), HSTRING::from(value));
    let mut data = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: `data`/`size` outlive the call and `size` is the byte size of `data`.
    let status = unsafe {
        RegGetValueW(
            root,
            &key,
            &name,
            RRF_RT_REG_DWORD,
            None,
            Some(std::ptr::addr_of_mut!(data).cast()),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then_some(data)
}

/// `\\?\Volume{1234-...}\` -> `{1234-...}`.
fn volume_guid(volume_name: &str) -> Option<&str> {
    let start = volume_name.find('{')?;
    let end = volume_name.find('}')?;
    (end > start).then(|| &volume_name[start..=end])
}

/// The mount point of the volume `path` is on (`C:\`), as Windows reports it.
fn volume_root(path: &Path) -> Option<String> {
    let file = HSTRING::from(path.as_os_str());
    let mut buf = vec![0u16; 1024];
    // SAFETY: `buf` is a valid writable buffer whose length is passed to the call.
    unsafe { GetVolumePathNameW(&file, &mut buf) }.ok()?;
    Some(wide_to_string(&buf))
}

/// What Windows says about the Recycle Bin of the volume mounted at `root`. Read-only.
fn bin_facts_for_root(root: &str) -> BinFacts {
    const POLICY: &str = r"Software\Microsoft\Windows\CurrentVersion\Policies\Explorer";
    const VOLUMES: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\BitBucket\Volume";
    let mut f = BinFacts {
        policy_deletes: reg_dword(HKEY_CURRENT_USER, POLICY, "NoRecycleFiles") == Some(1)
            || reg_dword(HKEY_LOCAL_MACHINE, POLICY, "NoRecycleFiles") == Some(1),
        ..BinFacts::default()
    };
    let root_w = HSTRING::from(root);
    // SAFETY: `root_w` is a valid NUL-terminated wide string for the duration of the call.
    // 3 is DRIVE_FIXED.
    f.fixed_drive = unsafe { GetDriveTypeW(&root_w) } == 3;
    if !f.fixed_drive {
        return f;
    }
    let mut name = [0u16; 64];
    // SAFETY: `name` is a valid writable buffer whose length is passed to the call.
    if unsafe { GetVolumeNameForVolumeMountPointW(&root_w, &mut name) }.is_ok() {
        if let Some(guid) = volume_guid(&wide_to_string(&name)) {
            f.volume_known = true;
            let key = format!(r"{VOLUMES}\{guid}");
            f.turned_off =
                reg_dword(HKEY_CURRENT_USER, &key, "NukeOnDelete").is_some_and(|v| v != 0);
            f.max_capacity_mib = reg_dword(HKEY_CURRENT_USER, &key, "MaxCapacity").map(u64::from);
        }
    }
    if f.max_capacity_mib.is_none() {
        // No explicit limit: Windows' default is about 5% of the volume. Assume that, so an item
        // the bin cannot hold is left alone instead of being deleted for good. Unknown size: 0,
        // which refuses everything rather than guessing.
        let mut total: u64 = 0;
        // SAFETY: `root_w` is a valid NUL-terminated wide string; `total` outlives the call.
        let ok = unsafe { GetDiskFreeSpaceExW(&root_w, None, Some(&mut total), None) }.is_ok();
        f.max_capacity_mib = Some(if ok { total / 20 / (1024 * 1024) } else { 0 });
    }
    f
}

/// Moves paths to the Recycle Bin with `SHFileOperationW`:
/// `FO_DELETE | FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI`.
/// `FOF_ALLOWUNDO` is what makes it a move to the Recycle Bin instead of a permanent delete.
/// Because `FOF_NOCONFIRMATION` also silently turns "cannot be recycled" into a permanent
/// delete, [`Recycler::will_recycle`] checks the drive's Recycle Bin settings first.
pub struct ShellRecycler {
    /// Recycle Bin facts per volume mount point (lower-case), so a long list asks Windows once.
    volumes: Mutex<HashMap<String, BinFacts>>,
}

impl ShellRecycler {
    pub fn new() -> ShellRecycler {
        ShellRecycler {
            volumes: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for ShellRecycler {
    fn default() -> Self {
        ShellRecycler::new()
    }
}

impl Recycler for ShellRecycler {
    fn will_recycle(&self, path: &Path, bytes: u64) -> Result<(), String> {
        let Some(root) = volume_root(path) else {
            return recycle_verdict(&BinFacts::default(), bytes);
        };
        let facts = {
            let mut cache = self.volumes.lock().unwrap_or_else(PoisonError::into_inner);
            *cache
                .entry(root.to_lowercase())
                .or_insert_with(|| bin_facts_for_root(&root))
        };
        recycle_verdict(&facts, bytes)
    }

    fn recycle(&self, paths: &[PathBuf]) -> Result<(), RecycleError> {
        let wide = pack_paths(paths).map_err(|reason| RecycleError { code: -1, reason })?;
        let flags = FOF_ALLOWUNDO.0 | FOF_NOCONFIRMATION.0 | FOF_SILENT.0 | FOF_NOERRORUI.0;
        let mut op = SHFILEOPSTRUCTW {
            hwnd: HWND::default(),
            wFunc: FO_DELETE,
            pFrom: PCWSTR(wide.as_ptr()),
            pTo: PCWSTR::null(),
            fFlags: flags as u16,
            fAnyOperationsAborted: BOOL(0),
            hNameMappings: std::ptr::null_mut(),
            lpszProgressTitle: PCWSTR::null(),
        };
        // SAFETY: `op` is fully initialised; `pFrom` points into `wide`, which outlives the call
        // and is terminated by a double NUL as the API requires.
        let code = unsafe { SHFileOperationW(&mut op) };
        // copy out of the (packed on x86) struct before using it
        let aborted = { op.fAnyOperationsAborted };
        if code != 0 {
            Err(RecycleError {
                code,
                reason: shell_error_text(code),
            })
        } else if aborted.as_bool() {
            Err(RecycleError {
                code: OP_CANCELLED,
                reason: shell_error_text(OP_CANCELLED),
            })
        } else {
            Ok(())
        }
    }
}

/// Total size and item count of the Recycle Bin across all drives. `None` if Windows will not say.
pub fn recycle_bin_size() -> Option<RecycleBinSize> {
    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        i64Size: 0,
        i64NumItems: 0,
    };
    // SAFETY: `info` is a valid, correctly sized SHQUERYRBINFO; a null root asks for all drives.
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.ok()?;
    let (size, items) = (info.i64Size, info.i64NumItems);
    Some(RecycleBinSize {
        bytes: size.max(0) as u64,
        items: items.max(0) as u64,
    })
}

/// The user's Downloads folder (it can live off the profile).
pub fn downloads_dir() -> Option<PathBuf> {
    // SAFETY: the returned PWSTR is copied, then freed with CoTaskMemFree as documented.
    unsafe {
        let p =
            SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, HANDLE::default()).ok()?;
        let path = PathBuf::from(OsString::from_wide(p.as_wide()));
        CoTaskMemFree(Some(p.0 as *const c_void));
        if path.as_os_str().is_empty() {
            None
        } else {
            Some(path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_list_is_double_nul_terminated() {
        let packed = pack_paths(&[PathBuf::from(r"C:\a\b.tmp"), PathBuf::from(r"C:\c")]).unwrap();
        let text: Vec<u16> = r"C:\a\b.tmp".encode_utf16().collect();
        assert_eq!(&packed[..text.len()], &text[..]);
        assert_eq!(packed[text.len()], 0);
        let tail: Vec<u16> = r"C:\c".encode_utf16().collect();
        let second = text.len() + 1;
        assert_eq!(&packed[second..second + tail.len()], &tail[..]);
        assert_eq!(&packed[packed.len() - 2..], &[0, 0], "ends with two NULs");
        assert_eq!(packed.iter().filter(|&&c| c == 0).count(), 3);
    }

    #[test]
    fn wildcards_empty_lists_and_empty_names_are_refused() {
        assert!(pack_paths(&[PathBuf::from(r"C:\dir\*.tmp")]).is_err());
        assert!(pack_paths(&[PathBuf::from(r"C:\dir\a?.tmp")]).is_err());
        assert!(pack_paths(&[]).is_err());
        assert!(pack_paths(&[PathBuf::new()]).is_err());
    }

    #[test]
    fn the_volume_guid_is_picked_out_of_the_volume_name() {
        assert_eq!(
            volume_guid(r"\\?\Volume{a1b2c3d4-0000-1111-2222-333344445555}\"),
            Some("{a1b2c3d4-0000-1111-2222-333344445555}")
        );
        assert_eq!(volume_guid(r"\\server\share\"), None);
        assert_eq!(volume_guid("}{"), None);
    }

    /// Read-only: what Windows says about the Recycle Bin of this machine's temp drive.
    #[test]
    #[ignore = "reads Recycle Bin settings from the real registry (read-only)"]
    fn recycle_bin_facts_for_the_temp_drive() {
        let temp = std::env::temp_dir();
        let root = volume_root(&temp).expect("volume root");
        let facts = bin_facts_for_root(&root);
        eprintln!("{root}: {facts:?}");
        assert!(facts.fixed_drive && facts.volume_known);
        assert!(ShellRecycler::new().will_recycle(&temp, 1024).is_ok() == !facts.turned_off);
    }

    /// Read-only: asks Windows for the Recycle Bin's size. Run with `--ignored`.
    #[test]
    #[ignore = "queries the real Recycle Bin (read-only)"]
    fn recycle_bin_size_can_be_read() {
        let s = recycle_bin_size().expect("SHQueryRecycleBinW answered");
        eprintln!("recycle bin: {} bytes in {} items", s.bytes, s.items);
    }

    /// Read-only: resolves the Downloads known folder. Run with `--ignored`.
    #[test]
    #[ignore = "queries the real known-folder table (read-only)"]
    fn downloads_folder_resolves() {
        let d = downloads_dir().expect("Downloads known folder");
        assert!(d.is_absolute(), "{d:?}");
    }

    /// Puts ONE file this test just created into the REAL Recycle Bin. Never run by default.
    #[test]
    #[ignore = "moves a scratch file to the real Recycle Bin"]
    fn really_moves_a_scratch_file_to_the_recycle_bin() {
        let dir = std::env::temp_dir().join(format!("krovi-clean-ignored-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("krovi-clean-scratch.tmp");
        std::fs::write(&f, b"scratch").unwrap();
        let canon = crate::fsx::canonical(&f).unwrap();
        ShellRecycler::new().recycle(&[canon]).expect("recycled");
        assert!(!f.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
