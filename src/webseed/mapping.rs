//! BEP 19 mapping from torrent byte offsets to web seed URLs.
//!
//! A torrent is one linear byte stream split across files; a web seed serves
//! those files individually over HTTP. [`FileMap`] resolves each file's URL once
//! and then translates any torrent offset into the per-file ranges that cover
//! it.
//!
//! This module deliberately knows nothing about librqbit so the mapping rules
//! stay directly testable.

/// One file of a torrent, as the mapper needs to see it.
#[derive(Debug, Clone)]
pub struct TorrentFile {
    /// Path components relative to the torrent root, without the torrent name.
    pub path: Vec<String>,
    /// Byte offset of this file within the torrent's linear byte stream.
    pub offset: u64,
    /// Length of the file in bytes.
    pub len: u64,
}

/// A torrent file paired with the URL it is fetched from on one web seed.
#[derive(Debug, Clone)]
pub struct MappedFile {
    pub offset: u64,
    pub len: u64,
    pub url: String,
}

/// A contiguous byte range within a single file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRange<'a> {
    /// URL the range is fetched from.
    pub url: &'a str,
    /// Index of the file within the torrent.
    pub file: usize,
    /// Offset of the range within that file.
    pub offset: u64,
    /// Length of the range in bytes.
    pub len: u64,
}

/// Maps torrent offsets onto per-file HTTP ranges for one web seed URL.
#[derive(Debug, Clone)]
pub struct FileMap {
    files: Vec<MappedFile>,
    total_length: u64,
}

impl FileMap {
    /// Resolve every file's URL against `base` following BEP 19.
    ///
    /// For a multi-file torrent each file lives under `<base>/<name>/<path>`.
    /// For a single-file torrent `base` is used as-is, unless it ends in `/`, in
    /// which case the torrent name is appended.
    pub fn new(base: &str, name: &str, multi_file: bool, files: &[TorrentFile]) -> Self {
        let total_length = files.iter().map(|f| f.len).sum();
        let mapped = files
            .iter()
            .map(|f| MappedFile {
                offset: f.offset,
                len: f.len,
                url: file_url(base, name, multi_file, &f.path),
            })
            .collect();
        Self {
            files: mapped,
            total_length,
        }
    }

    /// The mapped file at `index`.
    pub fn file(&self, index: usize) -> Option<&MappedFile> {
        self.files.get(index)
    }

    /// Split the torrent byte range `offset..offset + len` into per-file ranges,
    /// in torrent order. A range extending past the end of the torrent is
    /// truncated, so the returned lengths may sum to less than `len`.
    pub fn ranges(&self, offset: u64, len: u64) -> Vec<FileRange<'_>> {
        let end = offset.saturating_add(len).min(self.total_length);
        let mut pos = offset;
        // First file that still has bytes at or after `pos`. Zero-length files
        // compare as already passed, so they are skipped.
        let mut index = self.files.partition_point(|f| f.offset + f.len <= pos);
        let mut out = Vec::new();
        while pos < end {
            let Some(file) = self.files.get(index) else {
                break;
            };
            if file.len > 0 {
                let offset_in_file = pos - file.offset;
                let take = (file.len - offset_in_file).min(end - pos);
                out.push(FileRange {
                    url: &file.url,
                    file: index,
                    offset: offset_in_file,
                    len: take,
                });
                pos += take;
            }
            index += 1;
        }
        out
    }
}

/// Build the URL one file is fetched from.
fn file_url(base: &str, name: &str, multi_file: bool, path: &[String]) -> String {
    if !multi_file {
        return match base.ends_with('/') {
            true => format!("{base}{}", encode_segment(name)),
            false => base.to_string(),
        };
    }
    let mut url = String::with_capacity(base.len() + name.len() + 16);
    url.push_str(base);
    if !url.ends_with('/') {
        url.push('/');
    }
    url.push_str(&encode_segment(name));
    for component in path {
        url.push('/');
        url.push_str(&encode_segment(component));
    }
    url
}

