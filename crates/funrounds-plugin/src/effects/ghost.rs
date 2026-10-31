// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    capability::{Probe, imports},
    effects::transmit::{TransmitLease, TransmitPair},
    fault::{Fault, Mechanism},
    funrounds_native,
    game::LivePlayer,
    s2sdk,
};
use funrounds_core::{GhostFrame, GhostPose, GhostSettings, GhostState, PlayerIdentity, RoundGeneration, TeamFilter};
use plugify::{Arr, Str, Vec3};
use std::{
    collections::{BTreeMap, HashMap},
    sync::OnceLock,
};

const IN_ATTACK: u64 = 1 << 0;
const IN_DUCK: u64 = 1 << 2;
const IN_JUMP: u64 = 1 << 1;
const IN_USE: u64 = 1 << 5;
const IN_ATTACK2: u64 = 1 << 11;
const IN_RELOAD: u64 = 1 << 13;
const IN_SPEED: u64 = 1 << 16;
const FL_ONGROUND: i32 = 1 << 0;
const FL_DUCKING: i32 = 1 << 1;
const WEAPON_CLASS: &str = "CBasePlayerWeapon";
const CCS_WEAPON_CLASS: &str = "CCSWeaponBase";
const WEAPON_IN_RELOAD: &str = "m_bInReload";
// Hostage.Breath's shipped soundevent metadata declares a 17.259546-second
// sample. A shorter configured interval stops the old clip before restarting it.
const BREATH_CLIP_SECONDS: f32 = 17.259_546;
const BREATH_SOUND: &str = "Hostage.Breath";

pub(crate) fn visibility_probe() -> Probe {
    let mut probe = Probe::default();
    imports!(probe;
        __s2sdk_GetClientButtons, __s2sdk_GetClientFlags, __s2sdk_GetClientAbsVelocity,
        __s2sdk_GetClientActiveWeapon, __s2sdk_IsValidEntHandle, __s2sdk_GetEntityOwner,
        __s2sdk_GetClientWeapons, __s2sdk_EntHandleToEntPointer, __s2sdk_EntPointerToEntHandle,
        __s2sdk_GetEntSchema2,
        __s2sdk_GetClientTeam, __s2sdk_IsClientInGame, __s2sdk_IsClientAlive, __s2sdk_GetMaxClients,
        __s2sdk_GetHiddenTransmitEntities, __s2sdk_HideTransmitEntity, __s2sdk_ShowTransmitEntity,
        __s2sdk_FindConVar, __s2sdk_GetConVarValue, __s2sdk_SendConVarValue,
        __s2sdk_PlayerSlotToEntHandle,
        __s2sdk_GetClientSteamID64, __s2sdk_IsFakeClient,
        __s2sdk_GetClientPawn, __s2sdk_GetClientHealth, __s2sdk_GetEntityClassname, __s2sdk_GetEntSchemaEnt2,
    );
    probe.require(
        unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_GetSchemaOffset).read().is_some() },
        "import:GetSchemaOffset",
    );
    probe.require(
        reload_schema_class().is_some(),
        "schema:CBasePlayerWeapon.m_bInReload|CCSWeaponBase.m_bInReload",
    );
    let find_convar = unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_FindConVar).read() };
    if let Some(find_convar) = find_convar {
        for name in ["mp_playerid", "mp_teammates_are_enemies"] {
            let handle = unsafe { find_convar(&Str::from_str(name)) };
            probe.require(handle as u16 != u16::MAX, format!("convar:{name}"));
        }
    } else {
        probe.require(false, "convar:mp_playerid");
    }
    let has_support = unsafe { std::ptr::addr_of!(funrounds_native::__funrounds_native_HasGhostVisibilitySupport).read() };
    let set_targets = unsafe { std::ptr::addr_of!(funrounds_native::__funrounds_native_SetGhostVisibilityTargets).read() };
    probe.require(has_support.is_some(), "import:HasGhostVisibilitySupport");
    probe.require(set_targets.is_some(), "import:SetGhostVisibilityTargets");
    if let (Some(has_support), Some(_)) = (has_support, set_targets) {
        probe.require(unsafe { has_support() }, "signature:CCSBot::IsVisiblePlayer");
    }
    let radar_support = unsafe { std::ptr::addr_of!(funrounds_native::__funrounds_native_HasGhostRadarSupport).read() };
    probe.require(radar_support.is_some(), "import:HasGhostRadarSupport");
    probe.require(
        unsafe {
            std::ptr::addr_of!(funrounds_native::__funrounds_native_SetGhostRadarTargets)
                .read()
                .is_some()
        },
        "import:SetGhostRadarTargets",
    );
    if let Some(radar_support) = radar_support {
        probe.require(unsafe { radar_support() }, "native:GhostRadarMessageAndSpottedState");
    }
    probe.include(passive_sound_message_probe());
    probe.include(passive_noise_probe());
    probe
}

