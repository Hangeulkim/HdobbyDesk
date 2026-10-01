use super::keyboard_gamepad::GamepadReport;
use std::{
    collections::HashMap,
    ffi::c_void,
    sync::{Mutex, OnceLock},
};

const VIGEM_ERROR_NONE: u32 = 0x2000_0000;
const MAX_XINPUT_GAMEPADS: usize = 4;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct XusbReport {
    buttons: u16,
    left_trigger: u8,
    right_trigger: u8,
    left_x: i16,
    left_y: i16,
    right_x: i16,
    right_y: i16,
}

impl From<GamepadReport> for XusbReport {
    fn from(value: GamepadReport) -> Self {
        Self {
            buttons: value.buttons,
            left_trigger: value.left_trigger,
            right_trigger: value.right_trigger,
            left_x: value.left_x,
            left_y: value.left_y,
            right_x: value.right_x,
            right_y: value.right_y,
        }
    }
}

extern "C" {
    fn vigem_alloc() -> *mut c_void;
    fn vigem_free(client: *mut c_void);
    fn vigem_connect(client: *mut c_void) -> u32;
    fn vigem_disconnect(client: *mut c_void);
    fn vigem_target_x360_alloc() -> *mut c_void;
    fn vigem_target_free(target: *mut c_void);
    fn vigem_target_add(client: *mut c_void, target: *mut c_void) -> u32;
    fn vigem_target_remove(client: *mut c_void, target: *mut c_void) -> u32;
    fn vigem_target_x360_update(
        client: *mut c_void,
        target: *mut c_void,
        report: XusbReport,
    ) -> u32;
    fn vigem_target_x360_get_user_index(
        client: *mut c_void,
        target: *mut c_void,
        index: *mut u32,
    ) -> u32;
}

#[derive(Debug)]
struct Pad {
    target: usize,
    user_index: u32,
}

struct VigemBackend {
    client: usize,
    pads: HashMap<i32, Pad>,
}

impl VigemBackend {
    fn connect() -> Result<Self, String> {
        let client = unsafe { vigem_alloc() };
        if client.is_null() {
            return Err("ViGEmClient allocation failed".to_owned());
        }
        let status = unsafe { vigem_connect(client) };
        if status != VIGEM_ERROR_NONE {
            unsafe { vigem_free(client) };
            return Err(vigem_error("connect to signed ViGEmBus driver", status));
        }
        Ok(Self {
            client: client as usize,
            pads: HashMap::new(),
        })
    }

    fn add_pad(&mut self, connection_id: i32) -> Result<u32, String> {
        if let Some(pad) = self.pads.get(&connection_id) {
            return Ok(pad.user_index);
        }
        if self.pads.len() >= MAX_XINPUT_GAMEPADS {
            return Err("All four XInput gamepad slots are already in use".to_owned());
        }

        let target = unsafe { vigem_target_x360_alloc() };
        if target.is_null() {
            return Err("ViGEm Xbox 360 target allocation failed".to_owned());
        }
        let status = unsafe { vigem_target_add(self.client(), target) };
        if status != VIGEM_ERROR_NONE {
            unsafe { vigem_target_free(target) };
            return Err(vigem_error("create a virtual Xbox 360 controller", status));
        }

        let mut user_index = u32::MAX;
        let index_status =
            unsafe { vigem_target_x360_get_user_index(self.client(), target, &mut user_index) };
        if index_status != VIGEM_ERROR_NONE || user_index >= MAX_XINPUT_GAMEPADS as u32 {
            unsafe {
                let _ = vigem_target_remove(self.client(), target);
                vigem_target_free(target);
            }
            return Err(vigem_error("read the assigned XInput slot", index_status));
        }

        let neutral_status =
            unsafe { vigem_target_x360_update(self.client(), target, XusbReport::default()) };
        if neutral_status != VIGEM_ERROR_NONE {
            unsafe {
                let _ = vigem_target_remove(self.client(), target);
                vigem_target_free(target);
            }
            return Err(vigem_error(
                "initialize the virtual gamepad",
                neutral_status,
            ));
        }

        self.pads.insert(
            connection_id,
            Pad {
                target: target as usize,
                user_index,
            },
        );
        Ok(user_index)
    }

