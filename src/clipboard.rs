#[cfg(not(target_os = "android"))]
use arboard::{ClipboardData, ClipboardFormat};
#[cfg(target_os = "linux")]
use arboard::{LinuxClipboardKind, SetExtLinux};
use hbb_common::{bail, log, message_proto::*, ResultType};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

pub const CLIPBOARD_NAME: &'static str = "clipboard";
#[cfg(feature = "unix-file-copy-paste")]
pub const FILE_CLIPBOARD_NAME: &'static str = "file-clipboard";
pub const CLIPBOARD_INTERVAL: u64 = 333;

// This format is used to store the flag in the clipboard.
const HDOBBYDESK_CLIPBOARD_OWNER_FORMAT: &'static str = "dyn.com.hdobbydesk.owner";

// Add special format for Excel XML Spreadsheet
#[cfg(target_os = "windows")]
const CLIPBOARD_FORMAT_EXCEL_XML_SPREADSHEET: &'static str = "XML Spreadsheet";

#[cfg(not(target_os = "android"))]
lazy_static::lazy_static! {
    static ref ARBOARD_MTX: Arc<Mutex<()>> = Arc::new(Mutex::new(()));
    // cache the clipboard msg
    static ref LAST_MULTI_CLIPBOARDS: Arc<Mutex<MultiClipboards>> = Arc::new(Mutex::new(MultiClipboards::new()));
    // For updating in server and getting content in cm.
    // Clipboard on Linux is "server--clients" mode.
    // The clipboard content is owned by the server and passed to the clients when requested.
    // Plain text is the only exception, it does not require the server to be present.
    static ref CLIPBOARD_CTX: Arc<Mutex<Option<ClipboardContext>>> = Arc::new(Mutex::new(None));
}

#[cfg(not(target_os = "android"))]
const CLIPBOARD_GET_MAX_RETRY: usize = 3;
#[cfg(not(target_os = "android"))]
const CLIPBOARD_GET_RETRY_INTERVAL_DUR: Duration = Duration::from_millis(33);

#[cfg(not(target_os = "android"))]
const SUPPORTED_FORMATS: &[ClipboardFormat] = &[
    ClipboardFormat::Text,
    ClipboardFormat::Html,
    ClipboardFormat::Rtf,
    ClipboardFormat::ImageRgba,
    ClipboardFormat::ImagePng,
    ClipboardFormat::ImageSvg,
    #[cfg(feature = "unix-file-copy-paste")]
    ClipboardFormat::FileUrl,
    // This is a Windows registered format, not a valid macOS pasteboard UTI.
    #[cfg(target_os = "windows")]
    ClipboardFormat::Special(CLIPBOARD_FORMAT_EXCEL_XML_SPREADSHEET),
    ClipboardFormat::Special(HDOBBYDESK_CLIPBOARD_OWNER_FORMAT),
];

#[cfg(not(target_os = "android"))]
pub fn check_clipboard(
    ctx: &mut Option<ClipboardContext>,
    side: ClipboardSide,
    force: bool,
) -> Option<Message> {
    let (msg, clipboards) = read_clipboard_message(ctx, side, force)?;
    *LAST_MULTI_CLIPBOARDS.lock().unwrap() = clipboards;
    Some(msg)
}

#[cfg(target_os = "linux")]
pub fn peek_clipboard(
    ctx: &mut Option<ClipboardContext>,
    side: ClipboardSide,
    force: bool,
) -> Option<Message> {
    let (msg, _) = read_clipboard_message(ctx, side, force)?;
    Some(msg)
}

#[cfg(not(target_os = "android"))]
fn read_clipboard_message(
    ctx: &mut Option<ClipboardContext>,
    side: ClipboardSide,
    force: bool,
) -> Option<(Message, MultiClipboards)> {
    if ctx.is_none() {
        *ctx = ClipboardContext::new().ok();
    }
    let ctx2 = ctx.as_mut()?;
    match ctx2.get(side, force) {
        Ok(content) => {
            if !content.is_empty() {
                let mut msg = Message::new();
                let clipboards = proto::create_multi_clipboards(content);
                msg.set_multi_clipboards(clipboards.clone());
                return Some((msg, clipboards));
            }
        }
        Err(e) => {
            log::error!("Failed to get clipboard content. {}", e);
        }
    }
    None
}

#[cfg(all(feature = "unix-file-copy-paste", target_os = "macos"))]
pub fn is_file_url_set_by_hdobbydesk(url: &Vec<String>) -> bool {
    if url.len() != 1 {
        return false;
    }
    url.iter()
        .next()
        .map(|s| {
            for prefix in &["file:///tmp/.hdobbydesk_", "//tmp/.hdobbydesk_"] {
                if s.starts_with(prefix) {
                    return s[prefix.len()..].parse::<uuid::Uuid>().is_ok();
                }
            }
            false
        })
        .unwrap_or(false)
}

#[cfg(feature = "unix-file-copy-paste")]
pub fn check_clipboard_files(
    ctx: &mut Option<ClipboardContext>,
    side: ClipboardSide,
    force: bool,
) -> Option<Vec<String>> {
    if ctx.is_none() {
        *ctx = ClipboardContext::new().ok();
    }
    let ctx2 = ctx.as_mut()?;
    match ctx2.get_files(side, force) {
        Ok(Some(urls)) => {
            if !urls.is_empty() {
                return Some(urls);
            }
        }
        Err(e) => {
            log::error!("Failed to get clipboard file urls. {}", e);
        }
        _ => {}
    }
    None
}

