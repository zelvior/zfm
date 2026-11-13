//! UI-visible archive formats. Availability is ultimately checked by the engine.
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format { SevenZip, Zip, Tar, TarGzip, TarBzip2, TarXz, Gzip, Bzip2, Xz }
impl Format {
    pub const ALL: [Self; 9] = [Self::SevenZip, Self::Zip, Self::Tar, Self::TarGzip,
        Self::TarBzip2, Self::TarXz, Self::Gzip, Self::Bzip2, Self::Xz];
    pub fn suffix(self) -> &'static str { match self {
        Self::SevenZip => ".7z", Self::Zip => ".zip", Self::Tar => ".tar",
        Self::TarGzip => ".tar.gz", Self::TarBzip2 => ".tar.bz2", Self::TarXz => ".tar.xz",
        Self::Gzip => ".gz", Self::Bzip2 => ".bz2", Self::Xz => ".xz",
    } }
    pub fn single_file(self) -> bool { matches!(self, Self::Gzip | Self::Bzip2 | Self::Xz) }
    pub fn encrypted(self) -> bool { matches!(self, Self::SevenZip | Self::Zip) }
    pub fn matches(self, path: &Path) -> bool {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
        name.ends_with(self.suffix()) && (!self.single_file() || ![".tar.gz", ".tar.bz2", ".tar.xz"].iter().any(|s| name.ends_with(s)))
    }
}

/// File extensions for the direct extract actions. The engine may reject a
/// particular archive, encryption method, entry type, or malformed input.
pub fn can_extract(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).map_or(false, |ext|
        ["7z", "zip", "tar", "gz", "bz2", "xz", "tgz", "tbz", "tbz2", "txz", "rar", "iso", "cab", "arj", "lzh", "wim"]
            .iter().any(|candidate| ext.eq_ignore_ascii_case(candidate)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_are_explicit_and_streams_do_not_match_tar_containers() {
        for format in Format::ALL {
            assert!(format.matches(Path::new(&format!("data{}", format.suffix()))));
        }
        assert!(!Format::Gzip.matches(Path::new("data.tar.gz")));
        assert!(!Format::Bzip2.matches(Path::new("data.tar.bz2")));
        assert!(!Format::Xz.matches(Path::new("data.tar.xz")));
        for name in ["x.TBZ2", "x.tbz", "x.txz", "x.tgz", "x.7z", "x.gz", "x.rar"] {
            assert!(can_extract(Path::new(name)), "{name}");
        }
        #[cfg(target_os = "linux")]
        for (name, format) in [("x.TBZ2", crate::packed_archive::Format::TarBzip2), ("x.tbz", crate::packed_archive::Format::TarBzip2), ("x.txz", crate::packed_archive::Format::TarXz)] {
            assert_eq!(crate::packed_archive::format_for_path(Path::new(name)), Some(format));
        }
        assert!(!can_extract(Path::new("not-an-archive.txt")));
        assert!(Format::Zip.encrypted());
        assert!(!Format::Tar.encrypted());
    }
}
