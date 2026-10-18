// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::s2sdk;
use std::{collections::HashSet, sync::Mutex};

static KNIVES: Mutex<Option<HashSet<i32>>> = Mutex::new(None);

pub fn track(handle: i32) {
    KNIVES
        .lock()
        .expect("FunRounds touch registry poisoned")
        .get_or_insert_with(HashSet::new)
        .insert(handle);
}

pub fn forget(handle: i32) {
    if let Some(knives) = KNIVES.lock().expect("FunRounds touch registry poisoned").as_mut() {
        knives.remove(&handle);
    }
}

pub fn clear() {
    *KNIVES.lock().expect("FunRounds touch registry poisoned") = None;
}

pub fn owns(handle: i32) -> bool {
    KNIVES
        .lock()
        .expect("FunRounds touch registry poisoned")
        .as_ref()
        .is_some_and(|knives| knives.contains(&handle))
}

pub type TouchPreCallback = s2sdk::EntityTouchPreCallback;

pub fn probe() -> crate::capability::Probe {
    let mut probe = crate::capability::Probe::default();
    crate::capability::imports!(probe; __s2sdk_HookEntityTouchPre, __s2sdk_UnhookEntityTouchPre, __s2sdk_IsEntityTouchPreHookAvailable);
    if probe.available() {
        probe.require(s2sdk::IsEntityTouchPreHookAvailable(), "native:Touch detour/CBaseEntity::Touch gamedata");
    }
    probe
}

pub fn hook(entity_handle: i32, callback: TouchPreCallback) -> bool {
    let Some(hook) = (unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_HookEntityTouchPre).read() }) else {
        return false;
    };
    unsafe { hook(entity_handle, callback) }
}

pub fn unhook(callback: TouchPreCallback) -> bool {
    let Some(unhook) = (unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_UnhookEntityTouchPre).read() }) else {
        return false;
    };
    unsafe { unhook(callback) }
}