    fn update(&mut self, connection_id: i32, report: GamepadReport) -> Result<(), String> {
        let Some(pad) = self.pads.get(&connection_id) else {
            return Err("Virtual gamepad is not active for this connection".to_owned());
        };
        let status = unsafe {
            vigem_target_x360_update(self.client(), pad.target as *mut c_void, report.into())
        };
        if status == VIGEM_ERROR_NONE {
            Ok(())
        } else {
            Err(vigem_error("update the virtual gamepad", status))
        }
    }

    fn remove_pad(&mut self, connection_id: i32) {
        let Some(pad) = self.pads.remove(&connection_id) else {
            return;
        };
        let target = pad.target as *mut c_void;
        unsafe {
            let _ = vigem_target_x360_update(self.client(), target, XusbReport::default());
            let _ = vigem_target_remove(self.client(), target);
            vigem_target_free(target);
        }
    }

    fn client(&self) -> *mut c_void {
        self.client as *mut c_void
    }
}

impl Drop for VigemBackend {
    fn drop(&mut self) {
        let connection_ids: Vec<i32> = self.pads.keys().copied().collect();
        for connection_id in connection_ids {
            self.remove_pad(connection_id);
        }
        unsafe {
            vigem_disconnect(self.client());
            vigem_free(self.client());
        }
    }
}

#[derive(Default)]
struct GamepadManager {
    backend: Option<VigemBackend>,
}

impl GamepadManager {
    fn backend(&mut self) -> Result<&mut VigemBackend, String> {
        if self.backend.is_none() {
            self.backend = Some(VigemBackend::connect()?);
        }
        self.backend
            .as_mut()
            .ok_or_else(|| "Virtual gamepad backend initialization failed".to_owned())
    }
}

static MANAGER: OnceLock<Mutex<GamepadManager>> = OnceLock::new();

fn manager() -> &'static Mutex<GamepadManager> {
    MANAGER.get_or_init(|| Mutex::new(GamepadManager::default()))
}

pub(crate) fn driver_available() -> bool {
    let manager = manager().lock().unwrap();
    if let Some(backend) = manager.backend.as_ref() {
        return backend.pads.len() < MAX_XINPUT_GAMEPADS;
    }
    VigemBackend::connect().is_ok()
}

pub(crate) fn enable(connection_id: i32) -> Result<u32, String> {
    manager().lock().unwrap().backend()?.add_pad(connection_id)
}

pub(crate) fn update(connection_id: i32, report: GamepadReport) -> Result<(), String> {
    let mut manager = manager().lock().unwrap();
    let Some(backend) = manager.backend.as_mut() else {
        return Err("Signed ViGEmBus driver is not available".to_owned());
    };
    backend.update(connection_id, report)
}

pub(crate) fn disable(connection_id: i32) {
    let mut manager = manager().lock().unwrap();
    if let Some(backend) = manager.backend.as_mut() {
        backend.remove_pad(connection_id);
    }
}

fn vigem_error(action: &str, status: u32) -> String {
    match status {
        0xE000_0001 => format!("Could not {action}: signed ViGEmBus driver was not found"),
        0xE000_0002 => format!("Could not {action}: no virtual controller slot is free"),
        0xE000_0008 => format!("Could not {action}: driver and client versions do not match"),
        0xE000_0009 => format!("Could not {action}: access to ViGEmBus was denied"),
        _ => format!("Could not {action} (ViGEm status 0x{status:08X})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_report_layout_matches_xusb_report_layout() {
        assert_eq!(
            std::mem::size_of::<GamepadReport>(),
            std::mem::size_of::<XusbReport>()
        );
        assert_eq!(std::mem::size_of::<XusbReport>(), 12);
    }

    #[test]
    fn known_missing_driver_error_is_actionable() {
        let message = vigem_error("start gamepad", 0xE000_0001);
        assert!(message.contains("signed ViGEmBus driver"));
    }
}