fn passive_sound_message_probe() -> Probe {
    let mut probe = Probe::default();
    let support = unsafe { std::ptr::addr_of!(funrounds_native::__funrounds_native_HasGhostPassiveSoundMessageSupport).read() };
    probe.require(support.is_some(), "import:HasGhostPassiveSoundMessageSupport");
    if let Some(support) = support {
        probe.require(unsafe { support() }, "native:GhostPassiveSoundMessages");
    }
    probe
}

fn passive_noise_probe() -> Probe {
    let mut probe = Probe::default();
    let support = unsafe { std::ptr::addr_of!(funrounds_native::__funrounds_native_HasGhostPassiveNoiseSupport).read() };
    probe.require(support.is_some(), "import:HasGhostPassiveNoiseSupport");
    if let Some(support) = support {
        probe.require(unsafe { support() }, "native:GhostPassiveNoise");
    }
    probe
}

pub(crate) fn breath_probe() -> Probe {
    let mut probe = Probe::default();
    imports!(probe; __s2sdk_EmitSoundToClient, __s2sdk_StopSound, __s2sdk_GetClientAbsOrigin, __s2sdk_GetMaxClients, __s2sdk_IsClientInGame);
    probe
}

#[derive(Clone, Copy)]
pub(crate) struct GhostBreathSource {
    pub identity: PlayerIdentity,
    pub round: RoundGeneration,
    pub player: LivePlayer,
    pub settings: GhostSettings,
}

#[derive(Clone, Copy)]
struct BreathState {
    identity: PlayerIdentity,
    round: RoundGeneration,
    player: LivePlayer,
    next_emit_at: f32,
    clip_until: Option<f32>,
}

impl BreathState {
    fn matches(self, source: GhostBreathSource) -> bool {
        self.identity == source.identity && self.round == source.round && self.player == source.player
    }
}

#[derive(Default)]
pub(crate) struct GhostRuntime {
    published_targets: BTreeMap<i32, i32>,
    target_update_pending: bool,
    empty_target_clear_pending: bool,
    transmit: TransmitLease,
    breaths: HashMap<i32, BreathState>,
    identification: IdentificationLease,
    radar_targets: BTreeMap<i32, (i32, u64)>,
    radar_update_pending: bool,
}

impl GhostRuntime {
    pub(crate) fn publish_targets(&mut self, desired: &[(i32, i32)]) -> Result<(), Fault> {
        let desired = normalize_targets(desired.iter().copied());
        self.publish_target_map(desired)?;
        self.sync_transmit_targets()
    }

    fn publish_target_map(&mut self, desired: BTreeMap<i32, i32>) -> Result<(), Fault> {
        if self.empty_target_clear_pending {
            if let Err(error) = publish_native_targets(&[]) {
                self.target_update_pending = true;
                return Err(Fault::mechanism(Mechanism::GhostVisibility, error));
            }
            self.published_targets.clear();
            self.target_update_pending = false;
            self.empty_target_clear_pending = false;
        }
        if desired == self.published_targets && !self.target_update_pending {
            return Ok(());
        }
        diagnostic(format_args!("ghost invisible_targets_request={desired:?}"));
        let handles = desired.values().copied().filter(|handle| *handle != -1).collect::<Vec<_>>();
        if let Err(error) = publish_native_targets(&handles) {
            self.target_update_pending = true;
            return Err(Fault::mechanism(Mechanism::GhostVisibility, error));
        }
        self.published_targets = desired;
        self.target_update_pending = false;
        Ok(())
    }

    pub(crate) fn discard_map_state(&mut self) {
        // A full entity handle can be reused on the next map. Never restore or stop
        // resources through old-map handles; drop their ownership and clear the
        // native exact-handle snapshot instead.
        let had_native_targets = !self.published_targets.is_empty() || self.target_update_pending || self.empty_target_clear_pending;
        if had_native_targets || !self.identification.is_empty() {
            diagnostic(format_args!(
                "ghost map_discard invisible_targets={} name_leases={}",
                self.published_targets.len(),
                self.identification.owned.len()
            ));
        }
        self.breaths.clear();
        let had_radar = !self.radar_targets.is_empty() || self.radar_update_pending;
        self.radar_targets.clear();
        self.radar_update_pending = had_radar;
        if had_radar {
            let _ = self.publish_radar_map(BTreeMap::new());
        }
        // Client overrides belong to a connection; a map transition may already
        // have replaced the recipient, so never send restoration to reused slots.
        self.identification.discard_map_state();
        self.transmit = TransmitLease::default();
        self.published_targets.clear();
        self.target_update_pending = false;
        if !had_native_targets {
            self.empty_target_clear_pending = false;
            return;
        }
        self.empty_target_clear_pending = true;
        if publish_native_targets(&[]).is_ok() {
            self.empty_target_clear_pending = false;
        } else {
            self.target_update_pending = true;
        }
    }

