use super::local_file::LocalFile;
use crate::{platform::unix::local_file::construct_file_list, ClipboardFile, CliprdrError};
use hbb_common::{
    bytes::{BufMut, BytesMut},
    log,
};
use parking_lot::Mutex;
use std::{path::PathBuf, sync::Arc, time::SystemTime, usize};

lazy_static::lazy_static! {
    // local files are cached, this value should not be changed when copying files
    // Because `CliprdrFileContentsRequest` only contains the index of the file in the list.
    // We need to keep the file list in the same order as the remote side.
    // We may add a `FileId` field to `CliprdrFileContentsRequest` in the future.
    static ref CLIP_FILES: Arc<Mutex<ClipFiles>> = Default::default();
}

#[derive(Debug)]
enum FileContentsRequest {
    Size {
        stream_id: i32,
        file_idx: usize,
    },

    Range {
        stream_id: i32,
        file_idx: usize,
        offset: u64,
        length: u64,
    },
}

// Cheap fingerprint of one top-level selected entry. A change in size/mtime --
// or a directory in the selection -- forces sync_files() to rebuild (see below).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct FileSig {
    size: u64,
    mtime: Option<SystemTime>,
    is_dir: bool,
}

// Stat the top-level selected paths only (no recursion), same order as `files`.
fn fingerprint(files: &[String]) -> Vec<FileSig> {
    files
        .iter()
        .map(|s| match std::fs::metadata(s) {
            Ok(mt) => FileSig {
                size: mt.len(),
                mtime: mt.modified().ok(),
                is_dir: mt.is_dir(),
            },
            Err(_) => FileSig::default(),
        })
        .collect()
}

#[derive(Default)]
struct ClipFiles {
    files: Vec<String>,
    // Fingerprint of `files` (same len/order); detects in-place edits on re-copy.
    sigs: Vec<FileSig>,
    file_list: Vec<LocalFile>,
    first_file_index: usize,
    files_pdu: Vec<u8>,
}

impl ClipFiles {
    fn clear(&mut self) {
        self.files.clear();
        self.sigs.clear();
        self.file_list.clear();
        self.first_file_index = usize::MAX;
        self.files_pdu.clear();
    }

    fn sync_files(
        &mut self,
        clipboard_files: &[String],
        sigs: Vec<FileSig>,
    ) -> Result<(), CliprdrError> {
        let clipboard_paths = clipboard_files
            .iter()
            .map(|s| PathBuf::from(s))
            .collect::<Vec<_>>();
        self.file_list = construct_file_list(&clipboard_paths)?;
        self.first_file_index = self
            .file_list
            .iter()
            .position(|f| !f.path.is_dir())
            .unwrap_or(usize::MAX);
        self.files = clipboard_files.to_vec();
        self.sigs = sigs;
        Ok(())
    }

    fn build_file_list_pdu(&mut self) -> Result<(), CliprdrError> {
        let mut data = BytesMut::with_capacity(4 + 592 * self.file_list.len());
        data.put_u32_le(self.file_list.len() as u32);
        for file in self.file_list.iter() {
            data.put(file.as_bin()?.as_slice());
        }
        self.files_pdu = data.to_vec();
        Ok(())
    }

    fn get_files_for_audit(&self, request: &FileContentsRequest) -> Option<ClipboardFile> {
        if let FileContentsRequest::Range {
            file_idx, offset, ..
        } = request
        {
            if *file_idx == self.first_file_index && *offset == 0 {
                let files: Vec<(String, u64)> = self
                    .file_list
                    .iter()
                    .filter_map(|f| {
                        if f.path.is_file() {
                            Some((f.path.to_string_lossy().to_string(), f.size))
                        } else {
                            None
                        }
                    })
                    .collect::<_>();
                if files.is_empty() {
                    return None;
                } else {
                    return Some(ClipboardFile::Files { files });
                }
            }
        }
        None
    }

