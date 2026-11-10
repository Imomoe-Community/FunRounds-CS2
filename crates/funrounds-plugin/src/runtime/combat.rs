// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{
    Effect, EngineAction, Fault, FiredWeapon, LivePlayer, Mechanism, Phase, PlayerTask, Runtime, SelfDamage, Str, active_combat_phase, log, s2sdk,
    weapon_identity_is_current,
};
use crate::game;
use crate::{process_delayed_damage, process_reflection};
use funrounds_core::Headshots;
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct State {
    pub(super) lives: HashMap<i32, (PlayerTask, funrounds_core::CombatLife)>,
    pub(super) reflections: Vec<crate::damage::Reflection>,
    pub(super) delayed_damage: Vec<crate::damage::DelayedDamage>,
    pub(super) random_heals: Vec<RandomHealReceipt>,
    pub(super) native_self_damage_shots: HashMap<i32, NativeSelfDamageShot>,
}

impl State {
    pub(super) fn takeover(&mut self, owners: &[i32], task: PlayerTask, players: &funrounds_core::PlayerTracker) {
        let sources = owners
            .iter()
            .copied()
            .filter(|slot| {
                self.lives.get(slot).is_some_and(|(previous, _)| {
                    previous.round == task.round
                        && previous.player.pawn_handle == task.player.pawn_handle
                        && players.identity(*slot) == Some(previous.player)
                })
            })
            .collect::<Vec<_>>();
        let source = sources
            .iter()
            .copied()
            .find(|slot| *slot == task.player.slot)
            .or_else(|| sources.first().copied());
        let life = source.and_then(|slot| self.lives.remove(&slot)).map(|(_, life)| life);
        for slot in sources {
            if Some(slot) != source {
                self.lives.remove(&slot);
            }
        }
        if source != Some(task.player.slot) {
            self.lives.remove(&task.player.slot);
        }
        if let Some(life) = life {
            self.lives.insert(task.player.slot, (task, life));
        }
    }

    pub(super) fn remove_life(&mut self, slot: i32) {
        self.lives.remove(&slot);
        // A dead attacker still owns damage already dealt to another player.
        self.delayed_damage.retain(|hit| hit.target.player.slot != slot);
    }

    pub(super) fn cancel_spawn_work(&mut self, slot: i32) {
        self.native_self_damage_shots.remove(&slot);
        self.delayed_damage
            .retain(|hit| hit.target.player.slot != slot && hit.source.player.slot != slot);
    }

    pub(super) fn cancel_actions_for(&mut self, slots: &[i32]) {
        self.native_self_damage_shots.retain(|slot, _| !slots.contains(slot));
        self.reflections
            .retain(|hit| !slots.contains(&hit.target.player.slot) && !slots.contains(&hit.source.player.slot));
        self.delayed_damage
            .retain(|hit| !slots.contains(&hit.target.player.slot) && !slots.contains(&hit.source.player.slot));
        self.random_heals
            .retain(|receipt| !slots.contains(&receipt.target.player.slot) && !slots.contains(&receipt.source.player.slot));
    }

    pub(super) fn forget_player(&mut self, slot: i32) {
        self.lives.remove(&slot);
        self.cancel_actions_for(&[slot]);
    }

    pub(super) fn clear(&mut self) {
        self.lives.clear();
        self.reflections.clear();
        self.delayed_damage.clear();
        self.random_heals.clear();
        self.native_self_damage_shots.clear();
    }

    pub(super) fn expire_combat_receipts(&mut self, tick: i32, players: &funrounds_core::PlayerTracker) {
        self.native_self_damage_shots.retain(|slot, shot| {
            shot.tick == tick && players.identity(*slot) == Some(shot.task.player) && LivePlayer::resolve(*slot) == Some(shot.player)
        });
        let frame = crate::damage::current_damage_frame_token().filter(|token| *token != 0);
        self.random_heals.retain(|receipt| Some(receipt.frame_token) == frame);
    }

    fn take_native_player_hit(&mut self, task: PlayerTask, victim: LivePlayer, tick: i32) -> bool {
        let matches = self
            .native_self_damage_shots
            .get(&victim.slot)
            .is_some_and(|shot| shot.task == task && shot.player == victim && shot.tick == tick);
        if !matches {
            return false;
        }
        self.native_self_damage_shots
            .remove(&victim.slot)
            .is_some_and(|shot| shot.player_hit && weapon_identity_is_current(victim, &shot.weapon))
    }

