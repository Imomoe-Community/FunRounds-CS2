// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    capability::{Probe, imports},
    fault::{Fault, Mechanism},
    game::LivePlayer,
    s2sdk,
};
use funrounds_core::{Mode, PlayerIdentity, RoundGeneration};
use plugify::Str;

const FOV_CLASS: &str = "CBasePlayerController";
const FOV_FIELD: &str = "m_iDesiredFOV";
const HUD_CLASS: &str = "CBasePlayerPawn";
const HUD_FIELD: &str = "m_iHideHUD";
const CROSSHAIR_HUD_BIT: u32 = 1 << 8;
const SECONDARY_CLASS: &str = "CBasePlayerWeapon";
const SECONDARY_FIELD: &str = "m_nNextSecondaryAttackTick";
const SECONDARY_BLOCK_TICKS: i32 = 4096;
const SCOPED_WEAPONS: &[&str] = &[
    "weapon_awp",
    "weapon_ssg08",
    "weapon_scar20",
    "weapon_g3sg1",
    "weapon_aug",
    "weapon_sg556",
];

#[derive(Clone, Copy)]
struct FovSnapshot {
    original: i32,
    applied: Option<i32>,
}

#[derive(Clone, Copy)]
struct CrosshairSnapshot {
    originally_hidden: bool,
    hide_at: Option<f32>,
    hidden_by_us: bool,
}

#[derive(Clone, Copy)]
struct SecondaryAttackSnapshot {
    weapon: i32,
    original: i32,
    last_written: Option<i32>,
}

pub(crate) struct ViewEffects {
    identity: PlayerIdentity,
    round: RoundGeneration,
    controller_handle: i32,
    fov: Option<FovSnapshot>,
    crosshair: Option<CrosshairSnapshot>,
    secondary: Option<SecondaryAttackSnapshot>,
}

impl ViewEffects {
    pub(crate) fn capture(player: LivePlayer, identity: PlayerIdentity, round: RoundGeneration, mode: &Mode) -> Result<Self, Fault> {
        let fov = mode
            .fov
            .map(|_| {
                read_controller_value(player, FOV_CLASS, FOV_FIELD)
                    .map(|original| FovSnapshot { original, applied: None })
                    .map_err(|error| Fault::mechanism(Mechanism::Fov, error))
            })
            .transpose()?;
        let crosshair = mode
            .hide_crosshair_after
            .map(|_| {
                read_pawn_value(player, HUD_CLASS, HUD_FIELD)
                    .map(|flags| CrosshairSnapshot {
                        originally_hidden: flags as u32 & CROSSHAIR_HUD_BIT != 0,
                        hide_at: None,
                        hidden_by_us: false,
                    })
                    .map_err(|error| Fault::mechanism(Mechanism::Crosshair, error))
            })
            .transpose()?;
        Ok(Self {
            identity,
            round,
            controller_handle: player.controller_handle,
            fov,
            crosshair,
            secondary: None,
        })
    }

    pub(crate) fn apply(&mut self, player: LivePlayer, round: RoundGeneration, mode: &Mode, now: f32) -> Result<(), Fault> {
        self.validate(player, self.identity, round)?;
        if let (Some(snapshot), Some(desired)) = (&mut self.fov, mode.fov) {
            snapshot.applied = Some(desired);
            write_controller_value(player, FOV_CLASS, FOV_FIELD, desired).map_err(|error| Fault::mechanism(Mechanism::Fov, error))?;
        }
        if let (Some(snapshot), Some(delay)) = (&mut self.crosshair, mode.hide_crosshair_after) {
            snapshot.hide_at = Some(now + delay);
        }
        Ok(())
    }

    pub(crate) fn update(
        &mut self,
        player: LivePlayer,
        identity: PlayerIdentity,
        round: RoundGeneration,
        mode: &Mode,
        now: f32,
    ) -> Result<(), Fault> {
        self.validate(player, identity, round)?;
        if mode.no_scope {
            self.update_secondary_attack(player)?;
        } else {
            self.restore_secondary_attack()
                .map_err(|error| Fault::mechanism(Mechanism::NoScope, error))?;
        }
        if let Some(snapshot) = &mut self.crosshair
            && should_hide_crosshair(*snapshot, now)
        {
            let flags = read_pawn_value(player, HUD_CLASS, HUD_FIELD).map_err(|error| Fault::mechanism(Mechanism::Crosshair, error))? as u32;
            if flags & CROSSHAIR_HUD_BIT == 0 {
                snapshot.hidden_by_us = true;
                let hidden = crosshair_mask(flags, true) as i64;
                write_pawn_value(player, HUD_CLASS, HUD_FIELD, hidden).map_err(|error| Fault::mechanism(Mechanism::Crosshair, error))?;
            }
            snapshot.hide_at = None;
        }
        Ok(())
    }

