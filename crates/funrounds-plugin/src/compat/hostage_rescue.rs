// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    capability::{Probe, imports},
    game::{self, LivePlayer},
    s2sdk,
};
use funrounds_core::PlayerTask;
use plugify::Str;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ForwardAction {
    pub task: PlayerTask,
    pub player: LivePlayer,
    pub trigger: i32,
    pub hostage: i32,
}

#[derive(Default)]
pub(crate) struct State {
    attempts: Vec<ForwardAction>,
    available: bool,
    unavailable_reason: String,
    failure: Option<String>,
}

impl State {
    pub fn new() -> Self {
        let probe = probe();
        let available = probe.available();
        let mut reasons = Vec::new();
        let _ = probe.clone().record("hostage_rescue_compat", &mut reasons);
        Self {
            attempts: Vec::new(),
            available,
            unavailable_reason: reasons.join(";"),
            failure: None,
        }
    }

    pub fn available(&self) -> bool {
        self.available
    }

    pub fn reserve(&mut self, action: ForwardAction) -> bool {
        if self.attempts.contains(&action) {
            return false;
        }
        self.attempts.push(action);
        true
    }

    pub fn clear_attempts(&mut self) {
        self.attempts.clear();
    }

    pub fn set_failure(&mut self, reason: impl Into<String>) {
        self.failure = Some(reason.into());
    }

    pub fn clear_failure(&mut self) {
        self.failure = None;
    }

    pub fn status(&self, enabled: bool, active_round: bool) -> (bool, String) {
        if !self.available {
            return (false, self.unavailable_reason.clone());
        }
        if let Some(reason) = &self.failure {
            return (true, reason.clone());
        }
        if !enabled {
            return (true, "disabled by configuration".into());
        }
        if !active_round {
            return (true, "waiting for an active FunRounds round".into());
        }
        match enabled_rescue_zones() {
            Ok(zones) if !zones.is_empty() => (true, "ready".into()),
            Ok(_) => (true, "no enabled func_hostage_rescue on this map".into()),
            Err(reason) => (true, reason),
        }
    }
}

pub(crate) fn probe() -> Probe {
    let mut probe = crate::touch::probe();
    imports!(probe;
        __s2sdk_AcceptEntityInput,
        __s2sdk_FindConVar, __s2sdk_GetConVarValue,
        __s2sdk_GetGameRules,
        __s2sdk_GetFirstActiveEntity, __s2sdk_GetNextActiveEntity, __s2sdk_GetEntityClassname,
        __s2sdk_GetEntityParent,
        __s2sdk_IsValidEntHandle, __s2sdk_EntHandleToEntPointer, __s2sdk_EntPointerToEntHandle,
        __s2sdk_GetEntSchema2, __s2sdk_GetEntSchemaEnt2, __s2sdk_GetSchemaOffset,
        __s2sdk_GetMaxClients, __s2sdk_IsClientInGame, __s2sdk_IsClientAlive, __s2sdk_GetClientHealth,
        __s2sdk_GetClientPawn, __s2sdk_PlayerSlotToEntHandle, __s2sdk_GetClientTeam,
    );
    for (class, field) in [
        ("CCSGameRules", "m_bMapHasRescueZone"),
        ("CBaseTrigger", "m_bDisabled"),
        ("CCSPlayerPawn", "m_pHostageServices"),
        ("CBasePlayerPawn", "m_hController"),
        ("CCSPlayer_HostageServices", "m_hCarriedHostage"),
        ("CCSPlayer_HostageServices", "m_hCarriedHostageProp"),
        ("CHostage", "m_isRescued"),
        ("CHostage", "m_nHostageState"),
    ] {
        probe.schema(class, field);
    }
    probe
}

pub(crate) fn is_playing_team(team: i32) -> bool {
    matches!(team, 2 | 3)
}

pub(crate) fn is_custom_game(game_type: i32, game_mode: i32) -> bool {
    game_type == 3 && game_mode == 0
}

fn convar_value(name: &str) -> Option<i32> {
    let handle = s2sdk::FindConVar(&Str::from_str(name));
    if handle == 0 {
        return None;
    }
    s2sdk::GetConVarValue(handle).as_str().parse().ok()
}

