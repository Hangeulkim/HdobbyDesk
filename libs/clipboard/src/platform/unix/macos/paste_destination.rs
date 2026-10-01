//! File writes anchored to the paste destination, without following peer-supplied
//! symlinks or replacing existing files. No pasteboard or network access here.
use super::super::filetype::validate_relative_path;
use std::{
    ffi::{CStr, CString, OsStr, OsString},
    fs::{File, OpenOptions},
    io::{self, BufWriter, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::Path,
};

pub(super) struct PasteDestination {
    root: File,
}

#[derive(Debug)]
pub(super) struct PendingFile {
    pub writer: BufWriter<File>,
    parent: File,
    temporary: CString,
    desired: OsString,
    remove_temporary: bool,
}

fn component_name(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))
}

fn directory_at(parent: &File, name: &CStr) -> io::Result<File> {
    // The parent fd remains owned and open for the complete syscall.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        // A successful openat returns a new, exclusively owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

impl PasteDestination {
    pub fn new(path: &Path) -> io::Result<Self> {
        Ok(Self {
            root: OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(path)?,
        })
    }

    fn directory(&self, relative: &Path) -> io::Result<File> {
        let mut current = self.root.try_clone()?;
        for part in relative.components() {
            let std::path::Component::Normal(part) = part else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid paste directory",
                ));
            };
            let name = component_name(part)?;
            // mkdirat never follows a symlink at the new component. Existing
            // entries must also pass the no-follow directory open below.
            if unsafe { libc::mkdirat(current.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error);
                }
            }
            current = directory_at(&current, &name)?;
        }
        Ok(current)
    }

    pub fn create_directory(&self, relative: &Path) -> io::Result<()> {
        validate_relative_path(relative)?;
        self.directory(relative).map(|_| ())
    }

    pub fn create_file(&self, relative: &Path, capacity: usize) -> io::Result<PendingFile> {
        validate_relative_path(relative)?;
        let parent = self.directory(relative.parent().unwrap_or_else(|| Path::new("")))?;
        let desired = relative
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing file name"))?
            .to_owned();
        // A short independent name avoids exceeding NAME_MAX with the suffix.
        let temporary = component_name(OsStr::new(&format!(
            ".hdobby-{}.rddownload",
            uuid::Uuid::new_v4()
        )))?;
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                temporary.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // Only this PendingFile owns the new descriptor and temporary entry.
        let file = unsafe { File::from_raw_fd(fd) };
        Ok(PendingFile {
            writer: BufWriter::with_capacity(capacity, file),
            parent,
            temporary,
            desired,
            remove_temporary: true,
        })
    }
}

impl PendingFile {
    pub fn finish(&mut self) -> io::Result<()> {
        self.writer.flush()?;
        for index in 0..1024 {
            let desired = if index == 0 {
                self.desired.clone()
            } else {
                let path = Path::new(&self.desired);
                let mut name = path.file_stem().unwrap_or(&self.desired).to_owned();
                name.push(format!(" ({index})"));
                if let Some(extension) = path.extension() {
                    name.push(".");
                    name.push(extension);
                }
                name
            };
            let name = component_name(&desired)?;
            // Creating the final link is atomic and refuses every existing entry,
            // including a dangling symlink. It never truncates another file.
            if unsafe {
                libc::linkat(
                    self.parent.as_raw_fd(),
                    self.temporary.as_ptr(),
                    self.parent.as_raw_fd(),
                    name.as_ptr(),
                    0,
                )
            } == 0
            {
                self.remove_owned_temporary()?;
                return Ok(());
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::AlreadyExists {
                return Err(error);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "too many paste name conflicts",
        ))
    }

    fn remove_owned_temporary(&mut self) -> io::Result<()> {
        if !self.remove_temporary {
            return Ok(());
        }
        let owned = self.writer.get_ref().metadata()?;
        let mut current = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                self.parent.as_raw_fd(),
                self.temporary.as_ptr(),
                current.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        // fstatat initialized the structure on success.
        let current = unsafe { current.assume_init() };
        if current.st_dev as u64 != owned.dev() || current.st_ino != owned.ino() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "paste temporary file changed",
            ));
        }
        if unsafe { libc::unlinkat(self.parent.as_raw_fd(), self.temporary.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        self.remove_temporary = false;
        Ok(())
    }
}