    fn serve_file_contents(
        &mut self,
        conn_id: i32,
        request: FileContentsRequest,
    ) -> Result<ClipboardFile, CliprdrError> {
        let (_file_idx, file_contents_resp) = match request {
            FileContentsRequest::Size {
                stream_id,
                file_idx,
            } => {
                log::debug!("file contents (size) requested from conn: {}", conn_id);
                let Some(file) = self.file_list.get(file_idx) else {
                    log::error!(
                        "invalid file index {} requested from conn: {}",
                        file_idx,
                        conn_id
                    );
                    return Err(CliprdrError::InvalidRequest {
                        description: format!(
                            "invalid file index {} requested from conn: {}",
                            file_idx, conn_id
                        ),
                    });
                };

                log::debug!(
                    "conn {} requested file-{}: {}",
                    conn_id,
                    file_idx,
                    file.name
                );

                let size = file.size;
                (
                    file_idx,
                    ClipboardFile::FileContentsResponse {
                        msg_flags: 0x1,
                        stream_id,
                        requested_data: size.to_le_bytes().to_vec(),
                    },
                )
            }
            FileContentsRequest::Range {
                stream_id,
                file_idx,
                offset,
                length,
            } => {
                if length == 0 || length > super::BLOCK_SIZE as u64 {
                    return Err(CliprdrError::InvalidRequest {
                        description: "clipboard read length is outside the allowed block size"
                            .to_owned(),
                    });
                }
                log::debug!(
                    "file contents (range from {} length {}) request from conn: {}",
                    offset,
                    length,
                    conn_id
                );
                let Some(file) = self.file_list.get_mut(file_idx) else {
                    log::error!(
                        "invalid file index {} requested from conn: {}",
                        file_idx,
                        conn_id
                    );
                    return Err(CliprdrError::InvalidRequest {
                        description: format!(
                            "invalid file index {} requested from conn: {}",
                            file_idx, conn_id
                        ),
                    });
                };
                log::debug!(
                    "conn {} requested file-{}: {}",
                    conn_id,
                    file_idx,
                    file.name
                );

                if file.is_dir || offset > file.size {
                    log::error!("invalid reading offset requested from conn: {}", conn_id);
                    return Err(CliprdrError::InvalidRequest {
                        description: format!(
                            "invalid reading offset requested from conn: {}",
                            conn_id
                        ),
                    });
                }
                let read_size = length.min(file.size - offset);

                let mut buf = vec![0u8; read_size as usize];

                file.read_exact_at(&mut buf, offset)?;

                (
                    file_idx,
                    ClipboardFile::FileContentsResponse {
                        msg_flags: 0x1,
                        stream_id,
                        requested_data: buf,
                    },
                )
            }
        };

        log::debug!("file contents sent to conn: {}", conn_id);
        // Open later selections only when requested. A missing later file must
        // not invalidate the current file's successfully read response.
        Ok(file_contents_resp)
    }
}

#[inline]
pub fn clear_files() {
    CLIP_FILES.lock().clear();
}

pub fn read_file_contents(
    conn_id: i32,
    stream_id: i32,
    list_index: i32,
    dw_flags: i32,
    n_position_low: i32,
    n_position_high: i32,
    cb_requested: i32,
) -> Vec<Result<ClipboardFile, CliprdrError>> {
    if list_index < 0
        || (dw_flags == 0x2 && (cb_requested <= 0 || cb_requested as u32 > super::BLOCK_SIZE))
    {
        return vec![Err(CliprdrError::InvalidRequest {
            description: "invalid clipboard file index or read length".to_owned(),
        })];
    }
    let fcr = if dw_flags == 0x1 {
        FileContentsRequest::Size {
            stream_id,
            file_idx: list_index as usize,
        }
    } else if dw_flags == 0x2 {
        let offset = (n_position_high as u32 as u64) << 32 | n_position_low as u32 as u64;
        let length = cb_requested as u64;

        FileContentsRequest::Range {
            stream_id,
            file_idx: list_index as usize,
            offset,
            length,
        }
    } else {
        return vec![Err(CliprdrError::InvalidRequest {
            description: format!("got invalid FileContentsRequest, dw_flats: {dw_flags}"),
        })];
    };

    let mut clip_files = CLIP_FILES.lock();
    let mut res = vec![];
    if let Some(files_res) = clip_files.get_files_for_audit(&fcr) {
        res.push(Ok(files_res));
    }
    res.push(clip_files.serve_file_contents(conn_id, fcr));
    res
}

