// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{collections::HashSet, sync::Mutex};

#[derive(Default)]
struct Owners {
    projectiles: HashSet<i32>,
    rescue_zones: HashSet<i32>,
}

impl Owners {
    fn owns_touch_pair(&self, entity: i32, other: i32) -> bool {
        self.projectiles.contains(&entity) || self.rescue_zones.contains(&entity) || self.rescue_zones.contains(&other)
    }

    fn has_owners(&self) -> bool {
        !self.projectiles.is_empty() || !self.rescue_zones.is_empty()
    }
}

static OWNERS: Mutex<Option<Owners>> = Mutex::new(None);

// The native adapter intentionally permits a failed AddCallback/removal path to
// be retried against its retained partial detour record.
pub fn register_with_retry(mut register: impl FnMut() -> bool) -> bool {
    register() || register()
}

pub fn track(handle: i32) {
    OWNERS
        .lock()
        .expect("FunRounds touch registry poisoned")
        .get_or_insert_with(Owners::default)
        .projectiles
        .insert(handle);
}

pub fn forget(handle: i32) {
    if let Some(owners) = OWNERS.lock().expect("FunRounds touch registry poisoned").as_mut() {
        owners.projectiles.remove(&handle);
    }
}

pub fn clear() {
    if let Some(owners) = OWNERS.lock().expect("FunRounds touch registry poisoned").as_mut() {
        owners.projectiles.clear();
    }
}

pub fn track_rescue_zone(handle: i32) {
    OWNERS
        .lock()
        .expect("FunRounds touch registry poisoned")
        .get_or_insert_with(Owners::default)
        .rescue_zones
        .insert(handle);
}

pub fn clear_rescue_zones() {
    if let Some(owners) = OWNERS.lock().expect("FunRounds touch registry poisoned").as_mut() {
        owners.rescue_zones.clear();
    }
}

pub fn retain_rescue_zones(handles: &HashSet<i32>) {
    if let Some(owners) = OWNERS.lock().expect("FunRounds touch registry poisoned").as_mut() {
        owners.rescue_zones.retain(|handle| handles.contains(handle));
    }
}

pub fn owns_rescue_zone(handle: i32) -> bool {
    OWNERS
        .lock()
        .expect("FunRounds touch registry poisoned")
        .as_ref()
        .is_some_and(|owners| owners.rescue_zones.contains(&handle))
}

pub fn owns_touch_pair(entity: i32, other: i32) -> bool {
    let owners = OWNERS.lock().expect("FunRounds touch registry poisoned");
    owners.as_ref().is_some_and(|owners| owners.owns_touch_pair(entity, other))
}

pub fn owns_projectile(handle: i32) -> bool {
    OWNERS
        .lock()
        .expect("FunRounds touch registry poisoned")
        .as_ref()
        .is_some_and(|owners| owners.projectiles.contains(&handle))
}

pub fn has_owners() -> bool {
    let owners = OWNERS.lock().expect("FunRounds touch registry poisoned");
    owners.as_ref().is_some_and(Owners::has_owners)
}

pub fn probe() -> crate::capability::Probe {
    let mut probe = crate::capability::Probe::default();
    probe.require(
        crate::funrounds_native::IsEntityTouchPreHookAvailable(),
        "native:Touch detour/CBaseEntity::Touch gamedata",
    );
    probe
}