/// Percent-encode one path segment, leaving only the unreserved characters of
/// RFC 3986 untouched.
fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &[&str], offset: u64, len: u64) -> TorrentFile {
        TorrentFile {
            path: path.iter().map(|s| s.to_string()).collect(),
            offset,
            len,
        }
    }

    #[test]
    fn single_file_direct_url_is_used_as_is() {
        let files = [file(&["movie.mkv"], 0, 100)];
        let map = FileMap::new("https://example.com/movie.mkv", "movie.mkv", false, &files);
        assert_eq!(map.file(0).unwrap().url, "https://example.com/movie.mkv");
    }

    #[test]
    fn single_file_directory_url_gets_the_name_appended() {
        let files = [file(&["movie.mkv"], 0, 100)];
        let map = FileMap::new("https://example.com/files/", "movie.mkv", false, &files);
        assert_eq!(
            map.file(0).unwrap().url,
            "https://example.com/files/movie.mkv"
        );
    }

    #[test]
    fn multi_file_urls_include_name_and_encoded_path() {
        let files = [file(&["disc 1", "track.flac"], 0, 100)];
        let map = FileMap::new("https://example.com/files/", "album", true, &files);
        assert_eq!(
            map.file(0).unwrap().url,
            "https://example.com/files/album/disc%201/track.flac"
        );
    }

    #[test]
    fn multi_file_base_without_trailing_slash_still_separates() {
        let files = [file(&["a.bin"], 0, 10)];
        let map = FileMap::new("https://example.com/files", "set", true, &files);
        assert_eq!(
            map.file(0).unwrap().url,
            "https://example.com/files/set/a.bin"
        );
    }

    #[test]
    fn non_ascii_names_are_encoded() {
        let files = [file(&["é.bin"], 0, 10)];
        let map = FileMap::new("https://example.com/", "set", true, &files);
        assert_eq!(
            map.file(0).unwrap().url,
            "https://example.com/set/%C3%A9.bin"
        );
    }

    #[test]
    fn range_within_one_file() {
        let files = [file(&["a"], 0, 100), file(&["b"], 100, 100)];
        let map = FileMap::new("https://e.com/", "t", true, &files);
        let ranges = map.ranges(10, 20);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].file, 0);
        assert_eq!(ranges[0].offset, 10);
        assert_eq!(ranges[0].len, 20);
    }

    #[test]
    fn range_spanning_a_file_boundary_splits() {
        let files = [file(&["a"], 0, 100), file(&["b"], 100, 100)];
        let map = FileMap::new("https://e.com/", "t", true, &files);
        let ranges = map.ranges(90, 20);
        assert_eq!(ranges.len(), 2);
        assert_eq!(
            (ranges[0].file, ranges[0].offset, ranges[0].len),
            (0, 90, 10)
        );
        assert_eq!(
            (ranges[1].file, ranges[1].offset, ranges[1].len),
            (1, 0, 10)
        );
    }

    #[test]
    fn range_past_the_end_is_truncated() {
        let files = [file(&["a"], 0, 100)];
        let map = FileMap::new("https://e.com/", "t", true, &files);
        let ranges = map.ranges(90, 50);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].len, 10);
    }

    #[test]
    fn zero_length_files_are_skipped() {
        let files = [
            file(&["a"], 0, 50),
            file(&["empty"], 50, 0),
            file(&["b"], 50, 50),
        ];
        let map = FileMap::new("https://e.com/", "t", true, &files);
        let ranges = map.ranges(40, 20);
        assert_eq!(ranges.len(), 2);
        assert_eq!((ranges[0].file, ranges[0].len), (0, 10));
        assert_eq!(
            (ranges[1].file, ranges[1].offset, ranges[1].len),
            (2, 0, 10)
        );
    }

    #[test]
    fn range_starting_exactly_at_a_boundary() {
        let files = [file(&["a"], 0, 100), file(&["b"], 100, 100)];
        let map = FileMap::new("https://e.com/", "t", true, &files);
        let ranges = map.ranges(100, 10);
        assert_eq!(ranges.len(), 1);
        assert_eq!((ranges[0].file, ranges[0].offset), (1, 0));
    }
}
