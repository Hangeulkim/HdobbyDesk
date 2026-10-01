use hbb_common::{
    compress::{compress, decompress_limited},
    message_proto::{Clipboard, ClipboardFormat, Message, MultiClipboards},
};

pub const MAX_IOS_CLIPBOARD_TEXT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_IOS_CLIPBOARD_PNG_BYTES: usize = 24 * 1024 * 1024;
const MAX_IOS_CLIPBOARD_RGBA_BYTES: usize = 32 * 1024 * 1024;
const MAX_IOS_CLIPBOARD_WIRE_BYTES: usize = 32 * 1024 * 1024;
const MAX_IOS_CLIPBOARD_REPRESENTATIONS: usize = 16;
const MAX_IOS_CLIPBOARD_PIXELS: usize = 16 * 1024 * 1024;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

pub const IOS_CLIPBOARD_EMPTY: &str = "Clipboard is empty";
pub const IOS_CLIPBOARD_TOO_LARGE: &str = "Clipboard is too large";
pub const IOS_CLIPBOARD_INVALID_IMAGE: &str = "Clipboard image is invalid";
pub const IOS_CLIPBOARD_INVALID_TEXT: &str = "Clipboard text is invalid";
pub const IOS_CLIPBOARD_UNSUPPORTED: &str = "Clipboard format is unsupported";
pub const IOS_CLIPBOARD_SESSION_UNAVAILABLE: &str = "Remote session is unavailable";
pub const IOS_CLIPBOARD_DISABLED: &str = "Remote clipboard is disabled";
pub const IOS_CLIPBOARD_IMAGE_UNSUPPORTED: &str =
    "Remote device does not support image clipboard";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IosClipboardPayload {
    pub text: String,
    pub png: Vec<u8>,
}

impl IosClipboardPayload {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.png.is_empty()
    }
}

fn decode_clipboard_content(
    clipboard: &Clipboard,
    output_limit: usize,
) -> Result<Vec<u8>, &'static str> {
    if clipboard.content.len() > MAX_IOS_CLIPBOARD_WIRE_BYTES {
        return Err(IOS_CLIPBOARD_TOO_LARGE);
    }
    if clipboard.compress {
        decompress_limited(&clipboard.content, output_limit)
            .map_err(|_| IOS_CLIPBOARD_TOO_LARGE)
    } else if clipboard.content.len() <= output_limit {
        Ok(clipboard.content.to_vec())
    } else {
        Err(IOS_CLIPBOARD_TOO_LARGE)
    }
}

fn png_dimensions(png: &[u8]) -> Option<(usize, usize)> {
    if png.len() < 33
        || &png[..8] != PNG_SIGNATURE
        || u32::from_be_bytes(png[8..12].try_into().ok()?) != 13
        || &png[12..16] != b"IHDR"
    {
        return None;
    }
    let width = u32::from_be_bytes(png[16..20].try_into().ok()?) as usize;
    let height = u32::from_be_bytes(png[20..24].try_into().ok()?) as usize;
    let pixels = width.checked_mul(height)?;
    if width == 0 || height == 0 || pixels > MAX_IOS_CLIPBOARD_PIXELS {
        return None;
    }
    Some((width, height))
}

pub fn validate_png(png: &[u8]) -> Result<(), &'static str> {
    if png.len() > MAX_IOS_CLIPBOARD_PNG_BYTES {
        return Err(IOS_CLIPBOARD_TOO_LARGE);
    }
    png_dimensions(png)
        .map(|_| ())
        .ok_or(IOS_CLIPBOARD_INVALID_IMAGE)
}

