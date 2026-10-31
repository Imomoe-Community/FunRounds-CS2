// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;

use funrounds_core::{
    ChickenBlastConfig, ChickenDamageCause, ChickenDamageChainConfig, PlayerIdentity, RoundGeneration, filters_chicken_damage_chain,
};
use plugify::{Str, Vec3};

const BASE_ENTITY: &str = "CBaseEntity";
const EXPLOSION: &str = "CEnvExplosion";
const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };

#[derive(Clone, Copy)]
pub(crate) struct EntityDamageSource {
    pub victim: i32,
    pub attacker: i32,
    pub inflictor: i32,
    pub ability: i32,
    pub damage_type: u32,
    pub damage: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KillWeaponSource {
    pub attacker: PlayerIdentity,
    pub handle: i32,
    pub classname: String,
    pub item_definition: u16,
}

pub(crate) fn available() -> bool {
    use crate::s2sdk;

    let imports = unsafe {
        std::ptr::addr_of!(s2sdk::__s2sdk_CreateEntityByName).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_DispatchSpawn).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_RemoveEntity).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_IsValidEntHandle).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_EntHandleToEntPointer).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_EntPointerToEntHandle).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_GetEntSchema2).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_GetEntSchemaEnt2).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_SetEntSchema2).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_GetSchemaOffset).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_GetEntityAbsOrigin).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_TeleportEntity).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_AcceptEntityInput).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_SetEntSchemaFloat2).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_SetEntSchemaEnt2).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_SetEntityOwner).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_GetEntityOwner).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_EmitSoundToClient).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_GetClientAbsOrigin).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_GetClientPawn).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_IsClientInGame).read().is_some()
            && std::ptr::addr_of!(s2sdk::__s2sdk_GetClientTeam).read().is_some()
    };
    imports
        && ["m_iHealth", "m_bTakesDamage", "m_lifeState"]
            .into_iter()
            .all(|field| s2sdk::GetSchemaOffset(&Str::from_str(BASE_ENTITY), &Str::from_str(field)) >= 0)
        && ["m_iMagnitude", "m_flPlayerDamage", "m_iRadiusOverride", "m_hInflictor"]
            .into_iter()
            .all(|field| s2sdk::GetSchemaOffset(&Str::from_str(EXPLOSION), &Str::from_str(field)) >= 0)
}

/// Create an ordinary chicken. Partial creations are removed before the caller reports failure.
pub(crate) fn spawn(origin: Vec3) -> Result<i32, String> {
    use crate::s2sdk;

    let handle = s2sdk::CreateEntityByName(&Str::from_str("chicken"));
    if !s2sdk::IsValidEntHandle(handle) {
        return Err("chicken entity creation failed".into());
    }
    let result = (|| {
        s2sdk::DispatchSpawn(handle);
        if !s2sdk::IsValidEntHandle(handle) {
            return Err("chicken disappeared during spawn".into());
        }
        let pointer = s2sdk::EntHandleToEntPointer(handle);
        if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != handle {
            return Err("chicken pointer unavailable after spawn".into());
        }
        // The supported build spawns chickens invulnerable. Keep the normal engine death path.
        s2sdk::SetEntSchema2(pointer, &Str::from_str(BASE_ENTITY), &Str::from_str("m_bTakesDamage"), 1, true, 0);
        if s2sdk::GetEntSchema2(pointer, &Str::from_str(BASE_ENTITY), &Str::from_str("m_bTakesDamage"), 0) != 1
            || s2sdk::GetEntSchema2(pointer, &Str::from_str(BASE_ENTITY), &Str::from_str("m_iHealth"), 0) <= 0
            || s2sdk::GetEntSchema2(pointer, &Str::from_str(BASE_ENTITY), &Str::from_str("m_lifeState"), 0) != 0
        {
            return Err("fresh chicken damage or alive-state readback failed".into());
        }
        s2sdk::TeleportEntity(handle, &origin, &ZERO, &ZERO);
        if !s2sdk::IsValidEntHandle(handle) || !same_position(s2sdk::GetEntityAbsOrigin(handle), origin) {
            return Err("chicken placement readback failed".into());
        }
        Ok(handle)
    })();
    if result.is_err() {
        remove(handle);
    }
    result
}

