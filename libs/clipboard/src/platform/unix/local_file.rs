use super::{BLOCK_SIZE, LDAP_EPOCH_DELTA};
use crate::{
    platform::unix::{
        FLAGS_FD_ATTRIBUTES, FLAGS_FD_LAST_WRITE, FLAGS_FD_PROGRESSUI, FLAGS_FD_SIZE,
        FLAGS_FD_UNIX_MODE,
    },
    CliprdrError,
};
use hbb_common::{
    bytes::{BufMut, BytesMut},
    log,
};
use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader, Read, Seek},
    os::unix::prelude::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};
use utf16string::WString;

#[derive(Debug)]
pub(super) struct LocalFile {
    pub relative_root: PathBuf,
    pub path: PathBuf,

    pub handle: Option<BufReader<File>>,
    pub offset: AtomicU64,

    pub name: String,
    pub size: u64,
    pub last_write_time: SystemTime,
    pub is_dir: bool,
    pub perm: u32,
    pub read_only: bool,
    pub hidden: bool,
    pub system: bool,
    pub archive: bool,
    pub normal: bool,
}

impl LocalFile {
    pub fn try_open(relative_root: &Path, path: &Path) -> Result<Self, CliprdrError> {
        let relative =
            path.strip_prefix(relative_root)
                .map_err(|_| CliprdrError::InvalidRequest {
                    description: "clipboard file is outside its selected folder".to_owned(),
                })?;
        validate_wire_path(relative)?;
        let mt = std::fs::symlink_metadata(path).map_err(|e| CliprdrError::FileError {
            path: path.to_string_lossy().to_string(),
            err: e,
        })?;
        if !mt.is_file() && !mt.is_dir() {
            return Err(CliprdrError::InvalidRequest {
                description: "clipboard symlinks and special files are not supported".to_owned(),
            });
        }
        let size = if mt.is_file() { mt.len() } else { 0 };
        let is_dir = mt.is_dir();
        let read_only = mt.permissions().readonly();
        let system = false;
        let hidden = path.to_string_lossy().starts_with('.');
        let archive = false;
        let normal = !(is_dir || read_only || system || hidden || archive);
        let last_write_time = mt.modified().unwrap_or(SystemTime::UNIX_EPOCH);

        let perm = mt.permissions().mode();

        let name = path
            .display()
            .to_string()
            .trim_start_matches('/')
            .replace('/', "\\");

        // NOTE: open files lazily
        let handle = None;
        let offset = AtomicU64::new(0);

        Ok(Self {
            name,
            relative_root: relative_root.to_path_buf(),
            path: path.to_path_buf(),
            handle,
            offset,
            size,
            last_write_time,
            is_dir,
            read_only,
            system,
            hidden,
            perm,
            archive,
            normal,
        })
    }
    pub fn as_bin(&self) -> Result<Vec<u8>, CliprdrError> {
        let mut buf = BytesMut::with_capacity(592);

        let read_only_flag = if self.read_only { 0x1 } else { 0 };
        let hidden_flag = if self.hidden { 0x2 } else { 0 };
        let system_flag = if self.system { 0x4 } else { 0 };
        let directory_flag = if self.is_dir { 0x10 } else { 0 };
        let archive_flag = if self.archive { 0x20 } else { 0 };
        let normal_flag = if self.normal { 0x80 } else { 0 };

        let file_attributes: u32 = read_only_flag
            | hidden_flag
            | system_flag
            | directory_flag
            | archive_flag
            | normal_flag;

        let win32_time = self
            .last_write_time
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            / 100;
        let win32_time =
            u64::try_from(win32_time + u128::from(LDAP_EPOCH_DELTA)).map_err(|_| {
                CliprdrError::InvalidRequest {
                    description: "clipboard timestamp out of range".to_owned(),
                }
            })?;

        let size_high = (self.size >> 32) as u32;
        let size_low = (self.size & (u32::MAX as u64)) as u32;

        let path = self.path.strip_prefix(&self.relative_root).map_err(|_| {
            CliprdrError::InvalidRequest {
                description: "clipboard file is outside its selected folder".to_owned(),
            }
        })?;
        validate_wire_path(path)?;
        let path = path.to_string_lossy();

        let wstr: WString<utf16string::LE> = WString::from(path.as_ref());
        let name = wstr.as_bytes();

        log::trace!(
            "put file to list: name_len {}, name {}",
            name.len(),
            &self.name
        );

        let flags = FLAGS_FD_SIZE
            | FLAGS_FD_LAST_WRITE
            | FLAGS_FD_ATTRIBUTES
            | FLAGS_FD_PROGRESSUI
            | FLAGS_FD_UNIX_MODE;

        // flags, 4 bytes
        buf.put_u32_le(flags);
        // 32 bytes reserved
        buf.put(&[0u8; 32][..]);
        // file attributes, 4 bytes
        buf.put_u32_le(file_attributes);

        // NOTE: this is not used in windows
        // in the specification, this is 16 bytes reserved
        // lets use the last 4 bytes to store the file mode
        //
        // 12 bytes reserved
        buf.put(&[0u8; 12][..]);
        // file permissions, 4 bytes
        buf.put_u32_le(self.perm);

        // last write time, 8 bytes
        buf.put_u64_le(win32_time);
        // file size (high)
        buf.put_u32_le(size_high);
        // file size (low)
        buf.put_u32_le(size_low);
        // put name and padding to 520 bytes
        let name_len = name.len();
        buf.put(name);
        buf.put(&vec![0u8; 520 - name_len][..]);

        Ok(buf.to_vec())
    }