    pub(crate) fn sync_identification(&mut self, sources: &[GhostBreathSource]) -> Result<(), Fault> {
        if self.identification.is_empty() && sources.is_empty() {
            return Ok(());
        }
        let handle = s2sdk::FindConVar(&Str::from_str("mp_playerid"));
        if handle as u16 == u16::MAX {
            return Err(Fault::mechanism(Mechanism::GhostVisibility, "mp_playerid ConVar is unavailable"));
        }
        let server_value = s2sdk::GetConVarValue(handle).to_string();
        let ghosts = sources
            .iter()
            .filter(|source| identification_source_matches(source) && LivePlayer::resolve(source.player.slot) == Some(source.player))
            .filter_map(|source| {
                let team = s2sdk::GetClientTeam(source.player.slot) as i32;
                matches!(team, 2 | 3).then_some((source.player.slot, team))
            })
            .collect::<Vec<_>>();
        let players = (0..s2sdk::GetMaxClients())
            .filter(|slot| s2sdk::IsClientInGame(*slot) && !s2sdk::IsFakeClient(*slot))
            .filter_map(|slot| {
                let steam_id = s2sdk::GetClientSteamID64(slot);
                if steam_id == 0 {
                    return None;
                }
                let player = LivePlayer::resolve(slot)?;
                let team = s2sdk::GetClientTeam(slot) as i32;
                matches!(team, 2 | 3).then_some((slot, team, player, steam_id))
            })
            .collect::<Vec<_>>();
        let present = (0..s2sdk::GetMaxClients())
            .filter_map(current_client_fingerprint)
            .collect::<BTreeMap<_, _>>();
        self.identification
            .sync(handle, &server_value, &ghosts, &players, &present, teammates_are_enemies()?)
    }

    // Radar uses every active Ghost source, not the alpha-zero BOT/transmit list.
    pub(crate) fn sync_radar(&mut self, sources: &[GhostBreathSource]) -> Result<(), Fault> {
        if sources.is_empty() && self.radar_targets.is_empty() && !self.radar_update_pending {
            return Ok(());
        }
        let ghosts = sources
            .iter()
            .filter_map(|source| {
                let player = source.player;
                (identification_source_matches(source)
                    && LivePlayer::resolve(player.slot) == Some(player)
                    && source.settings.team.matches(s2sdk::GetClientTeam(player.slot) as i32))
                .then_some((player.slot, player.pawn_handle))
            })
            .collect::<Vec<_>>();
        let players = (0..s2sdk::GetMaxClients())
            .filter(|&slot| s2sdk::IsClientInGame(slot))
            .map(|slot| (slot, s2sdk::GetClientTeam(slot) as i32, LivePlayer::resolve(slot).is_some()))
            .collect::<Vec<_>>();
        let desired = desired_radar_targets(&ghosts, &players, teammates_are_enemies()?)
            .map_err(|error| Fault::mechanism(Mechanism::GhostVisibility, error))?;
        self.publish_radar_map(desired)
    }

    fn publish_radar_map(&mut self, desired: BTreeMap<i32, (i32, u64)>) -> Result<(), Fault> {
        if desired.is_empty() && self.radar_targets.is_empty() && !self.radar_update_pending {
            return Ok(());
        }
        let changed = desired != self.radar_targets || self.radar_update_pending;
        self.radar_update_pending = true;
        let Some(publish) = (unsafe { std::ptr::addr_of!(funrounds_native::__funrounds_native_SetGhostRadarTargets).read() }) else {
            return Err(Fault::mechanism(
                Mechanism::GhostVisibility,
                "native Ghost radar publisher is unavailable",
            ));
        };
        let handles = Arr::from(desired.values().map(|&(handle, _)| handle).collect::<Vec<_>>());
        let masks = Arr::from(desired.values().map(|&(_, mask)| mask).collect::<Vec<_>>());
        if !unsafe { publish(&handles, &masks) } {
            return Err(Fault::mechanism(
                Mechanism::GhostVisibility,
                "native Ghost radar publication/restore failed",
            ));
        }
        if changed {
            diagnostic(format_args!("ghost radar_targets_request={desired:?}"));
        }
        // Publish unchanged selections too: native refreshes current connection fingerprints.
        self.radar_targets = desired;
        self.radar_update_pending = false;
        Ok(())
    }

