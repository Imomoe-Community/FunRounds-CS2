// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    capability::Probe,
    fault::{Fault, Mechanism},
    game::LivePlayer,
    s2sdk,
};
use funrounds_core::{Ammo, Mode};
use plugify::Str;

const WEAPON: &str = "CBasePlayerWeapon";
const CLIP: &str = "m_iClip1";
const RESERVE: &str = "m_pReserveAmmo";
const PRIMARY: &str = "m_nNextPrimaryAttackTick";
const FRACTION: &str = "m_flNextPrimaryAttackTickRatio";
const SECONDARY: &str = "m_nNextSecondaryAttackTick";
const SECONDARY_FRACTION: &str = "m_flNextSecondaryAttackTickRatio";

#[derive(Default)]
pub(super) struct WeaponRules {
    ammo: Vec<AmmoState>,
    active: Option<i32>,
    failed_switch: Option<SwitchRollback>,
    reload_pressed: bool,
}

struct SwitchRollback {
    player: LivePlayer,
    weapon: i32,
    primary: i64,
    fraction: f64,
    secondary: i64,
    secondary_fraction: f64,
    next_attack: f64,
    tick: i64,
    time: f64,
}

struct AmmoState {
    handle: i32,
    original: [i64; 2],
    written: [Option<i64>; 2],
}

impl WeaponRules {
    pub(super) fn release_unowned(&mut self, player: LivePlayer) -> Result<(), Fault> {
        if self.ammo.is_empty() {
            return Ok(());
        }
        let inventory = s2sdk::GetClientWeapons(player.slot);
        for ammo in &mut self.ammo {
            if !inventory.iter().any(|handle| *handle == ammo.handle) {
                ammo.restore().map_err(|error| Fault::mechanism(Mechanism::SingleShot, error))?;
            }
        }
        self.ammo.retain(|ammo| inventory.iter().any(|handle| *handle == ammo.handle));
        Ok(())
    }