    #[inline]
    pub fn load_handle(&mut self) -> Result<(), CliprdrError> {
        if !self.is_dir && self.handle.is_none() {
            let handle = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(&self.path)
                .map_err(|e| CliprdrError::FileError {
                    path: self.path.to_string_lossy().to_string(),
                    err: e,
                })?;
            let metadata = handle.metadata().map_err(|err| CliprdrError::FileError {
                path: self.path.to_string_lossy().to_string(),
                err,
            })?;
            if !metadata.is_file() {
                return Err(CliprdrError::InvalidRequest {
                    description: "clipboard source is no longer a regular file".to_owned(),
                });
            }
            let mut reader = BufReader::with_capacity(BLOCK_SIZE as usize * 2, handle);
            reader.fill_buf().map_err(|e| CliprdrError::FileError {
                path: self.path.to_string_lossy().to_string(),
                err: e,
            })?;
            self.handle = Some(reader);
        };
        Ok(())
    }

    pub fn read_exact_at(&mut self, buf: &mut [u8], offset: u64) -> Result<(), CliprdrError> {
        self.load_handle()?;

        let Some(handle) = self.handle.as_mut() else {
            return Err(CliprdrError::FileError {
                path: self.path.to_string_lossy().to_string(),
                err: std::io::Error::new(std::io::ErrorKind::NotFound, "file handle not found"),
            });
        };

        let read_result = if offset != self.offset.load(Ordering::Relaxed) {
            handle
                .seek(std::io::SeekFrom::Start(offset))
                .and_then(|_| handle.read_exact(buf))
        } else {
            handle.read_exact(buf)
        };
        if let Err(e) = read_result {
            return Err(self.invalidate_handle(e));
        }
        let new_offset = offset + (buf.len() as u64);
        self.offset.store(new_offset, Ordering::Relaxed);

        // gc file handle
        if new_offset >= self.size {
            self.offset.store(0, Ordering::Relaxed);
            self.handle = None;
        }

        Ok(())
    }

    fn invalidate_handle(&mut self, err: std::io::Error) -> CliprdrError {
        self.offset.store(0, Ordering::Relaxed);
        self.handle = None;
        CliprdrError::FileError {
            path: self.path.to_string_lossy().to_string(),
            err,
        }
    }
}

fn validate_wire_path(path: &Path) -> Result<(), CliprdrError> {
    super::filetype::validate_relative_path(path).map_err(|_| CliprdrError::InvalidRequest {
        description: "invalid clipboard file path".to_owned(),
    })?;
    if path.to_string_lossy().encode_utf16().count() > 259 {
        return Err(CliprdrError::InvalidRequest {
            description: "clipboard file path exceeds the protocol limit".to_owned(),
        });
    }
    Ok(())
}