pub fn sync_files(files: &[String]) -> Result<(), CliprdrError> {
    // Dedup: skip the rebuild only when paths + sizes + mtimes match and no dir is
    // selected (a dir's own mtime doesn't change when a file inside it is edited).
    let current = fingerprint(files);
    let mut files_lock = CLIP_FILES.lock();
    if files_lock.files == files
        && files_lock.sigs == current
        && !current.iter().any(|sig| sig.is_dir)
    {
        return Ok(());
    }
    let result = files_lock
        .sync_files(files, current)
        .and_then(|_| files_lock.build_file_list_pdu());
    if result.is_err() {
        // An invalid new copy must not keep serving the previous selection.
        files_lock.clear();
    }
    result
}

pub fn get_file_list_pdu() -> Vec<u8> {
    CLIP_FILES.lock().files_pdu.clone()
}

#[cfg(test)]
mod sig_test {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    static CACHE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // Unique temp dir under the system temp dir; removed on drop (no dev-dep).
    struct TmpDir(PathBuf);
    impl TmpDir {
        fn new(tag: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let mut dir = std::env::temp_dir();
            dir.push(format!("rustdesk_sig_test_{}_{}", tag, nanos));
            fs::create_dir_all(&dir).unwrap();
            TmpDir(dir)
        }
        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn path_str(p: &PathBuf) -> String {
        p.to_string_lossy().to_string()
    }

    #[test]
    fn fingerprint_missing_path_is_default() {
        let tmp = TmpDir::new("missing");
        let missing = path_str(&tmp.join("does_not_exist.bin"));
        let sigs = fingerprint(&[missing]);
        assert_eq!(sigs.len(), 1);
        // A path that can't be stat'd -> default sig, which forces a rebuild.
        assert_eq!(sigs[0], FileSig::default());
        assert_eq!(sigs[0].mtime, None);
    }

    #[test]
    fn fingerprint_detects_inplace_edit() {
        let tmp = TmpDir::new("edit");
        let file = tmp.join("a.bin");
        fs::write(&file, b"small").unwrap();
        let p = path_str(&file);

        let before = fingerprint(&[p.clone()]);
        // Same content, same path: fingerprint must be stable.
        let again = fingerprint(&[p.clone()]);
        assert_eq!(before, again);
        assert_eq!(before[0].size, 5);
        assert!(!before[0].is_dir);

        // Edit in place so the file grows.
        fs::write(&file, b"much larger contents than before").unwrap();
        let after = fingerprint(&[p]);
        assert_ne!(before, after);
        assert!(after[0].size > before[0].size);
    }

    #[test]
    fn fingerprint_flags_directory() {
        let tmp = TmpDir::new("dir");
        let sub = tmp.join("subdir");
        fs::create_dir_all(&sub).unwrap();
        let sigs = fingerprint(&[path_str(&sub)]);
        assert_eq!(sigs.len(), 1);
        assert!(sigs[0].is_dir);
    }