    pub(crate) fn sync_breaths(&mut self, sources: &[GhostBreathSource], now: f32) -> Result<(), Fault> {
        if !now.is_finite() {
            return Err(Fault::mechanism(
                Mechanism::GhostBreath,
                "game time is not finite during ghost breath update",
            ));
        }
        let desired = sources
            .iter()
            .copied()
            .filter(|source| source.settings.breath_interval > 0.0 && source.player.pawn_handle != -1)
            .map(|source| (source.player.slot, source))
            .collect::<HashMap<_, _>>();
        let stale = self
            .breaths
            .iter()
            .filter_map(|(slot, breath)| desired.get(slot).is_none_or(|source| !breath.matches(*source)).then_some(*slot))
            .collect::<Vec<_>>();
        for slot in stale {
            self.stop_breath(slot)?;
        }

        for source in desired.values().copied() {
            let slot = source.player.slot;
            self.breaths.entry(slot).or_insert(BreathState {
                identity: source.identity,
                round: source.round,
                player: source.player,
                next_emit_at: now,
                clip_until: None,
            });
            let breath = self.breaths.get_mut(&slot).expect("ghost breath source was inserted");
            if breath.clip_until.is_some_and(|end| now >= end) {
                stop_clip(breath)?;
            }
            if breath.clip_until.is_none() && now >= breath.next_emit_at {
                let recipients = breath_recipients(source.player, source.settings.breath_distance);
                if !recipients.is_empty() {
                    let Some(emit) = (unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_EmitSoundToClient).read() }) else {
                        return Err(Fault::mechanism(Mechanism::GhostBreath, "EmitSoundToClient import disappeared"));
                    };
                    let recipients = Arr::from(recipients);
                    let sound = Str::from_str(BREATH_SOUND);
                    unsafe { emit(source.player.pawn_handle, &recipients, &sound, source.settings.breath_volume, 100.0) };
                    breath.clip_until = Some(now + breath_play_window(source.settings.breath_interval));
                }
                breath.next_emit_at = now + source.settings.breath_interval;
            }
        }
        Ok(())
    }

    pub(crate) fn remove_player(&mut self, slot: i32) -> Result<(), Fault> {
        let mut failure = self.stop_breath(slot).err();
        if self.identification.owned.contains_key(&slot) {
            let handle = s2sdk::FindConVar(&Str::from_str("mp_playerid"));
            if handle as u16 == u16::MAX {
                failure = Some(Fault::mechanism(Mechanism::GhostVisibility, "mp_playerid ConVar is unavailable"));
            } else {
                let value = s2sdk::GetConVarValue(handle).to_string();
                if let Err(error) =
                    self.identification
                        .remove_player(slot, handle, &value, current_client_fingerprint(slot).map(|(_, client)| client))
                {
                    failure = Some(error);
                }
            }
        }
        if self.published_targets.contains_key(&slot) || self.target_update_pending || self.empty_target_clear_pending {
            let mut targets = self.published_targets.clone();
            targets.remove(&slot);
            if let Err(error) = self.publish_target_map(targets)
                && failure.is_none()
            {
                failure = Some(error);
            }
        }
        if !self.radar_targets.is_empty() || self.radar_update_pending {
            let mut desired = self.radar_targets.clone();
            desired.remove(&slot);
            if (0..64).contains(&slot) {
                for (_, mask) in desired.values_mut() {
                    *mask &= !(1_u64 << slot);
                }
                desired.retain(|_, (_, mask)| *mask != 0);
            }
            if let Err(error) = self.publish_radar_map(desired)
                && failure.is_none()
            {
                failure = Some(error);
            }
        }
        if let Err(error) = self.sync_transmit_targets()
            && failure.is_none()
        {
            failure = Some(error);
        }
        failure.map_or(Ok(()), Err)
    }

    pub(crate) fn clear(&mut self) -> Result<(), Fault> {
        let mut failure = None;
        if !self.identification.is_empty() {
            let handle = s2sdk::FindConVar(&Str::from_str("mp_playerid"));
            if handle as u16 == u16::MAX {
                failure = Some(Fault::mechanism(Mechanism::GhostVisibility, "mp_playerid ConVar is unavailable"));
            } else {
                let server_value = s2sdk::GetConVarValue(handle).to_string();
                let present = (0..s2sdk::GetMaxClients())
                    .filter_map(current_client_fingerprint)
                    .collect::<BTreeMap<_, _>>();
                if let Err(error) = self.identification.sync(handle, &server_value, &[], &[], &present, false) {
                    failure = Some(error);
                }
            }
        }
        for slot in self.breaths.keys().copied().collect::<Vec<_>>() {
            if let Err(error) = self.stop_breath(slot)
                && failure.is_none()
            {
                failure = Some(error);
            }
        }
        if let Err(error) = self.publish_target_map(BTreeMap::new())
            && failure.is_none()
        {
            failure = Some(error);
        }
        if let Err(error) = self.publish_radar_map(BTreeMap::new())
            && failure.is_none()
        {
            failure = Some(error);
        }
        if !self.transmit.is_empty() {
            diagnostic(format_args!("ghost transmit_clear_request"));
        }
        if let Err(error) = self.transmit.clear().map_err(|error| Fault::mechanism(Mechanism::GhostVisibility, error))
            && failure.is_none()
        {
            failure = Some(error);
        }
        failure.map_or(Ok(()), Err)
    }

    pub(crate) fn is_clean(&self) -> bool {
        self.published_targets.is_empty()
            && !self.target_update_pending
            && !self.empty_target_clear_pending
            && self.transmit.is_empty()
            && self.breaths.is_empty()
            && self.identification.is_empty()
            && self.radar_targets.is_empty()
            && !self.radar_update_pending
    }

    fn sync_transmit_targets(&mut self) -> Result<(), Fault> {
        if self.published_targets.is_empty() {
            return self.sync_transmit(std::iter::empty());
        }
        let max_clients = s2sdk::GetMaxClients();
        let players = (0..max_clients)
            .filter(|&slot| s2sdk::IsClientInGame(slot))
            .map(|slot| (slot, s2sdk::GetClientTeam(slot) as i32, LivePlayer::resolve(slot).is_some()))
            .collect::<Vec<_>>();
        // A pawn transmit hide does not cover its separately networked weapons.
        // Include only full weapon handles still present in this pawn's inventory
        // whose current entity owner also resolves to the exact pawn.
        let mut targets = Vec::new();
        for (&slot, &handle) in &self.published_targets {
            targets.push((slot, handle));
            let Some(player) = LivePlayer::resolve(slot).filter(|player| player.pawn_handle == handle) else {
                continue;
            };
            targets.extend(super::render::carried_weapons(player).into_iter().map(|weapon| (slot, weapon)));
        }
        let desired = desired_enemy_transmit_pairs(&targets, &players, teammates_are_enemies()?);
        self.sync_transmit(desired)
    }

    fn sync_transmit(&mut self, desired: impl IntoIterator<Item = TransmitPair>) -> Result<(), Fault> {
        let (hide, show) = self
            .transmit
            .sync_with_changes(desired)
            .map_err(|error| Fault::mechanism(Mechanism::GhostVisibility, error))?;
        if !hide.is_empty() || !show.is_empty() {
            diagnostic(format_args!("ghost transmit_requests hide={hide:?} show={show:?}"));
        }
        Ok(())
    }

    fn stop_breath(&mut self, slot: i32) -> Result<(), Fault> {
        let Some(mut breath) = self.breaths.get(&slot).copied() else {
            return Ok(());
        };
        stop_clip(&mut breath)?;
        self.breaths.remove(&slot);
        Ok(())
    }
}

