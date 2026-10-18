// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::s2sdk;
use plugify::Str;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LivePlayer {
    pub slot: i32,
    pub controller_handle: i32,
    pub pawn_handle: i32,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct DamagePlayers {
    pub victim: Option<LivePlayer>,
    pub attacker: Option<LivePlayer>,
}

impl DamagePlayers {
    pub fn resolve(victim: i32, attacker: i32) -> Self {
        Self::from_players(victim, attacker, (0..s2sdk::GetMaxClients()).filter_map(LivePlayer::resolve_present))
    }

    fn from_players(victim: i32, attacker: i32, players: impl Iterator<Item = LivePlayer>) -> Self {
        let mut result = Self::default();
        for player in players {
            if player.pawn_handle == victim || player.controller_handle == victim {
                result.victim = Some(player);
            }
            if player.pawn_handle == attacker || player.controller_handle == attacker {
                result.attacker = Some(player);
            }
            if result.victim.is_some() && result.attacker.is_some() {
                break;
            }
        }
        result
    }
}

pub(crate) fn entity_handles(classname: &str) -> Vec<i32> {
    let mut handles = Vec::new();
    let mut current = s2sdk::GetFirstActiveEntity();
    while s2sdk::IsValidEntHandle(current) {
        if s2sdk::GetEntityClassname(current).as_str() == classname {
            handles.push(current);
        }
        current = s2sdk::GetNextActiveEntity(current);
    }
    handles
}

impl LivePlayer {
    fn item_services(self) -> Result<usize, String> {
        if Self::resolve(self.slot) != Some(self) {
            return Err("player identity changed before helmet access".into());
        }
        let class = Str::from_str("CBasePlayerPawn");
        let field = Str::from_str("m_pItemServices");
        if s2sdk::GetSchemaOffset(&class, &field) < 0
            || s2sdk::GetSchemaOffset(&Str::from_str("CCSPlayer_ItemServices"), &Str::from_str("m_bHasHelmet")) < 0
        {
            return Err("helmet schema unavailable".into());
        }
        let pawn = s2sdk::EntHandleToEntPointer(self.pawn_handle);
        let services = s2sdk::GetEntSchema2(pawn, &class, &field, 0) as usize;
        if services == 0 {
            Err("player item services unavailable".into())
        } else {
            Ok(services)
        }
    }

    pub fn helmet(self) -> Result<bool, String> {
        Ok(s2sdk::GetEntSchema2(
            self.item_services()?,
            &Str::from_str("CCSPlayer_ItemServices"),
            &Str::from_str("m_bHasHelmet"),
            0,
        ) != 0)
    }

    pub fn set_helmet(self, helmet: bool) -> Result<(), String> {
        s2sdk::SetEntSchema2(
            self.item_services()?,
            &Str::from_str("CCSPlayer_ItemServices"),
            &Str::from_str("m_bHasHelmet"),
            i64::from(helmet),
            true,
            0,
        );
        if self.helmet()? == helmet {
            Ok(())
        } else {
            Err("helmet write verification failed".into())
        }
    }

    // Call only on the game thread. Keep handles rather than borrowed engine pointers.
    pub fn resolve(slot: i32) -> Option<Self> {
        if slot < 0 || slot >= s2sdk::GetMaxClients() || !s2sdk::IsClientInGame(slot) || !s2sdk::IsClientAlive(slot) {
            return None;
        }
        Self::resolve_handles(slot)
    }

    // Death cleanup needs the current pawn identity without treating a dead player as active.
    pub fn resolve_present(slot: i32) -> Option<Self> {
        if slot < 0 || slot >= s2sdk::GetMaxClients() || !s2sdk::IsClientInGame(slot) {
            return None;
        }
        Self::resolve_handles(slot)
    }

    fn resolve_handles(slot: i32) -> Option<Self> {
        let controller_handle = s2sdk::PlayerSlotToEntHandle(slot);
        let pawn = s2sdk::GetClientPawn(slot);
        if pawn == 0 || !s2sdk::IsValidEntHandle(controller_handle) {
            return None;
        }
        let pawn_handle = s2sdk::EntPointerToEntHandle(pawn);
        if !s2sdk::IsValidEntHandle(pawn_handle) || s2sdk::EntHandleToEntPointer(pawn_handle) != pawn {
            return None;
        }
        Some(Self {
            slot,
            controller_handle,
            pawn_handle,
        })
    }
}

pub fn warmup() -> Option<bool> {
    let rules = s2sdk::GetGameRules();
    let class = Str::from_str("CCSGameRules");
    let field = Str::from_str("m_bWarmupPeriod");
    if rules == 0 || s2sdk::GetSchemaOffset(&class, &field) < 0 {
        return None;
    }
    Some(s2sdk::GetEntSchema2(rules, &class, &field, 0) != 0)
}

pub fn round_duration() -> Result<f32, String> {
    let rules = s2sdk::GetGameRules();
    let class = Str::from_str("CCSGameRules");
    let field = Str::from_str("m_iRoundTime");
    if rules == 0 || s2sdk::GetSchemaOffset(&class, &field) < 0 {
        return Err("effective round duration unavailable".into());
    }
    let seconds = s2sdk::GetEntSchema2(rules, &class, &field, 0);
    if seconds <= 0 {
        return Err("effective round duration must be positive".into());
    }
    Ok(seconds as f32)
}

pub fn team_score_probe() -> crate::capability::Probe {
    let mut probe = crate::capability::Probe::default();
    probe.schema("CTeam", "m_iScore");
    probe.schema("CBaseEntity", "m_iTeamNum");
    probe
}

fn winning_team(reason: s2sdk::CSRoundEndReason) -> Option<i32> {
    match reason {
        s2sdk::CSRoundEndReason::TerroristWin => Some(2),
        s2sdk::CSRoundEndReason::CTWin => Some(3),
        _ => None,
    }
}

pub fn award_team_round(reason: s2sdk::CSRoundEndReason) -> Result<(), String> {
    let Some(team) = winning_team(reason) else { return Ok(()) };
    for handle in entity_handles("cs_team_manager") {
        let pointer = s2sdk::EntHandleToEntPointer(handle);
        if s2sdk::GetEntSchema2(pointer, &Str::from_str("CBaseEntity"), &Str::from_str("m_iTeamNum"), 0) != i64::from(team) {
            continue;
        }
        let class = Str::from_str("CTeam");
        let field = Str::from_str("m_iScore");
        let score = s2sdk::GetEntSchema2(pointer, &class, &field, 0) + 1;
        s2sdk::SetEntSchema2(pointer, &class, &field, score, true, 0);
        if s2sdk::GetEntSchema2(pointer, &class, &field, 0) != score {
            return Err("native round score write verification failed".into());
        }
        s2sdk::PrintToServer(&Str::from_str(&format!("[FunRounds] native round win team={team} score={score}")));
        return Ok(());
    }
    Err(format!("native round score target unavailable for team={team}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_participants_resolve_controller_or_pawn_once_including_self_damage() {
        let victim = LivePlayer {
            slot: 2,
            controller_handle: 0x80001002_u32 as i32,
            pawn_handle: 0x81002002_u32 as i32,
        };
        let attacker = LivePlayer {
            slot: 5,
            controller_handle: 0x80001005_u32 as i32,
            pawn_handle: 0x81002005_u32 as i32,
        };
        let mut visited = Vec::new();
        let players = DamagePlayers::from_players(
            victim.controller_handle,
            attacker.pawn_handle,
            [attacker, victim, victim].into_iter().inspect(|player| visited.push(player.slot)),
        );
        assert_eq!(
            players,
            DamagePlayers {
                victim: Some(victim),
                attacker: Some(attacker)
            }
        );
        assert_eq!(visited, [5, 2]);
        let players = DamagePlayers::from_players(victim.pawn_handle, victim.controller_handle, [victim].into_iter());
        assert_eq!(players.victim, players.attacker);
        assert_eq!(players.victim, Some(victim));
    }

    #[test]
    fn damage_attribution_accepts_present_dead_players_but_not_old_reused_slot_handles() {
        let present = LivePlayer {
            slot: 2,
            controller_handle: 0x21002,
            pawn_handle: 0x22002,
        };
        // The input is resolve_present, deliberately independent of alive state.
        assert_eq!(
            DamagePlayers::from_players(present.pawn_handle, present.controller_handle, [present].into_iter()).attacker,
            Some(present)
        );
        let previous = LivePlayer {
            controller_handle: 0x11002,
            pawn_handle: 0x12002,
            ..present
        };
        let players = DamagePlayers::from_players(present.pawn_handle, previous.controller_handle, [present].into_iter());
        assert_eq!(players.victim, Some(present));
        assert_eq!(players.attacker, None);
        assert_eq!(
            DamagePlayers::from_players(-1, previous.pawn_handle, [present].into_iter()),
            DamagePlayers::default()
        );
    }

    #[test]
    fn native_round_score_counts_wins_not_team_kills() {
        assert_eq!(winning_team(s2sdk::CSRoundEndReason::TerroristWin), Some(2));
        assert_eq!(winning_team(s2sdk::CSRoundEndReason::CTWin), Some(3));
        assert_eq!(winning_team(s2sdk::CSRoundEndReason::Draw), None);
    }
}