#[cfg(all(target_os = "linux", feature = "unix-file-copy-paste"))]
pub fn update_clipboard_files(files: Vec<String>, side: ClipboardSide) {
    if !files.is_empty() {
        std::thread::spawn(move || {
            do_update_clipboard_(vec![ClipboardData::FileUrl(files)], side);
        });
    }
}

#[cfg(feature = "unix-file-copy-paste")]
pub fn try_empty_clipboard_files(_side: ClipboardSide, _conn_id: i32) {
    std::thread::spawn(move || {
        if let Err(e) = try_empty_clipboard_files_sync(_side, _conn_id) {
            log::error!("Failed to empty clipboard files: {}", e);
        }
    });
}

#[cfg(feature = "unix-file-copy-paste")]
pub fn try_empty_clipboard_files_sync(_side: ClipboardSide, _conn_id: i32) -> ResultType<()> {
    let mut ctx = CLIPBOARD_CTX.lock().unwrap();
    if ctx.is_none() {
        match ClipboardContext::new() {
            Ok(x) => {
                *ctx = Some(x);
            }
            Err(e) => {
                log::error!("Failed to create clipboard context: {}", e);
                bail!("Failed to create clipboard context: {}", e);
            }
        }
    }
    #[allow(unused_mut)]
    if let Some(mut ctx) = ctx.as_mut() {
        #[cfg(target_os = "linux")]
        {
            use clipboard::platform::unix;
            if unix::fuse::empty_local_files(_side == ClipboardSide::Client, _conn_id) {
                ctx.try_empty_clipboard_files(_side);
            }
        }
        #[cfg(target_os = "macos")]
        {
            ctx.try_empty_clipboard_files(_side);
            // No need to make sure the context is enabled.
            clipboard::ContextSend::proc(|context| -> ResultType<()> {
                if !context.empty_clipboard(_conn_id)? {
                    bail!("Failed to empty clipboard files for conn_id {}", _conn_id);
                }
                Ok(())
            })?;
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn try_empty_clipboard_files(side: ClipboardSide, conn_id: i32) {
    log::debug!("try to empty {} cliprdr for conn_id {}", side, conn_id);
    let _ = clipboard::ContextSend::proc(|context| -> ResultType<()> {
        context.empty_clipboard(conn_id)?;
        Ok(())
    });
}

#[cfg(target_os = "windows")]
pub fn check_clipboard_cm() -> ResultType<MultiClipboards> {
    let mut ctx = CLIPBOARD_CTX.lock().unwrap();
    if ctx.is_none() {
        match ClipboardContext::new() {
            Ok(x) => {
                *ctx = Some(x);
            }
            Err(e) => {
                hbb_common::bail!("Failed to create clipboard context: {}", e);
            }
        }
    }
    if let Some(ctx) = ctx.as_mut() {
        let content = ctx.get(ClipboardSide::Host, false)?;
        let clipboards = proto::create_multi_clipboards(content);
        Ok(clipboards)
    } else {
        hbb_common::bail!("Failed to create clipboard context");
    }
}

#[cfg(not(target_os = "android"))]
fn update_clipboard_(multi_clipboards: Vec<Clipboard>, side: ClipboardSide) {
    let to_update_data = proto::from_multi_clipboards(multi_clipboards);
    if to_update_data.is_empty() {
        return;
    }
    do_update_clipboard_(to_update_data, side);
}

#[cfg(not(target_os = "android"))]
fn do_update_clipboard_(mut to_update_data: Vec<ClipboardData>, side: ClipboardSide) {
    let mut ctx = CLIPBOARD_CTX.lock().unwrap();
    if ctx.is_none() {
        match ClipboardContext::new() {
            Ok(x) => {
                *ctx = Some(x);
            }
            Err(e) => {
                log::error!("Failed to create clipboard context: {}", e);
                return;
            }
        }
    }
    if let Some(ctx) = ctx.as_mut() {
        to_update_data = append_owner_marker(to_update_data, side);
        if let Err(e) = ctx.set(&to_update_data) {
            log::debug!("Failed to set clipboard: {}", e);
        } else {
            log::debug!("{} updated on {}", CLIPBOARD_NAME, side);
        }
    }
}

#[cfg(not(target_os = "android"))]
fn append_owner_marker(mut data: Vec<ClipboardData>, side: ClipboardSide) -> Vec<ClipboardData> {
    data.push(ClipboardData::Special((
        HDOBBYDESK_CLIPBOARD_OWNER_FORMAT.to_owned(),
        side.get_owner_data(),
    )));
    data
}

#[cfg(target_os = "linux")]
pub fn set_text_clipboard_with_owner_sync(text: &str, side: ClipboardSide) -> ResultType<()> {
    let mut ctx = CLIPBOARD_CTX.lock().unwrap();
    if ctx.is_none() {
        *ctx = Some(ClipboardContext::new()?);
    }
    let clipboard_ctx = match ctx.as_mut() {
        Some(ctx) => ctx,
        None => bail!("Failed to create clipboard context"),
    };
    let data = append_owner_marker(vec![ClipboardData::Text(text.to_owned())], side);
    clipboard_ctx.set_with_owner_marker_for_linux(&data)
}

#[cfg(not(target_os = "android"))]
pub fn update_clipboard(multi_clipboards: Vec<Clipboard>, side: ClipboardSide) {
    std::thread::spawn(move || {
        update_clipboard_(multi_clipboards, side);
    });
}

#[cfg(not(target_os = "android"))]
pub struct ClipboardContext {
    inner: arboard::Clipboard,
}

#[cfg(not(target_os = "android"))]
#[allow(unreachable_code)]
impl ClipboardContext {
    pub fn new() -> ResultType<ClipboardContext> {
        let board;
        #[cfg(not(target_os = "linux"))]
        {
            board = arboard::Clipboard::new()?;
        }
        #[cfg(target_os = "linux")]
        {
            let mut i = 1;
            loop {
                // Try 5 times to create clipboard
                // Arboard::new() connect to X server or Wayland compositor, which should be OK most times
                // But sometimes, the connection may fail, so we retry here.
                match arboard::Clipboard::new() {
                    Ok(x) => {
                        board = x;
                        break;
                    }
                    Err(e) => {
                        if i == 5 {
                            return Err(e.into());
                        } else {
                            std::thread::sleep(std::time::Duration::from_millis(30 * i));
                        }
                    }
                }
                i += 1;
            }
        }

        Ok(ClipboardContext { inner: board })
    }

    fn get_formats(&mut self, formats: &[ClipboardFormat]) -> ResultType<Vec<ClipboardData>> {
        // If there're multiple threads or processes trying to access the clipboard at the same time,
        // the previous clipboard owner will fail to access the clipboard.
        // `GetLastError()` will return `ERROR_CLIPBOARD_NOT_OPEN` (OSError(1418): Thread does not have a clipboard open) at this time.
        // See https://github.com/rustdesk-org/arboard/blob/747ab2d9b40a5c9c5102051cf3b0bb38b4845e60/src/platform/windows.rs#L34
        //
        // This is a common case on Windows, so we retry here.
        // Related issues:
        // https://github.com/rustdesk/rustdesk/issues/9263
        // https://github.com/rustdesk/rustdesk/issues/9222#issuecomment-2329233175
        for i in 0..CLIPBOARD_GET_MAX_RETRY {
            match self.inner.get_formats(formats) {
                Ok(data) => {
                    return Ok(data
                        .into_iter()
                        .filter(|c| !matches!(c, arboard::ClipboardData::None))
                        .collect())
                }
                Err(e) => match e {
                    arboard::Error::ClipboardOccupied => {
                        log::debug!("Failed to get clipboard formats, clipboard is occupied, retrying... {}", i + 1);
                        std::thread::sleep(CLIPBOARD_GET_RETRY_INTERVAL_DUR);
                    }
                    _ => {
                        log::error!("Failed to get clipboard formats, {}", e);
                        return Err(e.into());
                    }
                },
            }
        }
        bail!("Failed to get clipboard formats, clipboard is occupied, {CLIPBOARD_GET_MAX_RETRY} retries failed");
    }

    pub fn get(&mut self, side: ClipboardSide, force: bool) -> ResultType<Vec<ClipboardData>> {
        let data = self.get_formats_filter(SUPPORTED_FORMATS, side, force)?;
        // We have a separate service named `file-clipboard` to handle file copy-paste.
        // We need to read the file urls because file copy may set the other clipboard formats such as text.
        #[cfg(feature = "unix-file-copy-paste")]
        {
            if data.iter().any(|c| matches!(c, ClipboardData::FileUrl(_))) {
                return Ok(vec![]);
            }
        }
        Ok(data)
    }

    fn get_formats_filter(
        &mut self,
        formats: &[ClipboardFormat],
        side: ClipboardSide,
        force: bool,
    ) -> ResultType<Vec<ClipboardData>> {
        let _lock = ARBOARD_MTX.lock().unwrap();
        let data = self.get_formats(formats)?;
        if data.is_empty() {
            return Ok(data);
        }
        if !force {
            for c in data.iter() {
                if let ClipboardData::Special((s, d)) = c {
                    if s == HDOBBYDESK_CLIPBOARD_OWNER_FORMAT && side.is_owner(d) {
                        return Ok(vec![]);
                    }
                }
            }
        }
        Ok(data
            .into_iter()
            .filter(|c| match c {
                ClipboardData::Special((s, _)) => s != HDOBBYDESK_CLIPBOARD_OWNER_FORMAT,
                // Skip synchronizing empty text to the remote clipboard
                ClipboardData::Text(text) => !text.is_empty(),
                _ => true,
            })
            .collect())
    }

    #[cfg(feature = "unix-file-copy-paste")]
    pub fn get_files(
        &mut self,
        side: ClipboardSide,
        force: bool,
    ) -> ResultType<Option<Vec<String>>> {
        let data = self.get_formats_filter(
            &[
                ClipboardFormat::FileUrl,
                ClipboardFormat::Special(HDOBBYDESK_CLIPBOARD_OWNER_FORMAT),
            ],
            side,
            force,
        )?;
        Ok(data.into_iter().find_map(|c| match c {
            ClipboardData::FileUrl(urls) => Some(urls),
            _ => None,
        }))
    }

    fn set(&mut self, data: &[ClipboardData]) -> ResultType<()> {
        let _lock = ARBOARD_MTX.lock().unwrap();
        self.inner.set_formats(data)?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn set_with_owner_marker_for_linux(&mut self, data: &[ClipboardData]) -> ResultType<()> {
        let _lock = ARBOARD_MTX.lock().unwrap();
        self.inner
            .set()
            .clipboard(LinuxClipboardKind::Clipboard)
            .formats(data)?;
        if let Err(e) = self
            .inner
            .set()
            .clipboard(LinuxClipboardKind::Primary)
            .formats(data)
        {
            log::warn!("Failed to set PRIMARY clipboard with owner marker: {}", e);
        }
        Ok(())
    }

    #[cfg(all(feature = "unix-file-copy-paste", target_os = "macos"))]
    fn get_file_urls_set_by_hdobbydesk(
        data: Vec<ClipboardData>,
        _side: ClipboardSide,
    ) -> Vec<String> {
        for item in data.into_iter() {
            if let ClipboardData::FileUrl(urls) = item {
                if is_file_url_set_by_hdobbydesk(&urls) {
                    return urls;
                }
            }
        }
        vec![]
    }

    #[cfg(all(feature = "unix-file-copy-paste", target_os = "linux"))]
    fn get_file_urls_set_by_hdobbydesk(data: Vec<ClipboardData>, side: ClipboardSide) -> Vec<String> {
        let exclude_path =
            clipboard::platform::unix::fuse::get_exclude_paths(side == ClipboardSide::Client);
        data.into_iter()
            .filter_map(|c| match c {
                ClipboardData::FileUrl(urls) => Some(
                    urls.into_iter()
                        .filter(|s| s.starts_with(&*exclude_path))
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .flatten()
            .collect::<Vec<_>>()
    }

    #[cfg(feature = "unix-file-copy-paste")]
    fn try_empty_clipboard_files(&mut self, side: ClipboardSide) {
        let _lock = ARBOARD_MTX.lock().unwrap();
        if let Ok(data) = self.get_formats(&[ClipboardFormat::FileUrl]) {
            let urls = Self::get_file_urls_set_by_hdobbydesk(data, side);
            if !urls.is_empty() {
                // FIXME:
                // The host-side clear file clipboard `let _ = self.inner.clear();`,
                // does not work on KDE Plasma for the installed version.

                // Don't use `hbb_common::platform::linux::is_kde()` here.
                // It's not correct in the server process.
                #[cfg(target_os = "linux")]
                let is_kde_x11 = hbb_common::platform::linux::is_kde_session()
                    && crate::platform::linux::is_x11();
                #[cfg(target_os = "macos")]
                let is_kde_x11 = false;
                let clear_holder_text = if is_kde_x11 {
                    "HdobbyDesk placeholder to clear the file clipboard"
                } else {
                    ""
                }
                .to_string();
                self.inner
                    .set_formats(&[
                        ClipboardData::Text(clear_holder_text),
                        ClipboardData::Special((
                            HDOBBYDESK_CLIPBOARD_OWNER_FORMAT.to_owned(),
                            side.get_owner_data(),
                        )),
                    ])
                    .ok();
            }
        }
    }
}

pub fn is_support_multi_clipboard(peer_version: &str, peer_platform: &str) -> bool {
    use hbb_common::get_version_number;
    if get_version_number(peer_version) < get_version_number("1.3.0") {
        return false;
    }
    if ["", &hbb_common::whoami::Platform::Ios.to_string()].contains(&peer_platform) {
        return false;
    }
    if "Android" == peer_platform && get_version_number(peer_version) < get_version_number("1.3.3")
    {
        return false;
    }
    true
}

#[cfg(not(target_os = "android"))]
pub fn get_current_clipboard_msg(
    peer_version: &str,
    peer_platform: &str,
    side: ClipboardSide,
) -> Option<Message> {
    let mut multi_clipboards = LAST_MULTI_CLIPBOARDS.lock().unwrap();
    if multi_clipboards.clipboards.is_empty() {
        let mut ctx = ClipboardContext::new().ok()?;
        *multi_clipboards = proto::create_multi_clipboards(ctx.get(side, true).ok()?);
    }
    if multi_clipboards.clipboards.is_empty() {
        return None;
    }

    if is_support_multi_clipboard(peer_version, peer_platform) {
        let mut msg = Message::new();
        msg.set_multi_clipboards(multi_clipboards.clone());
        Some(msg)
    } else {
        // Find the first text clipboard and send it.
        multi_clipboards
            .clipboards
            .iter()
            .find(|c| c.format.enum_value() == Ok(hbb_common::message_proto::ClipboardFormat::Text))
            .map(|c| {
                let mut msg = Message::new();
                msg.set_clipboard(c.clone());
                msg
            })
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum ClipboardSide {
    Host,
    Client,
}

impl ClipboardSide {
    // 01: the clipboard is owned by the host
    // 10: the clipboard is owned by the client
    fn get_owner_data(&self) -> Vec<u8> {
        match self {
            ClipboardSide::Host => vec![0b01],
            ClipboardSide::Client => vec![0b10],
        }
    }

    fn is_owner(&self, data: &[u8]) -> bool {
        if data.len() == 0 {
            return false;
        }
        data[0] & 0b11 != 0
    }
}

impl std::fmt::Display for ClipboardSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipboardSide::Host => write!(f, "host"),
            ClipboardSide::Client => write!(f, "client"),
        }
    }
}

pub use proto::get_msg_if_not_support_multi_clip;
mod proto {
    #[cfg(not(target_os = "android"))]
    use arboard::ClipboardData;
    use hbb_common::{
        compress::{compress as compress_func, decompress_limited},
        message_proto::{Clipboard, ClipboardFormat, Message, MultiClipboards},
    };

    const MAX_REPRESENTATIONS: usize = 16;
    const MAX_REPRESENTATION_BYTES: usize = 64 * 1024 * 1024;
    // Leave room for protobuf fields under the Android 80 MiB message limit.
    const MAX_TOTAL_BYTES: usize = 80 * 1024 * 1024 - 128 * 1024;

    /// Validate the complete update before touching the OS clipboard or Android JNI.
    /// A bad representation must not turn into empty text or a partial update.
    pub(super) fn decode_multi_clipboards(mut clips: Vec<Clipboard>) -> Option<Vec<Clipboard>> {
        if clips.len() > MAX_REPRESENTATIONS {
            return None;
        }
        let mut remaining = MAX_TOTAL_BYTES;
        for clip in &mut clips {
            // Unused wire fields must not bypass the budget when sent through JNI.
            clip.special_fields.clear();
            let limit = remaining.min(MAX_REPRESENTATION_BYTES);
            if clip.content.len() > MAX_REPRESENTATION_BYTES || clip.special_name.len() > 4096 {
                return None;
            }
            if clip.compress {
                clip.content = decompress_limited(&clip.content, limit).ok()?.into();
                clip.compress = false;
            } else if clip.content.len() > limit {
                return None;
            }
            remaining = remaining.checked_sub(clip.content.len())?;
            match clip.format.enum_value() {
                Ok(ClipboardFormat::ImageRgba) => {
                    let width = usize::try_from(clip.width).ok()?;
                    let height = usize::try_from(clip.height).ok()?;
                    let required = width.checked_mul(height)?.checked_mul(4)?;
                    if width == 0 || height == 0 || required != clip.content.len() {
                        return None;
                    }
                }
                Ok(
                    ClipboardFormat::Text
                    | ClipboardFormat::Html
                    | ClipboardFormat::Rtf
                    | ClipboardFormat::ImageSvg,
                ) => {
                    std::str::from_utf8(&clip.content).ok()?;
                }
                _ => {}
            }
        }
        Some(clips)
    }

    fn plain_to_proto(s: String, format: ClipboardFormat) -> Clipboard {
        let compressed = compress_func(s.as_bytes());
        let compress = compressed.len() < s.as_bytes().len();
        let content = if compress {
            compressed
        } else {
            s.bytes().collect::<Vec<u8>>()
        };
        Clipboard {
            compress,
            content: content.into(),
            format: format.into(),
            ..Default::default()
        }
    }

    #[cfg(not(target_os = "android"))]
    fn image_to_proto(a: arboard::ImageData) -> Clipboard {
        match &a {
            arboard::ImageData::Rgba(rgba) => {
                let compressed = compress_func(&a.bytes());
                let compress = compressed.len() < a.bytes().len();
                let content = if compress {
                    compressed
                } else {
                    a.bytes().to_vec()
                };
                Clipboard {
                    compress,
                    content: content.into(),
                    width: rgba.width as _,
                    height: rgba.height as _,
                    format: ClipboardFormat::ImageRgba.into(),
                    ..Default::default()
                }
            }
            arboard::ImageData::Png(png) => Clipboard {
                compress: false,
                content: png.to_owned().to_vec().into(),
                format: ClipboardFormat::ImagePng.into(),
                ..Default::default()
            },
            arboard::ImageData::Svg(_) => {
                let compressed = compress_func(&a.bytes());
                let compress = compressed.len() < a.bytes().len();
                let content = if compress {
                    compressed
                } else {
                    a.bytes().to_vec()
                };
                Clipboard {
                    compress,
                    content: content.into(),
                    format: ClipboardFormat::ImageSvg.into(),
                    ..Default::default()
                }
            }
        }
    }

    fn special_to_proto(d: Vec<u8>, s: String) -> Clipboard {
        let compressed = compress_func(&d);
        let compress = compressed.len() < d.len();
        let content = if compress {
            compressed
        } else {
            d
        };
        Clipboard {
            compress,
            content: content.into(),
            format: ClipboardFormat::Special.into(),
            special_name: s,
            ..Default::default()
        }
    }

    #[cfg(not(target_os = "android"))]
    fn clipboard_data_to_proto(data: ClipboardData) -> Option<Clipboard> {
        let d = match data {
            ClipboardData::Text(s) => plain_to_proto(s, ClipboardFormat::Text),
            ClipboardData::Rtf(s) => plain_to_proto(s, ClipboardFormat::Rtf),
            ClipboardData::Html(s) => plain_to_proto(s, ClipboardFormat::Html),
            ClipboardData::Image(a) => image_to_proto(a),
            ClipboardData::Special((s, d)) => special_to_proto(d, s),
            _ => return None,
        };
        Some(d)
    }

    #[cfg(not(target_os = "android"))]
    pub fn create_multi_clipboards(vec_data: Vec<ClipboardData>) -> MultiClipboards {
        MultiClipboards {
            clipboards: vec_data
                .into_iter()
                .filter_map(clipboard_data_to_proto)
                .collect(),
            ..Default::default()
        }
    }

    #[cfg(not(target_os = "android"))]
    fn from_clipboard(clipboard: Clipboard) -> Option<ClipboardData> {
        let data: Vec<u8> = clipboard.content.into();
        match clipboard.format.enum_value() {
            Ok(ClipboardFormat::Text) => String::from_utf8(data).ok().map(ClipboardData::Text),
            Ok(ClipboardFormat::Rtf) => String::from_utf8(data).ok().map(ClipboardData::Rtf),
            Ok(ClipboardFormat::Html) => String::from_utf8(data).ok().map(ClipboardData::Html),
            Ok(ClipboardFormat::ImageRgba) => Some(ClipboardData::Image(arboard::ImageData::rgba(
                clipboard.width as _,
                clipboard.height as _,
                data.into(),
            ))),
            Ok(ClipboardFormat::ImagePng) => {
                Some(ClipboardData::Image(arboard::ImageData::png(data.into())))
            }
            Ok(ClipboardFormat::ImageSvg) => Some(ClipboardData::Image(arboard::ImageData::svg(
                std::str::from_utf8(&data).unwrap_or_default(),
            ))),
            Ok(ClipboardFormat::Special) => {
                Some(ClipboardData::Special((clipboard.special_name, data)))
            }
            _ => None,
        }
    }

    #[cfg(not(target_os = "android"))]
    pub fn from_multi_clipboards(multi_clipboards: Vec<Clipboard>) -> Vec<ClipboardData> {
        let Some(clipboards) = decode_multi_clipboards(multi_clipboards) else {
            hbb_common::log::warn!("Rejected malformed or oversized clipboard update");
            return Vec::new();
        };
        clipboards
            .into_iter()
            .filter_map(from_clipboard)
            .collect()
    }

    pub fn get_msg_if_not_support_multi_clip(
        version: &str,
        platform: &str,
        multi_clipboards: &MultiClipboards,
    ) -> Option<Message> {
        if crate::clipboard::is_support_multi_clipboard(version, platform) {
            return None;
        }

        // Find the first text clipboard and send it.
        multi_clipboards
            .clipboards
            .iter()
            .find(|c| c.format.enum_value() == Ok(ClipboardFormat::Text))
            .map(|c| {
                let mut msg = Message::new();
                msg.set_clipboard(c.clone());
                msg
            })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use hbb_common::compress::decompress;

        #[test]
        fn small_special_clipboard_preserves_payload_instead_of_format_name() {
            let payload = vec![0, 255, 1, 128];
            let clip = special_to_proto(payload.clone(), "application.test".into());
            assert!(!clip.compress);
            assert_eq!(clip.content.as_ref(), payload.as_slice());
            assert_eq!(clip.special_name, "application.test");
        }

        #[test]
        fn compressed_special_clipboard_preserves_payload() {
            let payload = vec![b'x'; 4096];
            let clip = special_to_proto(payload.clone(), "application.test".into());
            assert!(clip.compress);
            assert_eq!(decompress(&clip.content), payload);
        }

        #[cfg(not(target_os = "android"))]
        #[test]
        fn received_invalid_rgba_is_rejected_before_os_clipboard() {
            for (width, height, size) in [
                (-1, 1, 4),
                (1, -1, 4),
                (0, 1, 0),
                (2, 1, 4),
                (i32::MAX, i32::MAX, 4),
            ] {
                let clip = Clipboard {
                    format: ClipboardFormat::ImageRgba.into(),
                    width,
                    height,
                    content: vec![0; size].into(),
                    ..Default::default()
                };
                assert!(from_multi_clipboards(vec![clip]).is_empty());
            }
        }

        #[cfg(not(target_os = "android"))]
        #[test]
        fn received_corrupt_compression_does_not_become_empty_text() {
            let clip = Clipboard {
                format: ClipboardFormat::Text.into(),
                compress: true,
                content: b"not a zstd frame".to_vec().into(),
                ..Default::default()
            };
            assert!(from_multi_clipboards(vec![clip]).is_empty());
        }

        #[test]
        fn bounded_decoder_checks_exact_limit_truncation_and_concatenated_frames() {
            assert!(decompress_limited(b"", 100).is_err());
            assert_eq!(decompress_limited(&compress_func(b""), 0).unwrap(), b"");
            let encoded = compress_func(b"first");
            assert_eq!(decompress_limited(&encoded, 5).unwrap(), b"first");
            assert!(decompress_limited(&encoded, 4).is_err());
            assert!(decompress_limited(&encoded[..encoded.len() - 1], 100).is_err());
            let mut concatenated = encoded;
            concatenated.extend(compress_func(b"second"));
            assert_eq!(
                decompress_limited(&concatenated, 11).unwrap(),
                b"firstsecond"
            );
            assert!(decompress_limited(&concatenated, 10).is_err());
            assert!(decompress_limited(&concatenated, usize::MAX).is_err());
        }

        #[test]
        fn all_representations_share_one_decompressed_budget() {
            let chunk = MAX_TOTAL_BYTES / 5;
            let content = compress_func(&vec![b'x'; chunk]);
            let clip = Clipboard {
                format: ClipboardFormat::Text.into(),
                compress: true,
                content: content.into(),
                ..Default::default()
            };
            let mut at_limit = vec![clip.clone(); 5];
            at_limit.push(plain_to_proto(
                "x".repeat(MAX_TOTAL_BYTES % 5),
                ClipboardFormat::Text,
            ));
            // A valid compressed empty representation needs no output budget.
            at_limit.push(Clipboard {
                format: ClipboardFormat::Text.into(),
                compress: true,
                content: compress_func(b"").into(),
                ..Default::default()
            });
            assert!(decode_multi_clipboards(at_limit).is_some());
            assert!(decode_multi_clipboards(vec![clip; 6]).is_none());
        }

        #[test]
        fn representation_count_and_raw_size_are_bounded() {
            assert!(
                decode_multi_clipboards(vec![Clipboard::default(); MAX_REPRESENTATIONS]).is_some()
            );
            assert!(
                decode_multi_clipboards(vec![Clipboard::default(); MAX_REPRESENTATIONS + 1])
                    .is_none()
            );
            let clip = Clipboard {
                content: vec![0; MAX_REPRESENTATION_BYTES + 1].into(),
                ..Default::default()
            };
            assert!(decode_multi_clipboards(vec![clip]).is_none());
        }

        #[test]
        fn invalid_representation_rejects_whole_update() {
            let valid = plain_to_proto("preserve this text".into(), ClipboardFormat::Text);
            let invalid = Clipboard {
                format: ClipboardFormat::Html.into(),
                content: vec![0xff].into(),
                ..Default::default()
            };
            assert!(decode_multi_clipboards(vec![valid, invalid]).is_none());
        }

        #[test]
        fn unused_wire_fields_are_removed_before_android_serialization() {
            let mut clip = plain_to_proto("한글".into(), ClipboardFormat::Text);
            clip.special_fields
                .mut_unknown_fields()
                .add_length_delimited(99, vec![0; 512]);
            let decoded = decode_multi_clipboards(vec![clip]).unwrap();
            assert!(decoded[0]
                .special_fields
                .unknown_fields()
                .iter()
                .next()
                .is_none());
            assert_eq!(decoded[0].content.as_ref(), "한글".as_bytes());
        }

        #[cfg(not(target_os = "android"))]
        #[test]
        fn valid_unicode_rich_text_rgba_and_special_data_are_preserved() {
            let text = "한글 English 😀\n\t";
            let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255];
            let png = include_bytes!("../res/32x32.png");
            let input = vec![
                ClipboardData::Text(text.into()),
                ClipboardData::Html(format!("<b>{text}</b>")),
                ClipboardData::Rtf("{\\rtf1 example}".into()),
                ClipboardData::Image(arboard::ImageData::rgba(2, 1, rgba.clone().into())),
                ClipboardData::Special(("application.test".into(), vec![0, 255, 128])),
                ClipboardData::Image(arboard::ImageData::png(png.as_slice().into())),
            ];
            let output = from_multi_clipboards(create_multi_clipboards(input).clipboards);
            assert_eq!(output.len(), 6);
            assert!(matches!(&output[0], ClipboardData::Text(value) if value == text));
            assert!(
                matches!(&output[1], ClipboardData::Html(value) if value == &format!("<b>{text}</b>"))
            );
            assert!(matches!(&output[2], ClipboardData::Rtf(value) if value == "{\\rtf1 example}"));
            assert!(
                matches!(&output[3], ClipboardData::Image(arboard::ImageData::Rgba(value))
                if value.width == 2 && value.height == 1 && value.bytes.as_ref() == rgba)
            );
            assert!(matches!(&output[4], ClipboardData::Special((name, value))
                if name == "application.test" && value == &[0, 255, 128]));
            assert!(
                matches!(&output[5], ClipboardData::Image(arboard::ImageData::Png(value))
                if value.as_ref() == png)
            );
            let clear =
                from_multi_clipboards(vec![plain_to_proto("".into(), ClipboardFormat::Text)]);
            assert!(matches!(&clear[0], ClipboardData::Text(value) if value.is_empty()));
        }
    }
}

#[cfg(target_os = "android")]
pub fn handle_msg_clipboard(cb: Clipboard) {
    handle_msg_multi_clipboards(MultiClipboards {
        clipboards: vec![cb],
        ..Default::default()
    });
}

#[cfg(target_os = "android")]
pub fn handle_msg_multi_clipboards(mcb: MultiClipboards) {
    use hbb_common::protobuf::Message;

    let Some(clipboards) = proto::decode_multi_clipboards(mcb.clipboards) else {
        log::warn!("Rejected malformed or oversized clipboard update");
        return;
    };
    let decoded = MultiClipboards {
        clipboards,
        ..Default::default()
    };
    if let Ok(bytes) = decoded.write_to_bytes() {
        let _ = scrap::android::ffi::call_clipboard_manager_update_clipboard(&bytes);
    }
}

#[cfg(target_os = "android")]
pub fn get_clipboards_msg(client: bool) -> Option<Message> {
    let mut clipboards = scrap::android::ffi::get_clipboards(client)?;
    let mut msg = Message::new();
    for c in &mut clipboards.clipboards {
        let compressed = hbb_common::compress::compress(&c.content);
        let compress = compressed.len() < c.content.len();
        if compress {
            c.content = compressed.into();
        }
        c.compress = compress;
    }
    msg.set_multi_clipboards(clipboards);
    Some(msg)
}

// We need this mod to notify multiple subscribers when the clipboard changes.
// Because only one clipboard master(listener) can trigger the clipboard change event multiple listeners are created on Linux(x11).
// https://github.com/rustdesk-org/clipboard-master/blob/4fb62e5b62fb6350d82b571ec7ba94b3cd466695/src/master/x11.rs#L226
#[cfg(not(target_os = "android"))]
pub mod clipboard_listener {
    use clipboard_master::{CallbackResult, ClipboardHandler, Master, Shutdown};
    use hbb_common::{bail, log, ResultType};
    use std::{
        collections::HashMap,
        io,
        sync::mpsc::{channel, Sender},
        sync::{Arc, Mutex},
        thread::JoinHandle,
    };

    lazy_static::lazy_static! {
        pub static ref CLIPBOARD_LISTENER: Arc<Mutex<ClipboardListener>> = Default::default();
    }

    struct Handler {
        subscribers: Arc<Mutex<HashMap<String, Sender<CallbackResult>>>>,
    }

    impl ClipboardHandler for Handler {
        fn on_clipboard_change(&mut self) -> CallbackResult {
            let sub_lock = self.subscribers.lock().unwrap();
            for tx in sub_lock.values() {
                tx.send(CallbackResult::Next).ok();
            }
            CallbackResult::Next
        }

        fn on_clipboard_error(&mut self, error: io::Error) -> CallbackResult {
            let msg = format!("Clipboard listener error: {}", error);
            let sub_lock = self.subscribers.lock().unwrap();
            for tx in sub_lock.values() {
                tx.send(CallbackResult::StopWithError(io::Error::new(
                    io::ErrorKind::Other,
                    msg.clone(),
                )))
                .ok();
            }
            CallbackResult::Next
        }
    }

    #[derive(Default)]
    pub struct ClipboardListener {
        subscribers: Arc<Mutex<HashMap<String, Sender<CallbackResult>>>>,
        handle: Option<(Shutdown, JoinHandle<()>)>,
    }

    pub fn subscribe(name: String, tx: Sender<CallbackResult>) -> ResultType<()> {
        log::info!("Subscribe clipboard listener: {}", &name);
        let mut listener_lock = CLIPBOARD_LISTENER.lock().unwrap();
        listener_lock
            .subscribers
            .lock()
            .unwrap()
            .insert(name.clone(), tx);

        cleanup_stale_listener(&mut listener_lock);
        if listener_lock.handle.is_none() {
            log::info!("Start clipboard listener thread");
            let handler = Handler {
                subscribers: listener_lock.subscribers.clone(),
            };
            let (tx_start_res, rx_start_res) = channel();
            let h = start_clipboard_master_thread(handler, tx_start_res);
            let shutdown = match rx_start_res.recv() {
                Ok((Some(s), _)) => s,
                Ok((None, err)) => {
                    bail!(err);
                }

                Err(e) => {
                    bail!("Failed to create clipboard listener: {}", e);
                }
            };
            listener_lock.handle = Some((shutdown, h));
            log::info!("Clipboard listener thread started");
        }

        log::info!("Clipboard listener subscribed: {}", name);
        Ok(())
    }

    fn cleanup_stale_listener(listener: &mut ClipboardListener) {
        if !listener
            .handle
            .as_ref()
            .map(|(_, h)| h.is_finished())
            .unwrap_or(false)
        {
            return;
        }
        if let Some((shutdown, h)) = listener.handle.take() {
            log::warn!("Cleaning up stale clipboard listener handle");
            if let Err(e) = h.join() {
                log::error!("Clipboard listener thread panicked during stale cleanup: {:?}", e);
            }
            drop(shutdown);
        }
    }

    pub fn unsubscribe(name: &str) {
        log::info!("Unsubscribe clipboard listener: {}", name);
        let mut listener_lock = CLIPBOARD_LISTENER.lock().unwrap();
        let is_empty = {
            let mut sub_lock = listener_lock.subscribers.lock().unwrap();
            if let Some(tx) = sub_lock.remove(name) {
                tx.send(CallbackResult::Stop).ok();
            }
            sub_lock.is_empty()
        };
        if is_empty {
            if let Some((shutdown, h)) = listener_lock.handle.take() {
                log::info!("Stop clipboard listener thread");
                shutdown.signal();
                h.join().ok();
                log::info!("Clipboard listener thread stopped");
            }
        }
        log::info!("Clipboard listener unsubscribed: {}", name);
    }

    fn start_clipboard_master_thread(
        handler: impl ClipboardHandler + Send + 'static,
        tx_start_res: Sender<(Option<Shutdown>, String)>,
    ) -> JoinHandle<()> {
        // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getmessage#:~:text=The%20window%20must%20belong%20to%20the%20current%20thread.
        let h = std::thread::spawn(move || match Master::new(handler) {
            Ok(mut master) => {
                tx_start_res
                    .send((Some(master.shutdown_channel()), "".to_owned()))
                    .ok();
                log::debug!("Clipboard listener started");
                if let Err(err) = master.run() {
                    log::error!("Failed to run clipboard listener: {}", err);
                } else {
                    log::debug!("Clipboard listener stopped");
                }
            }
            Err(err) => {
                tx_start_res
                    .send((
                        None,
                        format!("Failed to create clipboard listener: {}", err),
                    ))
                    .ok();
            }
        });
        h
    }
}
