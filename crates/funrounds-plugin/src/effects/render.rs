// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    capability::{Probe, imports},
    fault::{Fault, Mechanism},
    game::LivePlayer,
    s2sdk,
};
use funrounds_core::{PlayerIdentity, RoundGeneration};
use plugify::Str;
use std::collections::{HashMap, HashSet};

const MODEL_CLASS: &str = "CBaseModelEntity";
const COLOR_FIELD: &str = "m_clrRender";
const MODE_FIELD: &str = "m_nRenderMode";
const TRANSLUCENT_MODE: u8 = 4;
const TRANSLUCENT_ALPHA: u8 = 127;

#[derive(Clone, Copy)]
struct RenderSnapshot {
    mode: u8,
    alpha: u8,
}

#[derive(Clone, Copy)]
struct RenderValues {
    mode: u8,
    color: u32,
}

pub(crate) struct Translucent {
    player: LivePlayer,
    identity: PlayerIdentity,
    round: RoundGeneration,
    body: Option<RenderSnapshot>,
    weapons: HashMap<i32, RenderSnapshot>,
}

impl Translucent {
    pub(crate) fn new(player: LivePlayer, identity: PlayerIdentity, round: RoundGeneration) -> Self {
        Self {
            player,
            identity,
            round,
            body: None,
            weapons: HashMap::new(),
        }
    }

    pub(crate) fn release_unowned_weapons(&mut self, round: RoundGeneration) -> Result<(), Fault> {
        if self.identity.slot != self.player.slot
            || self.identity.pawn_handle != self.player.pawn_handle as u32
            || self.round != round
            || LivePlayer::resolve(self.player.slot) != Some(self.player)
        {
            return self.restore().map_err(translucent_fault);
        }

        let owned = carried_weapons(self.player);
        let stale = self.weapons.keys().copied().filter(|handle| !owned.contains(handle)).collect::<Vec<_>>();
        let mut errors = Vec::new();
        for handle in stale {
            let snapshot = self.weapons[&handle];
            match restore_entity(handle, snapshot) {
                Ok(()) => {
                    self.weapons.remove(&handle);
                }
                Err(error) => errors.push(error),
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(translucent_fault(errors.join("; ")))
        }
    }

    pub(crate) fn update(&mut self, identity: PlayerIdentity, round: RoundGeneration) -> Result<(), Fault> {
        if identity != self.identity || round != self.round || LivePlayer::resolve(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("translucent effect identity or round changed"));
        }

        if self.body.is_none() {
            self.body = Some(capture_entity(self.player.pawn_handle).map_err(translucent_fault)?);
        }
        apply_entity(self.player.pawn_handle).map_err(translucent_fault)?;

        for handle in carried_weapons(self.player) {
            if let std::collections::hash_map::Entry::Vacant(entry) = self.weapons.entry(handle) {
                entry.insert(capture_entity(handle).map_err(translucent_fault)?);
            }
            apply_entity(handle).map_err(translucent_fault)?;
        }
        Ok(())
    }