fn identification_source_matches(source: &GhostBreathSource) -> bool {
    source.identity.pawn_handle == source.player.pawn_handle as u32
}

fn current_client_fingerprint(slot: i32) -> Option<(i32, ClientFingerprint)> {
    if !s2sdk::IsClientInGame(slot) {
        return None;
    }
    let controller = s2sdk::PlayerSlotToEntHandle(slot);
    let steam_id = s2sdk::GetClientSteamID64(slot);
    (steam_id != 0 && s2sdk::IsValidEntHandle(controller)).then_some((slot, ClientFingerprint { controller, steam_id }))
}

fn diagnostic(message: std::fmt::Arguments<'_>) {
    if let Some(print) = unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_PrintToServer).read() } {
        unsafe { print(&Str::from_str(&format!("[FunRounds] {message}"))) };
    }
}

fn send_player_id_value(slot: i32, handle: u64, value: &str, server_value: &str) -> Result<(), Fault> {
    let Some(send) = (unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_SendConVarValue).read() }) else {
        return Err(Fault::mechanism(Mechanism::GhostVisibility, "SendConVarValue import is unavailable"));
    };
    diagnostic(format_args!("ghost id_request slot={slot} server={server_value} requested={value}"));
    unsafe { send(slot, handle, &Str::from_str(value)) };
    Ok(())
}