fn custom_game() -> Result<(), String> {
    let game_type = convar_value("game_type").ok_or_else(|| "game_type unavailable".to_string())?;
    let game_mode = convar_value("game_mode").ok_or_else(|| "game_mode unavailable".to_string())?;
    if is_custom_game(game_type, game_mode) {
        Ok(())
    } else {
        Err(format!("requires game_type=3 game_mode=0; found {game_type}/{game_mode}"))
    }
}

fn rescue_flag() -> Result<bool, String> {
    let rules = s2sdk::GetGameRules();
    if rules == 0 {
        return Err("CCSGameRules unavailable".into());
    }
    Ok(s2sdk::GetEntSchema2(rules, &Str::from_str("CCSGameRules"), &Str::from_str("m_bMapHasRescueZone"), 0) != 0)
}

pub(crate) fn enabled_rescue_zones() -> Result<Vec<i32>, String> {
    custom_game()?;
    if !rescue_flag()? {
        return Err("m_bMapHasRescueZone is false".into());
    }
    Ok(game::entity_handles("func_hostage_rescue")
        .into_iter()
        .filter(|handle| s2sdk::IsValidEntHandle(*handle))
        .filter(|handle| {
            let entity = s2sdk::EntHandleToEntPointer(*handle);
            entity != 0
                && s2sdk::EntPointerToEntHandle(entity) == *handle
                && s2sdk::GetEntSchema2(entity, &Str::from_str("CBaseTrigger"), &Str::from_str("m_bDisabled"), 0) == 0
        })
        .collect())
}

fn entity_pointer(handle: i32) -> Option<usize> {
    if !s2sdk::IsValidEntHandle(handle) {
        return None;
    }
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    (pointer != 0 && s2sdk::EntPointerToEntHandle(pointer) == handle).then_some(pointer)
}

fn read_handle(pointer: usize, class: &str, field: &str) -> Option<i32> {
    let handle = s2sdk::GetEntSchemaEnt2(pointer, &Str::from_str(class), &Str::from_str(field), 0);
    s2sdk::IsValidEntHandle(handle).then_some(handle)
}

pub(crate) fn eligible_hostage(trigger: i32, player: LivePlayer) -> Option<i32> {
    custom_game().ok()?;
    if !rescue_flag().ok()? {
        return None;
    }
    let trigger_pointer = entity_pointer(trigger)?;
    if s2sdk::GetEntityClassname(trigger).as_str() != "func_hostage_rescue"
        || s2sdk::GetEntSchema2(trigger_pointer, &Str::from_str("CBaseTrigger"), &Str::from_str("m_bDisabled"), 0) != 0
    {
        return None;
    }
    let pawn = entity_pointer(player.pawn_handle)?;
    if s2sdk::GetEntityClassname(player.pawn_handle).as_str() != "player" {
        return None;
    }
    let services = s2sdk::GetEntSchema2(pawn, &Str::from_str("CCSPlayerPawn"), &Str::from_str("m_pHostageServices"), 0) as usize;
    if services == 0 {
        return None;
    }
    let hostage_handle = read_handle(services, "CCSPlayer_HostageServices", "m_hCarriedHostage")?;
    let prop_handle = read_handle(services, "CCSPlayer_HostageServices", "m_hCarriedHostageProp")?;
    let hostage = entity_pointer(hostage_handle)?;
    entity_pointer(prop_handle)?;
    let parent_handle = s2sdk::GetEntityParent(prop_handle);
    let parent = entity_pointer(parent_handle)?;
    if s2sdk::GetEntityClassname(hostage_handle).as_str() != "hostage_entity"
        || s2sdk::GetEntSchema2(hostage, &Str::from_str("CHostage"), &Str::from_str("m_isRescued"), 0) != 0
        || s2sdk::GetEntSchema2(hostage, &Str::from_str("CHostage"), &Str::from_str("m_nHostageState"), 0) != 3
        || parent_handle != player.pawn_handle
        || parent != pawn
    {
        return None;
    }
    Some(hostage_handle)
}