    pub(crate) fn release_inactive_secondary(&mut self, player: LivePlayer, identity: PlayerIdentity, round: RoundGeneration) -> Result<(), Fault> {
        self.validate(player, identity, round)?;
        let Some(snapshot) = self.secondary else { return Ok(()) };
        let active = s2sdk::GetClientActiveWeapon(player.slot);
        let valid = s2sdk::IsValidEntHandle(snapshot.weapon);
        let (owner, scoped) = if valid {
            (
                s2sdk::GetEntityOwner(snapshot.weapon),
                is_scoped_weapon(s2sdk::GetEntityClassname(snapshot.weapon).as_str()),
            )
        } else {
            (0, false)
        };
        let still_owned_and_active = secondary_is_active_for_player(snapshot, active, owner, player.pawn_handle, valid, scoped);
        if !still_owned_and_active {
            self.restore_secondary_attack()
                .map_err(|error| Fault::mechanism(Mechanism::NoScope, error))?;
        }
        Ok(())
    }

    pub(crate) fn restore(&mut self) -> Result<(), String> {
        let mut errors = Vec::new();
        if let Some(snapshot) = self.fov
            && let Some(applied) = snapshot.applied
            && let Some(player) = same_controller(self.identity.slot, self.controller_handle)
        {
            match read_controller_value(player, FOV_CLASS, FOV_FIELD) {
                Ok(current) if owns_value(current, applied) => {
                    if let Err(error) = write_controller_value(player, FOV_CLASS, FOV_FIELD, snapshot.original) {
                        errors.push(error);
                    }
                }
                Ok(_) => {}
                Err(error) => errors.push(error),
            }
        }
        if let Some(snapshot) = self.crosshair
            && snapshot.hidden_by_us
            && let Some(player) = same_pawn(self.identity.slot, self.identity, self.controller_handle)
        {
            match read_pawn_value(player, HUD_CLASS, HUD_FIELD) {
                Ok(current) if current as u32 & CROSSHAIR_HUD_BIT != 0 => {
                    let restored = crosshair_mask(current as u32, snapshot.originally_hidden);
                    if let Err(error) = write_pawn_value(player, HUD_CLASS, HUD_FIELD, i64::from(restored)) {
                        errors.push(error);
                    }
                }
                Ok(_) => {}
                Err(error) => errors.push(error),
            }
        }
        if let Err(error) = self.restore_secondary_attack() {
            errors.push(error);
        }
        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }

    fn validate(&self, player: LivePlayer, identity: PlayerIdentity, round: RoundGeneration) -> Result<(), Fault> {
        if self.identity != identity
            || self.round != round
            || self.identity.slot != player.slot
            || self.identity.pawn_handle != player.pawn_handle as u32
        {
            return Err(Fault::cancelled("view effect identity or round changed"));
        }
        if LivePlayer::resolve(player.slot) != Some(player) {
            return Err(Fault::cancelled("player identity changed before view effect"));
        }
        Ok(())
    }

    fn update_secondary_attack(&mut self, player: LivePlayer) -> Result<(), Fault> {
        let active = s2sdk::GetClientActiveWeapon(player.slot);
        let scoped = s2sdk::IsValidEntHandle(active)
            && s2sdk::GetEntityOwner(active) == player.pawn_handle
            && is_scoped_weapon(s2sdk::GetEntityClassname(active).as_str());
        if self.secondary.is_some_and(|snapshot| snapshot.weapon != active || !scoped) {
            self.restore_secondary_attack()
                .map_err(|error| Fault::mechanism(Mechanism::NoScope, error))?;
        }
        if !scoped {
            return Ok(());
        }
        let weapon = weapon_pointer(active, player).map_err(|error| Fault::mechanism(Mechanism::NoScope, error))?;
        let current = read_weapon_value(weapon).map_err(|error| Fault::mechanism(Mechanism::NoScope, error))?;
        if self.secondary.is_none() {
            self.secondary = Some(SecondaryAttackSnapshot {
                weapon: active,
                original: current,
                last_written: None,
            });
        }
        let target = blocked_secondary_tick(s2sdk::GetGameTickCount());
        self.secondary.as_mut().expect("scope state was captured").last_written = Some(target);
        if current != target {
            write_weapon_value(weapon, target).map_err(|error| Fault::mechanism(Mechanism::NoScope, error))?;
        }
        Ok(())
    }

