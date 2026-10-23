// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

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

pub fn probe() -> crate::capability::Probe {
    let mut probe = crate::capability::Probe::default();
    probe.require(
        crate::funrounds_native::IsEntityTouchPreHookAvailable(),
        "native:Touch detour/CBaseEntity::Touch gamedata",
    );
    probe
}
