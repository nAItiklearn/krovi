//! Windows Defender Firewall helpers for Quick Transfer.
//!
//! Krovi never touches the firewall on its own. The first time it listens, Windows shows its
//! own "Allow access" prompt; if that was dismissed (or the network is Public) the phone cannot
//! connect. The UI then explains this, and offers ONE explicit button, "Allow Krovi in the
//! firewall", which runs the commands built here behind a Windows UAC prompt:
//!
//!   1. delete the existing inbound rules for this exact exe (the stale allow/block rules
//!      Windows creates from its prompt would otherwise override ours);
//!   2. add one inbound rule: this exe only, TCP, Private profile only, remote addresses
//!      limited to the local subnet. Name: "Krovi Quick Transfer".
//!
//! The builders below are pure and unit-tested; only `run_*` touch the system.

use std::path::Path;

pub const RULE_NAME: &str = "Krovi Quick Transfer";

/// Quote for a PowerShell single-quoted string.
pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// `netsh` arguments that delete inbound rules for this exe.
pub fn netsh_delete_args(exe: &Path) -> Vec<String> {
    vec![
        "advfirewall".into(),
        "firewall".into(),
        "delete".into(),
        "rule".into(),
        "name=all".into(),
        "dir=in".into(),
        format!("program={}", exe.display()),
    ]
}

/// `netsh` arguments that add the one Quick Transfer rule.
pub fn netsh_add_args(exe: &Path) -> Vec<String> {
    vec![
        "advfirewall".into(),
        "firewall".into(),
        "add".into(),
        "rule".into(),
        format!("name={RULE_NAME}"),
        "dir=in".into(),
        "action=allow".into(),
        format!("program={}", exe.display()),
        "enable=yes".into(),
        "profile=private".into(),
        "protocol=TCP".into(),
        "remoteip=localsubnet".into(),
    ]
}

fn ps_array(args: &[String]) -> String {
    let items: Vec<String> = args.iter().map(|a| ps_quote(a)).collect();
    format!("@({})", items.join(","))
}

/// The script that runs elevated. Its exit code is `netsh add`'s.
pub fn elevated_script(exe: &Path) -> String {
    format!(
        "$ErrorActionPreference='Continue'\n\
$d={}\n$a={}\n\
& netsh.exe $d | Out-Null\n\
& netsh.exe $a\n\
exit $LASTEXITCODE\n",
        ps_array(&netsh_delete_args(exe)),
        ps_array(&netsh_add_args(exe))
    )
}