    pub(crate) fn restore(&mut self) -> Result<(), String> {
        let mut errors = Vec::new();
        if let Some(snapshot) = self.body {
            match restore_entity(self.player.pawn_handle, snapshot) {
                Ok(()) => self.body = None,
                Err(error) => errors.push(error),
            }
        }
        let weapons = self.weapons.iter().map(|(handle, snapshot)| (*handle, *snapshot)).collect::<Vec<_>>();
        for (handle, snapshot) in weapons {
            match restore_entity(handle, snapshot) {
                Ok(()) => {
                    self.weapons.remove(&handle);
                }
                Err(error) => errors.push(error),
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }
}

pub(crate) fn probe() -> Probe {
    let mut probe = Probe::default();
    imports!(probe;
        __s2sdk_GetClientWeapons, __s2sdk_GetEntityOwner, __s2sdk_IsValidEntHandle,
        __s2sdk_EntHandleToEntPointer, __s2sdk_EntPointerToEntHandle,
        __s2sdk_GetEntSchema2, __s2sdk_SetEntSchema2,
    );
    probe.schema(MODEL_CLASS, COLOR_FIELD);
    probe.schema(MODEL_CLASS, MODE_FIELD);
    probe
}

fn carried_weapons(player: LivePlayer) -> HashSet<i32> {
    let inventory = s2sdk::GetClientWeapons(player.slot).iter().copied().collect::<HashSet<_>>();
    inventory
        .iter()
        .copied()
        .filter(|handle| s2sdk::IsValidEntHandle(*handle) && is_carried_by(*handle, s2sdk::GetEntityOwner(*handle), player.pawn_handle, &inventory))
        .collect()
}

fn capture_entity(handle: i32) -> Result<RenderSnapshot, String> {
    let values = read_entity(handle)?;
    Ok(RenderSnapshot {
        mode: values.mode,
        alpha: alpha(values.color),
    })
}

fn apply_entity(handle: i32) -> Result<(), String> {
    let current = read_entity(handle)?;
    if alpha(current.color) != TRANSLUCENT_ALPHA {
        write_color(handle, set_alpha(current.color, TRANSLUCENT_ALPHA))?;
    }
    if current.mode != TRANSLUCENT_MODE {
        write_mode(handle, TRANSLUCENT_MODE)?;
    }
    let actual = read_entity(handle)?;
    if actual.mode == TRANSLUCENT_MODE && alpha(actual.color) == TRANSLUCENT_ALPHA {
        Ok(())
    } else {
        Err(format!("render write verification failed for handle {handle}"))
    }
}

fn restore_entity(handle: i32, snapshot: RenderSnapshot) -> Result<(), String> {
    let Some(_) = entity_pointer_for_restore(handle)? else { return Ok(()) };
    let current = read_entity(handle)?;
    let (mode, color) = values_to_restore(snapshot, current);
    if let Some(mode) = mode
        && mode != current.mode
    {
        write_mode(handle, mode)?;
    }
    if let Some(color) = color
        && color != current.color
    {
        write_color(handle, color)?;
    }
    let actual = read_entity(handle)?;
    if mode.is_some_and(|mode| actual.mode != mode) || color.is_some_and(|color| actual.color != color) {
        Err(format!("render restoration verification failed for handle {handle}"))
    } else {
        Ok(())
    }
}

fn values_to_restore(snapshot: RenderSnapshot, current: RenderValues) -> (Option<u8>, Option<u32>) {
    let mode = (current.mode == TRANSLUCENT_MODE).then_some(snapshot.mode);
    let color = (alpha(current.color) == TRANSLUCENT_ALPHA).then_some(set_alpha(current.color, snapshot.alpha));
    (mode, color)
}

fn decode_render_mode(value: i64) -> Result<u8, ()> {
    i8::try_from(value).map(|value| value as u8).map_err(|_| ())
}

fn decode_render_color(value: i64) -> Result<u32, ()> {
    i32::try_from(value).map(|value| value as u32).map_err(|_| ())
}

fn read_entity(handle: i32) -> Result<RenderValues, String> {
    let pointer = entity_pointer(handle)?;
    let mode = s2sdk::GetEntSchema2(pointer, &Str::from_str(MODEL_CLASS), &Str::from_str(MODE_FIELD), 0);
    let mode = decode_render_mode(mode).map_err(|()| format!("invalid {MODEL_CLASS}.{MODE_FIELD} for handle {handle}"))?;
    let color = s2sdk::GetEntSchema2(pointer, &Str::from_str(MODEL_CLASS), &Str::from_str(COLOR_FIELD), 0);
    let color = decode_render_color(color).map_err(|()| format!("invalid {MODEL_CLASS}.{COLOR_FIELD} for handle {handle}"))?;
    Ok(RenderValues { mode, color })
}

fn write_mode(handle: i32, value: u8) -> Result<(), String> {
    let pointer = entity_pointer(handle)?;
    s2sdk::SetEntSchema2(
        pointer,
        &Str::from_str(MODEL_CLASS),
        &Str::from_str(MODE_FIELD),
        i64::from(value as i8),
        true,
        0,
    );
    if read_entity(handle)?.mode == value {
        Ok(())
    } else {
        Err(format!("{MODEL_CLASS}.{MODE_FIELD} write failed for handle {handle}"))
    }
}

fn write_color(handle: i32, value: u32) -> Result<(), String> {
    let pointer = entity_pointer(handle)?;
    s2sdk::SetEntSchema2(
        pointer,
        &Str::from_str(MODEL_CLASS),
        &Str::from_str(COLOR_FIELD),
        i64::from(value as i32),
        true,
        0,
    );
    if read_entity(handle)?.color == value {
        Ok(())
    } else {
        Err(format!("{MODEL_CLASS}.{COLOR_FIELD} write failed for handle {handle}"))
    }
}

fn entity_pointer(handle: i32) -> Result<usize, String> {
    if !s2sdk::IsValidEntHandle(handle) {
        return Err(format!("render entity handle {handle} is invalid"));
    }
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != handle {
        Err(format!("render entity handle {handle} changed before access"))
    } else {
        Ok(pointer)
    }
}

fn entity_pointer_for_restore(handle: i32) -> Result<Option<usize>, String> {
    if !s2sdk::IsValidEntHandle(handle) {
        return Ok(None);
    }
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    if pointer == 0 {
        return Err(format!("render entity handle {handle} could not be resolved for restoration"));
    }
    Ok((s2sdk::EntPointerToEntHandle(pointer) == handle).then_some(pointer))
}

fn alpha(color: u32) -> u8 {
    (color >> 24) as u8
}

fn set_alpha(color: u32, alpha: u8) -> u32 {
    (color & 0x00ff_ffff) | (u32::from(alpha) << 24)
}

fn translucent_fault(error: impl Into<String>) -> Fault {
    Fault::mechanism(Mechanism::Translucent, error)
}

fn is_carried_by(handle: i32, owner: i32, player_pawn: i32, inventory: &HashSet<i32>) -> bool {
    inventory.contains(&handle) && owner == player_pawn
}