impl Drop for PendingFile {
    fn drop(&mut self) {
        if let Err(error) = self.remove_owned_temporary() {
            hbb_common::log::warn!("Could not remove owned paste temporary file: {}", error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("hdobby-paste-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn nested_unicode_file_and_directory_paste() {
        let scratch = Scratch::new();
        let destination = PasteDestination::new(&scratch.0).unwrap();
        destination
            .create_directory(Path::new("한글 폴더/빈 폴더"))
            .unwrap();
        let mut file = destination
            .create_file(Path::new("한글 폴더/🙂.txt"), 8)
            .unwrap();
        file.writer.write_all("한글 abc123".as_bytes()).unwrap();
        file.finish().unwrap();
        drop(file);
        assert_eq!(
            std::fs::read_to_string(scratch.0.join("한글 폴더/🙂.txt")).unwrap(),
            "한글 abc123"
        );
        assert!(scratch.0.join("한글 폴더/빈 폴더").is_dir());
    }

    #[test]
    fn refuses_symlink_parents_and_escape_paths() {
        let scratch = Scratch::new();
        let outside = Scratch::new();
        symlink(&outside.0, scratch.0.join("linked")).unwrap();
        let destination = PasteDestination::new(&scratch.0).unwrap();
        assert!(destination
            .create_file(Path::new("linked/escape"), 8)
            .is_err());
        assert!(destination
            .create_directory(Path::new("linked/new"))
            .is_err());
        assert!(destination.create_file(Path::new("../escape"), 8).is_err());
        assert!(destination
            .create_file(&outside.0.join("escape"), 8)
            .is_err());
        assert_eq!(std::fs::read_dir(&outside.0).unwrap().count(), 0);
    }

    #[test]
    fn does_not_replace_existing_or_dangling_symlink_files() {
        let scratch = Scratch::new();
        let destination = PasteDestination::new(&scratch.0).unwrap();
        let mut file = destination.create_file(Path::new("same.txt"), 8).unwrap();
        file.writer.write_all(b"new").unwrap();
        // Conflict introduced after the temporary file was opened.
        std::fs::write(scratch.0.join("same.txt"), b"original").unwrap();
        symlink("missing", scratch.0.join("same (1).txt")).unwrap();
        file.finish().unwrap();
        drop(file);
        assert_eq!(
            std::fs::read(scratch.0.join("same.txt")).unwrap(),
            b"original"
        );
        assert!(scratch
            .0
            .join("same (1).txt")
            .symlink_metadata()
            .unwrap()
            .is_symlink());
        assert_eq!(
            std::fs::read(scratch.0.join("same (2).txt")).unwrap(),
            b"new"
        );
        assert!(!scratch.0.join("missing").exists());
    }

    #[test]
    fn cancellation_removes_only_unfinished_file() {
        let scratch = Scratch::new();
        let destination = PasteDestination::new(&scratch.0).unwrap();
        let mut file = destination
            .create_file(Path::new("cancelled.txt"), 8)
            .unwrap();
        file.writer.write_all(b"partial").unwrap();
        drop(file);
        assert_eq!(std::fs::read_dir(&scratch.0).unwrap().count(), 0);
        let mut empty = destination.create_file(Path::new("empty.txt"), 8).unwrap();
        empty.finish().unwrap();
        drop(empty);
        assert_eq!(
            std::fs::metadata(scratch.0.join("empty.txt"))
                .unwrap()
                .len(),
            0
        );
    }
}
