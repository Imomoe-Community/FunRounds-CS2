// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    capability::Probe,
    fault::{Fault, Mechanism},
    s2sdk,
};
use funrounds_core::{Mode, TeamFilter};
use plugify::{Str, Vec3};
use rand::{SeedableRng, seq::IndexedRandom};

const DEATHMATCH_SPAWN: &str = "info_deathmatch_spawn";

pub(crate) fn capability_probe() -> Probe {
    let mut probe = Probe::default();
    crate::capability::imports!(probe;
        __s2sdk_FindEntityByClassname,
        __s2sdk_IsValidEntHandle,
        __s2sdk_GetEntityClassname,
        __s2sdk_GetEntityAbsOrigin,
        __s2sdk_GetEntityAngRotation,
        __s2sdk_TeleportEntity,
        __s2sdk_GetClientTeam,
        __s2sdk_IsClientAlive,
    );

    let has_support = unsafe { std::ptr::addr_of!(crate::funrounds_native::__funrounds_native_HasPlayerRandomSpawnPointSupport).read() };
    let select = unsafe { std::ptr::addr_of!(crate::funrounds_native::__funrounds_native_SelectPlayerRandomSpawnPoint).read() };
    probe.require(has_support.is_some(), "native_import:HasPlayerRandomSpawnPointSupport");
    probe.require(select.is_some(), "native_import:SelectPlayerRandomSpawnPoint");
    if let (Some(has_support), Some(_)) = (has_support, select) {
        probe.require(unsafe { has_support() }, "native:player_random_spawn_point_selector");
    }
    probe
}

fn native_selector_supported() -> bool {
    let has_support = unsafe { std::ptr::addr_of!(crate::funrounds_native::__funrounds_native_HasPlayerRandomSpawnPointSupport).read() };
    let select = unsafe { std::ptr::addr_of!(crate::funrounds_native::__funrounds_native_SelectPlayerRandomSpawnPoint).read() };
    has_support.zip(select).is_some_and(|(has_support, _)| unsafe { has_support() })
}

fn team_matches(filter: TeamFilter, team: i32) -> bool {
    matches!(team, 2 | 3) && filter.matches(team)
}

fn mechanism(message: impl Into<String>) -> Fault {
    Fault::mechanism(Mechanism::RandomSpawn, message)
}

fn deathmatch_spawn_points() -> Result<Vec<i32>, Fault> {
    let classname = Str::from_str(DEATHMATCH_SPAWN);
    let mut start_from = -1;
    let mut points = Vec::new();
    loop {
        let handle = s2sdk::FindEntityByClassname(start_from, &classname);
        if handle == -1 {
            break;
        }
        if handle == start_from {
            return Err(mechanism(format!("entity enumeration repeated info_deathmatch_spawn handle {handle}")));
        }
        if !s2sdk::IsValidEntHandle(handle) {
            return Err(mechanism(format!(
                "entity enumeration returned invalid info_deathmatch_spawn handle {handle}"
            )));
        }
        if s2sdk::GetEntityClassname(handle).as_str() == DEATHMATCH_SPAWN {
            points.push(handle);
        }
        start_from = handle;
    }
    Ok(points)
}

/// Move an eligible live player to a engine deathmatch spawn point before loadout application.
/// The caller supplies the action identity guard and must run this on the game thread.
pub(crate) fn apply(player: crate::game::LivePlayer, mode: &Mode, mut is_current: impl FnMut() -> bool) -> Result<(), Fault> {
    let Some(filter) = mode.random_spawn.filter(|filter| *filter != TeamFilter::None) else {
        return Ok(());
    };
    if !is_current() {
        return Err(Fault::cancelled("random spawn cancelled before player identity validation"));
    }
    let team = s2sdk::GetClientTeam(player.slot) as i32;
    if !team_matches(filter, team) {
        return Ok(());
    }
    if !s2sdk::IsClientAlive(player.slot) {
        return Err(Fault::cancelled("random spawn cancelled for a non-alive player"));
    }
    if !native_selector_supported() {
        return Err(mechanism("native deathmatch spawn selector is unavailable"));
    }

    let points = deathmatch_spawn_points()?;
    if !is_current() {
        return Err(Fault::cancelled("random spawn cancelled while enumerating map spawn points"));
    }
    let mut system_rng = rand::rngs::SysRng;
    let mut rng = rand::rngs::StdRng::try_from_rng(&mut system_rng)
        .map_err(|error| mechanism(format!("cannot initialize spawn selection randomness: {error}")))?;
    let Some(start_point) = points.choose(&mut rng).copied() else {
        return Err(mechanism("map has no info_deathmatch_spawn points"));
    };

    if !is_current() {
        return Err(Fault::cancelled("random spawn cancelled before native selection"));
    }
    let selected = crate::funrounds_native::SelectPlayerRandomSpawnPoint(player.pawn_handle, start_point);
    if !is_current() {
        return Err(Fault::cancelled("random spawn cancelled after native selection"));
    }
    if selected == -1 || !s2sdk::IsValidEntHandle(selected) || s2sdk::GetEntityClassname(selected).as_str() != DEATHMATCH_SPAWN {
        return Err(mechanism(format!(
            "native selector returned no valid info_deathmatch_spawn point (handle {selected})"
        )));
    }

    let origin = s2sdk::GetEntityAbsOrigin(selected);
    let angles = s2sdk::GetEntityAngRotation(selected);
    if !vector_is_finite(&origin) || !vector_is_finite(&angles) {
        return Err(mechanism(format!(
            "native selector returned non-finite position or direction for spawn point {selected}"
        )));
    }
    if !is_current() {
        return Err(Fault::cancelled("random spawn cancelled before teleport"));
    }
    let zero_velocity = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    s2sdk::TeleportEntity(player.pawn_handle, &origin, &angles, &zero_velocity);
    if !is_current() {
        return Err(Fault::cancelled("random spawn action became stale during teleport"));
    }
    Ok(())
}

fn vector_is_finite(vector: &Vec3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}