    fn restore_secondary_attack(&mut self) -> Result<(), String> {
        let Some(snapshot) = self.secondary else { return Ok(()) };
        let Some(weapon) = weapon_pointer_for_restore(snapshot.weapon) else {
            self.secondary = None;
            return Ok(());
        };
        let current = read_weapon_value(weapon)?;
        if let Some(original) = secondary_tick_to_restore(current, snapshot.last_written, snapshot.original) {
            write_weapon_value(weapon, original).map_err(|_| "secondary-attack tick restoration failed".to_owned())?;
        }
        self.secondary = None;
        Ok(())
    }
}

pub(crate) fn no_scope_probe() -> Probe {
    let mut probe = Probe::default();
    imports!(probe;
        __s2sdk_GetMaxClients, __s2sdk_IsClientInGame, __s2sdk_IsClientAlive,
        __s2sdk_PlayerSlotToEntHandle, __s2sdk_GetClientPawn,
        __s2sdk_GetClientActiveWeapon, __s2sdk_GetEntityOwner, __s2sdk_GetEntityClassname,
        __s2sdk_IsValidEntHandle, __s2sdk_EntHandleToEntPointer, __s2sdk_EntPointerToEntHandle,
        __s2sdk_GetGameTickCount, __s2sdk_GetEntSchema2, __s2sdk_SetEntSchema2,
    );
    probe.schema(SECONDARY_CLASS, SECONDARY_FIELD);
    probe
}

pub(crate) fn fov_probe() -> Probe {
    let mut probe = player_schema_probe();
    probe.schema(FOV_CLASS, FOV_FIELD);
    probe
}

pub(crate) fn crosshair_probe() -> Probe {
    let mut probe = player_schema_probe();
    probe.schema(HUD_CLASS, HUD_FIELD);
    probe
}

fn player_schema_probe() -> Probe {
    let mut probe = Probe::default();
    imports!(probe;
        __s2sdk_GetMaxClients, __s2sdk_PlayerSlotToEntHandle, __s2sdk_GetClientPawn, __s2sdk_IsClientInGame,
        __s2sdk_IsValidEntHandle, __s2sdk_EntHandleToEntPointer, __s2sdk_EntPointerToEntHandle,
        __s2sdk_GetEntSchema2, __s2sdk_SetEntSchema2,
    );
    probe
}

fn controller_pointer(player: LivePlayer) -> Result<Option<usize>, String> {
    let Some(current) = same_controller(player.slot, player.controller_handle) else {
        return Ok(None);
    };
    let pointer = s2sdk::EntHandleToEntPointer(current.controller_handle);
    if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != current.controller_handle {
        return Err("player controller handle changed before FOV access".into());
    }
    Ok(Some(pointer))
}

fn pawn_pointer(player: LivePlayer) -> Result<Option<usize>, String> {
    if LivePlayer::resolve_present(player.slot) != Some(player) {
        return Ok(None);
    }
    let pointer = s2sdk::EntHandleToEntPointer(player.pawn_handle);
    if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != player.pawn_handle {
        return Err("player pawn handle changed before HUD access".into());
    }
    Ok(Some(pointer))
}

fn same_controller(slot: i32, expected_handle: i32) -> Option<LivePlayer> {
    let current = LivePlayer::resolve_present(slot)?;
    (current.controller_handle == expected_handle).then_some(current)
}

fn same_pawn(slot: i32, identity: PlayerIdentity, controller_handle: i32) -> Option<LivePlayer> {
    LivePlayer::resolve_present(slot)
        .filter(|player| player.pawn_handle as u32 == identity.pawn_handle && player.controller_handle == controller_handle)
}

fn read_controller_value(player: LivePlayer, class: &str, field: &str) -> Result<i32, String> {
    let Some(pointer) = controller_pointer(player)? else {
        return Err("player controller unavailable".into());
    };
    let value = s2sdk::GetEntSchema2(pointer, &Str::from_str(class), &Str::from_str(field), 0);
    i32::try_from(value).map_err(|_| format!("invalid {class}.{field} value: {value}"))
}