pub(crate) fn remove(handle: i32) {
    let _ = std::panic::catch_unwind(|| {
        if crate::s2sdk::IsValidEntHandle(handle) {
            crate::s2sdk::RemoveEntity(handle);
        }
    });
}

pub(crate) fn health(handle: i32) -> Option<(i64, i64)> {
    use crate::s2sdk;

    if !s2sdk::IsValidEntHandle(handle) {
        return None;
    }
    let pointer = s2sdk::EntHandleToEntPointer(handle);
    if pointer == 0 || s2sdk::EntPointerToEntHandle(pointer) != handle {
        return None;
    }
    let health = s2sdk::GetEntSchema2(pointer, &Str::from_str(BASE_ENTITY), &Str::from_str("m_iHealth"), 0);
    let life_state = s2sdk::GetEntSchema2(pointer, &Str::from_str(BASE_ENTITY), &Str::from_str("m_lifeState"), 0);
    Some((health, life_state))
}

pub(crate) fn position(handle: i32) -> Option<Vec3> {
    if !crate::s2sdk::IsValidEntHandle(handle) {
        return None;
    }
    Some(crate::s2sdk::GetEntityAbsOrigin(handle))
}

pub(crate) fn prepare_blast(source: i32, attacker: Option<i32>, origin: Vec3, config: ChickenBlastConfig) -> Result<(i32, i32), String> {
    use crate::s2sdk;

    if !s2sdk::IsValidEntHandle(source) {
        return Err("chicken disappeared before blast".into());
    }
    let credited = blast_owner(source, attacker, current_player_pawn);
    let effect = s2sdk::CreateEntityByName(&Str::from_str("env_explosion"));
    if !s2sdk::IsValidEntHandle(effect) {
        return Err("chicken blast creation failed".into());
    }
    let result = (|| {
        let pointer = s2sdk::EntHandleToEntPointer(effect);
        if pointer == 0 {
            return Err("chicken blast entity unavailable before spawn".into());
        }
        let magnitude = config.damage.round().clamp(0.0, i32::MAX as f32) as i64;
        let radius = config.radius.round().clamp(0.0, i32::MAX as f32) as i64;
        s2sdk::SetEntSchema2(pointer, &Str::from_str(EXPLOSION), &Str::from_str("m_iMagnitude"), magnitude, false, 0);
        s2sdk::SetEntSchemaFloat2(
            pointer,
            &Str::from_str(EXPLOSION),
            &Str::from_str("m_flPlayerDamage"),
            f64::from(config.damage),
            false,
            0,
        );
        s2sdk::SetEntSchema2(pointer, &Str::from_str(EXPLOSION), &Str::from_str("m_iRadiusOverride"), radius, false, 0);
        s2sdk::SetEntityOwner(effect, credited);
        // Pointer-to-handle validates the active entity list, which CreateEntityByName has not populated yet.
        s2sdk::DispatchSpawn(effect);
        if !s2sdk::IsValidEntHandle(effect) {
            return Err("chicken blast disappeared during spawn".into());
        }
        let spawned_pointer = s2sdk::EntHandleToEntPointer(effect);
        if spawned_pointer == 0 || s2sdk::EntPointerToEntHandle(spawned_pointer) != effect {
            return Err("chicken blast pointer unavailable after spawn".into());
        }
        // The chicken remains the physical blast source; owner carries player credit.
        // Preserve the physical inflictor instead of substituting the temporary effect.
        s2sdk::SetEntSchemaEnt2(
            spawned_pointer,
            &Str::from_str(EXPLOSION),
            &Str::from_str("m_hInflictor"),
            source,
            false,
            0,
        );
        if !inflictor_matches(spawned_pointer, source) {
            return Err("chicken blast inflictor readback failed".into());
        }
        let mut credited = credited;
        if s2sdk::GetEntityOwner(effect) != credited {
            // A pawn can disconnect or be replaced after the lethal callback but
            // before this next-tick action. Keep the explosion and fall back to
            // its chicken source; never trust a reused client slot.
            credited = source;
            s2sdk::SetEntityOwner(effect, source);
            if s2sdk::GetEntityOwner(effect) != source {
                return Err("chicken blast source ownership readback failed".into());
            }
        }
        s2sdk::TeleportEntity(effect, &origin, &ZERO, &ZERO);
        if !s2sdk::IsValidEntHandle(effect) || !same_position(s2sdk::GetEntityAbsOrigin(effect), origin) {
            return Err("chicken blast placement readback failed".into());
        }
        Ok((effect, credited))
    })();
    if result.is_err() {
        remove(effect);
    }
    result
}