pub fn decode_incoming_clipboards(
    clipboards: Vec<Clipboard>,
) -> Result<IosClipboardPayload, &'static str> {
    if clipboards.is_empty() {
        return Err(IOS_CLIPBOARD_EMPTY);
    }
    if clipboards.len() > MAX_IOS_CLIPBOARD_REPRESENTATIONS {
        return Err(IOS_CLIPBOARD_TOO_LARGE);
    }
    let mut wire_bytes = 0usize;
    for clipboard in &clipboards {
        wire_bytes = wire_bytes
            .checked_add(clipboard.content.len())
            .ok_or(IOS_CLIPBOARD_TOO_LARGE)?;
        if wire_bytes > MAX_IOS_CLIPBOARD_WIRE_BYTES {
            return Err(IOS_CLIPBOARD_TOO_LARGE);
        }
    }

    let mut payload = IosClipboardPayload::default();
    let mut rgba: Option<(Vec<u8>, usize, usize)> = None;
    for clipboard in clipboards {
        match clipboard.format.enum_value() {
            Ok(ClipboardFormat::Text) if payload.text.is_empty() => {
                let content = decode_clipboard_content(
                    &clipboard,
                    MAX_IOS_CLIPBOARD_TEXT_BYTES,
                )?;
                payload.text = String::from_utf8(content)
                    .map_err(|_| IOS_CLIPBOARD_INVALID_TEXT)?;
            }
            Ok(ClipboardFormat::ImagePng) if payload.png.is_empty() => {
                let content = decode_clipboard_content(
                    &clipboard,
                    MAX_IOS_CLIPBOARD_PNG_BYTES,
                )?;
                validate_png(&content)?;
                payload.png = content;
            }
            Ok(ClipboardFormat::ImageRgba) if payload.png.is_empty() && rgba.is_none() => {
                let width = usize::try_from(clipboard.width)
                    .map_err(|_| IOS_CLIPBOARD_INVALID_IMAGE)?;
                let height = usize::try_from(clipboard.height)
                    .map_err(|_| IOS_CLIPBOARD_INVALID_IMAGE)?;
                let required = width
                    .checked_mul(height)
                    .and_then(|pixels| {
                        if pixels <= MAX_IOS_CLIPBOARD_PIXELS {
                            pixels.checked_mul(4)
                        } else {
                            None
                        }
                    })
                    .ok_or(IOS_CLIPBOARD_INVALID_IMAGE)?;
                if required == 0 || required > MAX_IOS_CLIPBOARD_RGBA_BYTES {
                    return Err(IOS_CLIPBOARD_TOO_LARGE);
                }
                let content = decode_clipboard_content(&clipboard, required)?;
                if content.len() != required {
                    return Err(IOS_CLIPBOARD_INVALID_IMAGE);
                }
                rgba = Some((content, width, height));
            }
            _ => {}
        }
    }

    if payload.png.is_empty() {
        if let Some((rgba, width, height)) = rgba {
            let width = u32::try_from(width).map_err(|_| IOS_CLIPBOARD_INVALID_IMAGE)?;
            let height = u32::try_from(height).map_err(|_| IOS_CLIPBOARD_INVALID_IMAGE)?;
            repng::encode(&mut payload.png, width, height, &rgba)
                .map_err(|_| IOS_CLIPBOARD_INVALID_IMAGE)?;
            validate_png(&payload.png)?;
        }
    }
    if payload.is_empty() {
        Err(IOS_CLIPBOARD_UNSUPPORTED)
    } else {
        Ok(payload)
    }
}

fn text_clipboard(text: String) -> Clipboard {
    let compressed = compress(text.as_bytes());
    let use_compressed = !compressed.is_empty() && compressed.len() < text.len();
    Clipboard {
        compress: use_compressed,
        content: if use_compressed {
            compressed.into()
        } else {
            text.into_bytes().into()
        },
        format: ClipboardFormat::Text.into(),
        ..Default::default()
    }
}

pub fn build_outgoing_message(
    text: String,
    png: Vec<u8>,
) -> Result<Message, &'static str> {
    if text.len() > MAX_IOS_CLIPBOARD_TEXT_BYTES {
        return Err(IOS_CLIPBOARD_TOO_LARGE);
    }
    if !png.is_empty() {
        validate_png(&png)?;
    }
    if text.is_empty() && png.is_empty() {
        return Err(IOS_CLIPBOARD_EMPTY);
    }

    let mut message = Message::new();
    if png.is_empty() {
        message.set_clipboard(text_clipboard(text));
        return Ok(message);
    }

    let mut clipboards = Vec::with_capacity(if text.is_empty() { 1 } else { 2 });
    if !text.is_empty() {
        clipboards.push(text_clipboard(text));
    }
    clipboards.push(Clipboard {
        compress: false,
        content: png.into(),
        format: ClipboardFormat::ImagePng.into(),
        ..Default::default()
    });
    message.set_multi_clipboards(MultiClipboards {
        clipboards,
        ..Default::default()
    });
    Ok(message)
}

