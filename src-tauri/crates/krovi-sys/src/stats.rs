//! Live system stats for the pill/Home cards: RAM, system-drive space, CPU %.
//!
//! Memory comes from `GlobalMemoryStatusEx`, disk from `GetDiskFreeSpaceExW` on the
//! `%SystemDrive%` root, and CPU % from deltas of `GetSystemTimes` between two
//! samples. The CPU math, drive-letter parsing and the sampler state machine are
//! pure and unit-tested; the three OS calls live in the `native` submodule.

use serde::Serialize;

/// Wire shape of the `stats-tick` event (camelCase, bytes).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStats {
    pub mem_used: u64,
    pub mem_total: u64,
    pub disk_used: u64,
    pub disk_total: u64,
    /// The system drive, e.g. `C:`.
    pub disk_label: String,
    /// 0..=100, best effort (0 on the very first sample, which has no delta).
    pub cpu: f64,
}

/// Cumulative CPU times from `GetSystemTimes`, in 100 ns ticks. Note `kernel`
/// *includes* `idle` (that is how Windows reports it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTimes {
    pub idle: u64,
    pub kernel: u64,
    pub user: u64,
}

/// Busy percentage between two cumulative samples, `None` if no time elapsed
/// (or the counters went backwards).
pub fn cpu_percent(prev: CpuTimes, cur: CpuTimes) -> Option<f64> {
    let total =
        (cur.kernel.checked_sub(prev.kernel)?).checked_add(cur.user.checked_sub(prev.user)?)?;
    let idle = cur.idle.checked_sub(prev.idle)?;
    if total == 0 {
        return None;
    }
    let busy = total.saturating_sub(idle);
    let pct = busy as f64 * 100.0 / total as f64;
    Some((pct.clamp(0.0, 100.0) * 10.0).round() / 10.0)
}

/// `"C:"`-style label from the `SystemDrive` environment value; falls back to `C:`
/// for anything that is not a single drive letter plus colon.
pub fn drive_label(env_value: Option<&str>) -> String {
    match env_value.map(str::trim) {
        Some(v)
            if v.len() == 2 && v.as_bytes()[1] == b':' && v.as_bytes()[0].is_ascii_alphabetic() =>
        {
            v.to_ascii_uppercase()
        }
        _ => "C:".to_string(),
    }
}

/// Stateful sampler: remembers the previous CPU counters so each call yields a
/// real usage percentage over the interval since the last call.
#[derive(Debug, Default)]
pub struct StatsSampler {
    prev_cpu: Option<CpuTimes>,
    last_cpu_pct: f64,
}

impl StatsSampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold a CPU counter reading into the sampler and return the % to report:
    /// the interval's usage, or the previous value when no time elapsed.
    pub fn fold_cpu(&mut self, cur: CpuTimes) -> f64 {
        if let Some(prev) = self.prev_cpu {
            if let Some(p) = cpu_percent(prev, cur) {
                self.last_cpu_pct = p;
            }
        }
        self.prev_cpu = Some(cur);
        self.last_cpu_pct
    }

    /// Take a full sample (memory + system drive + CPU since the previous call).
    #[cfg(all(windows, feature = "native"))]
    pub fn sample(&mut self) -> crate::Result<SystemStats> {
        let (mem_used, mem_total) = native::memory()?;
        let label = drive_label(std::env::var("SystemDrive").ok().as_deref());
        let (disk_used, disk_total) = native::disk(&label)?;
        let cpu = match native::cpu_times() {
            Ok(t) => self.fold_cpu(t),
            Err(_) => self.last_cpu_pct,
        };
        Ok(SystemStats {
            mem_used,
            mem_total,
            disk_used,
            disk_total,
            disk_label: label,
            cpu,
        })
    }

    /// See the native variant. Without `native` there is nothing to measure.
    #[cfg(not(all(windows, feature = "native")))]
    pub fn sample(&mut self) -> crate::Result<SystemStats> {
        Err(crate::SysError::Unsupported)
    }
}

#[cfg(all(windows, feature = "native"))]
mod native {
    use super::CpuTimes;
    use crate::SysError;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows::Win32::System::Threading::GetSystemTimes;

    fn err<E: std::fmt::Display>(e: E) -> SysError {
        SysError::Message(e.to_string())
    }