fn write_controller_value(player: LivePlayer, class: &str, field: &str, value: i32) -> Result<(), String> {
    let Some(pointer) = controller_pointer(player)? else {
        return Err("player controller unavailable".into());
    };
    s2sdk::SetEntSchema2(pointer, &Str::from_str(class), &Str::from_str(field), i64::from(value), true, 0);
    if read_controller_value(player, class, field)? == value {
        Ok(())
    } else {
        Err(format!("{class}.{field} write verification failed"))
    }
}

fn read_pawn_value(player: LivePlayer, class: &str, field: &str) -> Result<i64, String> {
    let Some(pointer) = pawn_pointer(player)? else {
        return Err("player pawn unavailable".into());
    };
    Ok(s2sdk::GetEntSchema2(pointer, &Str::from_str(class), &Str::from_str(field), 0))
}

fn write_pawn_value(player: LivePlayer, class: &str, field: &str, value: i64) -> Result<(), String> {
    let Some(pointer) = pawn_pointer(player)? else {
        return Err("player pawn unavailable".into());
    };
    s2sdk::SetEntSchema2(pointer, &Str::from_str(class), &Str::from_str(field), value, true, 0);
    if read_pawn_value(player, class, field)? == value {
        Ok(())
    } else {
        Err(format!("{class}.{field} write verification failed"))
    }
}

fn weapon_pointer(handle: i32, player: LivePlayer) -> Result<usize, String> {
    if LivePlayer::resolve(player.slot) != Some(player) || !s2sdk::IsValidEntHandle(handle) || s2sdk::GetEntityOwner(handle) != player.pawn_handle {
        return Err("scoped weapon identity changed before secondary-attack access".into());
    }
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != handle {
        return Err("scoped weapon handle changed before secondary-attack access".into());
    }
    Ok(pointer)
}

fn weapon_pointer_for_restore(handle: i32) -> Option<usize> {
    if !s2sdk::IsValidEntHandle(handle) {
        return None;
    }
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    (pointer != 0 && weapon_handle_matches(handle, s2sdk::EntPointerToEntHandle(pointer), true)).then_some(pointer)
}

fn weapon_handle_matches(expected: i32, resolved: i32, valid: bool) -> bool {
    valid && expected == resolved
}

fn read_weapon_value(weapon: usize) -> Result<i32, String> {
    let value = s2sdk::GetEntSchema2(weapon, &Str::from_str(SECONDARY_CLASS), &Str::from_str(SECONDARY_FIELD), 0);
    i32::try_from(value).map_err(|_| format!("invalid {SECONDARY_CLASS}.{SECONDARY_FIELD} value: {value}"))
}

fn write_weapon_value(weapon: usize, value: i32) -> Result<(), String> {
    s2sdk::SetEntSchema2(
        weapon,
        &Str::from_str(SECONDARY_CLASS),
        &Str::from_str(SECONDARY_FIELD),
        i64::from(value),
        true,
        0,
    );
    if read_weapon_value(weapon)? == value {
        Ok(())
    } else {
        Err("secondary-attack tick write verification failed".into())
    }
}

fn is_scoped_weapon(classname: &str) -> bool {
    SCOPED_WEAPONS.contains(&classname)
}

fn blocked_secondary_tick(current_tick: i32) -> i32 {
    current_tick.saturating_add(SECONDARY_BLOCK_TICKS)
}

fn crosshair_mask(current: u32, hidden: bool) -> u32 {
    if hidden {
        current | CROSSHAIR_HUD_BIT
    } else {
        current & !CROSSHAIR_HUD_BIT
    }
}

fn secondary_tick_to_restore(current: i32, last_written: Option<i32>, original: i32) -> Option<i32> {
    (last_written == Some(current)).then_some(original)
}

fn secondary_is_active_for_player(snapshot: SecondaryAttackSnapshot, active: i32, owner: i32, player_pawn: i32, valid: bool, scoped: bool) -> bool {
    valid && scoped && snapshot.weapon == active && owner == player_pawn
}

fn owns_value(current: i32, applied: i32) -> bool {
    current == applied
}

fn should_hide_crosshair(snapshot: CrosshairSnapshot, now: f32) -> bool {
    !snapshot.originally_hidden && !snapshot.hidden_by_us && snapshot.hide_at.is_some_and(|due| now >= due)
}