#[derive(Default)]
struct IdentificationLease {
    owned: BTreeMap<i32, OwnedIdentification>,
}

#[derive(Clone, Copy)]
struct ClientFingerprint {
    controller: i32,
    steam_id: u64,
}

#[derive(Clone, Copy)]
struct OwnedIdentification {
    player: LivePlayer,
    steam_id: u64,
    needs_restore: bool,
    value: &'static str,
    server_value: &'static str,
}

impl IdentificationLease {
    fn is_empty(&self) -> bool {
        self.owned.is_empty()
    }

    fn discard_map_state(&mut self) {
        for owned in self.owned.values_mut() {
            owned.needs_restore = true;
        }
    }

    fn remove_player(&mut self, slot: i32, handle: u64, server_value: &str, current: Option<ClientFingerprint>) -> Result<(), Fault> {
        if let Some(owned) = self.owned.get(&slot)
            && current.is_some_and(|client| {
                client.steam_id == owned.steam_id && (owned.needs_restore || client.controller == owned.player.controller_handle)
            })
        {
            send_player_id_value(slot, handle, server_value, server_value)?;
        }
        self.owned.remove(&slot);
        Ok(())
    }

    fn sync(
        &mut self,
        handle: u64,
        server_value: &str,
        ghosts: &[(i32, i32)],
        players: &[(i32, i32, LivePlayer, u64)],
        present: &BTreeMap<i32, ClientFingerprint>,
        ffa: bool,
    ) -> Result<(), Fault> {
        let desired = desired_identification_values(ghosts, players, ffa, server_value);
        let players_by_slot = players
            .iter()
            .map(|&(slot, _, player, steam_id)| (slot, (player, steam_id)))
            .collect::<BTreeMap<_, _>>();
        let mut next = BTreeMap::new();
        for (&slot, &owned) in &self.owned {
            let current = present.get(&slot).copied();
            if owned.needs_restore {
                match current {
                    Some(current) if current.steam_id == owned.steam_id && current.steam_id != 0 => {
                        send_player_id_value(slot, handle, server_value, server_value)?;
                    }
                    Some(_) => {}
                    None => {
                        next.insert(slot, owned);
                    }
                }
            } else if desired.contains_key(&slot)
                && players_by_slot
                    .get(&slot)
                    .is_some_and(|(player, steam_id)| player.controller_handle == owned.player.controller_handle && *steam_id == owned.steam_id)
            {
                let (player, steam_id) = players_by_slot[&slot];
                if desired.get(&slot) == Some(&owned.value) && owned.server_value == server_value.trim() {
                    next.insert(slot, OwnedIdentification { player, steam_id, ..owned });
                }
                // A changed restriction is replaced directly, without briefly restoring 0.
            } else if current.is_some_and(|current| current.controller == owned.player.controller_handle && current.steam_id == owned.steam_id) {
                send_player_id_value(slot, handle, server_value, server_value)?;
            }
        }
        for (slot, value) in desired {
            let (player, steam_id) = players_by_slot[&slot];
            if next.get(&slot).is_some_and(|owned| owned.player == player && owned.steam_id == steam_id) {
                continue;
            }
            send_player_id_value(slot, handle, value, server_value)?;
            next.insert(
                slot,
                OwnedIdentification {
                    player,
                    steam_id,
                    needs_restore: false,
                    value,
                    server_value: if server_value.trim() == "0" { "0" } else { "1" },
                },
            );
        }
        self.owned = next;
        Ok(())
    }
}

pub(crate) fn team_names(settings: GhostSettings) -> Result<Option<(&'static str, &'static str)>, Fault> {
    // mp_teamname_1 is CT; mp_teamname_2 is T.
    let names = match settings.team {
        TeamFilter::Terrorists => ("保卫者", "潜伏者"),
        TeamFilter::CounterTerrorists => ("潜伏者", "保卫者"),
        TeamFilter::None | TeamFilter::All => return Ok(None),
    };
    Ok((!teammates_are_enemies()?).then_some(names))
}

// Read the effective server rule, including inherited settings and external changes.
fn teammates_are_enemies() -> Result<bool, Fault> {
    let handle = s2sdk::FindConVar(&Str::from_str("mp_teammates_are_enemies"));
    if handle as u16 == u16::MAX {
        return Err(Fault::mechanism(Mechanism::GhostVisibility, "mp_teammates_are_enemies is unavailable"));
    }
    match s2sdk::GetConVarValue(handle).to_string().trim() {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err(Fault::mechanism(Mechanism::GhostVisibility, "invalid mp_teammates_are_enemies value")),
    }
}

