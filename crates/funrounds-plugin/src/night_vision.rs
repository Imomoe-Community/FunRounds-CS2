// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    capability::{Probe, imports},
    effects::transmit::{TransmitLease, TransmitPair},
    game::{self, LivePlayer},
    s2sdk,
};
use plugify::Str;
use std::collections::HashMap;

const POST_PROCESSING_VOLUME: &str = "CPostProcessingVolume";
const EXPOSURE: f64 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OwnedVolume {
    player: LivePlayer,
    handle: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RetiredVolume {
    slot: i32,
    handle: i32,
    removal_queued: bool,
}

pub(crate) struct NightVision {
    volumes: HashMap<i32, OwnedVolume>,
    retired: Vec<RetiredVolume>,
    transmit: TransmitLease,
    transmit_synced: bool,
}

impl Default for NightVision {
    fn default() -> Self {
        Self {
            volumes: HashMap::new(),
            retired: Vec::new(),
            transmit: TransmitLease::default(),
            // Unused night vision must add no SDK import or cleanup prerequisite.
            transmit_synced: true,
        }
    }
}

pub(crate) fn probe() -> Probe {
    let mut probe = Probe::default();
    imports!(probe;
        __s2sdk_CreateEntityByName, __s2sdk_DispatchSpawn, __s2sdk_RemoveEntity,
        __s2sdk_IsValidEntHandle, __s2sdk_EntHandleToEntPointer, __s2sdk_EntPointerToEntHandle,
        __s2sdk_GetEntityClassname, __s2sdk_GetSchemaOffset,
        __s2sdk_GetEntSchema2, __s2sdk_SetEntSchema2,
        __s2sdk_GetEntSchemaFloat2, __s2sdk_SetEntSchemaFloat2,
        __s2sdk_GetMaxClients, __s2sdk_IsClientInGame, __s2sdk_IsClientAlive,
        __s2sdk_GetHiddenTransmitEntities, __s2sdk_HideTransmitEntity, __s2sdk_ShowTransmitEntity,
    );
    for field in [
        "m_bMaster",
        "m_bExposureControl",
        "m_flMinExposure",
        "m_flMaxExposure",
        "m_flFadeDuration",
    ] {
        probe.schema(POST_PROCESSING_VOLUME, field);
    }
    probe
}

impl NightVision {
    pub(crate) fn discard_map_state(&mut self) {
        self.volumes.clear();
        self.retired.clear();
        self.transmit = TransmitLease::default();
        self.transmit_synced = true;
    }

    pub(crate) fn toggle(&mut self, player: LivePlayer) -> Result<bool, String> {
        if !probe().available() {
            return Err("post-processing night vision is unavailable in this server build".into());
        }
        if LivePlayer::resolve(player.slot) != Some(player) || !s2sdk::IsClientAlive(player.slot) {
            return Err("night vision requires the current live player pawn".into());
        }

        if self.volumes.get(&player.slot).is_some_and(|volume| volume.player == player) {
            self.remove_player(player.slot)?;
            return Ok(false);
        }
        if self.volumes.contains_key(&player.slot) {
            self.remove_player(player.slot)?;
        }
        self.retry_removals();
        if self.retired.iter().any(|volume| volume.slot == player.slot) {
            return Err("previous night-vision volume is still being removed".into());
        }

        let handle = s2sdk::CreateEntityByName(&Str::from_str("post_processing_volume"));
        if !s2sdk::IsValidEntHandle(handle) {
            return Err("night-vision volume creation failed".into());
        }
        if let Err(error) = initialize_volume(handle) {
            self.retire(player.slot, handle);
            let _ = self.sync_hidden();
            self.queue_removals();
            return Err(error);
        }
        if LivePlayer::resolve(player.slot) != Some(player) || !s2sdk::IsClientAlive(player.slot) {
            self.retire(player.slot, handle);
            let _ = self.sync_hidden();
            self.queue_removals();
            return Err("player identity changed while creating night vision".into());
        }

        self.volumes.insert(player.slot, OwnedVolume { player, handle });
        if let Err(error) = self.sync_hidden() {
            self.volumes.remove(&player.slot);
            self.retire(player.slot, handle);
            let _ = self.sync_hidden();
            self.queue_removals();
            return Err(error);
        }
        Ok(true)
    }

    pub(crate) fn remove_player(&mut self, slot: i32) -> Result<(), String> {
        let volume = self.volumes.remove(&slot);
        if let Some(volume) = volume {
            self.retire(slot, volume.handle);
        }
        let mut sync_error = self.sync_hidden().err();
        self.queue_removals();
        self.retry_removals();
        if let Err(error) = self.sync_hidden()
            && sync_error.is_none()
        {
            sync_error = Some(error);
        }
        sync_error.map_or(Ok(()), Err)
    }

    pub(crate) fn cleanup_all(&mut self) -> Result<(), String> {
        let active = std::mem::take(&mut self.volumes);
        for (slot, volume) in active {
            self.retire(slot, volume.handle);
        }
        let mut sync_error = self.sync_hidden().err();
        self.queue_removals();
        self.retry_removals();
        if let Err(error) = self.sync_hidden()
            && sync_error.is_none()
        {
            sync_error = Some(error);
        }
        sync_error.map_or(Ok(()), Err)
    }

    /// Called on the game thread. Reconciles current recipient slots and retires volumes
    /// whose owner disconnected, died, or had the slot/pawn reused.
    pub(crate) fn update(&mut self) -> Result<(), String> {
        let stale_slots = self
            .volumes
            .iter()
            .filter_map(|(slot, volume)| {
                (!owner_is_current(volume.player, LivePlayer::resolve(*slot), s2sdk::IsClientAlive(*slot))
                    || !s2sdk::IsValidEntHandle(volume.handle)
                    || s2sdk::GetEntityClassname(volume.handle).as_str() != "post_processing_volume")
                    .then_some(*slot)
            })
            .collect::<Vec<_>>();
        for slot in stale_slots {
            self.remove_player(slot)?;
        }

        self.retry_removals();
        self.sync_hidden()
    }

    pub(crate) fn is_clean(&self) -> bool {
        self.volumes.is_empty() && self.retired.is_empty() && self.transmit.is_empty() && self.transmit_synced
    }

    fn sync_hidden(&mut self) -> Result<(), String> {
        if self.volumes.is_empty() && self.retired.is_empty() && self.transmit.is_empty() {
            self.transmit_synced = true;
            return Ok(());
        }
        let max_clients = s2sdk::GetMaxClients();
        let mut desired = self
            .volumes
            .values()
            .filter(|volume| s2sdk::IsValidEntHandle(volume.handle))
            .flat_map(|volume| hidden_recipients(volume.player.slot, volume.handle, max_clients, s2sdk::IsClientInGame))
            .collect::<Vec<_>>();
        desired.extend(
            self.retired
                .iter()
                .filter(|volume| s2sdk::IsValidEntHandle(volume.handle))
                .flat_map(|volume| retired_recipients(volume.handle, max_clients, s2sdk::IsClientInGame)),
        );
        match self.transmit.sync(desired) {
            Ok(()) => {
                self.transmit_synced = true;
                Ok(())
            }
            Err(error) => {
                self.transmit_synced = false;
                Err(error)
            }
        }
    }

    fn retire(&mut self, slot: i32, handle: i32) {
        if self.retired.iter().any(|volume| volume.handle == handle) {
            return;
        }
        self.retired.push(RetiredVolume {
            slot,
            handle,
            removal_queued: false,
        });
    }

    fn queue_removals(&mut self) {
        for volume in &mut self.retired {
            if !volume.removal_queued {
                if s2sdk::IsValidEntHandle(volume.handle) {
                    s2sdk::RemoveEntity(volume.handle);
                }
                volume.removal_queued = true;
            }
        }
    }

    fn retry_removals(&mut self) {
        self.retired.retain(|volume| !game::engine_removal_acknowledged(volume.handle));
    }
}

fn initialize_volume(handle: i32) -> Result<(), String> {
    if !s2sdk::IsValidEntHandle(handle) {
        return Err("night-vision volume became invalid before spawn".into());
    }
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    if pointer == 0 {
        return Err("night-vision volume pointer is unavailable before spawn".into());
    }

    s2sdk::SetEntSchema2(pointer, &Str::from_str(POST_PROCESSING_VOLUME), &Str::from_str("m_bMaster"), 1, false, 0);
    s2sdk::SetEntSchema2(
        pointer,
        &Str::from_str(POST_PROCESSING_VOLUME),
        &Str::from_str("m_bExposureControl"),
        1,
        false,
        0,
    );
    s2sdk::SetEntSchemaFloat2(
        pointer,
        &Str::from_str(POST_PROCESSING_VOLUME),
        &Str::from_str("m_flFadeDuration"),
        0.0,
        false,
        0,
    );
    s2sdk::SetEntSchemaFloat2(
        pointer,
        &Str::from_str(POST_PROCESSING_VOLUME),
        &Str::from_str("m_flMinExposure"),
        EXPOSURE,
        false,
        0,
    );
    s2sdk::SetEntSchemaFloat2(
        pointer,
        &Str::from_str(POST_PROCESSING_VOLUME),
        &Str::from_str("m_flMaxExposure"),
        EXPOSURE,
        false,
        0,
    );

    s2sdk::DispatchSpawn(handle);
    if !s2sdk::IsValidEntHandle(handle) {
        return Err("night-vision volume disappeared during spawn".into());
    }
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != handle {
        return Err("night-vision volume identity failed after spawn".into());
    }
    if s2sdk::GetEntityClassname(handle).as_str() != "post_processing_volume"
        || s2sdk::GetEntSchema2(pointer, &Str::from_str(POST_PROCESSING_VOLUME), &Str::from_str("m_bMaster"), 0) != 1
        || s2sdk::GetEntSchema2(pointer, &Str::from_str(POST_PROCESSING_VOLUME), &Str::from_str("m_bExposureControl"), 0) != 1
        || !near(
            s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str(POST_PROCESSING_VOLUME), &Str::from_str("m_flFadeDuration"), 0),
            0.0,
        )
        || !near(
            s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str(POST_PROCESSING_VOLUME), &Str::from_str("m_flMinExposure"), 0),
            EXPOSURE,
        )
        || !near(
            s2sdk::GetEntSchemaFloat2(pointer, &Str::from_str(POST_PROCESSING_VOLUME), &Str::from_str("m_flMaxExposure"), 0),
            EXPOSURE,
        )
    {
        return Err("night-vision volume schema readback failed".into());
    }
    Ok(())
}

fn near(actual: f64, expected: f64) -> bool {
    actual.is_finite() && (actual - expected).abs() <= 0.0001
}

fn hidden_recipients(
    owner_slot: i32,
    volume_handle: i32,
    max_clients: i32,
    mut is_client_in_game: impl FnMut(i32) -> bool,
) -> impl Iterator<Item = TransmitPair> {
    (0..max_clients)
        .filter(move |slot| *slot != owner_slot && is_client_in_game(*slot))
        .map(move |slot| TransmitPair::new(slot, volume_handle))
}

fn retired_recipients(volume_handle: i32, max_clients: i32, mut is_client_in_game: impl FnMut(i32) -> bool) -> impl Iterator<Item = TransmitPair> {
    (0..max_clients)
        .filter(move |slot| is_client_in_game(*slot))
        .map(move |slot| TransmitPair::new(slot, volume_handle))
}

fn owner_is_current(expected: LivePlayer, current: Option<LivePlayer>, alive: bool) -> bool {
    alive && current == Some(expected)
}