pub(super) fn construct_file_list(paths: &[PathBuf]) -> Result<Vec<LocalFile>, CliprdrError> {
    fn constr_file_lst(
        relative_root: &Path,
        path: &Path,
        file_list: &mut Vec<LocalFile>,
        visited: &mut HashSet<PathBuf>,
    ) -> Result<(), CliprdrError> {
        // prevent fs loop
        if visited.contains(path) {
            return Ok(());
        }
        visited.insert(path.to_path_buf());

        if file_list.len() >= 65_536 {
            return Err(CliprdrError::InvalidRequest {
                description: "too many clipboard files".to_owned(),
            });
        }
        let local_file = LocalFile::try_open(relative_root, path)?;
        let is_dir = local_file.is_dir;
        file_list.push(local_file);
        if is_dir {
            let dir = std::fs::read_dir(path).map_err(|e| CliprdrError::FileError {
                path: path.to_string_lossy().to_string(),
                err: e,
            })?;
            for entry in dir {
                let entry = entry.map_err(|e| CliprdrError::FileError {
                    path: path.to_string_lossy().to_string(),
                    err: e,
                })?;
                let path = entry.path();
                constr_file_lst(relative_root, &path, file_list, visited)?;
            }
        }
        Ok(())
    }

    let mut file_list = Vec::new();
    let mut visited = HashSet::new();

    let relative_root = paths
        .first()
        .ok_or(CliprdrError::InvalidRequest {
            description: "empty file list".to_string(),
        })?
        .parent()
        .ok_or(CliprdrError::InvalidRequest {
            description: "empty parent".to_string(),
        })?
        .to_path_buf();
    for path in paths {
        constr_file_lst(&relative_root, path, &mut file_list, &mut visited)?;
    }
    Ok(file_list)
}

#[cfg(test)]
mod file_list_test {
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use hbb_common::bytes::{BufMut, BytesMut};

    use crate::{platform::unix::filetype::FileDescription, CliprdrError};

    use super::LocalFile;

    #[inline]
    fn generate_tree(prefix: &str) -> Vec<LocalFile> {
        // generate a tree of local files, no handles
        // - /
        // |- a.txt
        // |- b
        //    |- c.txt
        #[inline]
        fn generate_file(path: &str, name: &str, is_dir: bool) -> LocalFile {
            LocalFile {
                relative_root: PathBuf::new(),
                path: PathBuf::from(path),
                handle: None,
                name: name.to_string(),
                size: 0,
                offset: AtomicU64::new(0),
                last_write_time: std::time::SystemTime::UNIX_EPOCH,
                read_only: false,
                is_dir,
                perm: 0o754,
                hidden: false,
                system: false,
                archive: false,
                normal: false,
            }
        }

        let p = prefix;

        let (r_path, a_path, b_path, c_path) = if !prefix.is_empty() {
            (
                p.to_string(),
                format!("{}/a.txt", p),
                format!("{}/b", p),
                format!("{}/b/c.txt", p),
            )
        } else {
            (
                ".".to_string(),
                "a.txt".to_string(),
                "b".to_string(),
                "b/c.txt".to_string(),
            )
        };

        let root = generate_file(&r_path, ".", true);
        let a = generate_file(&a_path, "a.txt", false);
        let b = generate_file(&b_path, "b", true);
        let c = generate_file(&c_path, "c.txt", false);

        vec![root, a, b, c]
    }

