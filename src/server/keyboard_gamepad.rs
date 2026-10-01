use hbb_common::message_proto::{key_event, ControlKey, KeyEvent, KeyboardMode};
use std::collections::BTreeSet;

pub(crate) const BUTTON_DPAD_UP: u16 = 0x0001;
pub(crate) const BUTTON_DPAD_DOWN: u16 = 0x0002;
pub(crate) const BUTTON_DPAD_LEFT: u16 = 0x0004;
pub(crate) const BUTTON_DPAD_RIGHT: u16 = 0x0008;
pub(crate) const BUTTON_START: u16 = 0x0010;
pub(crate) const BUTTON_BACK: u16 = 0x0020;
pub(crate) const BUTTON_LEFT_SHOULDER: u16 = 0x0100;
pub(crate) const BUTTON_RIGHT_SHOULDER: u16 = 0x0200;
pub(crate) const BUTTON_A: u16 = 0x1000;
pub(crate) const BUTTON_B: u16 = 0x2000;
pub(crate) const BUTTON_X: u16 = 0x4000;
pub(crate) const BUTTON_Y: u16 = 0x8000;

const AXIS_MIN: i16 = i16::MIN;
const AXIS_MAX: i16 = i16::MAX;
const TRIGGER_MAX: u8 = u8::MAX;