/// Standard base64 of UTF-16LE: what `powershell -EncodedCommand` expects.
pub fn encoded_command(script: &str) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = script
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            A[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            A[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The outer (non-elevated) script: relaunch PowerShell elevated and wait for it.
/// Declining the UAC prompt exits with 1223 (ERROR_CANCELLED).
pub fn launcher_script(exe: &Path) -> String {
    let inner = encoded_command(&elevated_script(exe));
    format!(
        "try {{ $p = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru -WindowStyle Hidden \
-ArgumentList @('-NoProfile','-NonInteractive','-EncodedCommand','{inner}'); exit $p.ExitCode }} catch {{ exit 1223 }}"
    )
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallStatus {
    /// Our "Krovi Quick Transfer" rule exists and points at the running exe.
    pub rule_installed: bool,
    /// "Private" | "Public" | "DomainAuthenticated" for the active adapter, when known.
    pub profile: Option<String>,
}

/// Parse `netsh advfirewall firewall show rule name="Krovi Quick Transfer" verbose` output.
pub fn rule_points_at(netsh_output: &str, exe: &Path) -> bool {
    let want = exe.to_string_lossy().to_ascii_lowercase();
    netsh_output.lines().any(|l| {
        let l = l.trim();
        l.len() > 8
            && l[..8].eq_ignore_ascii_case("program:")
            && l[8..].trim().to_ascii_lowercase() == want
    })
}

#[cfg(windows)]
mod run {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn quiet(program: &str) -> Command {
        let mut c = Command::new(program);
        c.creation_flags(CREATE_NO_WINDOW);
        c
    }

    /// Ask Windows (UAC prompt) to install the rule. Only ever called from an explicit click.
    pub fn allow(exe: &Path) -> Result<(), String> {
        let out = quiet("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-EncodedCommand",
            ])
            .arg(encoded_command(&launcher_script(exe)))
            .output()
            .map_err(|e| format!("Could not start PowerShell: {e}"))?;
        match out.status.code() {
            Some(0) => Ok(()),
            Some(1223) => {
                Err("Cancelled: Windows did not get permission to change the firewall.".into())
            }
            c => Err(format!(
                "The firewall rule could not be added (exit code {c:?})."
            )),
        }
    }

    pub fn status(exe: &Path, adapter: Option<&str>) -> FirewallStatus {
        let rule_installed = quiet("netsh.exe")
            .args([
                "advfirewall",
                "firewall",
                "show",
                "rule",
                &format!("name={RULE_NAME}"),
                "verbose",
            ])
            .output()
            .map(|o| o.status.success() && rule_points_at(&String::from_utf8_lossy(&o.stdout), exe))
            .unwrap_or(false);
        let profile = adapter.and_then(|name| {
            let script = format!(
                "(Get-NetConnectionProfile -InterfaceAlias {} -ErrorAction SilentlyContinue | Select-Object -First 1).NetworkCategory",
                ps_quote(name)
            );
            let o = quiet("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .output()
                .ok()?;
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            (!s.is_empty() && s.len() < 40).then_some(s)
        });
        FirewallStatus {
            rule_installed,
            profile,
        }
    }
}

#[cfg(windows)]
pub use run::{allow as run_allow, status as run_status};

#[cfg(not(windows))]
pub fn run_allow(_exe: &Path) -> Result<(), String> {
    Err("Only available on Windows.".into())
}

#[cfg(not(windows))]
pub fn run_status(_exe: &Path, _adapter: Option<&str>) -> FirewallStatus {
    FirewallStatus {
        rule_installed: false,
        profile: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Program Files\Krovi\krovi-app.exe")
    }

    #[test]
    fn rule_is_scoped_to_exe_private_profile_tcp_and_local_subnet() {
        let a = netsh_add_args(&exe());
        assert!(a.contains(&"action=allow".to_string()));
        assert!(a.contains(&"dir=in".to_string()));
        assert!(a.contains(&"profile=private".to_string()));
        assert!(a.contains(&"protocol=TCP".to_string()));
        assert!(a.contains(&"remoteip=localsubnet".to_string()));
        assert!(a.contains(&r"program=C:\Program Files\Krovi\krovi-app.exe".to_string()));
        assert!(a.contains(&"name=Krovi Quick Transfer".to_string()));
        // never a blanket rule
        assert!(!a.iter().any(|x| x.contains("profile=any")
            || x.contains("profile=public")
            || x.contains("localport")));
    }

    #[test]
    fn delete_only_touches_inbound_rules_of_this_exe() {
        let d = netsh_delete_args(&exe());
        assert!(d.contains(&"dir=in".to_string()));
        assert!(d
            .iter()
            .any(|x| x.starts_with("program=") && x.ends_with("krovi-app.exe")));
        assert!(!d.iter().any(|x| x == "name=Krovi Quick Transfer"));
    }

    #[test]
    fn ps_quote_escapes_single_quotes() {
        assert_eq!(ps_quote("O'Brien"), "'O''Brien'");
        let s = elevated_script(Path::new(r"C:\Users\O'Brien\krovi.exe"));
        assert!(s.contains(r"program=C:\Users\O''Brien\krovi.exe"));
    }

    #[test]
    fn encoded_command_known_vector() {
        // "A" in UTF-16LE is 41 00 -> base64 "QQA="
        assert_eq!(encoded_command("A"), "QQA=");
        assert_eq!(encoded_command("Hi"), "SABpAA==");
    }

    #[test]
    fn launcher_requests_elevation_and_handles_cancel() {
        let s = launcher_script(&exe());
        assert!(s.contains("-Verb RunAs"));
        assert!(s.contains("1223"));
        assert!(s.contains("-EncodedCommand"));
    }

    #[test]
    fn parses_netsh_show_rule_output() {
        let out = "Rule Name:                            Krovi Quick Transfer\r\n\
Enabled:                              Yes\r\nProgram:                              C:\\Program Files\\Krovi\\krovi-app.exe\r\n";
        assert!(rule_points_at(out, &exe()));
        assert!(!rule_points_at(out, Path::new(r"C:\other\krovi-app.exe")));
        assert!(!rule_points_at(
            "No rules match the specified criteria.",
            &exe()
        ));
    }
}