    fn mark_native_shot_hit(&mut self, task: PlayerTask, player: LivePlayer, tick: i32, damage: &crate::damage::Damage) {
        let Some(shot) = self.native_self_damage_shots.get_mut(&player.slot) else {
            return;
        };
        if weapon_identity_is_current(player, &shot.weapon) {
            shot.mark_player_hit(task, player, tick, damage);
        }
    }

    pub(super) fn collect_reflections(&mut self, tick: i32, actions: &mut Vec<EngineAction>) {
        actions.extend(
            self.reflections
                .extract_if(.., |hit| tick > hit.tick)
                .map(|hit| EngineAction::Combat(Action::Reflection(hit))),
        );
    }

    pub(super) fn collect_delayed_damage(&mut self, now: f32, actions: &mut Vec<EngineAction>) {
        actions.extend(
            self.delayed_damage
                .extract_if(.., |hit| hit.is_due(now))
                .map(|hit| EngineAction::Combat(Action::DelayedDamage(hit))),
        );
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RandomHealReceipt {
    pub target: PlayerTask,
    pub source: PlayerTask,
    pub target_live: LivePlayer,
    pub source_live: LivePlayer,
    pub frame_token: u64,
    pub amount: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct NativeSelfDamageShot {
    pub(super) task: PlayerTask,
    pub(super) player: LivePlayer,
    pub(super) tick: i32,
    pub(super) weapon: FiredWeapon,
    pub(super) player_hit: bool,
}

impl NativeSelfDamageShot {
    fn matches_hit(&self, task: PlayerTask, player: LivePlayer, tick: i32, damage: &crate::damage::Damage) -> bool {
        self.task == task
            && self.player == player
            && self.tick == tick
            && self.weapon.handle == damage.weapon
            && damage.weapon_class.as_deref() == Some(self.weapon.classname.as_str())
    }

    fn mark_player_hit(&mut self, task: PlayerTask, player: LivePlayer, tick: i32, damage: &crate::damage::Damage) -> bool {
        if !self.matches_hit(task, player, tick, damage) {
            return false;
        }
        self.player_hit = true;
        true
    }
}

fn native_self_damage_signature(damage: &crate::damage::Damage, player: LivePlayer) -> bool {
    !damage.skip_plugin_rules
        && damage.victim == player.pawn_handle
        && damage.attacker == player.pawn_handle
        && damage.inflictor == player.pawn_handle
        && damage.weapon == -1
        && damage.weapon_kind == 0
        && damage.damage_type == 0
        && damage.amount.is_finite()
        && damage.amount > 0.0
}

fn sync_shield_hud(player: LivePlayer, armor: i32) -> Result<(), Fault> {
    if s2sdk::GetClientArmor(player.slot) != armor {
        s2sdk::SetClientArmor(player.slot, armor);
        if s2sdk::GetClientArmor(player.slot) != armor {
            return Err(Fault::mechanism(Mechanism::EnergyShield, "shield armor HUD write failed"));
        }
    }
    Ok(())
}

fn mode65_self_damage(damage: &crate::damage::Damage, pawn: i32, health: i32) -> bool {
    let damage_type = damage.damage_type;
    let native_self = damage.inflictor == pawn;
    // mp_weapon_self_inflict_amount uses pawn-attributed DMG_GENERIC, not DMG_BULLET.
    native_self && (damage_type == 0 || damage_type == 0x0200_0000 && health <= 1 && damage.amount == 1.0)
        || damage.weapon_kind == 1 && damage_type & crate::damage::DMG_BULLET != 0
}

fn self_damage_allowed(mode: &funrounds_core::Mode, damage: &crate::damage::Damage, pawn: i32, health: i32) -> bool {
    if damage.damage_type & 32 != 0 {
        return false;
    }
    match mode.self_damage {
        Some(SelfDamage::MissPenalty) => mode65_self_damage(damage, pawn, health),
        Some(SelfDamage::Native { amount }) => amount > 0.0,
        None => false,
    }
}

fn numeric_self_damage_enters_health(allowed_self: bool, blocked: bool, incoming: f32) -> bool {
    allowed_self && !blocked && incoming.is_finite() && incoming > 0.0
}

fn delayed_damage_protection_valid(protection_enabled: bool, target: PlayerTask, current: Option<(PlayerTask, bool)>) -> bool {
    !protection_enabled || current.is_some_and(|(task, protected)| task == target && !protected)
}

fn lifesteal_eligible(attacker: i32, victim: i32, attacker_alive: bool) -> bool {
    attacker >= 0 && victim >= 0 && attacker != victim && attacker_alive
}

impl Runtime {
    pub(super) fn capture_native_self_damage_shot(&mut self, player: LivePlayer, weapon: &str) {
        if active_combat_phase(self.controller.phase())
            && self
                .controller
                .current_mode()
                .is_some_and(|mode| matches!(mode.self_damage, Some(SelfDamage::Native { amount }) if amount > 0.0))
            && let Some(task) = self.combat_task(player)
            && let Some(weapon) = super::fired_weapon(player, weapon)
        {
            self.combat.native_self_damage_shots.insert(
                player.slot,
                NativeSelfDamageShot {
                    task,
                    player,
                    tick: s2sdk::GetGameTickCount(),
                    weapon,
                    player_hit: false,
                },
            );
        } else {
            self.combat.native_self_damage_shots.remove(&player.slot);
        }
    }

    pub fn headshots(&self) -> Headshots {
        if self.controller.phase() == Phase::Idle {
            return Headshots::Normal;
        }
        self.controller.current_mode().map_or(Headshots::Normal, |mode| mode.headshots)
    }

    pub(crate) fn classifies_native_he(&self) -> bool {
        self.classic_round()
            && self
                .controller
                .current_mode()
                .is_some_and(|mode| mode.hegrenade_impulse.is_some() || !mode.weapon_damage.is_empty())
    }

    pub fn classic_damage_scale(&self, damage: &crate::damage::Damage, players: &game::DamagePlayers, native_he: bool) -> Option<f32> {
        if !self.classic_round() {
            return Some(1.0);
        }
        let damage_type = damage.damage_type;
        let game::DamagePlayers { victim, attacker } = *players;
        let self_damage = victim.is_some() && victim == attacker;
        let mode = self.controller.current_mode()?;
        let he_multiplier = native_he.then(|| mode.weapon_damage.configured("hegrenade")).flatten();
        let self_damage_allowed = he_multiplier.is_some()
            || matches!(mode.self_damage, Some(SelfDamage::Native { amount }) if amount > 0.0)
            || matches!(mode.self_damage, Some(SelfDamage::MissPenalty))
                && victim.is_some_and(|player| mode65_self_damage(damage, player.pawn_handle, s2sdk::GetClientHealth(player.slot)));
        let attacker_health = if !mode.health_damage {
            None
        } else {
            attacker
                .filter(|player| {
                    LivePlayer::resolve(player.slot) == Some(*player)
                        && self.controllers.get(&player.slot) == Some(&player.controller_handle)
                        && self.effects.get(&player.slot).is_some_and(|effect| {
                            Some(effect.identity) == self.players.identity(player.slot)
                                && effect.identity.pawn_handle == player.pawn_handle as u32
                                && !effect.has_pending_loadout()
                        })
                })
                .map(|player| (s2sdk::GetClientHealth(player.slot), self.effects[&player.slot].attributes.health))
        };
        let suicide_scale = if players.victim.is_some_and(|victim| self.suicides.active(victim)) {
            5.0
        } else {
            1.0
        };
        let common = if victim.is_some() && attacker.is_some() && !self_damage && damage_type & 32 == 0 {
            mode.damage_multiplier
        } else {
            1.0
        } * suicide_scale
            * mode
                .health_curve
                .damage_scale(attacker_health, self_damage, damage_type & 32 != 0, self_damage_allowed)?;
        if native_he {
            return Some(common * he_multiplier.unwrap_or(1.0));
        }
        if mode.weapon_damage.is_empty() || attacker.is_none() || self_damage || damage_type & crate::damage::DMG_FALL != 0 {
            return Some(common);
        }
        let weapon = (damage.weapon != -1 && s2sdk::IsValidEntHandle(damage.weapon))
            .then(|| {
                let class = s2sdk::GetEntityClassname(damage.weapon);
                if !class.as_str().starts_with("weapon_") {
                    return class.to_string();
                }
                let definition = unsafe { crate::effects::__s2sdk_GetWeaponItemDefinition }.expect("required native ABI checked at startup");
                crate::effects::equipment::give_name_for_original(class.as_str(), unsafe { definition(damage.weapon) })
                    .unwrap_or_else(|| class.to_string())
            })
            .or_else(|| damage.weapon_class.clone());
        Some(
            common
                * funrounds_core::weapon_damage_scale(
                    mode,
                    weapon.as_deref().map(crate::damage::weapon_scale_id).filter(|id| *id != "hegrenade"),
                    true,
                ),
        )
    }

    pub fn combat_protected(&self, players: &game::DamagePlayers) -> bool {
        players.victim.and_then(|player| self.combat_task(player)).is_some_and(|task| {
            self.combat
                .lives
                .get(&task.player.slot)
                .is_some_and(|(expected, life)| *expected == task && life.protected(s2sdk::GetGameTime()))
        })
    }

    pub fn record_native_player_hit(&mut self, damage: &crate::damage::Damage, players: &game::DamagePlayers) {
        // The native classifier covers bullets, buckshot and Zeus shock damage.
        if matches!(damage.weapon_kind, 1 | 3)
            && !damage.skip_plugin_rules
            && damage.amount.is_finite()
            && damage.amount > 0.0
            && let (Some(victim), Some(attacker)) = (players.victim, players.attacker)
            && victim != attacker
            && self.combat_task(victim).is_some()
        {
            self.mark_native_shot_hit(attacker, damage);
        }
    }

    pub fn suppress_hit_native_self_damage(&mut self, damage: &crate::damage::Damage, players: &game::DamagePlayers) -> bool {
        let (Some(victim), Some(attacker)) = (players.victim, players.attacker) else {
            return false;
        };
        if victim != attacker
            || !native_self_damage_signature(damage, victim)
            || !active_combat_phase(self.controller.phase())
            || !self
                .controller
                .current_mode()
                .is_some_and(|mode| matches!(mode.self_damage, Some(SelfDamage::Native { amount }) if amount > 0.0))
        {
            return false;
        }
        let Some(task) = self.combat_task(victim) else { return false };
        if LivePlayer::resolve(victim.slot) != Some(victim) {
            return false;
        }
        let tick = s2sdk::GetGameTickCount();
        self.combat.take_native_player_hit(task, victim, tick)
    }

    pub(super) fn mark_native_shot_hit(&mut self, player: LivePlayer, damage: &crate::damage::Damage) {
        let Some(task) = self.combat_task(player) else { return };
        self.combat.mark_native_shot_hit(task, player, s2sdk::GetGameTickCount(), damage);
    }

    pub fn combat_damage(
        &mut self,
        damage: &crate::damage::Damage,
        players: &game::DamagePlayers,
        common_scale: f32,
        allow_reflection: bool,
        native_he: bool,
    ) -> bool {
        let Some(victim) = players.victim else { return false };
        let Some(task) = self.combat_task(victim) else { return false };
        let Some((expected, life)) = self.combat.lives.get_mut(&victim.slot).filter(|(expected, _)| *expected == task) else {
            return false;
        };
        let Some(mode) = self.controller.current_mode() else { return false };
        if players.attacker.is_some_and(|attacker| s2sdk::GetClientTeam(attacker.slot) as i32 == 1) {
            return true;
        }
        let eligible = players.attacker.is_some_and(|attacker| attacker != victim) && damage.damage_type & 32 == 0;
        let allowed_self =
            players.attacker == Some(victim) && self_damage_allowed(mode, damage, victim.pawn_handle, s2sdk::GetClientHealth(victim.slot));
        let he_self = native_he && players.attacker == Some(victim) && mode.weapon_damage.configured("hegrenade").is_some();
        let knife = damage.weapon_kind == 2
            || damage.weapon_class.as_deref().is_some_and(crate::projectiles::knife_fire_event)
            || damage.weapon != -1
                && s2sdk::IsValidEntHandle(damage.weapon)
                && matches!(s2sdk::GetEntityClassname(damage.weapon).as_str(), "weapon_knife" | "weapon_knife_t");
        let incoming = damage.amount * common_scale;
        if !incoming.is_finite() {
            return true;
        }
        let (blocked, reflected) = funrounds_core::combat_hit(mode, life, eligible, allowed_self || he_self, knife, incoming);
        let suppress_lifesteal = mode.lifesteal;
        let source = *expected;
        let hud = life.armor_hud();
        // Record the HP path, not the post-hit shield: the breaking hit is fully absorbed.
        if numeric_self_damage_enters_health(allowed_self && !he_self, blocked, incoming) {
            self.respawning.queue_finish(task);
        }
        if let Some(armor) = hud
            && let Err(error) = sync_shield_hud(victim, armor)
        {
            self.handle_fault(error);
            return true;
        }
        if let Some(amount) = reflected.filter(|_| allow_reflection)
            && let Some(attacker) = players.attacker
            && let Some(target) = self.combat_task(attacker)
        {
            self.combat.reflections.push(crate::damage::Reflection {
                target,
                source,
                target_live: attacker,
                source_live: victim,
                inflictor: damage.inflictor,
                weapon: damage.weapon,
                weapon_class: damage.weapon_class.clone(),
                amount,
                damage_type: damage.damage_type,
                tick: s2sdk::GetGameTickCount(),
                suppress_lifesteal,
            });
        }
        blocked
    }

    pub fn prepare_delayed_damage_with_heal(
        &self,
        damage: &crate::damage::Damage,
        players: &game::DamagePlayers,
        amount: f32,
        headshot: bool,
        random_heal: i32,
    ) -> Option<crate::damage::DelayedDamage> {
        let mode = self.controller.current_mode()?;
        if !active_combat_phase(self.controller.phase()) || !mode.delayed_damage {
            return None;
        }
        let (target, source) = (players.victim?, players.attacker?);
        if s2sdk::GetClientTeam(source.slot) as i32 == 1
            || !crate::damage::delayed_damage_eligible(target.slot, source.slot, damage.damage_type, amount)
        {
            return None;
        }
        let target_task = self.combat_task(target)?;
        if self.controllers.get(&source.slot) != Some(&source.controller_handle) {
            return None;
        }
        let source_task = self
            .players
            .damage_source_task(source.slot, self.controller.generation(), source.pawn_handle as u32)?;
        let now = s2sdk::GetGameTime();
        if !now.is_finite() {
            return None;
        }
        let mut hit = crate::damage::DelayedDamage::new(
            target_task,
            source_task,
            target,
            source,
            damage,
            crate::damage::DamageOutcome { amount, headshot },
            now,
        );
        hit.set_random_heal(random_heal);
        Some(hit)
    }

    pub fn defer_damage(&mut self, hit: crate::damage::DelayedDamage) {
        self.combat.delayed_damage.push(hit);
    }

    pub fn random_damage_roll(
        &mut self,
        damage: &crate::damage::Damage,
        players: &game::DamagePlayers,
        amount: f32,
    ) -> Option<funrounds_core::RandomDamage> {
        let mode = self.controller.current_mode()?;
        if !active_combat_phase(self.controller.phase()) || !mode.random_damage || damage.damage_type & crate::damage::DMG_FALL != 0 {
            return None;
        }
        let (victim, attacker) = (players.victim?, players.attacker?);
        if victim == attacker
            || self.combat_task(victim).is_none()
            || self.combat_task(attacker).is_none()
            || crate::damage::current_damage_frame_token().filter(|token| *token != 0).is_none()
        {
            return None;
        }
        funrounds_core::random_damage(amount, s2sdk::GetClientHealth(victim.slot), &mut self.rng)
    }

    pub fn queue_random_heal(&mut self, players: &game::DamagePlayers, amount: i32) -> Result<(), Fault> {
        if amount <= 0 {
            return Ok(());
        }
        let (Some(target_live), Some(source_live)) = (players.victim, players.attacker) else {
            return Ok(());
        };
        let (Some(target), Some(source)) = (self.combat_task(target_live), self.combat_task(source_live)) else {
            return Ok(());
        };
        let Some(frame_token) = crate::damage::current_damage_frame_token().filter(|token| *token != 0) else {
            return Err(Fault::mechanism(
                Mechanism::RandomDamage,
                "player damage frame token unavailable before random heal settlement",
            ));
        };
        self.combat.random_heals.push(RandomHealReceipt {
            target,
            source,
            target_live,
            source_live,
            frame_token,
            amount,
        });
        Ok(())
    }

    pub(super) fn settle_random_heal(&mut self, victim: i32, attacker: i32, event: usize) -> Option<i32> {
        if s2sdk::GetEventInt(event, &Str::from_str("dmg_health")) <= 0 {
            return None;
        }
        let receipt = if let Some(receipt) = crate::damage::internal_random_heal_hurt(victim, attacker) {
            Some(receipt)
        } else {
            let token = crate::damage::current_damage_frame_token().filter(|token| *token != 0)?;
            let index =
                self.combat.random_heals.iter().position(|receipt| {
                    receipt.target.player.slot == victim && receipt.source.player.slot == attacker && receipt.frame_token == token
                })?;
            Some(self.combat.random_heals.remove(index))
        };
        let receipt = receipt?;
        if !self.controller.current_mode().is_some_and(|mode| mode.random_damage)
            || receipt.amount <= 0
            || !self
                .players
                .validates_damage_source(receipt.target, self.controller.generation(), receipt.target_live.pawn_handle as u32)
            || !self
                .players
                .validates(receipt.source, self.controller.generation(), receipt.source_live.pawn_handle as u32)
            || LivePlayer::resolve_present(receipt.target_live.slot) != Some(receipt.target_live)
            || LivePlayer::resolve(receipt.source_live.slot) != Some(receipt.source_live)
            || crate::damage::current_damage_frame_token() != Some(receipt.frame_token)
        {
            return None;
        }
        if let Err(error) = Effect::random_damage_heal(receipt.source_live, receipt.amount) {
            self.handle_fault(Fault::mechanism(Mechanism::RandomDamage, error));
            return None;
        }
        Some(attacker)
    }

    pub(super) fn refresh_combat(&mut self) {
        if self.controller.current_mode().is_none() {
            return;
        }
        let now = s2sdk::GetGameTime();
        let mut failure = None;
        for (task, life) in self.combat.lives.values_mut() {
            let Some(player) = LivePlayer::resolve(task.player.slot) else { continue };
            if !self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                || !self
                    .effects
                    .get(&player.slot)
                    .is_some_and(|effect| effect.identity == task.player && !effect.has_pending_loadout())
            {
                continue;
            }
            let health = s2sdk::GetClientHealth(player.slot);
            if let Some(next) = life.recharge(now, health)
                && next != health
            {
                s2sdk::SetClientHealth(player.slot, next);
                if s2sdk::GetClientHealth(player.slot) != next {
                    failure = Some(Fault::mechanism(Mechanism::EnergyShield, "shield health conversion write failed"));
                    break;
                }
            }
            if let Some(armor) = life.armor_hud()
                && let Err(error) = sync_shield_hud(player, armor)
            {
                failure = Some(error);
                break;
            }
        }
        if let Some(error) = failure {
            self.handle_fault(error);
        }
    }

    pub(super) fn lifesteal(&mut self, event: usize, attacker: i32, victim: i32) -> Option<i32> {
        if self.controller.phase() == Phase::Idle || !self.controller.current_mode().is_some_and(|mode| mode.lifesteal) {
            return None;
        }
        let player = LivePlayer::resolve(attacker);
        if !lifesteal_eligible(attacker, victim, player.is_some()) {
            return None;
        }
        let player = player.expect("eligible attacker is alive");
        if !self.effects.get(&attacker).is_some_and(|effect| {
            Some(effect.identity) == self.players.identity(attacker)
                && effect.identity.pawn_handle == player.pawn_handle as u32
                && !effect.has_pending_loadout()
        }) || self.controllers.get(&attacker) != Some(&player.controller_handle)
        {
            return None;
        }
        let health = s2sdk::GetEventInt(event, &Str::from_str("dmg_health"));
        let armor = s2sdk::GetEventInt(event, &Str::from_str("dmg_armor"));
        let energy_shield = self.controller.current_mode().is_some_and(|mode| mode.energy_shield);
        let effect = self.effects.get_mut(&attacker)?;
        if let Err(error) = effect.lifesteal(player, health, armor, energy_shield) {
            self.handle_fault(Fault::mechanism(Mechanism::Lifesteal, error));
            return None;
        }
        if self.diagnostics() {
            log(&format!(
                "lifesteal slot={attacker} dmg_health={health} dmg_armor={armor} health={} armor={} helmet={:?}",
                s2sdk::GetClientHealth(attacker),
                s2sdk::GetClientArmor(attacker),
                player.helmet()
            ));
        }
        Some(attacker)
    }

    pub(super) fn delayed_damage_current(&self, hit: &crate::damage::DelayedDamage) -> bool {
        self.classic_round()
            && active_combat_phase(self.controller.phase())
            && self.controller.current_mode().is_some_and(|mode| {
                mode.delayed_damage
                    && delayed_damage_protection_valid(
                        mode.spawn_protection || mode.kill_protection,
                        hit.target,
                        self.combat
                            .lives
                            .get(&hit.target.player.slot)
                            .map(|(task, life)| (*task, life.protected(s2sdk::GetGameTime()))),
                    )
            })
            && hit.validates_players(
                self.controller.generation(),
                &self.players,
                LivePlayer::resolve(hit.target.player.slot),
                LivePlayer::resolve_present(hit.source.player.slot),
            )
            && hit.native_handles(s2sdk::IsValidEntHandle).is_some()
    }

    pub(super) fn reflection_current(&self, hit: &crate::damage::Reflection) -> bool {
        self.classic_round()
            && self.controller.current_mode().is_some_and(|mode| mode.reflect_damage.is_some())
            && LivePlayer::resolve(hit.target.player.slot) == Some(hit.target_live)
            && LivePlayer::resolve_present(hit.source.player.slot) == Some(hit.source_live)
            && self
                .players
                .validates(hit.target, self.controller.generation(), hit.target_live.pawn_handle as u32)
            && self
                .players
                .validates_damage_source(hit.source, self.controller.generation(), hit.source_live.pawn_handle as u32)
            && self
                .combat
                .lives
                .get(&hit.target.player.slot)
                .is_some_and(|(task, life)| *task == hit.target && !life.protected(s2sdk::GetGameTime()))
            && hit.native_handles(s2sdk::IsValidEntHandle).is_some()
    }
}

pub enum Action {
    Reflection(crate::damage::Reflection),
    DelayedDamage(crate::damage::DelayedDamage),
}

impl Action {
    pub(super) fn involves(&self, slots: &[i32]) -> bool {
        match self {
            Self::Reflection(hit) => slots.contains(&hit.target.player.slot) || slots.contains(&hit.source.player.slot),
            Self::DelayedDamage(hit) => slots.contains(&hit.target.player.slot) || slots.contains(&hit.source.player.slot),
        }
    }
}

impl Runtime {
    pub(super) fn combat_action_current(&self, action: &Action) -> bool {
        match action {
            Action::DelayedDamage(hit) => self.delayed_damage_current(hit),
            Action::Reflection(hit) => self.reflection_current(hit),
        }
    }
}

pub(crate) fn execute(action: Action) {
    match action {
        Action::DelayedDamage(hit) => process_delayed_damage(hit),
        Action::Reflection(hit) => process_reflection(hit),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn miss_penalty_respects_native_self_damage_and_final_health_point() {
        let mut mode = funrounds_core::Config::parse(include_str!("../../../../config/funrounds.example.toml"))
            .unwrap()
            .modes
            .remove(0);
        mode.self_damage = Some(funrounds_core::SelfDamage::MissPenalty);
        let mut damage = crate::damage::Damage {
            victim: 42,
            attacker: 42,
            inflictor: 42,
            weapon_kind: 0,
            hit_group: -1,
            traced_head: false,
            amount: 36.0,
            damage_type: 0,
            weapon: -1,
            weapon_class: None,
            skip_plugin_rules: false,
        };
        assert!(super::self_damage_allowed(&mode, &damage, 42, 200));
        assert!(!super::self_damage_allowed(&mode, &damage, 43, 200));
        for damage_type in [8, 64, 256, 0x0200_0000] {
            damage.damage_type = damage_type;
            assert!(!super::self_damage_allowed(&mode, &damage, 42, 200));
        }
        damage.amount = 1.0;
        assert!(super::self_damage_allowed(&mode, &damage, 42, 1));
        assert!(!super::self_damage_allowed(&mode, &damage, 42, 2));
        damage.damage_type = 0;
        assert!(super::self_damage_allowed(&mode, &damage, 42, 1));
        damage.damage_type = 2;
        assert!(!super::self_damage_allowed(&mode, &damage, 42, 200));
        damage.weapon_kind = 1;
        assert!(super::self_damage_allowed(&mode, &damage, 42, 200));
    }

    #[test]
    fn lifesteal_excludes_self_world_and_dead_attackers() {
        assert!(super::lifesteal_eligible(1, 2, true));
        assert!(!super::lifesteal_eligible(1, 1, true));
        assert!(!super::lifesteal_eligible(-1, 2, true));
        assert!(!super::lifesteal_eligible(1, -1, true));
        assert!(!super::lifesteal_eligible(1, 2, false));
    }
}