fn ghost_opponents(owner: i32, owner_team: i32, recipient: i32, recipient_team: i32, ffa: bool) -> bool {
    owner != recipient && matches!(owner_team, 2 | 3) && matches!(recipient_team, 2 | 3) && (ffa || owner_team != recipient_team)
}

fn desired_identification_values(
    ghosts: &[(i32, i32)],
    players: &[(i32, i32, LivePlayer, u64)],
    ffa: bool,
    server_value: &str,
) -> BTreeMap<i32, &'static str> {
    players
        .iter()
        .filter_map(|&(slot, team, _, steam_id)| {
            if steam_id == 0 {
                return None;
            }
            let opponents = ghosts
                .iter()
                .filter(|&&(owner, ghost_team)| ghost_opponents(owner, ghost_team, slot, team, ffa));
            // mp_playerid=1 still allows real-team names. Same-team hostiles need 2.
            let mut value = None;
            for &(_, ghost_team) in opponents {
                value = Some(if ghost_team == team { "2" } else { value.unwrap_or("1") });
            }
            let value = value?;
            let baseline = server_value.trim();
            ((baseline == "0") || (baseline == "1" && value == "2")).then_some((slot, value))
        })
        .collect()
}

fn stop_clip(breath: &mut BreathState) -> Result<(), Fault> {
    if breath.clip_until.is_none() {
        return Ok(());
    }
    let Some(stop) = (unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_StopSound).read() }) else {
        return Err(Fault::mechanism(Mechanism::GhostBreath, "StopSound import disappeared"));
    };
    let sound = Str::from_str(BREATH_SOUND);
    unsafe { stop(breath.player.pawn_handle, &sound) };
    breath.clip_until = None;
    Ok(())
}

fn publish_native_targets(handles: &[i32]) -> Result<(), &'static str> {
    let Some(set_targets) = (unsafe { std::ptr::addr_of!(funrounds_native::__funrounds_native_SetGhostVisibilityTargets).read() }) else {
        return Err("native ghost target publisher is unavailable");
    };
    let handles = Arr::from(handles.to_vec());
    if unsafe { set_targets(&handles) } {
        Ok(())
    } else {
        Err("native ghost target publication failed")
    }
}

fn breath_recipients(source: LivePlayer, distance: f32) -> Vec<i32> {
    let origin = s2sdk::GetClientAbsOrigin(source.slot);
    if !finite_vec3(origin) || !distance.is_finite() || distance < 0.0 {
        return Vec::new();
    }
    let max_distance_squared = distance * distance;
    (0..s2sdk::GetMaxClients())
        .filter(|slot| s2sdk::IsClientInGame(*slot))
        .filter(|slot| within_distance(origin, s2sdk::GetClientAbsOrigin(*slot), max_distance_squared))
        .collect()
}

fn finite_vec3(value: Vec3) -> bool {
    value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
}

fn within_distance(origin: Vec3, target: Vec3, distance_squared: f32) -> bool {
    if !finite_vec3(target) {
        return false;
    }
    let x = origin.x - target.x;
    let y = origin.y - target.y;
    let z = origin.z - target.z;
    x.mul_add(x, y.mul_add(y, z * z)) <= distance_squared
}

fn normalize_targets(targets: impl Iterator<Item = (i32, i32)>) -> BTreeMap<i32, i32> {
    targets.filter(|(_, handle)| *handle != -1).collect()
}

fn breath_play_window(configured_interval: f32) -> f32 {
    configured_interval.min(BREATH_CLIP_SECONDS)
}

fn desired_enemy_transmit_pairs(targets: &[(i32, i32)], players: &[(i32, i32, bool)], ffa: bool) -> Vec<TransmitPair> {
    targets
        .iter()
        .filter(|&&(owner_slot, handle)| {
            handle != -1
                && players
                    .iter()
                    .any(|&(slot, team, alive)| slot == owner_slot && matches!(team, 2 | 3) && alive)
        })
        .flat_map(|&(owner_slot, handle)| {
            let owner_team = players
                .iter()
                .find_map(|&(slot, team, alive)| (slot == owner_slot && alive).then_some(team));
            players.iter().filter_map(move |&(slot, team, alive)| {
                (alive && matches!(team, 2 | 3) && owner_team.is_some_and(|owner_team| ghost_opponents(owner_slot, owner_team, slot, team, ffa)))
                    .then_some(TransmitPair::new(slot, handle))
            })
        })
        .collect()
}

