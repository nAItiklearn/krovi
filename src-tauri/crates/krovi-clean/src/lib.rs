//! Krovi Clean My OS: scan -> review the real paths and sizes -> confirm -> Recycle Bin.
//!
//! The flow, and where each guarantee lives:
//!
//! 1. [`categories`] resolves the fixed list of junk locations (user temp, Windows temp, browser
//!    caches, thumbnail cache, crash reports, old installers, the Recycle Bin) for this PC.
//!    Nothing outside that list is ever scanned.
//! 2. [`scan::scan`] only reads. It never follows links (symlinks, junctions, any reparse point),
//!    caps depth and list length, honours a cancel flag and reports progress. Each item it lists
//!    is a file, or a folder whose whole contents it checked.
//! 3. [`delete::run_delete`] accepts only ids from that stored scan, re-validates every path
//!    right before moving it (see the module docs) and hands the shell the resolved path. The
//!    platform call is the [`delete::Recycler`] trait; the real one is `native::ShellRecycler`
//!    (`SHFileOperationW` with `FOF_ALLOWUNDO`), so everything is a move to the Recycle Bin that
//!    the user can undo. The Recycle Bin itself is only measured, never emptied.
//!
//! Disk space is not freed by a clean: it is moved to the Recycle Bin, and comes back when the
//! user empties it. The numbers in [`DeleteReport`] say "freed" for the bytes moved.
//!
//! No network, no logging of paths, no elevation.

pub mod categories;
pub mod delete;
pub mod fsx;
pub mod model;
#[cfg(all(windows, feature = "native"))]
pub mod native;
pub mod scan;
#[cfg(test)]
pub(crate) mod testutil;

pub use categories::{
    categories, category_infos, select, CategorySpec, Env, Kind, NameFilter, Root, Shape,
};
pub use delete::{
    recycle_verdict, run_delete, shell_error_text, BinFacts, DeleteOptions, RecycleError, Recycler,
    BATCH_SIZE, MAX_RECYCLE_ITEM_BYTES, STALE_SCAN,
};
pub use model::{
    CategoryInfo, CategoryResult, DeleteReport, DeleteRequest, Failure, RecycleBinSize, Scan,
    ScanItem,
};
pub use scan::{scan, Progress, ScanOptions};

#[cfg(test)]
mod live {
    use super::*;
    use std::sync::atomic::AtomicBool;

    /// Scans THIS machine's real folders and prints only counts. Read-only: nothing is deleted
    /// or even opened for writing. Run with `--ignored --nocapture`.
    #[test]
    #[ignore = "scans the real machine (read-only); prints counts only"]
    fn scan_the_real_machine_read_only() {
        let env = Env::from_os();
        let specs = categories(&env);
        let cancel = AtomicBool::new(false);
        let started = std::time::Instant::now();
        let s = scan(
            &specs,
            1,
            &ScanOptions::default(),
            &cancel,
            &|| system::recycle_bin_size(),
            &mut |_| {},
        );
        for c in &s.categories {
            eprintln!(
                "{:<14} roots={} items={} (+{} unlisted) files={} bytes={} recent={} links={} deep={} unreadable={}",
                c.id, c.roots_found, c.items.len(), c.unlisted_items, c.files, c.bytes,
                c.skipped_recent, c.skipped_links, c.skipped_deep, c.unreadable
            );
        }
        eprintln!("scan took {:?}", started.elapsed());
        assert!(!s.cancelled);
    }
}

/// What the running system provides. Without the `native` feature (or off Windows) these are
/// inert: no Recycle Bin size, no recycler, so nothing can be deleted.
pub mod system {
    use std::path::PathBuf;

    use crate::delete::Recycler;
    use crate::model::RecycleBinSize;

    /// The user's Downloads folder from the shell's known-folder table.
    pub fn downloads_dir() -> Option<PathBuf> {
        #[cfg(all(windows, feature = "native"))]
        {
            crate::native::downloads_dir()
        }
        #[cfg(not(all(windows, feature = "native")))]
        {
            None
        }
    }

    /// Size and item count of the Recycle Bin, if Windows reports it.
    pub fn recycle_bin_size() -> Option<RecycleBinSize> {
        #[cfg(all(windows, feature = "native"))]
        {
            crate::native::recycle_bin_size()
        }
        #[cfg(not(all(windows, feature = "native")))]
        {
            None
        }
    }

    /// The real "move to the Recycle Bin", or `None` when this build cannot do it.
    pub fn recycler() -> Option<Box<dyn Recycler + Send + Sync>> {
        #[cfg(all(windows, feature = "native"))]
        {
            Some(Box::new(crate::native::ShellRecycler::new()))
        }
        #[cfg(not(all(windows, feature = "native")))]
        {
            None
        }
    }
}