// Windows set-1 scan codes. Map-mode clients translate their physical key to
// the Windows host layout before the event reaches this mapper.
const SCAN_1: u32 = 0x02;
const SCAN_2: u32 = 0x03;
const SCAN_Q: u32 = 0x10;
const SCAN_W: u32 = 0x11;
const SCAN_E: u32 = 0x12;
const SCAN_U: u32 = 0x16;
const SCAN_I: u32 = 0x17;
const SCAN_A: u32 = 0x1E;
const SCAN_S: u32 = 0x1F;
const SCAN_D: u32 = 0x20;
const SCAN_J: u32 = 0x24;
const SCAN_K: u32 = 0x25;
const SCAN_LEFT_SHIFT: u32 = 0x2A;
const SCAN_RIGHT_SHIFT: u32 = 0x36;
const SCAN_SPACE: u32 = 0x39;
const SCAN_UP: u32 = 0xE048;
const SCAN_LEFT: u32 = 0xE04B;
const SCAN_RIGHT: u32 = 0xE04D;
const SCAN_DOWN: u32 = 0xE050;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct GamepadReport {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum GamepadKey {
    LeftUp,
    LeftDown,
    LeftLeft,
    LeftRight,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    A,
    B,
    X,
    Y,
    LeftShoulder,
    RightShoulder,
    LeftTrigger,
    RightTrigger,
    Back,
    Start,
}

#[derive(Debug, Default)]
pub(crate) struct KeyboardGamepadMapper {
    held: BTreeSet<GamepadKey>,
}

impl KeyboardGamepadMapper {
    /// Converts one remote key message into zero, one, or two complete pad
    /// reports. A `press` packet becomes a down report followed by an up report.
    /// Gamepad mode consumes the keyboard event even when this selected layout
    /// does not map that key, so an unmapped key cannot leak into the desktop.
    pub(crate) fn apply_event(&mut self, event: &KeyEvent) -> Vec<GamepadReport> {
        let Some(key) = mapped_key(event) else {
            return Vec::new();
        };

        let mut reports = Vec::with_capacity(if event.press { 2 } else { 1 });
        if event.press {
            self.update_key(key, true, &mut reports);
            self.update_key(key, false, &mut reports);
        } else {
            self.update_key(key, event.down, &mut reports);
        }
        reports
    }

    pub(crate) fn neutralize(&mut self) -> Option<GamepadReport> {
        if self.held.is_empty() {
            return None;
        }
        self.held.clear();
        Some(GamepadReport::default())
    }

    fn update_key(&mut self, key: GamepadKey, down: bool, reports: &mut Vec<GamepadReport>) {
        let changed = if down {
            self.held.insert(key)
        } else {
            self.held.remove(&key)
        };
        if changed {
            reports.push(self.report());
        }
    }

    fn report(&self) -> GamepadReport {
        let left_x = axis(
            self.held.contains(&GamepadKey::LeftLeft),
            self.held.contains(&GamepadKey::LeftRight),
        );
        let left_y = axis(
            self.held.contains(&GamepadKey::LeftDown),
            self.held.contains(&GamepadKey::LeftUp),
        );
        let mut buttons = 0;
        set_direction_pair(
            &mut buttons,
            self.held.contains(&GamepadKey::DpadUp),
            self.held.contains(&GamepadKey::DpadDown),
            BUTTON_DPAD_UP,
            BUTTON_DPAD_DOWN,
        );
        set_direction_pair(
            &mut buttons,
            self.held.contains(&GamepadKey::DpadLeft),
            self.held.contains(&GamepadKey::DpadRight),
            BUTTON_DPAD_LEFT,
            BUTTON_DPAD_RIGHT,
        );
        for (key, bit) in [
            (GamepadKey::A, BUTTON_A),
            (GamepadKey::B, BUTTON_B),
            (GamepadKey::X, BUTTON_X),
            (GamepadKey::Y, BUTTON_Y),
            (GamepadKey::LeftShoulder, BUTTON_LEFT_SHOULDER),
            (GamepadKey::RightShoulder, BUTTON_RIGHT_SHOULDER),
            (GamepadKey::Back, BUTTON_BACK),
            (GamepadKey::Start, BUTTON_START),
        ] {
            if self.held.contains(&key) {
                buttons |= bit;
            }
        }

        GamepadReport {
            buttons,
            left_trigger: if self.held.contains(&GamepadKey::LeftTrigger) {
                TRIGGER_MAX
            } else {
                0
            },
            right_trigger: if self.held.contains(&GamepadKey::RightTrigger) {
                TRIGGER_MAX
            } else {
                0
            },
            left_x,
            left_y,
            right_x: 0,
            right_y: 0,
        }
    }
}

fn axis(negative: bool, positive: bool) -> i16 {
    match (negative, positive) {
        (true, false) => AXIS_MIN,
        (false, true) => AXIS_MAX,
        _ => 0,
    }
}

fn set_direction_pair(
    buttons: &mut u16,
    first: bool,
    second: bool,
    first_bit: u16,
    second_bit: u16,
) {
    match (first, second) {
        (true, false) => *buttons |= first_bit,
        (false, true) => *buttons |= second_bit,
        _ => {}
    }
}

fn mapped_key(event: &KeyEvent) -> Option<GamepadKey> {
    match event.union.as_ref()? {
        key_event::Union::ControlKey(value) => match value.enum_value().ok()? {
            ControlKey::UpArrow => Some(GamepadKey::DpadUp),
            ControlKey::DownArrow => Some(GamepadKey::DpadDown),
            ControlKey::LeftArrow => Some(GamepadKey::DpadLeft),
            ControlKey::RightArrow => Some(GamepadKey::DpadRight),
            ControlKey::Shift | ControlKey::RShift => Some(GamepadKey::LeftTrigger),
            ControlKey::Space => Some(GamepadKey::RightTrigger),
            _ => None,
        },
        key_event::Union::Chr(code) => {
            let mode = event.mode.enum_value().unwrap_or(KeyboardMode::Legacy);
            if mode == KeyboardMode::Legacy {
                mapped_legacy_character(*code)
            } else {
                mapped_windows_scan_code(*code)
            }
        }
        _ => None,
    }
}

fn mapped_legacy_character(code: u32) -> Option<GamepadKey> {
    let normalized = char::from_u32(code)?.to_ascii_lowercase();
    match normalized {
        'w' => Some(GamepadKey::LeftUp),
        's' => Some(GamepadKey::LeftDown),
        'a' => Some(GamepadKey::LeftLeft),
        'd' => Some(GamepadKey::LeftRight),
        'j' => Some(GamepadKey::A),
        'k' => Some(GamepadKey::B),
        'u' => Some(GamepadKey::X),
        'i' => Some(GamepadKey::Y),
        'q' => Some(GamepadKey::LeftShoulder),
        'e' => Some(GamepadKey::RightShoulder),
        '1' => Some(GamepadKey::Back),
        '2' => Some(GamepadKey::Start),
        ' ' => Some(GamepadKey::RightTrigger),
        _ => None,
    }
}

fn mapped_windows_scan_code(code: u32) -> Option<GamepadKey> {
    match code {
        SCAN_W => Some(GamepadKey::LeftUp),
        SCAN_S => Some(GamepadKey::LeftDown),
        SCAN_A => Some(GamepadKey::LeftLeft),
        SCAN_D => Some(GamepadKey::LeftRight),
        SCAN_UP => Some(GamepadKey::DpadUp),
        SCAN_DOWN => Some(GamepadKey::DpadDown),
        SCAN_LEFT => Some(GamepadKey::DpadLeft),
        SCAN_RIGHT => Some(GamepadKey::DpadRight),
        SCAN_J => Some(GamepadKey::A),
        SCAN_K => Some(GamepadKey::B),
        SCAN_U => Some(GamepadKey::X),
        SCAN_I => Some(GamepadKey::Y),
        SCAN_Q => Some(GamepadKey::LeftShoulder),
        SCAN_E => Some(GamepadKey::RightShoulder),
        SCAN_LEFT_SHIFT | SCAN_RIGHT_SHIFT => Some(GamepadKey::LeftTrigger),
        SCAN_SPACE => Some(GamepadKey::RightTrigger),
        SCAN_1 => Some(GamepadKey::Back),
        SCAN_2 => Some(GamepadKey::Start),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hbb_common::protobuf::EnumOrUnknown;

    fn scan(code: u32, down: bool) -> KeyEvent {
        let mut event = KeyEvent {
            down,
            mode: EnumOrUnknown::new(KeyboardMode::Map),
            ..Default::default()
        };
        event.set_chr(code);
        event
    }

    #[test]
    fn opposite_wasd_keys_cancel_and_restore_axis() {
        let mut mapper = KeyboardGamepadMapper::default();
        let up = mapper.apply_event(&scan(SCAN_W, true));
        assert_eq!(up.last().unwrap().left_y, AXIS_MAX);
        let cancelled = mapper.apply_event(&scan(SCAN_S, true));
        assert_eq!(cancelled.last().unwrap().left_y, 0);
        let restored = mapper.apply_event(&scan(SCAN_S, false));
        assert_eq!(restored.last().unwrap().left_y, AXIS_MAX);
    }

    #[test]
    fn repeated_down_does_not_emit_duplicate_report() {
        let mut mapper = KeyboardGamepadMapper::default();
        assert_eq!(mapper.apply_event(&scan(SCAN_J, true)).len(), 1);
        assert!(mapper.apply_event(&scan(SCAN_J, true)).is_empty());
    }

    #[test]
    fn press_packet_emits_down_and_up_reports() {
        let mut event = scan(SCAN_K, false);
        event.press = true;
        let mut mapper = KeyboardGamepadMapper::default();
        let reports = mapper.apply_event(&event);
        assert_eq!(reports.len(), 2);
        assert_ne!(reports[0].buttons & BUTTON_B, 0);
        assert_eq!(reports[1].buttons & BUTTON_B, 0);
    }

    #[test]
    fn neutralize_releases_every_held_input() {
        let mut mapper = KeyboardGamepadMapper::default();
        mapper.apply_event(&scan(SCAN_W, true));
        mapper.apply_event(&scan(SCAN_J, true));
        assert_eq!(mapper.neutralize(), Some(GamepadReport::default()));
        assert_eq!(mapper.neutralize(), None);
    }

    #[test]
    fn control_keys_map_to_dpad_and_triggers() {
        let mut event = KeyEvent {
            down: true,
            mode: EnumOrUnknown::new(KeyboardMode::Legacy),
            ..Default::default()
        };
        event.set_control_key(ControlKey::LeftArrow);
        let mut mapper = KeyboardGamepadMapper::default();
        let report = mapper.apply_event(&event).pop().unwrap();
        assert_ne!(report.buttons & BUTTON_DPAD_LEFT, 0);
    }
}