fn desired_radar_targets(ghosts: &[(i32, i32)], players: &[(i32, i32, bool)], ffa: bool) -> Result<BTreeMap<i32, (i32, u64)>, &'static str> {
    let mut desired = BTreeMap::new();
    for &(owner, handle) in ghosts {
        let mut mask = 0_u64;
        for pair in desired_enemy_transmit_pairs(&[(owner, handle)], players, ffa) {
            if !(0..64).contains(&pair.recipient_slot) {
                return Err("Ghost radar supports recipient slots 0 through 63");
            }
            mask |= 1_u64 << pair.recipient_slot;
        }
        if mask != 0 {
            desired.insert(owner, (handle, mask));
        }
    }
    Ok(desired)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GhostEffect {
    state: GhostState,
    alpha: u8,
}

impl Default for GhostEffect {
    fn default() -> Self {
        Self {
            state: GhostState::default(),
            alpha: 127,
        }
    }
}

impl GhostEffect {
    pub(crate) fn reset(&mut self) {
        self.state = GhostState::default();
        self.alpha = 127;
    }

    pub(crate) fn update(&mut self, player: LivePlayer, settings: GhostSettings, now: f32) -> u8 {
        let buttons = s2sdk::GetClientButtons(player.slot, 0);
        let flags = s2sdk::GetClientFlags(player.slot);
        let weapon_reloading = active_weapon_reloading(player);
        let velocity = s2sdk::GetClientAbsVelocity(player.slot);
        let input = frame(buttons, flags, weapon_reloading, velocity);
        let previous = self.alpha;
        self.alpha = self.state.alpha(settings, input, now);
        if previous != self.alpha && (previous == 0 || self.alpha == 0) {
            diagnostic(format_args!(
                "ghost visibility slot={} controller={:#x} pawn={:#x} desired_alpha={previous}->{} speed={:.1} pose={:?} attack={} reload={} use={}",
                player.slot,
                player.controller_handle as u32,
                player.pawn_handle as u32,
                self.alpha,
                input.horizontal_speed,
                input.pose,
                input.attacking,
                input.reloading,
                input.using
            ));
        }
        self.alpha
    }

    pub(crate) fn fully_invisible(&self) -> bool {
        self.alpha == 0
    }
}

fn frame(buttons: u64, flags: i32, weapon_reloading: bool, velocity: Vec3) -> GhostFrame {
    let on_ground = flags & FL_ONGROUND != 0;
    let ducking = if on_ground {
        flags & FL_DUCKING != 0 || buttons & IN_DUCK != 0
    } else {
        buttons & IN_DUCK != 0
    };
    GhostFrame {
        pose: if on_ground && velocity.x.is_finite() && velocity.y.is_finite() && velocity.x * velocity.x + velocity.y * velocity.y < 1.0 {
            GhostPose::Still
        } else if ducking {
            GhostPose::Crouching
        } else if !on_ground {
            GhostPose::Normal
        } else if buttons & IN_SPEED != 0 {
            GhostPose::SlowWalking
        } else {
            GhostPose::Normal
        },
        horizontal_speed: if velocity.x.is_finite() && velocity.y.is_finite() {
            velocity.x.hypot(velocity.y)
        } else {
            f32::NAN
        },
        bunny_hop: buttons & (IN_DUCK | IN_JUMP) == (IN_DUCK | IN_JUMP)
            && velocity.x.is_finite()
            && velocity.y.is_finite()
            && velocity.x.hypot(velocity.y) >= 1.0,
        attacking: buttons & (IN_ATTACK | IN_ATTACK2) != 0,
        reloading: buttons & IN_RELOAD != 0 || weapon_reloading,
        using: buttons & IN_USE != 0,
    }
}

fn active_weapon_reloading(player: LivePlayer) -> bool {
    let Some(class) = reload_schema_class() else { return false };
    let weapon = s2sdk::GetClientActiveWeapon(player.slot);
    if !s2sdk::IsValidEntHandle(weapon) || s2sdk::GetEntityOwner(weapon) != player.pawn_handle {
        return false;
    }
    let pointer = s2sdk::EntHandleToEntPointer(weapon);
    if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != weapon {
        return false;
    }
    s2sdk::GetEntSchema2(pointer, &Str::from_str(class), &Str::from_str(WEAPON_IN_RELOAD), 0) != 0
}

fn reload_schema_class() -> Option<&'static str> {
    static CLASS: OnceLock<Option<&'static str>> = OnceLock::new();
    *CLASS.get_or_init(|| {
        let lookup = unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_GetSchemaOffset).read() }?;
        [WEAPON_CLASS, CCS_WEAPON_CLASS]
            .into_iter()
            .find(|&class| unsafe { lookup(&Str::from_str(class), &Str::from_str(WEAPON_IN_RELOAD)) } >= 0)
    })
}