    /// `(used, total)` physical memory in bytes.
    pub(super) fn memory() -> crate::Result<(u64, u64)> {
        let mut m = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        unsafe { GlobalMemoryStatusEx(&mut m) }.map_err(err)?;
        Ok((
            m.ullTotalPhys.saturating_sub(m.ullAvailPhys),
            m.ullTotalPhys,
        ))
    }

    /// `(used, total)` bytes of the volume at `label` (e.g. `C:`), measured the
    /// way Explorer / `Get-PSDrive` do: total minus total-free.
    pub(super) fn disk(label: &str) -> crate::Result<(u64, u64)> {
        let root: Vec<u16> = format!("{label}\\")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let (mut _free_to_caller, mut total, mut total_free) = (0u64, 0u64, 0u64);
        unsafe {
            GetDiskFreeSpaceExW(
                PCWSTR(root.as_ptr()),
                Some(&mut _free_to_caller),
                Some(&mut total),
                Some(&mut total_free),
            )
        }
        .map_err(err)?;
        Ok((total.saturating_sub(total_free), total))
    }

    fn ticks(ft: FILETIME) -> u64 {
        (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime)
    }

    pub(super) fn cpu_times() -> crate::Result<CpuTimes> {
        let (mut idle, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }
            .map_err(err)?;
        Ok(CpuTimes {
            idle: ticks(idle),
            kernel: ticks(kernel),
            user: ticks(user),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(idle: u64, kernel: u64, user: u64) -> CpuTimes {
        CpuTimes { idle, kernel, user }
    }

    #[test]
    fn cpu_percent_counts_non_idle_share_of_kernel_plus_user() {
        // delta: kernel 1000 (of which idle 800), user 200 -> total 1200, busy 400.
        let p = cpu_percent(t(0, 0, 0), t(800, 1000, 200)).unwrap();
        assert!((p - 33.3).abs() < 0.05, "{p}");
        // fully idle
        assert_eq!(cpu_percent(t(0, 0, 0), t(1000, 1000, 0)), Some(0.0));
        // fully busy
        assert_eq!(cpu_percent(t(0, 0, 0), t(0, 500, 500)), Some(100.0));
    }

    #[test]
    fn cpu_percent_rejects_no_elapsed_time_and_counter_regressions() {
        assert_eq!(cpu_percent(t(5, 10, 10), t(5, 10, 10)), None);
        assert_eq!(cpu_percent(t(5, 10, 10), t(4, 9, 9)), None);
    }

    #[test]
    fn cpu_percent_is_clamped_when_idle_exceeds_total() {
        // idle ran ahead of kernel (sampling skew): never negative / over 100.
        let p = cpu_percent(t(0, 0, 0), t(2000, 1000, 0)).unwrap();
        assert_eq!(p, 0.0);
    }

    #[test]
    fn drive_label_parsing() {
        assert_eq!(drive_label(Some("c:")), "C:");
        assert_eq!(drive_label(Some(" D: ")), "D:");
        assert_eq!(drive_label(Some("C:\\")), "C:");
        assert_eq!(drive_label(Some("")), "C:");
        assert_eq!(drive_label(Some("1:")), "C:");
        assert_eq!(drive_label(None), "C:");
    }

    #[test]
    fn sampler_reports_zero_first_then_interval_usage() {
        let mut s = StatsSampler::new();
        assert_eq!(s.fold_cpu(t(100, 200, 100)), 0.0); // no previous sample yet
                                                       // +kernel 1000 (idle 500) +user 0 -> 50 %
        assert_eq!(s.fold_cpu(t(600, 1200, 100)), 50.0);
        // identical counters: keep the last value instead of dropping to 0
        assert_eq!(s.fold_cpu(t(600, 1200, 100)), 50.0);
    }

    #[test]
    fn stats_serialize_camel_case() {
        let s = SystemStats {
            mem_used: 1,
            mem_total: 2,
            disk_used: 3,
            disk_total: 4,
            disk_label: "C:".into(),
            cpu: 12.5,
        };
        let v = serde_json::to_value(&s).unwrap();
        for k in [
            "memUsed",
            "memTotal",
            "diskUsed",
            "diskTotal",
            "diskLabel",
            "cpu",
        ] {
            assert!(v.get(k).is_some(), "missing {k}");
        }
        assert_eq!(v["diskLabel"], "C:");
    }

    #[cfg(not(all(windows, feature = "native")))]
    #[test]
    fn sample_is_unsupported_without_native() {
        assert!(matches!(
            StatsSampler::new().sample(),
            Err(crate::SysError::Unsupported)
        ));
    }
}