pub fn peer_supports_multi_clipboard(peer_version: &str, peer_platform: &str) -> bool {
    use hbb_common::{get_version_number, whoami::Platform};

    if get_version_number(peer_version) < get_version_number("1.3.0") {
        return false;
    }
    if peer_platform.is_empty() || peer_platform == Platform::Ios.to_string() {
        return false;
    }
    peer_platform != "Android" || get_version_number(peer_version) >= get_version_number("1.3.3")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hbb_common::message_proto::message;

    fn one_pixel_png() -> Vec<u8> {
        let mut png = Vec::new();
        repng::encode(&mut png, 1, 1, &[0x11, 0x22, 0x33, 0xff]).unwrap();
        png
    }

    #[test]
    fn text_only_uses_legacy_message_for_compatibility() {
        let message = build_outgoing_message("안녕".to_owned(), vec![]).unwrap();
        let Some(message::Union::Clipboard(clipboard)) = message.union else {
            panic!("expected legacy clipboard");
        };
        let decoded = decode_incoming_clipboards(vec![clipboard]).unwrap();
        assert_eq!(decoded.text, "안녕");
        assert!(decoded.png.is_empty());
    }

    #[test]
    fn text_and_png_round_trip_as_one_atomic_update() {
        let png = one_pixel_png();
        let message = build_outgoing_message("image".to_owned(), png.clone()).unwrap();
        let Some(message::Union::MultiClipboards(clipboards)) = message.union else {
            panic!("expected multi clipboard");
        };
        let decoded = decode_incoming_clipboards(clipboards.clipboards).unwrap();
        assert_eq!(decoded.text, "image");
        assert_eq!(decoded.png, png);
    }

    #[test]
    fn rgba_is_converted_to_png() {
        let decoded = decode_incoming_clipboards(vec![Clipboard {
            content: vec![0xff, 0, 0, 0xff].into(),
            width: 1,
            height: 1,
            format: ClipboardFormat::ImageRgba.into(),
            ..Default::default()
        }])
        .unwrap();
        assert!(png_dimensions(&decoded.png).is_some());
    }

    #[test]
    fn malformed_png_and_rgba_are_rejected() {
        assert_eq!(
            build_outgoing_message(String::new(), b"not png".to_vec()).unwrap_err(),
            IOS_CLIPBOARD_INVALID_IMAGE
        );
        assert_eq!(
            decode_incoming_clipboards(vec![Clipboard {
                content: vec![0; 3].into(),
                width: 1,
                height: 1,
                format: ClipboardFormat::ImageRgba.into(),
                ..Default::default()
            }])
            .unwrap_err(),
            IOS_CLIPBOARD_INVALID_IMAGE
        );
    }

    #[test]
    fn decompression_and_representation_counts_are_bounded() {
        let compressed = compress(&vec![b'x'; MAX_IOS_CLIPBOARD_TEXT_BYTES + 1]);
        assert_eq!(
            decode_incoming_clipboards(vec![Clipboard {
                compress: true,
                content: compressed.into(),
                format: ClipboardFormat::Text.into(),
                ..Default::default()
            }])
            .unwrap_err(),
            IOS_CLIPBOARD_TOO_LARGE
        );
        assert_eq!(
            decode_incoming_clipboards(vec![Clipboard::new();
                MAX_IOS_CLIPBOARD_REPRESENTATIONS + 1])
            .unwrap_err(),
            IOS_CLIPBOARD_TOO_LARGE
        );
    }

    #[test]
    fn multi_clipboard_compatibility_matches_supported_mobile_peers() {
        assert!(peer_supports_multi_clipboard("1.3.0", "Windows"));
        assert!(!peer_supports_multi_clipboard("1.2.9", "Windows"));
        assert!(!peer_supports_multi_clipboard("1.4.0", "iOS"));
        assert!(!peer_supports_multi_clipboard("1.3.2", "Android"));
        assert!(peer_supports_multi_clipboard("1.3.3", "Android"));
    }
}