    #[test]
    fn invalid_new_copy_clears_previous_file_selection() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        let tmp = TmpDir::new("invalid_copy");
        fs::write(tmp.join("valid.txt"), b"previous selection").unwrap();
        std::os::unix::fs::symlink(tmp.join("valid.txt"), tmp.join("link")).unwrap();
        clear_files();
        sync_files(&[path_str(&tmp.join("valid.txt"))]).unwrap();
        assert!(!get_file_list_pdu().is_empty());
        assert!(sync_files(&[path_str(&tmp.join("link"))]).is_err());
        assert!(get_file_list_pdu().is_empty());
        assert!(CLIP_FILES.lock().file_list.is_empty());
        sync_files(&[path_str(&tmp.join("valid.txt"))]).unwrap();
        let decoded =
            super::super::FileDescription::parse_file_descriptors(get_file_list_pdu(), 1).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].name, PathBuf::from("valid.txt"));
        clear_files();
    }

    #[test]
    fn recopy_after_edit_refreshes_cached_size() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        let tmp = TmpDir::new("recopy");
        let file = tmp.join("doc.bin");
        fs::write(&file, b"v1").unwrap(); // 2 bytes
        let files = vec![path_str(&file)];

        // Drive the public, guarded `sync_files` over the global CLIP_FILES;
        // reset first (this is the only test that touches the global).
        clear_files();

        sync_files(&files).unwrap();
        {
            let cache = CLIP_FILES.lock();
            let idx = cache.first_file_index;
            assert_eq!(cache.file_list[idx].size, 2);
        }

        // In-place edit grows the file; the re-copy must rebuild. Pre-fix the
        // path-only guard early-returned and left the cached size stale at 2.
        fs::write(&file, b"v2 is bigger").unwrap(); // 12 bytes
        sync_files(&files).unwrap();
        {
            let cache = CLIP_FILES.lock();
            let idx = cache.first_file_index;
            assert_eq!(cache.file_list[idx].size, 12);
        }

        clear_files(); // leave the global clean for other tests
    }

    #[test]
    fn rejects_invalid_file_read_lengths_before_allocation() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        let tmp = TmpDir::new("read_bounds");
        fs::write(tmp.join("source"), b"abc").unwrap();
        clear_files();
        sync_files(&[path_str(&tmp.join("source"))]).unwrap();
        for length in [-1, 0, super::super::BLOCK_SIZE as i32 + 1] {
            let response = read_file_contents(1, 1, 0, 2, 1, 0, length);
            assert!(
                matches!(response.last(), Some(Err(_))),
                "accepted length {length}"
            );
        }
        clear_files();
    }

    #[test]
    fn file_read_offset_preserves_unsigned_low_word_above_two_gib() {
        use std::io::{Seek, SeekFrom, Write};
        let _guard = CACHE_TEST_LOCK.lock().unwrap();
        let tmp = TmpDir::new("large_offset");
        let path = tmp.join("sparse");
        let mut file = fs::File::create(&path).unwrap();
        file.seek(SeekFrom::Start(0x8000_0000)).unwrap();
        file.write_all(b"END").unwrap();
        drop(file);
        clear_files();
        sync_files(&[path_str(&path)]).unwrap();
        let response = read_file_contents(1, 1, 0, 2, i32::MIN, 0, 3);
        assert!(
            matches!(response.last(), Some(Ok(ClipboardFile::FileContentsResponse { requested_data, .. })) if requested_data == b"END")
        );
        clear_files();
    }

    #[test]
    fn missing_later_file_does_not_discard_current_read() {
        let tmp = TmpDir::new("later_missing");
        fs::write(tmp.join("first"), b"abc").unwrap();
        fs::write(tmp.join("later"), b"xyz").unwrap();
        let paths = [path_str(&tmp.join("first")), path_str(&tmp.join("later"))];
        let mut cache = ClipFiles::default();
        cache.sync_files(&paths, fingerprint(&paths)).unwrap();
        fs::remove_file(tmp.join("later")).unwrap();
        let response = cache.serve_file_contents(
            1,
            FileContentsRequest::Range {
                stream_id: 1,
                file_idx: 0,
                offset: 0,
                length: 3,
            },
        );
        assert!(
            matches!(response, Ok(ClipboardFile::FileContentsResponse { requested_data, .. }) if requested_data == b"abc")
        );
        assert!(cache
            .serve_file_contents(
                1,
                FileContentsRequest::Range {
                    stream_id: 1,
                    file_idx: 1,
                    offset: 0,
                    length: 3
                }
            )
            .is_err());
    }
}