fn blast_owner(source: i32, attacker: Option<i32>, is_current_player: impl FnOnce(i32) -> bool) -> i32 {
    attacker.filter(|handle| is_current_player(*handle)).unwrap_or(source)
}

fn current_player_pawn(handle: i32) -> bool {
    use crate::s2sdk;
    if !s2sdk::IsValidEntHandle(handle) {
        return false;
    }
    (0..s2sdk::GetMaxClients()).any(|slot| {
        s2sdk::IsClientInGame(slot)
            && matches!(s2sdk::GetClientTeam(slot) as i32, 2 | 3)
            && s2sdk::EntPointerToEntHandle(s2sdk::GetClientPawn(slot)) == handle
    })
}

fn inflictor_matches(effect: usize, source: i32) -> bool {
    crate::s2sdk::GetEntSchemaEnt2(effect, &Str::from_str(EXPLOSION), &Str::from_str("m_hInflictor"), 0) == source
}

pub(crate) fn detonate(effect: i32, source: i32) {
    crate::s2sdk::AcceptEntityInput(
        effect,
        &Str::from_str("Explode"),
        source,
        source,
        &plugify::Var::default(),
        crate::s2sdk::FieldType::Auto,
        0,
    );
}

pub(crate) fn emit_blast_sound(effect: i32, origin: Vec3, radius: f32) {
    use crate::s2sdk;
    let recipients = (0..s2sdk::GetMaxClients())
        .filter(|slot| s2sdk::IsClientInGame(*slot) && matches!(s2sdk::GetClientTeam(*slot) as i32, 2 | 3))
        .filter(|slot| distance_squared(s2sdk::GetClientAbsOrigin(*slot), origin) <= radius * radius)
        .collect::<Vec<_>>();
    if recipients.is_empty() {
        return;
    }
    s2sdk::EmitSoundToClient(effect, &plugify::Arr::from(recipients), &Str::from_str("BaseGrenade.Explode"), 1.0, 100.0);
}

fn distance_squared(left: Vec3, right: Vec3) -> f32 {
    let x = left.x - right.x;
    let y = left.y - right.y;
    let z = left.z - right.z;
    x * x + y * y + z * z
}

fn damage_can_kill(health: i64, damage: f32) -> bool {
    health > 0 && damage.is_finite() && damage >= health as f32
}

