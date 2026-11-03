//! The only folders the organizer and the file search ever touch: the user's known folders.
//!
//! [`KnownFolders`] holds each folder's canonical path (as Windows reports it, resolved once), so
//! every later check is a component-wise comparison against a fixed list. Nothing outside these six
//! folders is ever listed, searched or written to.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fsx::{canonical, inside_strict, normal_depth};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KnownFolder {
    Downloads,
    Documents,
    Pictures,
    Desktop,
    Videos,
    Music,
}

impl KnownFolder {
    pub const ALL: [KnownFolder; 6] = [
        KnownFolder::Downloads,
        KnownFolder::Documents,
        KnownFolder::Pictures,
        KnownFolder::Desktop,
        KnownFolder::Videos,
        KnownFolder::Music,
    ];

    pub fn id(self) -> &'static str {
        match self {
            KnownFolder::Downloads => "downloads",
            KnownFolder::Documents => "documents",
            KnownFolder::Pictures => "pictures",
            KnownFolder::Desktop => "desktop",
            KnownFolder::Videos => "videos",
            KnownFolder::Music => "music",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            KnownFolder::Downloads => "Downloads",
            KnownFolder::Documents => "Documents",
            KnownFolder::Pictures => "Pictures",
            KnownFolder::Desktop => "Desktop",
            KnownFolder::Videos => "Videos",
            KnownFolder::Music => "Music",
        }
    }

    pub fn parse(id: &str) -> Option<KnownFolder> {
        let id = id.trim().to_ascii_lowercase();
        KnownFolder::ALL.into_iter().find(|k| k.id() == id)
    }
}

/// The known folders that exist on this PC, canonical.
#[derive(Debug, Clone, Default)]
pub struct KnownFolders {
    entries: Vec<(KnownFolder, PathBuf)>,
}

impl KnownFolders {
    /// Build from explicit paths (tests use scratch folders). A path that does not exist, is not a
    /// folder, or is a drive root is dropped.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (KnownFolder, PathBuf)>) -> KnownFolders {
        let mut entries: Vec<(KnownFolder, PathBuf)> = Vec::new();
        for (kind, path) in pairs {
            if entries.iter().any(|(k, _)| *k == kind) {
                continue;
            }
            let Ok(canon) = canonical(&path) else {
                continue;
            };
            if !canon.is_dir() || normal_depth(&canon) < 1 {
                continue;
            }
            entries.push((kind, canon));
        }
        KnownFolders { entries }
    }

    /// The real folders from the Windows shell. Empty without the `native` feature (or off
    /// Windows): the features that need folders are then inert, never guessing a path.
    pub fn from_os() -> KnownFolders {
        #[cfg(all(windows, feature = "native"))]
        {
            KnownFolders::from_pairs(
                KnownFolder::ALL
                    .into_iter()
                    .filter_map(|k| crate::native::known_folder(k).map(|p| (k, p))),
            )
        }
        #[cfg(not(all(windows, feature = "native")))]
        {
            KnownFolders::default()
        }
    }

    pub fn get(&self, kind: KnownFolder) -> Option<&Path> {
        self.entries
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, p)| p.as_path())
    }

    pub fn all(&self) -> &[(KnownFolder, PathBuf)] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The known folder `path` lies strictly inside (the deepest one if they nest).
    pub fn containing(&self, path: &Path) -> Option<KnownFolder> {
        self.entries
            .iter()
            .filter(|(_, root)| inside_strict(path, root))
            .max_by_key(|(_, root)| normal_depth(root))
            .map(|(k, _)| *k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn ids_round_trip_and_unknown_ids_are_refused() {
        for k in KnownFolder::ALL {
            assert_eq!(KnownFolder::parse(k.id()), Some(k));
        }
        assert_eq!(
            KnownFolder::parse(" Downloads "),
            Some(KnownFolder::Downloads)
        );
        assert_eq!(KnownFolder::parse("windows"), None);
        assert_eq!(KnownFolder::parse(""), None);
        assert_eq!(KnownFolder::parse("..\\downloads"), None);
    }

    #[test]
    fn only_existing_folders_are_kept_and_paths_are_canonical() {
        let t = TempDir::new("known");
        let docs = t.mkdir("docs");
        let f = t.write("file.txt", 1);
        let k = KnownFolders::from_pairs([
            (KnownFolder::Documents, docs.clone()),
            (KnownFolder::Music, t.path().join("missing")),
            (KnownFolder::Videos, f),
            (KnownFolder::Documents, t.mkdir("other")), // a second entry for the same kind
        ]);
        assert_eq!(k.all().len(), 1);
        assert!(k.get(KnownFolder::Documents).is_some());
        assert!(k.get(KnownFolder::Music).is_none());
        assert!(k.get(KnownFolder::Videos).is_none());
        let p = k
            .get(KnownFolder::Documents)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(!p.starts_with(r"\\?\"), "{p}");
    }

    #[test]
    fn containing_is_strict_and_prefers_the_deepest_folder() {
        let t = TempDir::new("known-nest");
        let docs = t.mkdir("docs");
        let pics = t.mkdir("docs/pics");
        let k = KnownFolders::from_pairs([
            (KnownFolder::Documents, docs.clone()),
            (KnownFolder::Pictures, pics.clone()),
        ]);
        let k_docs = k.get(KnownFolder::Documents).unwrap().to_path_buf();
        let k_pics = k.get(KnownFolder::Pictures).unwrap().to_path_buf();
        assert_eq!(
            k.containing(&k_pics.join("a.jpg")),
            Some(KnownFolder::Pictures)
        );
        assert_eq!(
            k.containing(&k_docs.join("a.txt")),
            Some(KnownFolder::Documents)
        );
        assert_eq!(
            k.containing(&k_docs),
            None,
            "the folder itself is not inside itself"
        );
        assert_eq!(k.containing(t.path()), None);
    }
}