    fn as_bin_parse_test(prefix: &str) -> Result<(), CliprdrError> {
        let tree = generate_tree(prefix);
        let mut pdu = BytesMut::with_capacity(4 + 592 * tree.len());
        pdu.put_u32_le(tree.len() as u32);
        for file in tree {
            pdu.put(file.as_bin()?.as_slice());
        }

        let parsed = FileDescription::parse_file_descriptors(pdu.to_vec(), 0)?;
        assert_eq!(parsed.len(), 4);

        if !prefix.is_empty() {
            assert_eq!(parsed[0].name.to_str().unwrap(), format!("{}", prefix));
            assert_eq!(
                parsed[1].name.to_str().unwrap(),
                format!("{}/a.txt", prefix)
            );
            assert_eq!(parsed[2].name.to_str().unwrap(), format!("{}/b", prefix));
            assert_eq!(
                parsed[3].name.to_str().unwrap(),
                format!("{}/b/c.txt", prefix)
            );
        } else {
            assert_eq!(parsed[0].name.to_str().unwrap(), ".");
            assert_eq!(parsed[1].name.to_str().unwrap(), "a.txt");
            assert_eq!(parsed[2].name.to_str().unwrap(), "b");
            assert_eq!(parsed[3].name.to_str().unwrap(), "b/c.txt");
        }

        assert!(parsed[0].perm & 0o777 == 0o754);
        assert!(parsed[1].perm & 0o777 == 0o754);
        assert!(parsed[2].perm & 0o777 == 0o754);
        assert!(parsed[3].perm & 0o777 == 0o754);

        Ok(())
    }

    #[test]
    fn test_parse_file_descriptors() -> Result<(), CliprdrError> {
        assert!(as_bin_parse_test("").is_err());
        assert!(as_bin_parse_test("/").is_err());
        as_bin_parse_test("test")?;
        as_bin_parse_test("한글 폴더")?;
        assert!(as_bin_parse_test("/test").is_err());
        Ok(())
    }

    #[test]
    fn rejects_symlink_and_overlong_source_names() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("hdobby-source-test-{}", std::process::id()));
        std::fs::create_dir(&root)?;
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            std::fs::write(root.join("original"), b"private")?;
            std::os::unix::fs::symlink(root.join("original"), root.join("link"))?;
            assert!(LocalFile::try_open(&root, &root.join("link")).is_err());
            let nested = root.join("a".repeat(200));
            std::fs::create_dir(&nested)?;
            let long_file = nested.join("b".repeat(60));
            std::fs::write(&long_file, b"data")?;
            assert!(LocalFile::try_open(&root, &long_file).is_err());
            let directory = LocalFile::try_open(&root, &nested)?;
            assert_eq!(directory.size, 0);
            let mut regular = LocalFile::try_open(&root, &root.join("original"))?;
            std::fs::remove_file(root.join("original"))?;
            std::os::unix::fs::symlink(root.join("missing"), root.join("original"))?;
            assert!(regular.load_handle().is_err());
            Ok(())
        })();
        std::fs::remove_dir_all(root)?;
        result
    }

    #[test]
    fn filetime_round_trip_uses_windows_epoch_without_nanosecond_overflow(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut file = generate_tree("folder").remove(1);
        let seconds = 20_000_000_000;
        file.last_write_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds);
        let record = file.as_bin()?;
        let ticks = u64::from_le_bytes(record[56..64].try_into()?);
        assert_eq!(ticks, 116_444_736_000_000_000 + seconds * 10_000_000);
        let mut pdu = 1u32.to_le_bytes().to_vec();
        pdu.extend(record);
        let decoded = FileDescription::parse_file_descriptors(pdu, 0)?;
        assert_eq!(decoded[0].last_modified, file.last_write_time);
        Ok(())
    }

    #[test]
    fn read_exact_at_reopens_after_read_failure() -> Result<(), Box<dyn std::error::Error>> {
        let file_path = std::env::temp_dir().join(format!(
            "rustdesk-clipboard-local-file-{}",
            std::process::id()
        ));
        std::fs::write(&file_path, b"")?;

        let mut file = LocalFile::try_open(&std::env::temp_dir(), &file_path)?;
        file.size = 1;

        let mut buf = [0u8; 1];
        assert!(file.read_exact_at(&mut buf, 0).is_err());
        assert!(file.handle.is_none());
        assert_eq!(file.offset.load(Ordering::Relaxed), 0);

        std::fs::write(&file_path, [42u8])?;

        file.read_exact_at(&mut buf, 0)?;
        assert_eq!(buf, [42u8]);
        assert!(file.handle.is_none());

        std::fs::remove_file(file_path)?;
        Ok(())
    }
}
