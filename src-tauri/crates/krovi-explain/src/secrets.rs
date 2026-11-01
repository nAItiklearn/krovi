//! API keys live in the Windows Credential Manager (generic credential, per-user, local
//! machine persistence). Never in a file, never in SQLite, never in localStorage, never
//! returned to the webview: the UI only ever learns "a key is saved" and its last four
//! characters.

/// Credential Manager target name for the user's ElevenLabs key.
pub const ELEVEN_TARGET: &str = "Krovi/Explain/ElevenLabs";

/// Last four characters of a key, for "ends in ...abcd" - only when the key is long enough that
/// four characters reveal nothing useful.
pub fn key_hint(key: &str) -> Option<String> {
    let chars: Vec<char> = key.trim().chars().collect();
    if chars.len() < 20 {
        return None;
    }
    Some(chars[chars.len() - 4..].iter().collect())
}

#[cfg(all(windows, feature = "native"))]
mod imp {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{ERROR_NOT_FOUND, FILETIME};
    use windows::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_FLAGS,
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn store(target: &str, secret: &str) -> Result<(), String> {
        let mut target_w = wide(target);
        let mut user_w = wide("Krovi");
        let bytes = secret.as_bytes();
        // Credential Manager limit (CRED_MAX_CREDENTIAL_BLOB_SIZE).
        if bytes.is_empty() || bytes.len() > 2560 {
            return Err("That key has an unexpected length.".into());
        }
        let cred = CREDENTIALW {
            Flags: CRED_FLAGS(0),
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target_w.as_mut_ptr()),
            Comment: PWSTR::null(),
            LastWritten: FILETIME::default(),
            CredentialBlobSize: bytes.len() as u32,
            CredentialBlob: bytes.as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: PWSTR::null(),
            UserName: PWSTR(user_w.as_mut_ptr()),
        };
        // SAFETY: `cred` points at buffers that outlive the call; Windows copies them.
        unsafe { CredWriteW(&cred, 0) }
            .map_err(|e| format!("Windows Credential Manager refused to save the key ({e})"))
    }

    pub fn read(target: &str) -> Result<Option<String>, String> {
        let target_w = wide(target);
        let mut out: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: `out` receives a Windows-allocated CREDENTIALW that we free with CredFree.
        let res = unsafe { CredReadW(PCWSTR(target_w.as_ptr()), CRED_TYPE_GENERIC, 0, &mut out) };
        match res {
            Ok(()) => {
                // SAFETY: CredReadW succeeded, so `out` is valid until CredFree.
                let text = unsafe {
                    let c = &*out;
                    let slice =
                        std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize);
                    let s = String::from_utf8(slice.to_vec());
                    CredFree(out as *const _);
                    s
                };
                text.map(Some)
                    .map_err(|_| "The saved key could not be read.".to_string())
            }
            Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => Ok(None),
            Err(e) => Err(format!(
                "Windows Credential Manager could not be read ({e})"
            )),
        }
    }

    pub fn delete(target: &str) -> Result<bool, String> {
        let target_w = wide(target);
        // SAFETY: plain call with a NUL-terminated wide string.
        match unsafe { CredDeleteW(PCWSTR(target_w.as_ptr()), CRED_TYPE_GENERIC, 0) } {
            Ok(()) => Ok(true),
            Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => Ok(false),
            Err(e) => Err(format!(
                "Windows Credential Manager could not remove the key ({e})"
            )),
        }
    }
}

#[cfg(not(all(windows, feature = "native")))]
mod imp {
    pub fn store(_: &str, _: &str) -> Result<(), String> {
        Err("Saving a key needs Windows Credential Manager.".into())
    }
    pub fn read(_: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    pub fn delete(_: &str) -> Result<bool, String> {
        Ok(false)
    }
}

/// Save (or replace) a secret under `target`.
pub fn store(target: &str, secret: &str) -> Result<(), String> {
    imp::store(target, secret)
}
/// The secret under `target`, or `None` when nothing is saved.
pub fn read(target: &str) -> Result<Option<String>, String> {
    imp::read(target)
}
/// Remove the secret; `true` when something was removed.
pub fn delete(target: &str) -> Result<bool, String> {
    imp::delete(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_shows_only_the_last_four_of_a_long_key() {
        assert_eq!(
            key_hint("sk_0123456789abcdef0123456789"),
            Some("6789".into())
        );
        assert_eq!(key_hint("short"), None);
        assert_eq!(key_hint("  "), None);
    }

    #[cfg(all(windows, feature = "native"))]
    #[test]
    fn credential_manager_round_trip() {
        let target = format!("Krovi/Explain/test-{}", std::process::id());
        assert_eq!(read(&target).unwrap(), None);
        assert!(!delete(&target).unwrap());
        store(&target, "sk_test_0123456789abcdef").unwrap();
        assert_eq!(
            read(&target).unwrap().as_deref(),
            Some("sk_test_0123456789abcdef")
        );
        // replace
        store(&target, "sk_test_replaced_0123456").unwrap();
        assert_eq!(
            read(&target).unwrap().as_deref(),
            Some("sk_test_replaced_0123456")
        );
        assert!(delete(&target).unwrap());
        assert_eq!(read(&target).unwrap(), None);
    }
}
