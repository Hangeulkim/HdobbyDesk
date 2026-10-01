use super::paste_destination::{PasteDestination, PendingFile};
use crate::{
    platform::unix::{FileDescription, FileType, BLOCK_SIZE},
    send_data, ClipboardFile, CliprdrError, ProgressPercent,
};
use hbb_common::{log, tokio::time::Instant};
use std::{
    cmp::min,
    fs::{File, FileTimes},
    io::Write,
    os::macos::fs::FileTimesExt,
    path::PathBuf,
    sync::{
        mpsc::{Receiver, RecvTimeoutError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime},
};
use xattr::FileExt;

const RECV_RETRY_TIMES: usize = 3;

const RECEIVE_WAIT_TIMEOUT: Duration = Duration::from_millis(5_000);

// https://stackoverflow.com/a/15112784/1926020
// "1984-01-24 08:00:00 +0000"
const TIMESTAMP_FOR_FILE_PROGRESS_COMPLETED: u64 = 443779200;
const ATTR_PROGRESS_FRACTION_COMPLETED: &str = "com.apple.progress.fractionCompleted";

pub struct FileContentsResponse {
    pub conn_id: i32,
    pub msg_flags: i32,
    pub stream_id: i32,
    pub requested_data: Vec<u8>,
}

#[derive(Debug)]
struct PasteTaskProgress {
    // Use list index to identify the file
    // `list_index` is also used as the stream id
    list_index: i32,
    offset: u64,
    total_size: u64,
    current_size: u64,
    last_sent_time: Instant,
    download_file_index: i32,
    download_file_size: u64,
    download_file_current_size: u64,
    file_handle: Option<PendingFile>,
    error: Option<CliprdrError>,
    is_canceled: bool,
}

struct PasteTaskHandle {
    progress: PasteTaskProgress,
    destination: PasteDestination,
    files: Vec<FileDescription>,
}

pub struct PasteTask {
    exit: Arc<Mutex<bool>>,
    handle: Arc<Mutex<Option<PasteTaskHandle>>>,
    handle_worker: Option<thread::JoinHandle<()>>,
}

impl Drop for PasteTask {
    fn drop(&mut self) {
        *self.exit.lock().unwrap() = true;
        if let Some(handle_worker) = self.handle_worker.take() {
            handle_worker.join().ok();
        }
    }
}

impl PasteTask {
    const INVALID_FILE_INDEX: i32 = -1;

    pub fn new(rx_file_contents: Receiver<FileContentsResponse>) -> Self {
        let exit = Arc::new(Mutex::new(false));
        let handle = Arc::new(Mutex::new(None));
        let handle_worker =
            Self::init_worker_thread(exit.clone(), handle.clone(), rx_file_contents);
        Self {
            handle,
            exit,
            handle_worker: Some(handle_worker),
        }
    }

    pub fn start(
        &mut self,
        target_dir: PathBuf,
        files: Vec<FileDescription>,
    ) -> Result<(), CliprdrError> {
        let mut task_lock = self.handle.lock().unwrap();
        if task_lock
            .as_ref()
            .map(|x| !x.is_finished())
            .unwrap_or(false)
        {
            log::error!("Previous paste task is not finished, ignore new request.");
            return Err(CliprdrError::ClipboardOccupied);
        }
        for file in &files {
            super::super::filetype::validate_relative_path(&file.name).map_err(paste_io_error)?;
            if file.kind == FileType::Symlink {
                return Err(CliprdrError::InvalidRequest {
                    description: "symlink paste is not supported".to_owned(),
                });
            }
        }
        let total_size = files
            .iter()
            .filter(|f| f.kind == FileType::File)
            .try_fold(0u64, |total, f| total.checked_add(f.size))
            .ok_or_else(|| CliprdrError::InvalidRequest {
                description: "clipboard file sizes overflow".to_owned(),
            })?;
        let destination = PasteDestination::new(&target_dir).map_err(paste_io_error)?;
        let mut task_handle = PasteTaskHandle {
            progress: PasteTaskProgress {
                list_index: -1,
                offset: 0,
                total_size,
                current_size: 0,
                last_sent_time: Instant::now(),
                download_file_index: Self::INVALID_FILE_INDEX,
                download_file_size: 0,
                download_file_current_size: 0,
                file_handle: None,
                error: None,
                is_canceled: false,
            },
            destination,
            files,
        };
        task_handle.update_next(0)?;
        if task_handle.is_finished() {
            task_handle.on_finished();
        } else {
            if let Err(e) = task_handle.send_file_contents_request() {
                log::error!("Failed to send file contents request, error: {}", &e);
                task_handle.on_error(e);
            }
        }
        *task_lock = Some(task_handle);
        Ok(())
    }

    pub fn cancel(&self) {
        let mut task_handle = self.handle.lock().unwrap();
        if let Some(task_handle) = task_handle.as_mut() {
            task_handle.progress.is_canceled = true;
            task_handle.on_cancelled();
        }
    }

    fn init_worker_thread(
        exit: Arc<Mutex<bool>>,
        handle: Arc<Mutex<Option<PasteTaskHandle>>>,
        rx_file_contents: Receiver<FileContentsResponse>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let mut retry_count = 0;
            loop {
                if *exit.lock().unwrap() {
                    break;
                }

                match rx_file_contents.recv_timeout(Duration::from_millis(300)) {
                    Ok(file_contents) => {
                        let mut task_lock = handle.lock().unwrap();
                        let Some(task_handle) = task_lock.as_mut() else {
                            continue;
                        };
                        if task_handle.is_finished() {
                            continue;
                        }

                        if file_contents.stream_id != task_handle.progress.list_index {
                            // ignore invalid stream id
                            continue;
                        } else if file_contents.msg_flags != 0x01 {
                            retry_count += 1;
                            if retry_count > RECV_RETRY_TIMES {
                                task_handle.progress.error = Some(CliprdrError::InvalidRequest {
                                    description: format!(
                                        "Failed to read file contents, stream id: {}, msg_flags: {}",
                                        file_contents.stream_id,
                                        file_contents.msg_flags
                                    ),
                                });
                            }
                        } else {
                            let resp_list_index = file_contents.stream_id;
                            let Some(file) = &task_handle.files.get(resp_list_index as usize)
                            else {
                                // unreachable
                                // Because `task_handle.progress.list_index >= task_handle.files.len()` should always be false
                                log::warn!(
                                    "Invalid response list index: {}, file length: {}",
                                    resp_list_index,
                                    task_handle.files.len()
                                );
                                continue;
                            };
                            if file.conn_id != file_contents.conn_id {
                                // unreachable
                                // We still add log here to make sure we can see the error message when it happens.
                                log::error!(
                                    "Invalid response conn id: {}, expected: {}",
                                    file_contents.conn_id,
                                    file.conn_id
                                );
                                continue;
                            }

                            if let Err(e) = task_handle.handle_file_contents_response(file_contents)
                            {
                                log::error!("Failed to handle file contents response: {}", &e);
                                task_handle.on_error(e);
                            }
                        }

                        if !task_handle.is_finished() {
                            if let Err(e) = task_handle.send_file_contents_request() {
                                log::error!("Failed to send file contents request: {}", &e);
                                task_handle.on_error(e);
                            }
                        } else {
                            retry_count = 0;
                            task_handle.on_finished();
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        let mut task_lock = handle.lock().unwrap();
                        if let Some(task_handle) = task_lock.as_mut() {
                            if task_handle.check_receive_timemout() {
                                retry_count = 0;
                                task_handle.on_finished();
                            }
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        break;
                    }
                }
            }
        })
    }

    pub fn is_finished(&self) -> bool {
        self.handle
            .lock()
            .unwrap()
            .as_ref()
            .map(|handle| handle.is_finished())
            .unwrap_or(true)
    }

    pub fn progress_percent(&self) -> Option<ProgressPercent> {
        self.handle
            .lock()
            .unwrap()
            .as_ref()
            .map(|handle| handle.progress_percent())
    }
}

impl PasteTaskHandle {
    fn update_next(&mut self, size: u64) -> Result<(), CliprdrError> {
        if self.is_finished() {
            return Ok(());
        }
        self.progress.current_size =
            self.progress
                .current_size
                .checked_add(size)
                .ok_or_else(|| CliprdrError::InvalidRequest {
                    description: "clipboard progress overflow".to_owned(),
                })?;

        let is_start = self.progress.list_index == -1;
        if is_start || (self.progress.offset + size) >= self.progress.download_file_size {
            if !is_start {
                self.on_done()?;
            }
            for i in (self.progress.list_index + 1)..self.files.len() as i32 {
                let Some(file_desc) = self.files.get(i as usize) else {
                    return Err(CliprdrError::InvalidRequest {
                        description: format!("Invalid file index: {}", i),
                    });
                };
                match file_desc.kind {
                    FileType::File => {
                        if file_desc.size == 0 {
                            let mut file = self
                                .destination
                                .create_file(&file_desc.name, 0)
                                .map_err(paste_io_error)?;
                            Self::set_file_metadata(file.writer.get_ref(), file_desc);
                            file.finish().map_err(paste_io_error)?;
                        } else {
                            self.progress.list_index = i;
                            self.progress.offset = 0;
                            self.open_new_writer()?;
                            break;
                        }
                    }
                    FileType::Directory => {
                        self.destination
                            .create_directory(&file_desc.name)
                            .map_err(paste_io_error)?;
                    }
                    FileType::Symlink => {
                        // to-do: handle symlink
                    }
                }
            }
        } else {
            self.progress.offset += size;
            self.progress.download_file_current_size += size;
            self.update_progress_completed(None);
        }
        if self.progress.file_handle.is_none() {
            self.progress.list_index = self.files.len() as i32;
            self.progress.offset = 0;
            self.progress.download_file_size = 0;
            self.progress.download_file_current_size = 0;
        }
        Ok(())
    }

    fn start_progress_completed(&self) {
        if let Some(file) = self.progress.file_handle.as_ref() {
            let creation_time =
                SystemTime::UNIX_EPOCH + Duration::from_secs(TIMESTAMP_FOR_FILE_PROGRESS_COMPLETED);
            file.writer
                .get_ref()
                .set_times(FileTimes::new().set_created(creation_time))
                .ok();
            file.writer
                .get_ref()
                .set_xattr(ATTR_PROGRESS_FRACTION_COMPLETED, b"0.0")
                .ok();
        }
    }

    fn update_progress_completed(&mut self, fraction_completed: Option<f64>) {
        let fraction_completed = fraction_completed.unwrap_or_else(|| {
            let current_size = self.progress.download_file_current_size as f64;
            let total_size = self.progress.download_file_size as f64;
            if total_size > 0.0 {
                current_size / total_size
            } else {
                1.0
            }
        });
        if let Some(file) = self.progress.file_handle.as_ref() {
            file.writer
                .get_ref()
                .set_xattr(
                    ATTR_PROGRESS_FRACTION_COMPLETED,
                    fraction_completed.to_string().as_bytes(),
                )
                .ok();
        }
    }

    fn open_new_writer(&mut self) -> Result<(), CliprdrError> {
        let file = self
            .files
            .get(self.progress.list_index as usize)
            .ok_or_else(|| CliprdrError::InvalidRequest {
                description: "invalid clipboard file index".to_owned(),
            })?;
        let pending = self
            .destination
            .create_file(&file.name, BLOCK_SIZE as usize * 2)
            .map_err(paste_io_error)?;
        self.progress.download_file_index = self.progress.list_index;
        self.progress.download_file_size = file.size;
        self.progress.download_file_current_size = 0;
        self.progress.file_handle = Some(pending);
        self.start_progress_completed();
        Ok(())
    }

    fn progress_percent(&self) -> ProgressPercent {
        let percent = if self.progress.total_size == 0 {
            1.0
        } else {
            self.progress.current_size as f64 / self.progress.total_size as f64
        };
        ProgressPercent {
            percent,
            is_canceled: self.progress.is_canceled,
            is_failed: self.progress.error.is_some(),
        }
    }

    fn is_finished(&self) -> bool {
        self.progress.is_canceled
            || self.progress.error.is_some()
            || self.progress.list_index >= self.files.len() as i32
    }

    fn check_receive_timemout(&mut self) -> bool {
        if !self.is_finished() {
            if self.progress.last_sent_time.elapsed() > RECEIVE_WAIT_TIMEOUT {
                self.progress.error = Some(CliprdrError::InvalidRequest {
                    description: "Failed to read file contents".to_string(),
                });
                return true;
            }
        }
        false
    }

    fn on_finished(&mut self) {
        if self.progress.error.is_some() || self.progress.is_canceled {
            self.on_cancelled();
        } else if let Err(error) = self.on_done() {
            self.on_error(error);
        }
        if self.progress.current_size != self.progress.total_size {
            self.progress.error = Some(CliprdrError::InvalidRequest {
                description: "Failed to download all files".to_string(),
            });
        }
    }

    fn on_error(&mut self, error: CliprdrError) {
        self.progress.error = Some(error);
        self.on_cancelled();
    }

    fn on_cancelled(&mut self) {
        // Dropping the handle removes only its own unfinished temporary file.
        self.progress.file_handle = None;
        self.progress.download_file_index = PasteTask::INVALID_FILE_INDEX;
    }

    fn on_done(&mut self) -> Result<(), CliprdrError> {
        let Some(mut pending) = self.progress.file_handle.take() else {
            return Ok(());
        };
        let file_desc = self
            .files
            .get(self.progress.download_file_index as usize)
            .ok_or_else(|| CliprdrError::InvalidRequest {
                description: "invalid completed clipboard file index".to_owned(),
            })?;
        pending.writer.flush().map_err(paste_io_error)?;
        pending
            .writer
            .get_ref()
            .remove_xattr(ATTR_PROGRESS_FRACTION_COMPLETED)
            .ok();
        Self::set_file_metadata(pending.writer.get_ref(), file_desc);
        pending.finish().map_err(paste_io_error)?;
        self.progress.download_file_index = PasteTask::INVALID_FILE_INDEX;
        Ok(())
    }

    #[inline]
    fn set_file_metadata(f: &File, file_desc: &FileDescription) {
        let times = FileTimes::new()
            .set_accessed(file_desc.atime)
            .set_modified(file_desc.last_modified)
            .set_created(file_desc.creation_time);
        f.set_times(times).ok();
    }

    fn send_file_contents_request(&mut self) -> Result<(), CliprdrError> {
        if self.is_finished() {
            return Ok(());
        }

        let stream_id = self.progress.list_index;
        let list_index = self.progress.list_index;
        let Some(file) = &self.files.get(list_index as usize) else {
            // unreachable
            return Err(CliprdrError::InvalidRequest {
                description: format!("Invalid file index: {}", list_index),
            });
        };
        let cb_requested = min(BLOCK_SIZE as u64, file.size - self.progress.offset);
        let conn_id = file.conn_id;

        let (n_position_high, n_position_low) = (
            (self.progress.offset >> 32) as i32,
            (self.progress.offset & (u32::MAX as u64)) as i32,
        );
        let request = ClipboardFile::FileContentsRequest {
            stream_id,
            list_index,
            dw_flags: 2,
            n_position_low,
            n_position_high,
            cb_requested: cb_requested as _,
            have_clip_data_id: false,
            clip_data_id: 0,
        };
        send_data(conn_id, request)?;
        self.progress.last_sent_time = Instant::now();

        Ok(())
    }

    fn handle_file_contents_response(
        &mut self,
        file_contents: FileContentsResponse,
    ) -> Result<(), CliprdrError> {
        let descriptor = self
            .files
            .get(self.progress.list_index as usize)
            .ok_or_else(|| CliprdrError::InvalidRequest {
                description: "invalid clipboard response file index".to_owned(),
            })?;
        if file_contents.conn_id != descriptor.conn_id
            || file_contents.stream_id != self.progress.list_index
            || file_contents.msg_flags != 1
        {
            return Err(CliprdrError::InvalidRequest {
                description: "clipboard response does not match request".to_owned(),
            });
        }
        let data = file_contents.requested_data.as_slice();
        let remaining = self
            .progress
            .download_file_size
            .checked_sub(self.progress.offset)
            .ok_or_else(|| CliprdrError::InvalidRequest {
                description: "invalid clipboard file offset".to_owned(),
            })?;
        if data.is_empty() || data.len() as u64 > min(BLOCK_SIZE as u64, remaining) {
            return Err(CliprdrError::InvalidRequest {
                description: "invalid clipboard file response length".to_owned(),
            });
        }
        let file =
            self.progress
                .file_handle
                .as_mut()
                .ok_or_else(|| CliprdrError::InvalidRequest {
                    description: "clipboard file is not open".to_owned(),
                })?;
        file.writer.write_all(data).map_err(paste_io_error)?;
        self.update_next(data.len() as u64)
    }
}

fn paste_io_error(error: std::io::Error) -> CliprdrError {
    CliprdrError::FileError {
        path: "clipboard destination".to_owned(),
        err: error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("hdobby-paste-task-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn description(name: &str, kind: FileType, size: u64) -> FileDescription {
        FileDescription {
            conn_id: 1,
            name: name.into(),
            kind,
            size,
            perm: 0o600,
            atime: SystemTime::UNIX_EPOCH,
            last_modified: SystemTime::UNIX_EPOCH,
            last_metadata_changed: SystemTime::UNIX_EPOCH,
            creation_time: SystemTime::UNIX_EPOCH,
        }
    }

    fn handle(scratch: &Scratch, files: Vec<FileDescription>) -> PasteTaskHandle {
        PasteTaskHandle {
            destination: PasteDestination::new(&scratch.0).unwrap(),
            progress: PasteTaskProgress {
                list_index: -1,
                offset: 0,
                total_size: files
                    .iter()
                    .filter(|f| f.kind == FileType::File)
                    .map(|f| f.size)
                    .sum(),
                current_size: 0,
                last_sent_time: Instant::now(),
                download_file_index: -1,
                download_file_size: 0,
                download_file_current_size: 0,
                file_handle: None,
                error: None,
                is_canceled: false,
            },
            files,
        }
    }

    fn response(index: i32, bytes: &[u8]) -> FileContentsResponse {
        FileContentsResponse {
            conn_id: 1,
            msg_flags: 1,
            stream_id: index,
            requested_data: bytes.to_vec(),
        }
    }

    #[test]
    fn receives_nested_files_in_chunks_and_preserves_existing_files() {
        let scratch = Scratch::new();
        std::fs::create_dir(scratch.0.join("한글")).unwrap();
        std::fs::write(scratch.0.join("한글/🙂.txt"), b"original").unwrap();
        let mut task = handle(
            &scratch,
            vec![
                description("한글", FileType::Directory, 4096),
                description("한글/🙂.txt", FileType::File, 6),
                description("한글/empty", FileType::File, 0),
            ],
        );
        task.update_next(0).unwrap();
        task.handle_file_contents_response(response(1, b"abc"))
            .unwrap();
        assert!(!task.is_finished());
        assert_eq!(task.progress.offset, 3);
        task.handle_file_contents_response(response(1, b"123"))
            .unwrap();
        task.on_finished();
        assert!(task.is_finished());
        assert!(task.progress.error.is_none());
        assert_eq!(task.progress_percent().percent, 1.0);
        assert_eq!(
            std::fs::read(scratch.0.join("한글/🙂.txt")).unwrap(),
            b"original"
        );
        assert_eq!(
            std::fs::read(scratch.0.join("한글/🙂 (1).txt")).unwrap(),
            b"abc123"
        );
        assert_eq!(
            std::fs::metadata(scratch.0.join("한글/empty"))
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            std::fs::read_dir(scratch.0.join("한글")).unwrap().count(),
            3
        );
    }

    #[test]
    fn rejects_empty_oversized_and_unrelated_response_before_writing() {
        let scratch = Scratch::new();
        let mut task = handle(&scratch, vec![description("result", FileType::File, 3)]);
        task.update_next(0).unwrap();
        assert!(task
            .handle_file_contents_response(response(0, b""))
            .is_err());
        assert!(task
            .handle_file_contents_response(response(0, b"toolong"))
            .is_err());
        let mut wrong = response(0, b"abc");
        wrong.conn_id = 2;
        assert!(task.handle_file_contents_response(wrong).is_err());
        assert!(task
            .handle_file_contents_response(response(1, b"abc"))
            .is_err());
        assert_eq!(task.progress.current_size, 0);
        task.handle_file_contents_response(response(0, b"abc"))
            .unwrap();
        task.on_finished();
        assert_eq!(std::fs::read(scratch.0.join("result")).unwrap(), b"abc");
    }

    #[test]
    fn cancelled_and_timed_out_pastes_remove_partial_files() {
        let scratch = Scratch::new();
        for cancel in [true, false] {
            let mut task = handle(&scratch, vec![description("partial", FileType::File, 5)]);
            task.update_next(0).unwrap();
            task.handle_file_contents_response(response(0, b"ab"))
                .unwrap();
            if cancel {
                task.progress.is_canceled = true;
            } else {
                task.progress.last_sent_time =
                    Instant::now() - RECEIVE_WAIT_TIMEOUT - Duration::from_secs(1);
                assert!(task.check_receive_timemout());
            }
            task.on_finished();
            assert!(task.is_finished());
            assert_eq!(std::fs::read_dir(&scratch.0).unwrap().count(), 0);
        }
    }

    #[test]
    fn rejects_total_size_overflow_before_creating_files() {
        let scratch = Scratch::new();
        let (_tx, rx) = std::sync::mpsc::channel();
        let mut task = PasteTask::new(rx);
        assert!(task
            .start(
                scratch.0.clone(),
                vec![
                    description("a", FileType::File, u64::MAX),
                    description("b", FileType::File, 1)
                ]
            )
            .is_err());
        assert_eq!(std::fs::read_dir(&scratch.0).unwrap().count(), 0);
    }
}