fn same_position(left: Vec3, right: Vec3) -> bool {
    left.x.is_finite()
        && left.y.is_finite()
        && left.z.is_finite()
        && (left.x - right.x).abs() < 0.1
        && (left.y - right.y).abs() < 0.1
        && (left.z - right.z).abs() < 0.1
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Chicken {
    round: RoundGeneration,
    dead: bool,
    blast_pending: bool,
    killer: Option<i32>,
    killer_weapon: Option<KillWeaponSource>,
}

/// Full-handle ownership for ordinary chickens and temporary chicken blasts.
/// Map entities are never inferred to be ours from classname alone.
#[derive(Default)]
pub(crate) struct Chickens {
    chickens: HashMap<i32, Chicken>,
    explosions: HashMap<i32, (RoundGeneration, i32)>,
}

impl Chickens {
    pub(crate) fn track(&mut self, handle: i32, round: RoundGeneration) -> bool {
        if self.chickens.contains_key(&handle) || self.explosions.contains_key(&handle) {
            return false;
        }
        self.chickens.insert(
            handle,
            Chicken {
                round,
                dead: false,
                blast_pending: false,
                killer: None,
                killer_weapon: None,
            },
        );
        true
    }

    pub(crate) fn owns(&self, handle: i32, round: RoundGeneration) -> bool {
        self.chickens.get(&handle).is_some_and(|chicken| chicken.round == round)
    }

    pub(crate) fn owned_handles(&self, round: RoundGeneration) -> Vec<i32> {
        let mut handles = self
            .chickens
            .iter()
            .filter_map(|(handle, chicken)| (chicken.round == round && !chicken.dead).then_some(*handle))
            .collect::<Vec<_>>();
        handles.sort_unstable();
        handles
    }

    /// Keep living ordinary chickens across a round generation change. Dead or stale
    /// handles are returned for removal; temporary blast ownership is round-scoped.
    pub(crate) fn advance_round(&mut self, round: RoundGeneration, mut alive: impl FnMut(i32) -> bool) -> Vec<i32> {
        let mut stale = self
            .chickens
            .iter()
            .filter_map(|(&handle, chicken)| (chicken.dead || !alive(handle)).then_some(handle))
            .collect::<Vec<_>>();
        for handle in &stale {
            self.chickens.remove(handle);
        }
        for chicken in self.chickens.values_mut() {
            chicken.round = round;
            chicken.dead = false;
            chicken.blast_pending = false;
            chicken.killer = None;
            chicken.killer_weapon = None;
        }
        stale.extend(self.explosions.keys().copied());
        self.explosions.clear();
        stale.sort_unstable();
        stale.dedup();
        stale
    }

    pub(crate) fn has_only_active_chickens(&self) -> bool {
        self.explosions.is_empty() && self.chickens.values().all(|chicken| !chicken.dead)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.chickens.is_empty() && self.explosions.is_empty()
    }

    pub(crate) fn track_explosion(&mut self, handle: i32, round: RoundGeneration, source: i32) -> bool {
        if self.chickens.contains_key(&handle) || self.explosions.contains_key(&handle) {
            return false;
        }
        self.explosions.insert(handle, (round, source));
        true
    }

    pub(crate) fn is_chicken_explosion(&self, handle: i32, round: RoundGeneration) -> bool {
        self.explosions.get(&handle).is_some_and(|&(generation, _)| generation == round)
            || self
                .explosions
                .values()
                .any(|&(generation, source)| generation == round && source == handle)
    }

    /// Return true exactly once, after a readback observes the actual dead state.
    pub(crate) fn mark_dead(&mut self, handle: i32, round: RoundGeneration) -> bool {
        let Some(chicken) = self.chickens.get_mut(&handle).filter(|chicken| chicken.round == round && !chicken.dead) else {
            return false;
        };
        chicken.dead = true;
        chicken.blast_pending = true;
        true
    }

    pub(crate) fn record_lethal_attacker(
        &mut self,
        damage: EntityDamageSource,
        round: RoundGeneration,
        weapon_source: impl FnOnce() -> Option<KillWeaponSource>,
    ) {
        let Some(chicken) = self
            .chickens
            .get_mut(&damage.victim)
            .filter(|chicken| chicken.round == round && !chicken.dead)
        else {
            return;
        };
        let Some((health, _)) = health(damage.victim) else { return };
        if !damage_can_kill(health, damage.damage) {
            return;
        }
        // Only retain a live player pawn. World and entity attackers must not become kill credit.
        let attacker = (0..crate::s2sdk::GetMaxClients()).find_map(|slot| {
            (crate::s2sdk::IsClientInGame(slot)
                && crate::s2sdk::EntPointerToEntHandle(crate::s2sdk::GetClientPawn(slot)) == damage.attacker
                && crate::s2sdk::GetClientTeam(slot) as i32 >= 2)
                .then_some(damage.attacker)
        });
        chicken.killer = attacker;
        chicken.killer_weapon = weapon_source()
            .filter(|source| attacker == Some(source.attacker.pawn_handle as i32) && source.attacker.pawn_handle as i32 == damage.attacker);
    }

    pub(crate) fn killer(&self, handle: i32, round: RoundGeneration) -> Option<i32> {
        self.chickens
            .get(&handle)
            .filter(|chicken| chicken.round == round)
            .and_then(|chicken| chicken.killer)
    }

    pub(crate) fn blast_kill_weapon_source(&self, handles: &[i32], round: RoundGeneration) -> Option<KillWeaponSource> {
        let source = handles
            .iter()
            .find_map(|handle| {
                self.explosions
                    .get(handle)
                    .filter(|(generation, _)| *generation == round)
                    .map(|(_, source)| *source)
            })
            .or_else(|| {
                self.explosions
                    .values()
                    .find_map(|(generation, source)| (*generation == round && handles.contains(source)).then_some(*source))
            })?;
        self.chickens
            .get(&source)
            .filter(|chicken| chicken.round == round && chicken.dead)
            .and_then(|chicken| chicken.killer_weapon.clone())
    }

    /// Reserves a chicken for one accepted E interaction without scheduling the kill-blast path.
    pub(crate) fn mark_consumed(&mut self, handle: i32, round: RoundGeneration) -> bool {
        let Some(chicken) = self.chickens.get_mut(&handle).filter(|chicken| chicken.round == round && !chicken.dead) else {
            return false;
        };
        chicken.dead = true;
        chicken.blast_pending = false;
        chicken.killer = None;
        chicken.killer_weapon = None;
        true
    }

    /// Undo a reservation immediately when the life-bound eating rule rejects it.
    pub(crate) fn release_consumed(&mut self, handle: i32, round: RoundGeneration) {
        if let Some(chicken) = self
            .chickens
            .get_mut(&handle)
            .filter(|chicken| chicken.round == round && !chicken.blast_pending)
        {
            chicken.dead = false;
        }
    }

    pub(crate) fn take_blast(&mut self, handle: i32, round: RoundGeneration) -> bool {
        let Some(chicken) = self.chickens.get_mut(&handle).filter(|chicken| chicken.round == round && chicken.dead) else {
            return false;
        };
        std::mem::replace(&mut chicken.blast_pending, false)
    }

    pub(crate) fn remove_chicken(&mut self, handle: i32, round: RoundGeneration) -> bool {
        if self.chickens.get(&handle).is_some_and(|chicken| chicken.round == round) {
            self.chickens.remove(&handle);
            true
        } else {
            false
        }
    }

    pub(crate) fn remove_explosion(&mut self, handle: i32, round: RoundGeneration) -> bool {
        if self.explosions.get(&handle).is_some_and(|&(generation, _)| generation == round) {
            self.explosions.remove(&handle);
            true
        } else {
            false
        }
    }

    pub(crate) fn damage_blocked(&self, damage: EntityDamageSource, round: RoundGeneration, chain: ChickenDamageChainConfig) -> bool {
        if !self.owns(damage.victim, round) {
            return false;
        }
        let cause = if damage.damage_type & (1 << 3) != 0 {
            ChickenDamageCause::Fire
        } else if [damage.inflictor, damage.attacker, damage.ability]
            .into_iter()
            .any(|handle| self.is_chicken_explosion(handle, round))
        {
            ChickenDamageCause::ChickenExplosion
        } else if damage.damage_type & (1 << 6) != 0 {
            ChickenDamageCause::Explosion
        } else {
            ChickenDamageCause::Other
        };
        filters_chicken_damage_chain(chain, cause)
    }

    /// A new map may reuse complete handles; forget old ownership without engine cleanup.
    pub(crate) fn discard_map_state(&mut self) {
        self.chickens.clear();
        self.explosions.clear();
    }

    pub(crate) fn clear(&mut self) -> Vec<i32> {
        let mut handles = self.chickens.keys().chain(self.explosions.keys()).copied().collect::<Vec<_>>();
        self.chickens.clear();
        self.explosions.clear();
        handles.sort_unstable();
        handles.dedup();
        handles
    }

    pub(crate) fn take_blast_config(&mut self, handle: i32, round: RoundGeneration, config: ChickenBlastConfig) -> Option<ChickenBlastConfig> {
        self.take_blast(handle, round).then_some(config)
    }
}