    pub(super) fn update(&mut self, player: LivePlayer, mode: &Mode) -> Result<(), Fault> {
        let single_shot = mode.ammo == Ammo::SingleShot;
        if !single_shot && !mode.fast_switch {
            return Ok(());
        }
        if LivePlayer::resolve(player.slot) != Some(player) {
            return Err(Fault::cancelled("player changed before weapon rules"));
        }
        let inventory = s2sdk::GetClientWeapons(player.slot);
        if single_shot {
            let reload_pressed = s2sdk::GetClientButtons(player.slot, 0) & 8192 != 0;
            // Legacy OnReload clears the magazine only for human manual reloads, not Bot auto-reload input.
            let reload_started = reload_pressed && !self.reload_pressed && !s2sdk::IsFakeClient(player.slot);
            self.reload_pressed = reload_pressed;
            let active = s2sdk::GetClientActiveWeapon(player.slot);
            for handle in inventory.iter().copied() {
                if !s2sdk::IsValidEntHandle(handle) || s2sdk::GetEntityOwner(handle) != player.pawn_handle {
                    continue;
                }
                let name = s2sdk::GetEntityClassname(handle);
                if !super::recoil::is_firearm(name.as_str()) || name.as_str() == "weapon_taser" {
                    continue;
                }
                let pointer = weapon_pointer(handle).map_err(|error| Fault::mechanism(Mechanism::SingleShot, error))?;
                let current = [read(pointer, CLIP), read(pointer, RESERVE)];
                if current[0] < 0 {
                    continue;
                }
                let index = self.ammo.iter().position(|ammo| ammo.handle == handle).unwrap_or_else(|| {
                    self.ammo.push(AmmoState {
                        handle,
                        original: current,
                        written: [None; 2],
                    });
                    self.ammo.len() - 1
                });
                // Keep the native reload; an empty magazine is never refilled by us.
                let target = single_shot_ammo(current[0], reload_started && active == handle);
                for (field_index, field) in [CLIP, RESERVE].into_iter().enumerate() {
                    if current[field_index] != target[field_index] {
                        self.ammo[index].written[field_index] = Some(target[field_index]);
                        write(pointer, field, target[field_index]).map_err(|error| Fault::mechanism(Mechanism::SingleShot, error))?;
                    }
                }
            }
        }
        if mode.fast_switch {
            let active = s2sdk::GetClientActiveWeapon(player.slot);
            let owned = inventory.iter().any(|handle| *handle == active)
                && s2sdk::IsValidEntHandle(active)
                && s2sdk::GetEntityOwner(active) == player.pawn_handle;
            if switched(self.active, active, owned) {
                let pointer = weapon_pointer(active).map_err(|error| Fault::mechanism(Mechanism::FastSwitch, error))?;
                let pawn = s2sdk::EntHandleToEntPointer(player.pawn_handle);
                let services = s2sdk::GetEntSchema2(pawn, &Str::from_str("CBasePlayerPawn"), &Str::from_str("m_pWeaponServices"), 0) as usize;
                if services == 0 {
                    return Err(Fault::mechanism(Mechanism::FastSwitch, "weapon services missing"));
                }
                let tick = i64::from(s2sdk::GetGameTickCount());
                let now = f64::from(s2sdk::GetGameTime());
                self.failed_switch = Some(SwitchRollback {
                    player,
                    weapon: active,
                    primary: read(pointer, PRIMARY),
                    secondary: read(pointer, SECONDARY),
                    secondary_fraction: s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str(WEAPON), &Str::from_str(SECONDARY_FRACTION), 0),
                    fraction: s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str(WEAPON), &Str::from_str(FRACTION), 0),
                    next_attack: s2sdk::GetEntSchemaFloat2(services, &Str::from_str("CCSPlayer_WeaponServices"), &Str::from_str("m_flNextAttack"), 0),
                    tick,
                    time: now,
                });
                write(pointer, PRIMARY, tick).map_err(|error| Fault::mechanism(Mechanism::FastSwitch, error))?;
                write(pointer, SECONDARY, tick).map_err(|error| Fault::mechanism(Mechanism::FastSwitch, error))?;
                s2sdk::SetEntSchemaFloat2(pointer, &Str::from_str(WEAPON), &Str::from_str(SECONDARY_FRACTION), 0.0, true, 0);
                s2sdk::SetEntSchemaFloat2(pointer, &Str::from_str(WEAPON), &Str::from_str(FRACTION), 0.0, true, 0);
                s2sdk::SetEntSchemaFloat2(
                    services,
                    &Str::from_str("CCSPlayer_WeaponServices"),
                    &Str::from_str("m_flNextAttack"),
                    now,
                    true,
                    0,
                );
                if s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str(WEAPON), &Str::from_str(SECONDARY_FRACTION), 0) != 0.0
                    || s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str(WEAPON), &Str::from_str(FRACTION), 0) != 0.0
                    || (s2sdk::GetEntSchemaFloat2(services, &Str::from_str("CCSPlayer_WeaponServices"), &Str::from_str("m_flNextAttack"), 0) - now)
                        .abs()
                        > 0.0001
                {
                    return Err(Fault::mechanism(Mechanism::FastSwitch, "attack readiness write failed"));
                }
                // A successful switch is a one-shot time action, not a persistent cooldown override.
                self.failed_switch = None;
            }
            self.active = owned.then_some(active);
        }
        Ok(())
    }

    pub(super) fn restore(&mut self) -> Result<(), String> {
        if let Some(rollback) = &self.failed_switch {
            rollback.restore()?;
        }
        self.failed_switch = None;
        for ammo in &mut self.ammo {
            ammo.restore()?;
        }
        self.ammo.clear();
        self.active = None;
        self.reload_pressed = false;
        Ok(())
    }
}

impl SwitchRollback {
    fn restore(&self) -> Result<(), String> {
        if LivePlayer::resolve_present(self.player.slot) != Some(self.player) {
            return Ok(());
        }
        if s2sdk::IsValidEntHandle(self.weapon) {
            let pointer = weapon_pointer(self.weapon)?;
            if read(pointer, PRIMARY) == self.tick {
                write(pointer, PRIMARY, self.primary)?;
            }
            if read(pointer, SECONDARY) == self.tick {
                write(pointer, SECONDARY, self.secondary)?;
            }
            if s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str(WEAPON), &Str::from_str(SECONDARY_FRACTION), 0) == 0.0 {
                s2sdk::SetEntSchemaFloat2(
                    pointer,
                    &Str::from_str(WEAPON),
                    &Str::from_str(SECONDARY_FRACTION),
                    self.secondary_fraction,
                    true,
                    0,
                );
                if s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str(WEAPON), &Str::from_str(SECONDARY_FRACTION), 0) != self.secondary_fraction {
                    return Err("switch secondary attack fraction rollback failed".into());
                }
            }
            if s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str(WEAPON), &Str::from_str(FRACTION), 0) == 0.0 {
                s2sdk::SetEntSchemaFloat2(pointer, &Str::from_str(WEAPON), &Str::from_str(FRACTION), self.fraction, true, 0);
                if s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str(WEAPON), &Str::from_str(FRACTION), 0) != self.fraction {
                    return Err("switch attack fraction rollback failed".into());
                }
            }
        }
        let pawn = s2sdk::EntHandleToEntPointer(self.player.pawn_handle);
        let services = s2sdk::GetEntSchema2(pawn, &Str::from_str("CBasePlayerPawn"), &Str::from_str("m_pWeaponServices"), 0) as usize;
        if services == 0 {
            return Err("switch rollback weapon services missing".into());
        }
        if s2sdk::GetEntSchemaFloat2(services, &Str::from_str("CCSPlayer_WeaponServices"), &Str::from_str("m_flNextAttack"), 0) == self.time {
            s2sdk::SetEntSchemaFloat2(
                services,
                &Str::from_str("CCSPlayer_WeaponServices"),
                &Str::from_str("m_flNextAttack"),
                self.next_attack,
                true,
                0,
            );
            if s2sdk::GetEntSchemaFloat2(services, &Str::from_str("CCSPlayer_WeaponServices"), &Str::from_str("m_flNextAttack"), 0)
                != self.next_attack
            {
                return Err("switch next attack rollback failed".into());
            }
        }
        Ok(())
    }
}

