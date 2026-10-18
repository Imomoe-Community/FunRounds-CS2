// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

pub type Callback = unsafe extern "C" fn(i32, i32);
type Hook = unsafe extern "C" fn(Callback);

#[unsafe(no_mangle)]
static mut __s2sdk_OnClientDisconnect_Register: Option<Hook> = None;
#[unsafe(no_mangle)]
static mut __s2sdk_OnClientDisconnect_Unregister: Option<Hook> = None;

pub struct Bridge {
    unhook: Hook,
    callback: Callback,
}

impl Bridge {
    pub fn subscribe(callback: Callback) -> Option<Self> {
        let hook = unsafe { __s2sdk_OnClientDisconnect_Register }?;
        let unhook = unsafe { __s2sdk_OnClientDisconnect_Unregister }?;
        unsafe { hook(callback) };
        Some(Self { unhook, callback })
    }

    pub fn unsubscribe(self) {
        unsafe { (self.unhook)(self.callback) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static REGISTERED: AtomicU32 = AtomicU32::new(0);
    unsafe extern "C" fn callback(_: i32, _: i32) {}
    unsafe extern "C" fn hook(_: Callback) {
        REGISTERED.fetch_add(1, Ordering::Relaxed);
    }
    unsafe extern "C" fn unhook(_: Callback) {
        REGISTERED.fetch_sub(1, Ordering::Relaxed);
    }

    #[test]
    fn old_sdk_or_missing_unhook_never_registers_a_callback() {
        assert!(Bridge::subscribe(callback).is_none());
        unsafe { __s2sdk_OnClientDisconnect_Register = Some(hook) };
        assert!(Bridge::subscribe(callback).is_none());
        assert_eq!(REGISTERED.load(Ordering::Relaxed), 0);
        unsafe { __s2sdk_OnClientDisconnect_Unregister = Some(unhook) };
        let bridge = Bridge::subscribe(callback).unwrap();
        assert_eq!(REGISTERED.load(Ordering::Relaxed), 1);
        bridge.unsubscribe();
        assert_eq!(REGISTERED.load(Ordering::Relaxed), 0);
        unsafe {
            __s2sdk_OnClientDisconnect_Register = None;
            __s2sdk_OnClientDisconnect_Unregister = None;
        }
    }
}
