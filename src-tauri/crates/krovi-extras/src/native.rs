//! The Windows-only piece, behind the `native` feature: the user's known folders from the shell
//! (they can be moved off the profile, or into OneDrive).

use std::ffi::{c_void, OsString};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures,
    FOLDERID_Videos, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
};

use crate::known::KnownFolder;

/// The folder `kind` as the shell reports it, or `None` if Windows will not say.
pub fn known_folder(kind: KnownFolder) -> Option<PathBuf> {
    // SAFETY: the returned PWSTR is copied, then freed with CoTaskMemFree as documented.
    unsafe {
        let p = match kind {
            KnownFolder::Downloads => {
                SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, HANDLE::default())
            }
            KnownFolder::Documents => {
                SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, HANDLE::default())
            }
            KnownFolder::Pictures => {
                SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, HANDLE::default())
            }
            KnownFolder::Desktop => {
                SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, HANDLE::default())
            }
            KnownFolder::Videos => {
                SHGetKnownFolderPath(&FOLDERID_Videos, KF_FLAG_DEFAULT, HANDLE::default())
            }
            KnownFolder::Music => {
                SHGetKnownFolderPath(&FOLDERID_Music, KF_FLAG_DEFAULT, HANDLE::default())
            }
        }
        .ok()?;
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

    /// Read-only: asks the shell where the six known folders are. Run with `--ignored`.
    #[test]
    #[ignore = "queries the real known-folder table (read-only)"]
    fn all_six_known_folders_resolve() {
        for k in KnownFolder::ALL {
            let p = known_folder(k).unwrap_or_else(|| panic!("{} did not resolve", k.label()));
            assert!(p.is_absolute(), "{p:?}");
        }
    }
}