impl AmmoState {
    fn restore(&mut self) -> Result<(), String> {
        if !s2sdk::IsValidEntHandle(self.handle) {
            self.written = [None; 2];
            return Ok(());
        }
        let pointer = weapon_pointer(self.handle)?;
        for (index, field) in [CLIP, RESERVE].into_iter().enumerate() {
            if restore_value(read(pointer, field), self.written[index], self.original[index]) {
                write(pointer, field, self.original[index])?;
            }
            self.written[index] = None;
        }
        Ok(())
    }
}

fn switched(previous: Option<i32>, active: i32, owned: bool) -> bool {
    owned && previous != Some(active)
}

fn single_shot_ammo(clip: i64, reload_started: bool) -> [i64; 2] {
    // Loaded weapons need no reserve: otherwise native reload keeps trying to fill a full-size magazine.
    if reload_started { [0, 1] } else { [clip.min(1), i64::from(clip == 0)] }
}

fn restore_value(current: i64, written: Option<i64>, original: i64) -> bool {
    written == Some(current) && current != original
}

fn weapon_pointer(handle: i32) -> Result<usize, String> {
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != handle {
        return Err("weapon changed before ammunition/readiness access".into());
    }
    Ok(pointer)
}

fn read(pointer: usize, field: &str) -> i64 {
    s2sdk::GetEntSchema2(pointer, &Str::from_str(WEAPON), &Str::from_str(field), 0)
}

fn write(pointer: usize, field: &str, value: i64) -> Result<(), String> {
    s2sdk::SetEntSchema2(pointer, &Str::from_str(WEAPON), &Str::from_str(field), value, true, 0);
    if read(pointer, field) == value {
        Ok(())
    } else {
        Err(format!("{WEAPON}.{field} write failed"))
    }
}

fn probe() -> Probe {
    let mut probe = Probe::default();
    crate::capability::imports!(probe;
        __s2sdk_GetClientWeapons, __s2sdk_GetClientActiveWeapon, __s2sdk_GetEntityOwner,
        __s2sdk_GetEntityClassname, __s2sdk_IsValidEntHandle, __s2sdk_EntHandleToEntPointer,
        __s2sdk_EntPointerToEntHandle, __s2sdk_GetEntSchema2, __s2sdk_SetEntSchema2,
    );
    probe
}

pub(crate) fn single_shot_probe() -> Probe {
    let mut probe = probe();
    crate::capability::imports!(probe; __s2sdk_GetClientButtons, __s2sdk_IsFakeClient);
    for field in [CLIP, RESERVE] {
        probe.schema(WEAPON, field);
    }
    probe
}

pub(crate) fn fast_switch_probe() -> Probe {
    let mut probe = probe();
    crate::capability::imports!(probe; __s2sdk_GetGameTime, __s2sdk_GetGameTickCount, __s2sdk_GetEntSchemaFloat2, __s2sdk_SetEntSchemaFloat2);
    for field in [PRIMARY, FRACTION, SECONDARY, SECONDARY_FRACTION] {
        probe.schema(WEAPON, field);
    }
    probe.schema("CBasePlayerPawn", "m_pWeaponServices");
    probe.schema("CCSPlayer_WeaponServices", "m_flNextAttack");
    probe
}
