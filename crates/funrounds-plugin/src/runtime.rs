// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    commands::Command,
    effects::{Effect, equipment},
    fault::{Fault, Faults, Mechanism, Recovery, Scope},
    game::{self, LivePlayer},
    parameters::{self, Parameters},
    s2sdk,
};
use funrounds_core::{
    Ammo, Config, Headshots, Phase, PlayerIdentity, PlayerTask, PlayerTracker, RoundController, RoundGeneration, RoundKey, SelfDamage, TeamDeathmatch,
};
use plugify::Str;
use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};
use std::collections::HashMap;
use std::path::PathBuf;
use std::thread::{ThreadId, current};

#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingAction {
    Loadout,
    Grenade(u64),
    RecoilSettle,
    RecoilObservation,
    AmmoObservation(i32),
}

pub struct EventCapabilities {
    pub grenade_thrown: bool,
    pub weapon_fire: bool,
    pub player_hurt: bool,
    pub damage: bool,
    pub combat_events: bool,
    pub round_prestart: bool,
    pub bombs: crate::capability::Probe,
    pub moving_blast: crate::capability::Probe,
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

// Execute after releasing the runtime lock: these engine calls can fire synchronous events.
pub enum EngineAction {
    Rollback(PlayerTask, LivePlayer),
    Loadout(PlayerTask, LivePlayer, Vec<String>),
    Upgrade(PlayerTask, LivePlayer, i32, String),
    GrantC4(PlayerTask, LivePlayer),
    RemoveC4(PlayerTask, LivePlayer, Vec<i32>),
    Respawn(PlayerTask, LivePlayer, i32),
    LowHealthFinish(PlayerTask, LivePlayer, bool),
    EndRound(RoundGeneration, s2sdk::CSRoundEndReason),
    CenterDisplay { generation: RoundGeneration, text: String },
    Projectile(crate::projectiles::Action),
    Bomb(crate::bombs::Action),
    Equipment(equipment::Action),
    ProjectileImpact(crate::projectiles::Impact),
    Reflection(crate::damage::Reflection),
    DelayedDamage(crate::damage::DelayedDamage),
    Suicide(crate::suicide::Action),
}

pub struct Runtime {
    pub controller: RoundController,
    config_path: PathBuf,
    game_thread: ThreadId,
    rng: StdRng,
    last_tick: Option<i32>,
    round: Option<RoundKey>,
    players: PlayerTracker,
    controllers: HashMap<i32, i32>,
    pending: Vec<(PlayerTask, i32, PendingAction)>,
    refills: equipment::Refills,
    center_display: Option<(RoundGeneration, i32, String)>,
    effects: HashMap<i32, Effect>,
    retired_effects: Vec<Effect>,
    equipment: Vec<String>,
    parameters: Parameters,
    team_names: Parameters,
    last_name: Option<String>,
    welcomes: Vec<(PlayerIdentity, f32)>,
    preferences: crate::preferences::Preferences,
    effects_ready: bool,
    capability_reasons: Vec<String>,
    faults: Faults,
    team_match: Option<TeamDeathmatch>,
    periodic_movement: Option<(RoundGeneration, funrounds_core::PeriodicMovement)>,
    respawns: Vec<(PlayerTask, LivePlayer, i32, f32)>,
    low_health_finishes: Vec<PlayerTask>,
    bombs: crate::bombs::Bombs,
    projectiles: crate::projectiles::Projectiles,
    impacts: Vec<crate::projectiles::Impact>,
    combat: HashMap<i32, (PlayerTask, funrounds_core::CombatLife)>,
    reflections: Vec<crate::damage::Reflection>,
    delayed_damage: Vec<crate::damage::DelayedDamage>,
    random_heals: Vec<RandomHealReceipt>,
    suicides: crate::suicide::Suicides,
    c4_assigned: bool,
    engine_actions: Vec<EngineAction>,
    equipment_in_flight: usize,
}

impl Runtime {
    pub fn new(events: EventCapabilities) -> Result<Self, String> {
        let config_path = PathBuf::from(plugify::get_configs_dir().to_string()).join("funrounds.toml");
        let config = read_config(&config_path);
        if let Err(error) = &config {
            s2sdk::PrintToServer(&plugify::Str::from_str(&format!("[FunRounds] ordinary gameplay: {error}")));
        }
        let mut capability_reasons = Vec::new();
        let mut capabilities = parameters::capabilities(events.weapon_fire, events.round_prestart, &mut capability_reasons);
        capabilities.translucent = crate::effects::translucent_probe().record("translucent", &mut capability_reasons);
        capabilities.grenade_replenishment = equipment::refill_probe(events.grenade_thrown).record("grenade_replenishment", &mut capability_reasons);
        capabilities.lifesteal = events.player_hurt;
        capabilities.headshots = events.damage;
        let mut random_damage = crate::capability::Probe::default();
        random_damage.require(events.damage, "hook:entity_damage");
        random_damage.require(events.player_hurt, "event:player_hurt");
        random_damage.require(events.combat_events, "events:player_spawn/player_death");
        random_damage.require(crate::damage::damage_frame_token_available(), "import:GetCurrentDamageFrameToken");
        random_damage.require(unsafe { crate::s2sdk::__s2sdk_SetClientHealth }.is_some(), "import:SetClientHealth");
        capabilities.random_damage = random_damage.record("random_damage", &mut capability_reasons);
        let mut suicide = crate::suicide::probe(crate::CAN_ACQUIRE_HOOKED.load(std::sync::atomic::Ordering::Acquire));
        suicide.require(events.player_hurt, "event:player_hurt");
        suicide.require(events.combat_events, "events:player_spawn/player_death");
        suicide.require(
            crate::KNIFE_DEATH_PRE_HOOKED.load(std::sync::atomic::Ordering::Acquire),
            "hook:player_death_pre",
        );
        suicide.require(unsafe { s2sdk::__s2sdk_SetEventString }.is_some(), "import:SetEventString");
        capabilities.suicide = suicide.record("suicide", &mut capability_reasons);
        let mut source_damage = crate::capability::Probe::default();
        source_damage.require(events.damage, "hook:entity_damage");
        capabilities.weapon_damage = source_damage.clone().record("weapon_damage", &mut capability_reasons);
        source_damage.require(crate::damage::reflection_available(), "import:TakeEntityDamageWithWeapon");
        capabilities.native_self_damage = source_damage.clone().record("native_self_damage", &mut capability_reasons);
        capabilities.reflect_damage = source_damage.record("reflect_damage", &mut capability_reasons);
        let mut reflection_lifesteal_frame = crate::capability::Probe::default();
        reflection_lifesteal_frame.require(crate::damage::damage_frame_token_available(), "import:GetCurrentDamageFrameToken");
        capabilities.reflection_lifesteal_frame = reflection_lifesteal_frame.record("reflection_lifesteal_frame", &mut capability_reasons);
        let mut delayed_damage = crate::capability::Probe::default();
        delayed_damage.require(events.damage, "hook:entity_damage");
        delayed_damage.require(crate::damage::reflection_available(), "import:TakeEntityDamageWithWeapon");
        delayed_damage.require(events.combat_events, "events:player_spawn/player_death");
        capabilities.delayed_damage = delayed_damage.record("delayed_damage", &mut capability_reasons);
        let mut combat = crate::capability::Probe::default();
        combat.require(events.damage, "hook:entity_damage");
        combat.require(events.combat_events, "events:player_spawn/player_death");
        capabilities.energy_shield = combat.clone().record("energy_shield", &mut capability_reasons);
        capabilities.combat_protection = combat.record("combat_protection", &mut capability_reasons);
        let bomb_capabilities = crate::bombs::capabilities(events.bombs, events.moving_blast, events.round_prestart, &mut capability_reasons);
        capabilities.bomb_timing = bomb_capabilities.bomb_timing;
        capabilities.chicken_bomb = bomb_capabilities.chicken_bomb;
        capabilities.carry_bomb = bomb_capabilities.carry_bomb;
        capabilities.bomb_bounce = bomb_capabilities.bomb_bounce;
        let (mut decoys, mut knives) = crate::projectiles::Projectiles::capabilities(events.damage);
        decoys.require(events.grenade_thrown, "event:grenade_thrown");
        knives.require(events.weapon_fire, "event:weapon_fire");
        capabilities.dodgeball = decoys.record("dodgeball", &mut capability_reasons);
        capabilities.throwing_knives = knives.record("throwing_knives", &mut capability_reasons);
        let mut speed = crate::capability::Probe::default();
        crate::capability::imports!(speed; __s2sdk_GetClientSpeed, __s2sdk_SetClientSpeed);
        capabilities.health_speed = speed.record("health_speed", &mut capability_reasons);
        capabilities.health_damage = events.damage;
        let mut decay = crate::capability::Probe::default();
        crate::capability::imports!(decay; __s2sdk_GetClientHealth, __s2sdk_SetClientHealth);
        capabilities.health_decay = decay.record("health_decay", &mut capability_reasons);
        let mut push = crate::capability::Probe::default();
        crate::capability::imports!(push;
            __s2sdk_GetClientAbsVelocity, __s2sdk_SetClientAbsVelocity, __s2sdk_GetClientEyeAngles,
            __s2sdk_GetClientButtons, __s2sdk_GetClientFlags,
        );
        push.require(events.weapon_fire, "event:weapon_fire");
        capabilities.self_push = push.record("self_push", &mut capability_reasons);
        let mut c4 = crate::suicide::c4_probe(false);
        c4.require(capabilities.c4_assignment, "convar:mp_give_player_c4/event:round_prestart");
        capabilities.c4_assignment = c4.record("c4_assignment", &mut capability_reasons);
        capabilities.c4_purchase = crate::suicide::c4_probe(true).record("c4_purchase", &mut capability_reasons);
        let mut respawn = crate::capability::Probe::default();
        crate::capability::imports!(respawn; __s2sdk_RespawnClient);
        respawn.require(events.combat_events, "events:player_spawn/player_death");
        capabilities.round_respawn = respawn.clone().record("round_respawn", &mut capability_reasons);
        capabilities.respawn = respawn.record("respawn", &mut capability_reasons);
        if !events.player_hurt {
            capability_reasons.push("lifesteal:[event:player_hurt]".into());
        }
        if !events.damage {
            capability_reasons.push("headshots:[hook:entity_damage]".into());
        }
        log(&format!(
            "capabilities={capabilities:?} capability_reasons={}",
            capability_reasons.join(";")
        ));
        let preferences = crate::preferences::Preferences::new();
        if let Some(error) = preferences.load_error() {
            log(&format!("player preferences: {error}; session choices remain available"));
        }
        Ok(Self {
            controller: RoundController::new(config.ok(), capabilities)?,
            config_path,
            game_thread: current().id(),
            rng: StdRng::try_from_rng(&mut rand::rngs::SysRng).map_err(|error| format!("cannot initialize randomness: {error}"))?,
            last_tick: None,
            round: None,
            players: PlayerTracker::default(),
            controllers: HashMap::new(),
            pending: Vec::new(),
            refills: equipment::Refills::default(),
            center_display: None,
            effects: HashMap::new(),
            retired_effects: Vec::new(),
            equipment: Vec::new(),
            parameters: Parameters::default(),
            team_names: Parameters::default(),
            last_name: None,
            welcomes: Vec::new(),
            preferences,
            effects_ready: true,
            capability_reasons,
            faults: Faults::new(capabilities),
            team_match: None,
            periodic_movement: None,
            respawns: Vec::new(),
            low_health_finishes: Vec::new(),
            bombs: crate::bombs::Bombs::default(),
            projectiles: crate::projectiles::Projectiles::default(),
            impacts: Vec::new(),
            combat: HashMap::new(),
            reflections: Vec::new(),
            delayed_damage: Vec::new(),
            random_heals: Vec::new(),
            suicides: crate::suicide::Suicides::default(),
            c4_assigned: false,
            engine_actions: Vec::new(),
            equipment_in_flight: 0,
        })
    }

    pub fn on_game_thread(&self) -> bool {
        current().id() == self.game_thread
    }

    pub fn headshots(&self) -> Headshots {
        if self.controller.phase() == Phase::Idle {
            return Headshots::Normal;
        }
        self.controller.current_mode().map_or(Headshots::Normal, |mode| mode.headshots)
    }

    pub fn projectile_damage(&mut self, damage: &mut crate::damage::Damage) -> Option<(f32, bool)> {
        if let Some(impact) = self.projectiles.attribute_synthetic_impact(damage) {
            return Some((1.0, impact.headshot));
        }
        let mode = self.controller.current_mode();
        let claim = self.projectiles.claim_damage(
            damage,
            self.controller.generation(),
            &self.players,
            mode.is_some_and(|mode| mode.dodgeball),
            mode.is_some_and(|mode| mode.throwing_knives),
        );
        (!self.queue_projectile_claim(claim)).then_some((1.0, false))
    }

    pub fn active_knife_death(&self, victim: i32, attacker: i32) -> bool {
        self.projectiles.active_knife_death(victim, attacker)
    }

    pub fn projectile_touch(&mut self, entity: i32, other: i32) -> bool {
        let claim = self.projectiles.claim_touch(
            entity,
            other,
            self.controller.generation(),
            &self.players,
            self.controller.phase() == Phase::Running && self.controller.current_mode().is_some_and(|mode| mode.throwing_knives),
        );
        self.queue_projectile_claim(claim)
    }

    fn queue_projectile_claim(&mut self, claim: crate::projectiles::Claim) -> bool {
        match claim {
            crate::projectiles::Claim::Ignore => false,
            crate::projectiles::Claim::Block { projectile, remove } => {
                if remove {
                    self.projectiles.retire(projectile);
                }
                true
            }
            crate::projectiles::Claim::Apply(impact) => {
                self.impacts.push(impact);
                true
            }
        }
    }

    pub fn prepare_impact(&mut self, mut impact: crate::projectiles::Impact) -> Option<crate::projectiles::Impact> {
        let mode = self.controller.current_mode()?;
        if self.controller.phase() != Phase::Running
            || !(match impact.kind {
                crate::projectiles::Kind::Knife => mode.throwing_knives,
                crate::projectiles::Kind::Dodgeball => mode.dodgeball,
            })
            || !LivePlayer::resolve(impact.task.player.slot).is_some_and(|player| {
                self.players
                    .validates(impact.task, self.controller.generation(), player.pawn_handle as u32)
            })
            || LivePlayer::resolve(impact.victim.slot) != Some(impact.victim)
            || !self
                .players
                .validates(impact.victim_task, self.controller.generation(), impact.victim.pawn_handle as u32)
            || !s2sdk::IsValidEntHandle(impact.projectile)
        {
            return None;
        }
        if impact.kind == crate::projectiles::Kind::Dodgeball {
            impact.amount = funrounds_core::dodgeball_damage(s2sdk::GetClientHealth(impact.victim.slot));
        }
        self.projectiles.begin_native_impact(impact).then_some(impact)
    }

    pub fn finish_impact(&mut self, impact: crate::projectiles::Impact) {
        self.projectiles.finish_native_impact(impact)
    }

    pub fn take_projectile_cleanup(&mut self) -> Vec<crate::projectiles::Action> {
        self.projectiles.cleanup_actions()
    }

    pub fn finish_projectile_action(&mut self, action: crate::projectiles::Action, result: crate::projectiles::ActionResult) {
        let accept = self.action_valid(&EngineAction::Projectile(action));
        if let Err(error) = self.projectiles.action_finished(action, result, accept) {
            self.handle_fault(error);
        }
    }

    pub fn diagnostics(&self) -> bool {
        self.controller.config().is_some_and(|config| config.diagnostics)
    }

    pub fn classic_enabled(&self) -> bool {
        self.effects_ready && self.controller.enabled()
    }

    pub fn classic_round(&self) -> bool {
        self.classic_enabled() && self.controller.phase() != Phase::Idle
    }

    pub fn classic_damage_scale(&self, damage: &crate::damage::Damage, players: &game::DamagePlayers) -> Option<f32> {
        if !self.classic_round() {
            return Some(1.0);
        }
        let damage_type = damage.damage_type;
        let game::DamagePlayers { victim, attacker } = *players;
        let self_damage = victim.is_some() && victim == attacker;
        let mode = self.controller.current_mode()?;
        let self_damage_allowed = matches!(mode.self_damage, Some(SelfDamage::Native { amount }) if amount > 0.0)
            || matches!(mode.self_damage, Some(SelfDamage::MissPenalty))
                && victim.is_some_and(|player| mode65_self_damage(damage, player.pawn_handle, s2sdk::GetClientHealth(player.slot)));
        let attacker_health = if !mode.health_damage {
            None
        } else {
            attacker
                .filter(|player| {
                    s2sdk::IsClientAlive(player.slot)
                        && self.controllers.get(&player.slot) == Some(&player.controller_handle)
                        && self.effects.get(&player.slot).is_some_and(|effect| {
                            Some(effect.identity) == self.players.identity(player.slot) && effect.identity.pawn_handle == player.pawn_handle as u32
                        })
                })
                .map(|player| (s2sdk::GetClientHealth(player.slot), mode.health))
        };
        let suicide_scale = if players.victim.is_some_and(|victim| self.suicides.active(victim)) {
            5.0
        } else {
            1.0
        };
        let common = suicide_scale * funrounds_core::classic_damage_scale(attacker_health, self_damage, damage_type & 32 != 0, self_damage_allowed)?;
        let weapon = (damage.weapon != -1 && s2sdk::IsValidEntHandle(damage.weapon))
            .then(|| s2sdk::GetEntityClassname(damage.weapon).to_string())
            .or_else(|| damage.weapon_class.clone());
        Some(
            common
                * funrounds_core::weapon_damage_scale(
                    mode,
                    weapon.as_deref().map(crate::damage::weapon_scale_class),
                    attacker.is_some() && !self_damage && damage_type & 32 == 0,
                ),
        )
    }

    fn combat_task(&self, player: LivePlayer) -> Option<PlayerTask> {
        let identity = self.players.identity(player.slot)?;
        (identity.pawn_handle == player.pawn_handle as u32 && self.controllers.get(&player.slot) == Some(&player.controller_handle)).then_some(
            PlayerTask {
                round: self.controller.generation(),
                player: identity,
            },
        )
    }

    pub fn take_equipment_effect(&mut self, action: &EngineAction) -> Result<Option<(Effect, funrounds_core::Mode)>, Fault> {
        if !self.action_valid(action) {
            return Ok(None);
        }
        let mode = self.controller.current_mode().expect("active mode").clone();
        let prepared = match action {
            EngineAction::Loadout(task, player, _) => {
                if self.effects.contains_key(&player.slot) {
                    return Ok(None);
                }
                Some((Effect::capture(*player, task.player, task.round, &mode, self.diagnostics())?, mode))
            }
            EngineAction::Upgrade(task, player, _, _) => {
                if self.effects.get(&player.slot).is_none_or(|effect| effect.identity != task.player) {
                    return Ok(None);
                }
                self.effects.remove(&player.slot).map(|effect| (effect, mode))
            }
            _ => None,
        };
        if prepared.is_some() {
            self.equipment_in_flight += 1;
        }
        Ok(prepared)
    }

    pub fn take_rollbacks(&self) -> Vec<EngineAction> {
        self.effects
            .values()
            .filter(|effect| effect.rollback_ready())
            .filter_map(|effect| {
                let player = LivePlayer::resolve(effect.identity.slot)?;
                Some(EngineAction::Rollback(
                    PlayerTask {
                        player: effect.identity,
                        round: effect.round,
                    },
                    player,
                ))
            })
            .collect()
    }

    pub fn take_rollback_effect(&mut self, action: &EngineAction) -> Option<Effect> {
        let EngineAction::Rollback(task, _) = action else { return None };
        if !self.action_valid(action) || !self.effects.get(&task.player.slot)?.rollback_ready() {
            return None;
        }
        let effect = self.effects.remove(&task.player.slot)?;
        self.equipment_in_flight += 1;
        Some(effect)
    }

    pub fn finish_equipment_effect(&mut self, action: &EngineAction, effect: Effect, result: Result<(), Fault>) {
        self.equipment_in_flight -= 1;
        let current = self.action_valid(action);
        let slot = effect.identity.slot;
        if current
            && result.is_ok()
            && let EngineAction::Loadout(_, _, equipment) = action
            && let Some(mode) = self.controller.current_mode()
        {
            log(&format!(
                "applied mode={} slot={slot} health={} armor={} gravity={} equipment={equipment:?}",
                mode.id, mode.health, mode.armor, mode.gravity
            ));
        }
        let same_life = self.players.identity(slot) == Some(effect.identity)
            && LivePlayer::resolve(slot).is_some_and(|player| player.pawn_handle as u32 == effect.identity.pawn_handle)
            && self
                .controllers
                .get(&slot)
                .is_some_and(|handle| *handle == s2sdk::PlayerSlotToEntHandle(slot));
        if same_life && !self.effects.contains_key(&slot) {
            self.effects.insert(slot, effect);
        } else {
            self.retired_effects.push(effect);
        }
        if !current {
            self.effects_ready = false;
        }
        if let Err(error) = result {
            self.handle_fault(error);
        }
    }

    pub fn combat_protected(&self, players: &game::DamagePlayers) -> bool {
        players.victim.and_then(|player| self.combat_task(player)).is_some_and(|task| {
            self.combat
                .get(&task.player.slot)
                .is_some_and(|(expected, life)| *expected == task && life.protected(s2sdk::GetGameTime()))
        })
    }

    pub fn combat_damage(
        &mut self,
        damage: &crate::damage::Damage,
        players: &game::DamagePlayers,
        common_scale: f32,
        allow_reflection: bool,
    ) -> bool {
        let Some(victim) = players.victim else { return false };
        let Some(task) = self.combat_task(victim) else { return false };
        let Some((expected, life)) = self.combat.get_mut(&victim.slot).filter(|(expected, _)| *expected == task) else {
            return false;
        };
        let Some(mode) = self.controller.current_mode() else { return false };
        if players.attacker.is_some_and(|attacker| s2sdk::GetClientTeam(attacker.slot) as i32 == 1) {
            return true;
        }
        let eligible = players.attacker.is_some_and(|attacker| attacker != victim) && damage.damage_type & 32 == 0;
        let allowed_self =
            players.attacker == Some(victim) && self_damage_allowed(mode, damage, victim.pawn_handle, s2sdk::GetClientHealth(victim.slot));
        let knife = damage.weapon_kind == 2
            || damage
                .weapon_class
                .as_deref()
                .map(crate::damage::weapon_scale_class)
                .is_some_and(|class| matches!(class, "weapon_knife" | "weapon_knife_t"))
            || damage.weapon != -1
                && s2sdk::IsValidEntHandle(damage.weapon)
                && matches!(s2sdk::GetEntityClassname(damage.weapon).as_str(), "weapon_knife" | "weapon_knife_t");
        let incoming = damage.amount * common_scale;
        if !incoming.is_finite() {
            return true;
        }
        let (blocked, reflected) = funrounds_core::combat_hit(mode, life, eligible, allowed_self, knife, incoming);
        let source = *expected;
        let hud = life.armor_hud();
        // Record the HP path, not the post-hit shield: the breaking hit is fully absorbed.
        if numeric_self_damage_enters_health(allowed_self, blocked, incoming) && !self.low_health_finishes.contains(&task) {
            self.low_health_finishes.push(task);
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
            self.reflections.push(crate::damage::Reflection {
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
                suppress_lifesteal: mode.lifesteal,
            });
        }
        blocked
    }

    pub fn delay_damage_with_heal(
        &mut self,
        damage: &crate::damage::Damage,
        players: &game::DamagePlayers,
        amount: f32,
        headshot: bool,
        random_heal: i32,
    ) -> bool {
        let Some(mode) = self.controller.current_mode() else { return false };
        if self.controller.phase() != Phase::Running || !mode.delayed_damage {
            return false;
        }
        let (Some(target), Some(source)) = (players.victim, players.attacker) else {
            return false;
        };
        if s2sdk::GetClientTeam(source.slot) as i32 == 1
            || !crate::damage::delayed_damage_eligible(target.slot, source.slot, damage.damage_type, amount)
        {
            return false;
        }
        let Some(target_task) = self.combat_task(target) else { return false };
        if self.controllers.get(&source.slot) != Some(&source.controller_handle) {
            return false;
        }
        let Some(source_task) = self
            .players
            .damage_source_task(source.slot, self.controller.generation(), source.pawn_handle as u32)
        else {
            return false;
        };
        let now = s2sdk::GetGameTime();
        if !now.is_finite() {
            return false;
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
        self.delayed_damage.push(hit);
        true
    }

    pub fn random_damage_roll(
        &mut self,
        damage: &crate::damage::Damage,
        players: &game::DamagePlayers,
        amount: f32,
    ) -> Option<funrounds_core::RandomDamage> {
        let mode = self.controller.current_mode()?;
        if self.controller.phase() != Phase::Running || !mode.random_damage || damage.damage_type & crate::damage::DMG_FALL != 0 {
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
        self.random_heals.push(RandomHealReceipt {
            target,
            source,
            target_live,
            source_live,
            frame_token,
            amount,
        });
        Ok(())
    }

    fn settle_random_heal(&mut self, victim: i32, attacker: i32, event: usize) -> Option<i32> {
        if s2sdk::GetEventInt(event, &Str::from_str("dmg_health")) <= 0 {
            return None;
        }
        let receipt = if let Some(receipt) = crate::damage::internal_random_heal_hurt(victim, attacker) {
            Some(receipt)
        } else {
            let token = crate::damage::current_damage_frame_token().filter(|token| *token != 0)?;
            let index = self
                .random_heals
                .iter()
                .position(|receipt| receipt.target.player.slot == victim && receipt.source.player.slot == attacker && receipt.frame_token == token)?;
            Some(self.random_heals.remove(index))
        };
        let receipt = receipt?;
        if !self.controller.current_mode().is_some_and(|mode| mode.random_damage)
            || receipt.amount <= 0
            || receipt.target.round != self.controller.generation()
            || receipt.source.round != self.controller.generation()
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

    fn refresh_translucent(&mut self) {
        let generation = self.controller.generation();
        let mut failure = None;
        for effect in self.effects.values_mut().chain(self.retired_effects.iter_mut()) {
            if let Err(error) = effect.release_unowned_translucent_weapons(generation) {
                failure = Some(error);
                break;
            }
        }
        if let Some(error) = failure {
            self.handle_fault(error);
            return;
        }
        let Some(mode) = self.controller.current_mode() else { return };
        for effect in self.effects.values_mut() {
            let slot = effect.identity.slot;
            if Some(effect.identity) != self.players.identity(slot) {
                continue;
            }
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            if self.controllers.get(&slot) != Some(&player.controller_handle) || effect.identity.pawn_handle != player.pawn_handle as u32 {
                continue;
            }
            if let Err(error) = effect.update_translucent(mode, generation) {
                failure = Some(error);
                break;
            }
        }
        if let Some(error) = failure {
            self.handle_fault(error);
        }
    }

    fn refresh_combat(&mut self) {
        let Some(mode) = self.controller.current_mode() else { return };
        let now = s2sdk::GetGameTime();
        let mut failure = None;
        for (task, life) in self.combat.values_mut() {
            let Some(player) = LivePlayer::resolve(task.player.slot) else { continue };
            if !self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                || !self.effects.get(&player.slot).is_some_and(|effect| effect.identity == task.player)
            {
                continue;
            }
            let health = s2sdk::GetClientHealth(player.slot);
            if let Some(next) = life.recharge(now, health, mode.health)
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

    fn refresh_classic(&mut self, slots: Option<&[i32]>) {
        if !self.classic_round() || self.controller.phase() != Phase::Running {
            return;
        }
        let Some(mode) = self.controller.current_mode() else { return };
        let now = s2sdk::GetGameTime();
        let mut failure = None;
        for effect in self.effects.values_mut() {
            let slot = effect.identity.slot;
            if slots.is_some_and(|slots| !slots.contains(&slot)) || Some(effect.identity) != self.players.identity(slot) {
                continue;
            }
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            if self.controllers.get(&slot) != Some(&player.controller_handle) || effect.identity.pawn_handle != player.pawn_handle as u32 {
                continue;
            }
            if let Err(error) = effect.update_classic(mode, now, self.suicides.active(player)) {
                failure = Some(error);
                break;
            }
        }
        if let Some(error) = failure {
            self.handle_fault(error);
        }
    }

    fn refresh_view(&mut self) {
        if !self.classic_round() {
            return;
        }
        let generation = self.controller.generation();
        let now = s2sdk::GetGameTime();
        let mut failure = None;
        for effect in self.effects.values_mut() {
            let slot = effect.identity.slot;
            if Some(effect.identity) != self.players.identity(slot) {
                continue;
            }
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            if self.controllers.get(&slot) != Some(&player.controller_handle) || effect.identity.pawn_handle != player.pawn_handle as u32 {
                continue;
            }
            if let Err(error) = effect.release_inactive_scoped_weapon(generation)
                && error.scope != Scope::Cancelled
            {
                failure = Some(error);
                break;
            }
        }
        if let Some(error) = failure {
            self.handle_fault(error);
            return;
        }
        let Some(mode) = self.controller.current_mode() else { return };
        for effect in self.effects.values_mut() {
            let slot = effect.identity.slot;
            if Some(effect.identity) != self.players.identity(slot) {
                continue;
            }
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            if self.controllers.get(&slot) != Some(&player.controller_handle) || effect.identity.pawn_handle != player.pawn_handle as u32 {
                continue;
            }
            if let Err(error) = effect.update_view(mode, now, generation)
                && error.scope != Scope::Cancelled
            {
                failure = Some(error);
                break;
            }
        }
        if let Some(error) = failure {
            self.handle_fault(error);
        }
    }

    fn refresh_periodic_movement(&mut self) {
        if !self.classic_round() || self.controller.phase() != Phase::Running {
            return;
        }
        let generation = self.controller.generation();
        let scale = self
            .periodic_movement
            .as_mut()
            .filter(|(round, _)| *round == generation)
            .and_then(|(_, movement)| {
                movement.update(s2sdk::GetGameTime(), &mut self.rng);
                movement.current()
            });
        let Some(scale) = scale else { return };
        let mut failure = None;
        for effect in self.effects.values_mut() {
            let slot = effect.identity.slot;
            if Some(effect.identity) != self.players.identity(slot) {
                continue;
            }
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            if self.controllers.get(&slot) != Some(&player.controller_handle) || effect.identity.pawn_handle != player.pawn_handle as u32 {
                continue;
            }
            if let Err(error) =
                effect.update_periodic_movement(scale, self.controller.current_mode().expect("active mode"), self.suicides.active(player))
            {
                failure = Some(error);
                break;
            }
        }
        if let Some(error) = failure {
            self.handle_fault(error);
        }
    }

    fn send_welcomes(&mut self) {
        if !self.classic_enabled() {
            self.welcomes.clear();
            return;
        }
        let now = s2sdk::GetGameTime();
        for (identity, due) in std::mem::take(&mut self.welcomes) {
            if !self.players.validates_connection(identity)
                || !s2sdk::IsClientInGame(identity.slot)
                || self.controllers.get(&identity.slot) != Some(&s2sdk::PlayerSlotToEntHandle(identity.slot))
            {
                continue;
            }
            if now < due {
                self.welcomes.push((identity, due));
                continue;
            }
            if !s2sdk::IsFakeClient(identity.slot) {
                s2sdk::PrintToChat(identity.slot, &Str::from_str(&format!("欢迎来到 FunRounds {}", funrounds_core::VERSION)));
                for line in crate::commands::HELP.lines() {
                    s2sdk::PrintToChat(identity.slot, &Str::from_str(line));
                }
            }
        }
    }

    pub fn action_valid(&self, action: &EngineAction) -> bool {
        match action {
            EngineAction::Rollback(task, player) => {
                self.players.identity(player.slot) == Some(task.player)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && task.player.pawn_handle == player.pawn_handle as u32
                    && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            }
            EngineAction::Loadout(task, player, _)
            | EngineAction::Upgrade(task, player, _, _)
            | EngineAction::GrantC4(task, player)
            | EngineAction::RemoveC4(task, player, _) => {
                self.classic_round()
                    && self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && self.controllers.get(&player.slot) == Some(&player.controller_handle)
            }
            EngineAction::Suicide(action) => {
                (match action {
                    crate::suicide::Action::Purchase { start, .. } => self.c4_purchase_enabled(*start),
                    _ => self.suicide_enabled(),
                }) && self
                    .players
                    .validates(action.task(), self.controller.generation(), action.player().pawn_handle as u32)
                    && LivePlayer::resolve(action.player().slot) == Some(action.player())
            }
            EngineAction::DelayedDamage(hit) => {
                self.classic_round()
                    && self.controller.phase() == Phase::Running
                    && self.controller.current_mode().is_some_and(|mode| {
                        mode.delayed_damage
                            && delayed_damage_protection_valid(
                                mode.combat_protection,
                                hit.target,
                                self.combat
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
            EngineAction::Reflection(hit) => {
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
                        .get(&hit.target.player.slot)
                        .is_some_and(|(task, life)| *task == hit.target && !life.protected(s2sdk::GetGameTime()))
                    && hit.native_handles(s2sdk::IsValidEntHandle).is_some()
            }
            EngineAction::CenterDisplay { generation, .. } => {
                center_display_action_valid(*generation == self.controller.generation(), self.controller.phase(), self.effects_ready)
            }
            EngineAction::Projectile(action) => {
                self.projectiles.action_current(*action)
                    && match action.kind {
                        crate::projectiles::ActionKind::SpawnKnife { task, player } => {
                            self.effects_ready
                                && self.controller.phase() == Phase::Running
                                && self.controller.current_mode().is_some_and(|mode| mode.throwing_knives)
                                && self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
                                && LivePlayer::resolve(player.slot) == Some(player)
                        }
                        crate::projectiles::ActionKind::Remove { .. } => true,
                    }
            }
            EngineAction::Bomb(action) => self.bombs.action_valid(*action, self.controller.generation(), &self.players),
            EngineAction::Equipment(action) => {
                self.refills.current(*action)
                    && match action {
                        equipment::Action::Refill(refill) => {
                            self.effects_ready
                                && self.controller.phase() != Phase::Idle
                                && self.controller.current_mode().is_some_and(|mode| mode.replenish.contains(&refill.kind))
                                && self
                                    .players
                                    .validates(refill.task, self.controller.generation(), refill.player.pawn_handle as u32)
                                && self.controllers.get(&refill.player.slot) == Some(&refill.player.controller_handle)
                                && self
                                    .effects
                                    .get(&refill.player.slot)
                                    .is_some_and(|effect| effect.identity == refill.task.player)
                                && LivePlayer::resolve(refill.player.slot) == Some(refill.player)
                        }
                        equipment::Action::Remove { .. } => true,
                    }
            }
            EngineAction::ProjectileImpact(_) => true,
            EngineAction::Respawn(task, player, team) => {
                respawn_window_open(
                    self.controller.phase(),
                    self.team_match.as_ref().map(|game| game.remaining(s2sdk::GetGameTime())),
                ) && self.players.validates_respawn(*task, self.controller.generation())
                    && respawn_target_valid(
                        *player,
                        LivePlayer::resolve_present(player.slot),
                        *team,
                        s2sdk::GetClientTeam(player.slot) as i32,
                        s2sdk::IsClientAlive(player.slot),
                    )
            }
            EngineAction::LowHealthFinish(task, player, _) => {
                self.controller.phase() != Phase::Idle
                    && self.controller.current_mode().is_some_and(|mode| mode.self_damage_finish_enabled())
                    && self.players.validates(*task, self.controller.generation(), player.pawn_handle as u32)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && funrounds_core::low_health_finish(s2sdk::GetClientHealth(player.slot))
            }
            EngineAction::EndRound(round, _) => {
                self.controller.phase() == Phase::Running && self.controller.generation() == *round && self.team_match.is_some()
            }
        }
    }

    pub fn cancel_bomb_action(&mut self, action: crate::bombs::Action) {
        self.bombs.action_cancelled(action);
    }

    pub fn finish_equipment_action(&mut self, action: equipment::Action, result: equipment::ActionResult) {
        if !self.action_valid(&EngineAction::Equipment(action)) {
            self.refills.cancel_action(action);
        }
        match self.refills.finish(action, result) {
            Ok(completion) => {
                if let Some((task, id)) = completion.retry {
                    self.queue(task.player, s2sdk::GetGameTickCount(), PendingAction::Grenade(id));
                }
                if let Some(diagnostic) = completion.diagnostic {
                    log(&diagnostic);
                }
            }
            Err(error) => self.handle_fault(error),
        }
    }

    pub fn take_equipment_cleanup(&mut self) -> Vec<equipment::Action> {
        self.refills.cleanup_actions()
    }

    pub fn finish_bomb_action(&mut self, action: crate::bombs::Action, result: Result<crate::bombs::ActionResult, Fault>) {
        let accept = self.action_valid(&EngineAction::Bomb(action));
        if let Err(error) = self.bombs.action_finished(action, result, accept) {
            self.handle_fault(error);
        }
    }

    pub fn prepare_carrier_disconnect(&mut self, slot: i32) -> Option<crate::bombs::Action> {
        let player = LivePlayer::resolve_present(slot)?;
        // Death retires the effect life, but cleanup must still release that connection's native pawn.
        self.players.connection_identity(slot)?;
        if self.controllers.get(&slot) != Some(&player.controller_handle) {
            return None;
        }
        self.bombs.prepare_carrier_disconnect(self.controller.generation(), player)
    }

    pub fn take_bomb_actions(&mut self) -> Vec<crate::bombs::Action> {
        self.bombs.take_actions()
    }

    pub fn take_bomb_explosion(&mut self) -> Option<crate::bombs::Explosion> {
        self.bombs.take_explosion(self.controller.generation())
    }

    pub fn finish_bomb_explosion(&mut self, explosion: crate::bombs::Explosion, temporary: i32, result: Result<(), Fault>) {
        if self.bombs.finish_explosion(explosion, temporary)
            && let Err(error) = result
        {
            self.handle_fault(error);
        }
    }

    pub fn blocks_native_bomb_damage(&self, inflictor: i32) -> bool {
        self.bombs.blocks_native_explosion(inflictor)
    }

    pub fn update(&mut self) -> Vec<EngineAction> {
        let tick = self.observe_tick(None);
        let frame = crate::damage::current_damage_frame_token().filter(|token| *token != 0);
        self.random_heals.retain(|receipt| Some(receipt.frame_token) == frame);
        if !self.effects_ready {
            self.cleanup_effects(false);
            self.restore_team_names();
            self.finish_recovery();
        }
        let center_display = self.send_pending_center(tick);
        let mut actions = std::mem::take(&mut self.engine_actions);
        actions.extend(self.take_rollbacks());
        actions.extend(self.apply_pending(tick));
        actions.extend(self.update_suicides(tick));
        self.refresh_translucent();
        self.refresh_combat();
        self.refresh_classic(None);
        self.refresh_view();
        self.refresh_periodic_movement();
        self.send_welcomes();
        let bomb_settings = self.controller.current_mode().and_then(|mode| mode.bomb.as_ref());
        if let Err(error) = self
            .bombs
            .update(bomb_settings, self.controller.generation(), &self.players, &mut self.rng)
        {
            self.handle_fault(error);
        }
        actions.extend(center_display);
        actions.extend(self.take_equipment_cleanup().into_iter().map(EngineAction::Equipment));
        actions.extend(std::mem::take(&mut self.impacts).into_iter().map(EngineAction::ProjectileImpact));
        actions.extend(self.bombs.take_actions().into_iter().map(EngineAction::Bomb));
        for hit in std::mem::take(&mut self.reflections) {
            if tick > hit.tick {
                actions.push(EngineAction::Reflection(hit));
            } else {
                self.reflections.push(hit);
            }
        }
        let now = s2sdk::GetGameTime();
        for hit in std::mem::take(&mut self.delayed_damage) {
            if hit.is_due(now) {
                actions.push(EngineAction::DelayedDamage(hit));
            } else {
                self.delayed_damage.push(hit);
            }
        }
        if self.controller.phase() == Phase::Running {
            if self.controller.current_mode().is_some_and(|mode| mode.dodgeball) {
                self.projectiles.discover_decoys(now, self.controller.generation(), &self.players);
            }
            if self.controller.current_mode().is_some_and(|mode| mode.throwing_knives) {
                actions.extend(
                    self.projectiles
                        .take_due_knives(tick, self.controller.generation(), &self.players)
                        .into_iter()
                        .map(EngineAction::Projectile),
                );
            }
            self.projectiles.sweep(now, self.controller.generation(), &self.players);
        }
        actions.extend(self.take_projectile_cleanup().into_iter().map(EngineAction::Projectile));
        if let Some(result) = self.team_match.as_mut().and_then(|game| game.finish(now)) {
            let reason = match result.winner {
                Some(2) => s2sdk::CSRoundEndReason::TerroristWin,
                Some(3) => s2sdk::CSRoundEndReason::CTWin,
                _ => s2sdk::CSRoundEndReason::Draw,
            };
            s2sdk::PrintToChatAll(&Str::from_str(&format!(
                "[FunRounds] 团队竞技结束 T {} : {} CT",
                result.scores[0], result.scores[1]
            )));
            self.respawns.clear();
            actions.push(EngineAction::EndRound(self.controller.generation(), reason));
        }
        for (task, player, team, due) in std::mem::take(&mut self.respawns) {
            if !self.players.validates_respawn(task, self.controller.generation())
                || !respawn_window_open(self.controller.phase(), self.team_match.as_ref().map(|game| game.remaining(now)))
                || !respawn_target_valid(
                    player,
                    LivePlayer::resolve_present(task.player.slot),
                    team,
                    s2sdk::GetClientTeam(task.player.slot) as i32,
                    s2sdk::IsClientAlive(task.player.slot),
                )
            {
                continue;
            }
            if now < due {
                self.respawns.push((task, player, team, due));
            } else {
                log(&format!("respawn slot={} team={team}", task.player.slot));
                actions.push(EngineAction::Respawn(task, player, team));
            }
        }
        for task in std::mem::take(&mut self.low_health_finishes) {
            if let Some(player) = LivePlayer::resolve(task.player.slot)
                && self.players.validates(task, self.controller.generation(), player.pawn_handle as u32)
                && self.controller.current_mode().is_some_and(|mode| mode.self_damage_finish_enabled())
                && funrounds_core::low_health_finish(s2sdk::GetClientHealth(player.slot))
            {
                actions.push(EngineAction::LowHealthFinish(
                    task,
                    player,
                    self.controller.current_mode().is_some_and(|mode| mode.native_self_damage_enabled()),
                ));
            }
        }
        actions
    }

    fn send_pending_center(&mut self, tick: i32) -> Option<EngineAction> {
        let (generation, scheduled, text) = self.center_display.take()?;
        let current_generation = self.controller.generation();
        let phase = self.controller.phase();
        let current = center_display_action_valid(generation == current_generation, phase, self.effects_ready);
        if center_display_due(current, phase, tick, scheduled) {
            Some(EngineAction::CenterDisplay { generation, text })
        } else if current {
            self.center_display = Some((generation, scheduled, text));
            None
        } else {
            None
        }
    }

    fn observe_tick(&mut self, event: Option<&str>) -> i32 {
        let tick = s2sdk::GetGameTickCount();
        let previous = self.last_tick;
        if observe_lifecycle_tick(&mut self.last_tick, tick, event) {
            log(&format!(
                "game lifecycle tick moved backwards previous={previous:?} current={tick}; discarding map state"
            ));
            // The new map may reuse old handles; old pawn properties no longer belong to us.
            discard_map_effects(&mut self.effects, &mut self.retired_effects);
            self.bombs.discard_map_state();
            self.projectiles.discard_map_state();
            self.refills.discard_map_state();
            self.impacts.clear();
            self.suicides.entries.clear();
            self.suicides.pending.clear();
            self.suicides.buying.clear();
            self.players.clear();
            self.controllers.clear();
            self.welcomes.clear();
            self.preferences.clear_sessions();
            self.last_name = None;
            self.restore_team_names();
            self.cleanup_effects(false);
            self.controller.reset();
            self.round = None;
        }
        if let Some(round) = self.round
            && self.controller.phase() != Phase::Idle
            && game::warmup() != Some(false)
        {
            log("warmup resumed or became unavailable; cleaning active round");
            self.cleanup_effects(false);
            self.controller.start_round(round, true, &mut self.rng);
        }
        tick
    }

    fn apply_pending(&mut self, tick: i32) -> Vec<EngineAction> {
        let mut actions = Vec::new();
        let pending = std::mem::take(&mut self.pending);
        for (mut task, scheduled, action) in pending {
            if task.round != self.controller.generation() || self.controller.phase() == Phase::Idle {
                self.cancel_pending(action);
                continue;
            }
            if tick <= scheduled {
                self.pending.push((task, scheduled, action));
                continue;
            }
            let Some(player) = LivePlayer::resolve(task.player.slot) else {
                if self.players.identity(task.player.slot) == Some(task.player)
                    || self.players.pending_identity(task.player.slot) == Some(task.player)
                {
                    self.pending.push((task, scheduled, action));
                } else {
                    self.cancel_pending(action);
                }
                continue;
            };
            if self.controllers.get(&player.slot) != Some(&player.controller_handle) {
                self.cancel_pending(action);
                continue;
            }
            if task.player.pawn_handle == u32::MAX {
                let Some(identity) = self.players.bind_spawn(task.player, player.pawn_handle as u32) else {
                    continue;
                };
                if let Some((combat_task, _)) = self.combat.get_mut(&player.slot)
                    && *combat_task == task
                {
                    combat_task.player = identity;
                }
                task.player = identity;
            }
            if !self.players.validates(task, self.controller.generation(), player.pawn_handle as u32) {
                self.cancel_pending(action);
                continue;
            }
            if action == PendingAction::RecoilSettle {
                if !self.controller.current_mode().is_some_and(|mode| mode.no_recoil) {
                    continue;
                }
                let Some(effect) = self.effects.get(&player.slot).filter(|effect| effect.identity == task.player) else {
                    continue;
                };
                match effect.settle_recoil() {
                    Ok(diagnostic) => {
                        if let Some(diagnostic) = diagnostic {
                            log(&diagnostic);
                            self.queue(task.player, tick, PendingAction::RecoilObservation);
                        }
                    }
                    Err(error) => {
                        self.handle_fault(error.context("post-shot recoil cleanup failed"));
                        continue;
                    }
                }
                continue;
            }
            if action == PendingAction::RecoilObservation {
                if self.diagnostics()
                    && let Some(effect) = self.effects.get(&player.slot).filter(|effect| effect.identity == task.player)
                {
                    match effect.observe_recoil() {
                        Ok(diagnostic) => log(&diagnostic),
                        Err(error) => log(&format!("post-shot diagnostic unavailable: {error}")),
                    }
                }
                continue;
            }
            if let PendingAction::AmmoObservation(handle) = action {
                if self.diagnostics()
                    && self.controller.current_mode().is_some_and(|mode| mode.ammo == Ammo::Clip)
                    && let Some(effect) = self.effects.get(&player.slot).filter(|effect| effect.identity == task.player)
                {
                    match effect.observe_ammo(handle) {
                        Ok(diagnostic) => log(&diagnostic),
                        Err(error) => log(&format!("post-shot ammo unavailable: {error}")),
                    }
                }
                continue;
            }
            if let PendingAction::Grenade(id) = action {
                if let Some(action) = self.refills.start(id) {
                    actions.push(EngineAction::Equipment(action));
                }
                continue;
            }
            if self.effects.get(&player.slot).is_some_and(|effect| effect.identity == task.player) {
                continue;
            }
            let Some(mode) = self.controller.current_mode() else { continue };
            let equipment = if mode.random_loadout {
                mode.select_equipment(&mut self.rng)
                    .expect("validated individual loadout")
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            } else {
                self.equipment.clone()
            };
            actions.push(EngineAction::Loadout(task, player, equipment));
        }
        actions
    }

    fn cancel_pending(&mut self, action: PendingAction) {
        if let PendingAction::Grenade(id) = action {
            self.refills.cancel(id);
        }
    }

    pub fn event(&mut self, name: &str, event: usize) {
        self.observe_tick(Some(name));
        match name {
            "round_prestart" => {
                // Native C4 allocation precedes round_start, even when a restart omits round_end.
                if let Err(error) = self.parameters.restore_c4_assignment() {
                    self.handle_fault(Fault::global(error));
                }
                if let Err(error) = self.bombs.cleanup() {
                    self.handle_fault(Fault::global(error));
                }
            }
            "bomb_exploded" => self.bombs.capture_explosion(self.controller.generation()),
            "bomb_beginplant" | "bomb_begindefuse" => {
                if self.controller.phase() != Phase::Running || self.controller.current_mode().is_none_or(|mode| mode.bomb.is_none()) {
                    return;
                }
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                if let Some(player) = self.players.identity(slot) {
                    let task = PlayerTask {
                        player,
                        round: self.controller.generation(),
                    };
                    if name == "bomb_beginplant" {
                        self.bombs.begin_plant(task);
                    } else {
                        if let Err(error) = self.bombs.begin_defuse(task, s2sdk::GetEventBool(event, &Str::from_str("haskit"))) {
                            self.handle_fault(error);
                        }
                    }
                }
            }
            "bomb_abortplant" => {
                if let Err(error) = self.bombs.abort_plant() {
                    self.handle_fault(error);
                }
            }
            "bomb_abortdefuse" => {
                if let Err(error) = self.bombs.abort_defuse() {
                    self.handle_fault(error);
                }
            }
            "bomb_planted" => {
                if self.controller.phase() == Phase::Running && self.controller.current_mode().is_some_and(|mode| mode.bomb.is_some()) {
                    let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                    let task = self.players.identity(slot).map(|player| PlayerTask {
                        player,
                        round: self.controller.generation(),
                    });
                    self.bombs.planted(self.controller.generation(), task);
                }
            }
            "round_start" => {
                let tick = s2sdk::GetGameTickCount();
                if tick < 0 {
                    return;
                }
                let key = self.controller.round_key(tick as u64);
                if self.controller.is_new_round(key) {
                    if let Some(previous) = self.round {
                        self.controller.end_round(previous);
                    }
                    self.cleanup_effects(false);
                    self.restore_team_names();
                    self.finish_recovery();
                }
                if self
                    .controller
                    .start_round(key, game::warmup() != Some(false) || !self.effects_ready, &mut self.rng)
                {
                    self.round = Some(key);
                    if !self.classic_enabled() {
                        self.restore_team_names();
                        self.welcomes.clear();
                    }
                    if let Some(mode) = self.controller.current_mode() {
                        self.equipment = mode
                            .select_equipment(&mut self.rng)
                            .expect("active mode was validated")
                            .into_iter()
                            .map(str::to_owned)
                            .collect();
                        log(&format!(
                            "round mode={} name={} equipment={:?} headshots={:?}",
                            mode.id, mode.name, self.equipment, mode.headshots
                        ));
                        if let Err(error) = self.parameters.apply(mode) {
                            self.handle_fault(error);
                            return;
                        }
                        if mode.dodgeball {
                            self.projectiles.baseline_decoys();
                        }
                        if parameters::has_convar("mp_teamname_1") && parameters::has_convar("mp_teamname_2") {
                            let display = self
                                .team_names
                                .set_string("mp_teamname_1", &mode.name)
                                .and_then(|()| self.team_names.set_string("mp_teamname_2", self.last_name.as_deref().unwrap_or("")));
                            if let Err(error) = display {
                                self.handle_fault(error);
                                return;
                            }
                        }
                        for line in mode.classic_chat() {
                            s2sdk::PrintToChatAll(&Str::from_str(&line));
                        }
                        self.center_display = Some((self.controller.generation(), tick, escape_html_text(&mode.name)));
                        s2sdk::PrintToConsoleAll(&Str::from_str(&separate_console_output(mode.classic_console())));
                        self.last_name = Some(mode.name.clone());
                        for slot in 0..s2sdk::GetMaxClients() {
                            if LivePlayer::resolve(slot).is_some() {
                                self.spawn(slot, tick);
                            } else if self.controller.current_mode().is_some_and(|mode| mode.round_respawn) {
                                self.queue_respawn(slot, 0.1);
                            }
                        }
                    }
                }
            }
            "round_freeze_end" => {
                if let Some(round) = self.round {
                    let started = self.controller.freeze_end(round);
                    if started {
                        self.queue_c4_assignment();
                    }
                    if started
                        && self
                            .controller
                            .current_mode()
                            .is_some_and(|mode| mode.periodic_speed || mode.periodic_gravity)
                    {
                        match funrounds_core::PeriodicMovement::start(s2sdk::GetGameTime()) {
                            Ok(movement) => self.periodic_movement = Some((self.controller.generation(), movement)),
                            Err(error) => self.handle_fault(Fault::mechanism(Mechanism::PeriodicMovement, error)),
                        }
                    }
                    if started
                        && self
                            .controller
                            .current_mode()
                            .is_some_and(|mode| mode.win_condition == funrounds_core::WinCondition::TeamDeathmatch)
                    {
                        match game::round_duration().and_then(|duration| TeamDeathmatch::start(s2sdk::GetGameTime(), duration)) {
                            Ok(game) => {
                                log(&format!("team match started duration={}", game.remaining(s2sdk::GetGameTime())));
                                self.team_match = Some(game);
                            }
                            Err(error) => self.handle_fault(Fault::mechanism(Mechanism::TeamDeathmatch, error)),
                        }
                    }
                }
            }
            "round_end" => {
                if let Some(round) = self.round
                    && self.controller.end_round(round)
                {
                    self.cleanup_effects(true);
                }
            }
            "player_spawn" => self.spawn(s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid")), s2sdk::GetGameTickCount()),
            "grenade_thrown" => {
                let weapon = s2sdk::GetEventString(event, &Str::from_str("weapon")).to_string();
                let kind = match weapon.strip_prefix("weapon_").unwrap_or(&weapon) {
                    "hegrenade" => Some(equipment::GrenadeKind::He),
                    "flashbang" => Some(equipment::GrenadeKind::Flash),
                    "smokegrenade" => Some(equipment::GrenadeKind::Smoke),
                    "decoy" => Some(equipment::GrenadeKind::Decoy),
                    "molotov" => Some(equipment::GrenadeKind::Molotov),
                    "incgrenade" => Some(equipment::GrenadeKind::Incendiary),
                    _ => None,
                };
                if let Some(kind) = kind.filter(|kind| self.controller.current_mode().is_some_and(|mode| mode.replenish.contains(kind))) {
                    let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                    if let Some(identity) = self.players.identity(slot) {
                        if kind == equipment::GrenadeKind::Decoy && self.controller.current_mode().is_some_and(|mode| mode.dodgeball) {
                            self.projectiles.observe_decoy_throw(
                                PlayerTask {
                                    player: identity,
                                    round: self.controller.generation(),
                                },
                                s2sdk::GetGameTime(),
                            );
                        }
                        if let Some(player) = LivePlayer::resolve(slot)
                            && player.pawn_handle as u32 == identity.pawn_handle
                            && self.controllers.get(&slot) == Some(&player.controller_handle)
                        {
                            let task = PlayerTask {
                                player: identity,
                                round: self.controller.generation(),
                            };
                            let (id, spent) = self.refills.queue(task, player, kind, self.diagnostics());
                            self.queue(identity, s2sdk::GetGameTickCount(), PendingAction::Grenade(id));
                            if self.diagnostics() {
                                log(&format!(
                                    "grenade refill queued slot={slot} weapon={weapon} spent_handle={spent:?} transaction={id}"
                                ));
                            }
                        }
                    }
                }
            }
            "weapon_fire" => {
                let weapon = s2sdk::GetEventString(event, &Str::from_str("weapon")).to_string();
                if matches!(weapon.as_str(), "knife" | "knife_t" | "weapon_knife" | "weapon_knife_t")
                    && self.controller.phase() == Phase::Running
                    && self.controller.current_mode().is_some_and(|mode| mode.throwing_knives)
                {
                    let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                    if let Some(player) = self.players.identity(slot) {
                        self.projectiles.schedule_knife(
                            PlayerTask {
                                player,
                                round: self.controller.generation(),
                            },
                            s2sdk::GetGameTickCount(),
                        );
                    }
                    return;
                }
                if !crate::effects::firearm_event(&weapon) {
                    return;
                }
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                let Some(player) = LivePlayer::resolve(slot) else { return };
                let Some(mode) = self.controller.current_mode() else { return };
                let identity = self.players.identity(slot);
                let knock = self.preferences.value(slot, crate::preferences::authenticated_id(slot));
                if mode.self_push
                    && self.classic_round()
                    && self.controller.phase() == Phase::Running
                    && knock.permits(s2sdk::GetClientFlags(slot))
                    && self.controllers.get(&slot) == Some(&player.controller_handle)
                    && let Some(effect) = self
                        .effects
                        .get(&slot)
                        .filter(|effect| Some(effect.identity) == identity && effect.identity.pawn_handle == player.pawn_handle as u32)
                    && let Err(error) = effect.self_push(player, &weapon)
                {
                    self.handle_fault(error);
                    return;
                }
                if matches!(mode.self_damage, Some(SelfDamage::MissPenalty))
                    && funrounds_core::low_health_finish(s2sdk::GetClientHealth(slot))
                    && let Some(player) = identity
                {
                    let task = PlayerTask {
                        player,
                        round: self.controller.generation(),
                    };
                    if !self.low_health_finishes.contains(&task) {
                        self.low_health_finishes.push(task);
                    }
                }
                let diagnostics = self.diagnostics();
                let no_recoil = mode.no_recoil;
                let recoil_effect = no_recoil || mode.recoil_view.is_some();
                let observe_ammo = diagnostics && mode.ammo == Ammo::Clip;
                if !diagnostics && !recoil_effect {
                    return;
                }
                let Some(effect) = self.effects.get_mut(&slot).filter(|effect| Some(effect.identity) == identity) else {
                    return;
                };
                match effect.weapon_fire(player, mode) {
                    Ok(diagnostic) => {
                        if let Some(diagnostic) = diagnostic {
                            log(&diagnostic);
                        }
                        if observe_ammo && let Some(identity) = identity {
                            self.queue(
                                identity,
                                s2sdk::GetGameTickCount(),
                                PendingAction::AmmoObservation(s2sdk::GetClientActiveWeapon(player.slot)),
                            );
                        }
                        if no_recoil && let Some(identity) = identity {
                            self.queue(identity, s2sdk::GetGameTickCount(), PendingAction::RecoilSettle);
                        }
                    }
                    Err(error) => {
                        if !recoil_effect {
                            log(&format!("shooting diagnostic unavailable: {error}"));
                            return;
                        }
                        self.handle_fault(error.context("shooting effect failed"));
                    }
                }
            }
            "player_disconnect" => {
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                self.forget_player(slot);
            }
            "player_death" => {
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                self.projectiles
                    .retire_player(slot, self.players.identity(slot).map(|player| player.pawn_handle));
                let attacker = s2sdk::GetEventPlayerSlot(event, &Str::from_str("attacker"));
                if self.diagnostics() && self.controller.current_mode().is_some_and(|mode| mode.headshots == Headshots::Only) {
                    log(&format!(
                        "death victim={slot} attacker={attacker} weapon={} headshot={}",
                        s2sdk::GetEventString(event, &Str::from_str("weapon")),
                        s2sdk::GetEventBool(event, &Str::from_str("headshot"))
                    ));
                }
                if self.controller.phase() == Phase::Running {
                    if self.controller.current_mode().is_some_and(|mode| mode.combat_protection)
                        && let Some(player) = LivePlayer::resolve(attacker)
                        && !s2sdk::IsFakeClient(attacker)
                        && let Some(task) = self.combat_task(player)
                        && let Some((expected, life)) = self.combat.get_mut(&attacker)
                        && *expected == task
                    {
                        life.protect(s2sdk::GetGameTime());
                    }
                    if attacker >= 0
                        && slot >= 0
                        && let Some(game) = self.team_match.as_mut()
                        && game.record_kill(
                            s2sdk::GetGameTime(),
                            s2sdk::GetClientTeam(attacker) as i32,
                            s2sdk::GetClientTeam(slot) as i32,
                            attacker != slot,
                        )
                    {
                        log(&format!("team score T={} CT={}", game.scores()[0], game.scores()[1]));
                    }
                    self.kill_loadout(attacker, slot, &s2sdk::GetEventString(event, &Str::from_str("weapon")));
                }
                if let Err(error) = self.suicides.forget(slot) {
                    self.record_fault(Fault::global(error));
                }
                self.players.death(slot);
                self.combat.remove(&slot);
                self.delayed_damage.retain(|hit| hit.target.player.slot != slot);
                self.refills.cancel_player(slot);
                self.retire_effect(slot);
                self.pending.retain(|(task, _, _)| task.player.slot != slot);
                if let Some(respawn) = self.controller.current_mode().and_then(|mode| mode.respawn.as_ref())
                    && respawn.team.matches(s2sdk::GetClientTeam(slot) as i32)
                {
                    self.queue_respawn(slot, respawn.delay_seconds);
                }
            }
            "player_hurt" => {
                let victim = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                let attacker = s2sdk::GetEventPlayerSlot(event, &Str::from_str("attacker"));
                let weapon = s2sdk::GetEventString(event, &Str::from_str("weapon")).to_string();
                if self.diagnostics()
                    && self.controller.current_mode().is_some_and(|mode| {
                        mode.lifesteal
                            || matches!(mode.self_damage, Some(SelfDamage::MissPenalty))
                            || mode.headshots == Headshots::Only
                            || (mode.win_condition == funrounds_core::WinCondition::TeamDeathmatch)
                    })
                {
                    log(&format!(
                        "hurt victim={victim} attacker={attacker} weapon={} dmg_health={} dmg_armor={} health={} armor={} hitgroup={}",
                        weapon,
                        s2sdk::GetEventInt(event, &Str::from_str("dmg_health")),
                        s2sdk::GetEventInt(event, &Str::from_str("dmg_armor")),
                        s2sdk::GetEventInt(event, &Str::from_str("health")),
                        s2sdk::GetEventInt(event, &Str::from_str("armor")),
                        s2sdk::GetEventInt(event, &Str::from_str("hitgroup"))
                    ));
                }
                self.suicide_hurt(event, victim, attacker);
                let healed = match crate::damage::internal_damage_hurt(victim, attacker) {
                    Ok(true) => None,
                    Ok(false) => self.lifesteal(event, attacker, victim),
                    Err(error) => {
                        self.handle_fault(error);
                        None
                    }
                };
                let random_healed = self.settle_random_heal(victim, attacker, event);
                self.refresh_classic(Some(&[victim, random_healed.or(healed).unwrap_or(-1)]));
            }
            "player_connect_full" => {
                self.connect(s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid")));
            }
            "player_team" => {
                let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
                if let Err(error) = self.suicides.forget(slot) {
                    self.handle_fault(Fault::global(error));
                }
                self.players.death(slot);
                self.combat.remove(&slot);
                self.reflections
                    .retain(|hit| hit.target.player.slot != slot && hit.source.player.slot != slot);
                self.delayed_damage
                    .retain(|hit| hit.target.player.slot != slot && hit.source.player.slot != slot);
                self.random_heals
                    .retain(|hit| hit.target.player.slot != slot && hit.source.player.slot != slot);
                self.refills.cancel_player(slot);
                self.pending.retain(|(task, ..)| task.player.slot != slot);
                self.retire_effect(slot);
                self.respawns.retain(|(task, _, _, _)| task.player.slot != slot);
            }
            _ => {}
        }
    }

    pub(crate) fn handle_fault(&mut self, fault: impl Into<Fault>) {
        let fault = fault.into();
        if fault.scope == Scope::Cancelled {
            log(&format!("action cancelled: {fault}"));
            return;
        }
        self.record_fault(fault);
        self.cleanup_effects(false);
        self.restore_team_names();
    }

    fn record_fault(&mut self, fault: Fault) {
        let changed = self.faults.record(&fault);
        self.controller.set_capabilities(self.faults.capabilities());
        if let Some(round) = self.round {
            self.controller.end_round(round);
        }
        self.effects_ready = false;
        self.welcomes.clear();
        if changed {
            log(&format!("fault scope={:?} recovery={:?}: {fault}", fault.scope, self.faults.recovery));
        }
    }

    fn cleanup_complete(&self) -> bool {
        self.equipment_in_flight == 0
            && self.suicides.entries.is_empty()
            && self.suicides.pending.is_empty()
            && self.suicides.buying.is_empty()
            && self.suicides.ff.is_restored()
            && self.effects.is_empty()
            && self.retired_effects.is_empty()
            && self.parameters.is_restored()
            && self.team_names.is_restored()
            && self.bombs.is_clean()
            && self.refills.is_clean()
            && self.projectiles.is_clean()
            && self.pending.is_empty()
            && self.impacts.is_empty()
    }

    fn finish_recovery(&mut self) {
        let complete = self.cleanup_complete();
        if self.faults.finish_cleanup(complete) {
            log("owned cleanup complete; quarantined capabilities remain excluded; waiting for a new round");
        }
        self.effects_ready = complete && self.faults.recovery == Recovery::Ready;
    }

    fn lifesteal(&mut self, event: usize, attacker: i32, victim: i32) -> Option<i32> {
        if self.controller.phase() == Phase::Idle || !self.controller.current_mode().is_some_and(|mode| mode.lifesteal) {
            return None;
        }
        let player = LivePlayer::resolve(attacker);
        if !lifesteal_eligible(attacker, victim, player.is_some()) {
            return None;
        }
        let player = player.expect("eligible attacker is alive");
        if !self.effects.get(&attacker).is_some_and(|effect| {
            Some(effect.identity) == self.players.identity(attacker) && effect.identity.pawn_handle == player.pawn_handle as u32
        }) || self.controllers.get(&attacker) != Some(&player.controller_handle)
        {
            return None;
        }
        let health = s2sdk::GetEventInt(event, &Str::from_str("dmg_health"));
        let armor = s2sdk::GetEventInt(event, &Str::from_str("dmg_armor"));
        let energy_shield = self.controller.current_mode().is_some_and(|mode| mode.energy_shield);
        if let Err(error) = Effect::lifesteal(player, health, armor, energy_shield) {
            self.handle_fault(error);
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

    fn kill_loadout(&mut self, attacker: i32, victim: i32, event_weapon: &str) {
        if attacker == victim || victim < 0 {
            return;
        }
        let Some(player) = LivePlayer::resolve(attacker) else { return };
        if self.controllers.get(&attacker) != Some(&player.controller_handle) {
            return;
        }
        let Some(mode) = self.controller.current_mode().filter(|mode| mode.kill_loadout) else {
            return;
        };
        let Some(effect) = self
            .effects
            .get(&attacker)
            .filter(|effect| Some(effect.identity) == self.players.identity(attacker))
        else {
            return;
        };
        if effect.identity.pawn_handle != player.pawn_handle as u32 {
            return;
        }
        if effect.round != self.controller.generation() {
            return;
        }
        let weapons = s2sdk::GetClientWeapons(attacker);
        let target = effect
            .upgraded_weapon()
            .filter(|(handle, classname)| {
                s2sdk::IsValidEntHandle(*handle)
                    && weapons.iter().any(|owned| owned == handle)
                    && s2sdk::GetEntityClassname(*handle).as_str() == classname
            })
            .map(|(handle, _)| handle)
            .or_else(|| event_weapon_handle(attacker, event_weapon));
        let Some(target) = target else { return };
        let classname = s2sdk::GetEntityClassname(target).to_string();
        if !classname.starts_with("weapon_") || matches!(classname.as_str(), "weapon_knife" | "weapon_knife_t" | "weapon_c4") {
            return;
        }
        let weapon = mode.select_kill_weapon(&mut self.rng).expect("validated kill loadout").to_owned();
        let task = PlayerTask {
            player: effect.identity,
            round: effect.round,
        };
        self.engine_actions.push(EngineAction::Upgrade(task, player, target, weapon));
    }

    fn spawn(&mut self, slot: i32, tick: i32) {
        if slot < 0 || tick < 0 || !s2sdk::IsClientInGame(slot) {
            return;
        }
        self.connect(slot);
        let controller = s2sdk::PlayerSlotToEntHandle(slot);
        if !s2sdk::IsValidEntHandle(controller) {
            return;
        }
        let previous = self.players.identity(slot).or_else(|| self.players.pending_identity(slot));
        let pawn = LivePlayer::resolve(slot).map_or(u32::MAX, |player| player.pawn_handle as u32);
        self.players.spawn(slot, tick as u64, pawn);
        let identity = self.players.identity(slot).or_else(|| self.players.pending_identity(slot));
        if previous != identity {
            self.suicides.entries.remove(&slot);
            self.suicides.buying.remove(&slot);
            self.suicides.pending.retain(|action| action.task().player.slot != slot);
            self.refills.cancel_player(slot);
            self.retire_effect(slot);
            self.pending.retain(|(task, _, _)| task.player.slot != slot);
            self.delayed_damage
                .retain(|hit| hit.target.player.slot != slot && hit.source.player.slot != slot);
        }
        if self.controller.phase() != Phase::Idle
            && let Some(identity) = identity
        {
            if let Some(mode) = self.controller.current_mode()
                && combat_life_required(mode)
            {
                let task = PlayerTask {
                    player: identity,
                    round: self.controller.generation(),
                };
                if !self.combat.get(&slot).is_some_and(|(expected, _)| *expected == task) {
                    self.combat
                        .insert(slot, (task, funrounds_core::CombatLife::new(mode, s2sdk::GetGameTime())));
                }
            }
            self.queue(identity, tick, PendingAction::Loadout);
        }
    }

    fn connect(&mut self, slot: i32) {
        if slot < 0 || slot >= s2sdk::GetMaxClients() || !s2sdk::IsClientInGame(slot) {
            return;
        }
        let controller = s2sdk::PlayerSlotToEntHandle(slot);
        if !s2sdk::IsValidEntHandle(controller) || self.controllers.get(&slot) == Some(&controller) {
            return;
        }
        self.forget_player(slot);
        self.controllers.insert(slot, controller);
        self.players.connect(slot);
        if self.classic_enabled()
            && !s2sdk::IsFakeClient(slot)
            && let Some(identity) = self.players.connection_identity(slot)
        {
            self.welcomes.push((identity, s2sdk::GetGameTime() + 15.0));
        }
    }

    fn queue(&mut self, player: PlayerIdentity, tick: i32, action: PendingAction) {
        let task = PlayerTask {
            player,
            round: self.controller.generation(),
        };
        if !self.pending.iter().any(|(pending, _, queued)| *pending == task && *queued == action) {
            self.pending.push((task, tick, action));
        }
    }

    fn queue_respawn(&mut self, slot: i32, delay: f32) {
        if self.controller.phase() == Phase::Idle || s2sdk::IsClientAlive(slot) {
            return;
        }
        let team = s2sdk::GetClientTeam(slot) as i32;
        if !matches!(team, 2 | 3) {
            return;
        }
        self.connect(slot);
        let Some(player) = LivePlayer::resolve_present(slot) else { return };
        let Some(task) = self.players.respawn_task(slot, self.controller.generation()) else {
            return;
        };
        if !self.respawns.iter().any(|(pending, ..)| *pending == task) {
            self.respawns.push((task, player, team, s2sdk::GetGameTime() + delay));
        }
    }

    fn queue_c4_assignment(&mut self) {
        if self.c4_assigned {
            return;
        }
        self.c4_assigned = true;
        let Some(count) = self.controller.current_mode().and_then(|mode| mode.c4_count) else {
            return;
        };
        let count = usize::from(count);
        let mut candidates = Vec::new();
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            let Some(task) = self.combat_task(player) else { continue };
            let native = s2sdk::GetClientWeapons(slot)
                .iter()
                .copied()
                .filter(|handle| crate::suicide::real_c4(player, *handle) && !self.bombs.excludes_suicide_c4(player, *handle))
                .collect();
            self.engine_actions.push(EngineAction::RemoveC4(task, player, native));
            if s2sdk::GetClientTeam(slot) as i32 == 2 {
                candidates.push((task, player));
            }
        }
        candidates.shuffle(&mut self.rng);
        self.engine_actions.extend(
            candidates
                .into_iter()
                .take(count)
                .map(|(task, player)| EngineAction::GrantC4(task, player)),
        );
    }

    fn forget_player(&mut self, slot: i32) {
        if let Err(error) = self.suicides.forget(slot) {
            self.record_fault(Fault::global(error));
        }
        self.combat.remove(&slot);
        self.delayed_damage
            .retain(|hit| hit.target.player.slot != slot && hit.source.player.slot != slot);
        self.random_heals
            .retain(|receipt| receipt.target.player.slot != slot && receipt.source.player.slot != slot);
        self.reflections
            .retain(|hit| hit.target.player.slot != slot && hit.source.player.slot != slot);
        self.refills.cancel_player(slot);
        self.projectiles
            .retire_player(slot, self.players.identity(slot).map(|player| player.pawn_handle));
        self.retire_effect(slot);
        self.players.disconnect(slot);
        self.controllers.remove(&slot);
        self.preferences.forget(slot);
        self.welcomes.retain(|(identity, _)| identity.slot != slot);
        self.pending.retain(|(task, _, _)| task.player.slot != slot);
        self.respawns.retain(|(task, _, _, _)| task.player.slot != slot);
    }

    fn retire_effect(&mut self, slot: i32) {
        let Some(effect) = self.effects.remove(&slot) else { return };
        let index = self.retired_effects.len();
        self.retired_effects.push(effect);
        if let Err(error) = restore_or_retain(&mut self.retired_effects, index, |effect| {
            effect.restore_retired_state(&mut self.refills)?;
            Ok(effect.equipment_restored())
        }) {
            self.handle_fault(Fault::global(format!("effect restoration deferred for slot={slot}: {error}")));
        }
    }

    fn cleanup_effects(&mut self, round_end: bool) {
        if let Err(error) = self.suicides.cleanup() {
            self.record_fault(Fault::global(error));
        }
        self.combat.clear();
        self.reflections.clear();
        self.delayed_damage.clear();
        self.random_heals.clear();
        self.center_display = None;
        self.refills.cancel_all();
        self.impacts.clear();
        self.projectiles.clear();
        self.pending.clear();
        self.team_match = None;
        self.periodic_movement = None;
        self.c4_assigned = false;
        self.engine_actions.clear();
        self.respawns.clear();
        self.low_health_finishes.clear();
        let bomb_cleanup = if round_end {
            self.bombs.cleanup_after_round_end()
        } else {
            self.bombs.cleanup()
        };
        if let Err(error) = bomb_cleanup {
            self.record_fault(Fault::global(error));
        }
        let slots = self.effects.keys().copied().collect::<Vec<_>>();
        for slot in slots {
            let effect = self.effects.get_mut(&slot).expect("effect slot exists");
            let result = if self.players.identity(effect.identity.slot) == Some(effect.identity) {
                effect.restore(&mut self.refills)
            } else {
                effect.restore_retired_state(&mut self.refills)
            };
            match result {
                Ok(()) if effect.equipment_restored() => {
                    self.effects.remove(&slot);
                }
                Ok(()) => {}
                Err(error) => self.record_fault(Fault::global(error)),
            }
        }
        for index in (0..self.retired_effects.len()).rev() {
            if let Err(error) = restore_or_retain(&mut self.retired_effects, index, |effect| {
                effect.restore_retired_state(&mut self.refills)?;
                Ok(effect.equipment_restored())
            }) {
                self.record_fault(Fault::global(error));
            }
        }
        if let Err(error) = self.parameters.restore() {
            self.record_fault(Fault::global(error));
        }
    }
    pub fn shutdown(&mut self) -> Result<(), String> {
        self.welcomes.clear();
        self.bombs.acknowledge_shutdown_removals();
        self.cleanup_effects(false);
        self.restore_team_names();
        if self.cleanup_complete() {
            Ok(())
        } else {
            Err("owned properties or ConVars remain unrestored; snapshots retained for shutdown retry".into())
        }
    }

    fn restore_team_names(&mut self) {
        if let Err(error) = self.team_names.restore() {
            self.record_fault(Fault::global(error));
        }
    }

    fn suicide_enabled(&self) -> bool {
        self.classic_enabled() && self.controller.phase() == Phase::Running && self.controller.current_mode().is_some_and(|mode| mode.suicide)
    }

    fn c4_purchase_enabled(&self, start: bool) -> bool {
        self.classic_enabled()
            && self.controller.phase() == Phase::Running
            && self
                .controller
                .current_mode()
                .is_some_and(|mode| if start { mode.suicide } else { mode.c4_buy })
    }

    fn qualifying_c4(&self, player: LivePlayer) -> Option<i32> {
        s2sdk::GetClientWeapons(player.slot)
            .iter()
            .copied()
            .find(|weapon| crate::suicide::real_c4(player, *weapon) && !self.bombs.excludes_suicide_c4(player, *weapon))
    }

    fn request_suicide(&mut self, slot: i32, start: bool) -> Result<String, String> {
        if !self.c4_purchase_enabled(start) {
            return Err("现在不能购买C4或自爆".into());
        }
        let player = LivePlayer::resolve(slot).ok_or("需要存活的游戏内玩家")?;
        let task = self.combat_task(player).ok_or("玩家生命尚未就绪")?;
        if self.suicides.buying.get(&slot) == Some(&task) {
            return Err("C4事务正在处理".into());
        }
        if self.suicides.active(player) {
            return Err("已启动自爆".into());
        }
        if let Some(c4) = self.qualifying_c4(player) {
            if self.suicide_enabled() {
                self.suicides.qualify(task, player, c4);
            }
            if start {
                self.suicides.begin(slot, s2sdk::GetGameTickCount(), s2sdk::GetGameTime())?;
            }
            return Ok(if self.suicide_enabled() {
                "[FunRounds] 已有C4，无需重复购买；持C4检视可启动自爆"
            } else {
                "[FunRounds] 已有C4，无需重复购买"
            }
            .into());
        }
        if self.suicides.pending.iter().any(|action| action.task() == task) {
            return Err("C4事务正在处理".into());
        }
        if s2sdk::GetClientTeam(slot) as i32 != 2 {
            return Err("只有T可以购买C4".into());
        }
        self.suicides.buying.insert(slot, task);
        self.suicides.pending.push(crate::suicide::Action::Purchase { task, player, start });
        Ok("[FunRounds] 正在购买C4，价格10000现金".into())
    }

    fn update_suicides(&mut self, tick: i32) -> Vec<EngineAction> {
        if !self.suicide_enabled() {
            return std::mem::take(&mut self.suicides.pending)
                .into_iter()
                .filter(|action| matches!(action, crate::suicide::Action::Purchase { .. }))
                .map(EngineAction::Suicide)
                .collect();
        }
        let now = s2sdk::GetGameTime();
        let mut failures = Vec::new();
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            let Some(task) = self.combat_task(player) else { continue };
            if let Some(entry) = self.suicides.entries.get(&slot)
                && entry.task != task
            {
                // A new life may reuse the same pawn; its fresh properties are not ours.
                self.suicides.entries.remove(&slot);
            }
            if let Some(c4) = self.qualifying_c4(player) {
                self.suicides.qualify(task, player, c4);
            }
            let Some(entry) = self.suicides.entries.get_mut(&slot) else { continue };
            if let Some(sequence) = entry.sequence.as_mut() {
                if sequence.take_due(tick, now) {
                    self.suicides.pending.push(crate::suicide::Action::Blast { task, player });
                }
            } else if !crate::suicide::real_c4(player, entry.c4) {
                self.suicides.entries.remove(&slot);
            } else {
                let look = crate::funrounds_native::GetLookAtWeaponButtonState(player.pawn_handle);
                let edge = look == 1 && !entry.look_down;
                entry.look_down = look == 1;
                if edge
                    && s2sdk::GetClientActiveWeapon(slot) == entry.c4
                    && let Err(error) = self.suicides.begin(slot, tick, now)
                {
                    failures.push(error);
                }
            }
        }
        if let Some(error) = failures.pop() {
            self.handle_fault(Fault::mechanism(Mechanism::Suicide, error));
        }
        let pending = std::mem::take(&mut self.suicides.pending);
        let mut due = Vec::new();
        for action in pending {
            if matches!(action, crate::suicide::Action::Finish { tick: queued, .. } if tick <= queued) {
                self.suicides.pending.push(action);
            } else {
                due.push(EngineAction::Suicide(action));
            }
        }
        due
    }

    pub fn blocks_suicide_pickup(&self, pawn: i32, controller: i32) -> bool {
        self.suicide_enabled()
            && self.suicides.entries.values().any(|entry| {
                entry.player.pawn_handle == pawn
                    && entry.player.controller_handle == controller
                    && entry.sequence.is_some()
                    && self.combat_task(entry.player) == Some(entry.task)
            })
    }

    pub fn existing_suicide_c4(&self, player: LivePlayer) -> Option<i32> {
        self.qualifying_c4(player)
    }

    pub fn suicide_buyer_current(&self, action: crate::suicide::Action) -> bool {
        self.on_game_thread()
            && self.players.validates_connection(action.task().player)
            && self.controllers.get(&action.player().slot) == Some(&action.player().controller_handle)
    }

    pub fn finish_suicide_purchase(&mut self, action: crate::suicide::Action, result: Result<i32, String>) {
        let crate::suicide::Action::Purchase { task, player, start } = action else {
            return;
        };
        if self.suicides.buying.get(&player.slot) == Some(&task) {
            self.suicides.buying.remove(&player.slot);
        }
        if !self.action_valid(&EngineAction::Suicide(action)) {
            return;
        }
        let message = match result {
            Ok(c4) => {
                if self.suicide_enabled() {
                    self.suicides.qualify(task, player, c4);
                }
                if start && let Err(error) = self.suicides.begin(player.slot, s2sdk::GetGameTickCount(), s2sdk::GetGameTime()) {
                    self.handle_fault(Fault::mechanism(Mechanism::Suicide, error));
                }
                if start {
                    "C4购买成功，已请求自爆".to_owned()
                } else {
                    if self.suicide_enabled() {
                        "C4购买成功；持C4检视启动自爆"
                    } else {
                        "C4购买成功"
                    }
                    .to_owned()
                }
            }
            Err(error) => error,
        };
        s2sdk::PrintToChat(player.slot, &Str::from_str(&format!("[FunRounds] {message}")));
    }

    pub fn prepare_suicide_blast(&mut self, action: crate::suicide::Action) -> Result<Vec<(PlayerTask, LivePlayer, f32)>, Fault> {
        let source = action.player();
        let origin = s2sdk::GetEntityAbsOrigin(source.pawn_handle);
        let tick = s2sdk::GetGameTickCount();
        let self_amount = s2sdk::GetClientHealth(source.slot) as f32 * 10.0;
        self.suicides.pending.push(crate::suicide::Action::Finish {
            task: action.task(),
            player: source,
            amount: self_amount,
            tick,
        });
        let mut targets = Vec::new();
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(player) = LivePlayer::resolve(slot).filter(|player| *player != source) else {
                continue;
            };
            let Some(task) = self.combat_task(player) else { continue };
            let position = s2sdk::GetEntityAbsOrigin(player.pawn_handle);
            let distance = ((origin.x - position.x).powi(2) + (origin.y - position.y).powi(2) + (origin.z - position.z).powi(2)).sqrt();
            let amount = funrounds_core::suicide_blast_damage(s2sdk::GetClientHealth(slot), distance);
            if amount <= 0.0 {
                continue;
            }
            if let Some(sequence) = self.suicides.entries.get_mut(&slot).and_then(|entry| entry.sequence.as_mut()) {
                sequence.chain_next_tick(tick);
                continue;
            }
            targets.push((task, player, amount));
        }
        for name in [
            "mp_friendlyfire",
            "ff_damage_reduction_bullets",
            "ff_damage_reduction_grenade",
            "ff_damage_reduction_other",
        ] {
            self.suicides.ff.set_number(name, 1.0)?;
        }
        Ok(targets)
    }

    pub fn suicide_target_valid(&self, task: PlayerTask, player: LivePlayer) -> bool {
        self.suicide_enabled() && self.combat_task(player) == Some(task) && LivePlayer::resolve(player.slot) == Some(player)
    }

    fn suicide_hurt(&mut self, event: usize, victim: i32, attacker: i32) {
        if !crate::damage::internal_suicide_hurt(victim, attacker) {
            return;
        }
        let Some(player) = LivePlayer::resolve_present(victim) else { return };
        let Some(task) = self.combat_task(player) else { return };
        if let Some((expected, life)) = self.combat.get_mut(&victim).filter(|(expected, _)| *expected == task) {
            let _ = expected;
            life.lose_native_armor(s2sdk::GetEventInt(event, &Str::from_str("dmg_armor")));
            if life.protected(s2sdk::GetGameTime()) && s2sdk::IsClientAlive(victim) {
                let restored = s2sdk::GetClientHealth(victim).saturating_add(s2sdk::GetEventInt(event, &Str::from_str("dmg_health")).max(0));
                s2sdk::SetClientHealth(victim, restored);
                if s2sdk::GetClientHealth(victim) != restored {
                    self.handle_fault(Fault::mechanism(Mechanism::Suicide, "自爆保护补偿失败"));
                }
            }
        }
    }

    pub fn finish_suicide_blast(&mut self) {
        if let Err(error) = self.suicides.ff.restore() {
            self.handle_fault(Fault::global(error));
        }
    }

    pub fn command(&mut self, command: Command, slot: i32) -> Result<String, String> {
        match command {
            Command::Status => {
                let summary = format!(
                    "[FunRounds] {} enabled={} phase={:?} current={} next={} pending_reload={} effects_ready={} diagnostics={}",
                    funrounds_core::VERSION,
                    self.controller.enabled(),
                    self.controller.phase(),
                    self.controller.current_mode().map_or("ordinary", |mode| mode.id.as_str()),
                    self.controller.forced_next().unwrap_or("random"),
                    self.controller.pending_config().is_some(),
                    self.effects_ready,
                    self.diagnostics(),
                );
                let details = format!(
                    "config={}\ncapabilities={:?}\nexcluded={}\ncapability_reasons={}",
                    self.config_path.display(),
                    self.controller.capabilities(),
                    self.controller
                        .config()
                        .map(|config| config
                            .modes
                            .iter()
                            .filter(|mode| mode.enabled)
                            .filter_map(|mode| {
                                let missing = self.controller.capabilities().missing_for(mode);
                                (!missing.is_empty()).then(|| format!("{}:[{}]", mode.id, missing.join(",")))
                            })
                            .collect::<Vec<_>>()
                            .join(";"))
                        .unwrap_or_default(),
                    self.capability_reasons.join(";"),
                );
                Ok(format_status(
                    &summary,
                    &self.faults,
                    !self.effects_ready && !self.cleanup_complete(),
                    &details,
                ))
            }
            Command::List => Ok(self.controller.config().map_or_else(
                || "[FunRounds] no valid active configuration".into(),
                |config| {
                    config
                        .modes
                        .iter()
                        .map(|mode| format!("{} enabled={} {}", mode.id, mode.enabled, mode.name))
                        .collect::<Vec<_>>()
                        .join("\n")
                },
            )),
            Command::Next(next) => {
                self.controller.force_next(next)?;
                Ok(format!("[FunRounds] next={}", self.controller.forced_next().unwrap_or("random")))
            }
            Command::Reload => {
                let source = std::fs::read_to_string(&self.config_path).map_err(|error| format!("{}: {error}", self.config_path.display()))?;
                self.controller.queue_reload(&source)?;
                Ok("[FunRounds] valid configuration queued for the next round".into())
            }
            Command::Enable(enabled) => {
                if !enabled {
                    self.cleanup_effects(false);
                    self.restore_team_names();
                    self.welcomes.clear();
                }
                self.controller.set_enabled(enabled);
                Ok(format!("[FunRounds] enabled={enabled}; activation begins at the next round"))
            }
            Command::Info => Ok(format!(
                "FunRounds {}\n{}",
                funrounds_core::VERSION,
                self.controller.current_mode().map_or_else(
                    || "当前模式：普通游戏".into(),
                    |mode| format!(
                        "状态：{}\n模式：{} {}\n说明：{}",
                        if self.controller.phase() == Phase::Preparing {
                            "准备"
                        } else {
                            "进行"
                        },
                        mode.id,
                        mode.name,
                        mode.description
                    )
                )
            ) + &team_match_info(
                self.controller
                    .current_mode()
                    .is_some_and(|mode| mode.win_condition == funrounds_core::WinCondition::TeamDeathmatch),
                self.team_match.as_ref(),
                s2sdk::GetGameTime(),
            ) + &self
                .combat
                .get(&slot)
                .and_then(|(task, life)| (Some(task.player) == self.players.identity(slot)).then(|| life.shield()).flatten())
                .map_or_else(String::new, |shield| format!("\n实际护盾：{shield}"))),
            Command::C4 => self.request_suicide(slot, false),
            Command::Suicide => self.request_suicide(slot, true),
            Command::Help => Ok(crate::commands::HELP.into()),
            Command::Knock(enabled) => {
                if !self.classic_enabled() {
                    return Err("FunRounds公共行为当前不可用".into());
                }
                self.connect(slot);
                if self.players.connection_identity(slot).is_none() {
                    return Err("fr_knock requires an in-game player".into());
                }
                Ok(self.preferences.set(slot, crate::preferences::authenticated_id(slot), enabled))
            }
        }
    }
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

fn event_weapon_handle(slot: i32, event_weapon: &str) -> Option<i32> {
    unique_event_weapon(
        event_weapon,
        s2sdk::GetClientWeapons(slot)
            .iter()
            .copied()
            .filter(|handle| s2sdk::IsValidEntHandle(*handle))
            .map(|handle| (handle, s2sdk::GetEntityClassname(handle).to_string())),
    )
}

fn unique_event_weapon(event_weapon: &str, weapons: impl IntoIterator<Item = (i32, String)>) -> Option<i32> {
    if !upgrade_event_weapon(event_weapon) {
        return None;
    }
    let mut matching = weapons.into_iter().filter(|(_, classname)| event_weapon_matches(event_weapon, classname));
    let (handle, _) = matching.next()?;
    matching.next().is_none().then_some(handle)
}

fn upgrade_event_weapon(event_weapon: &str) -> bool {
    let name = event_weapon.strip_prefix("weapon_").unwrap_or(event_weapon);
    !name.is_empty()
        && !matches!(
            name,
            "knife" | "knife_t" | "c4" | "hegrenade" | "flashbang" | "smokegrenade" | "decoy" | "molotov" | "incgrenade" | "healthshot"
        )
}

fn event_weapon_matches(event_weapon: &str, classname: &str) -> bool {
    let event_weapon = event_weapon.strip_prefix("weapon_").unwrap_or(event_weapon);
    classname.strip_prefix("weapon_") == Some(event_weapon)
}

fn format_status(summary: &str, faults: &Faults, cleanup_pending: bool, details: &str) -> String {
    format!(
        "{summary} recovery={:?} cleanup_pending={cleanup_pending}\nfaults={}\n{details}",
        faults.recovery,
        faults.reasons(),
    )
}

fn read_config(path: &std::path::Path) -> Result<Config, String> {
    let source = std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Config::parse(&source).map_err(|error| format!("{}: {error}", path.display()))
}

pub(crate) fn remove_native_c4(player: LivePlayer, handles: &[i32]) -> Result<(), Fault> {
    if LivePlayer::resolve(player.slot) != Some(player) {
        return Err(Fault::cancelled("player identity changed before native C4 cleanup"));
    }
    for handle in handles.iter().copied() {
        if !s2sdk::IsValidEntHandle(handle) || s2sdk::GetEntityClassname(handle).as_str() != "weapon_c4" {
            continue;
        }
        s2sdk::RemoveWeapon(player.slot, handle);
        if LivePlayer::resolve(player.slot) != Some(player) {
            return Err(Fault::cancelled("player identity changed during native C4 cleanup"));
        }
        if s2sdk::GetClientWeapons(player.slot).iter().any(|current| *current == handle) {
            return Err(Fault::mechanism(Mechanism::NoC4, "engine retained the native C4 after removal"));
        }
        if s2sdk::IsValidEntHandle(handle) && matches!(s2sdk::GetEntityOwner(handle), owner if owner == -1 || owner == player.pawn_handle) {
            s2sdk::RemoveEntity(handle);
            if s2sdk::IsValidEntHandle(handle) {
                return Err(Fault::mechanism(Mechanism::C4Assignment, "native C4 entity survived cleanup"));
            }
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

fn combat_life_required(mode: &funrounds_core::Mode) -> bool {
    mode.combat_protection
        || mode.energy_shield
        || mode.weapon_damage.knife == 0.0
        || mode.reflect_damage.is_some()
        || mode.random_damage
        || matches!(mode.self_damage, Some(SelfDamage::Native { amount }) if amount > 0.0)
}

fn delayed_damage_protection_valid(protection_enabled: bool, target: PlayerTask, current: Option<(PlayerTask, bool)>) -> bool {
    !protection_enabled || current.is_some_and(|(task, protected)| task == target && !protected)
}

fn team_match_info(selected: bool, game: Option<&TeamDeathmatch>, now: f32) -> String {
    if !selected {
        return String::new();
    }
    game.map_or_else(
        || "\n比分：T 0 : 0 CT\n剩余时间：计时未开始（冻结结束后开始）".into(),
        |game| {
            format!(
                "\n比分：T {} : {} CT\n剩余时间：{:.0}s",
                game.scores()[0],
                game.scores()[1],
                game.remaining(now)
            )
        },
    )
}

fn observe_lifecycle_tick(last: &mut Option<i32>, tick: i32, event: Option<&str>) -> bool {
    // Damage callbacks can rewind ticks; round boundaries can precede the first new-map frame.
    if event.is_some_and(|name| !matches!(name, "round_prestart" | "round_start")) {
        return false;
    }
    last.replace(tick).is_some_and(|previous| tick < previous)
}

fn discard_map_effects<T>(effects: &mut HashMap<i32, T>, retired_effects: &mut Vec<T>) {
    effects.clear();
    retired_effects.clear();
}

fn restore_or_retain<T>(retained: &mut Vec<T>, index: usize, restore: impl FnOnce(&mut T) -> Result<bool, String>) -> Result<(), String> {
    if restore(&mut retained[index])? {
        retained.remove(index);
    }
    Ok(())
}

fn log(message: &str) {
    s2sdk::PrintToServer(&Str::from_str(&format!("[FunRounds] {message}")));
}

fn respawn_target_valid(expected: LivePlayer, current: Option<LivePlayer>, team: i32, current_team: i32, alive: bool) -> bool {
    current == Some(expected) && matches!(team, 2 | 3) && current_team == team && !alive
}

fn lifesteal_eligible(attacker: i32, victim: i32, attacker_alive: bool) -> bool {
    attacker >= 0 && victim >= 0 && attacker != victim && attacker_alive
}

fn respawn_window_open(phase: Phase, remaining: Option<f32>) -> bool {
    phase == Phase::Preparing || (phase == Phase::Running && remaining.is_none_or(|seconds| seconds > 0.0))
}

fn center_display_due(current: bool, phase: Phase, tick: i32, scheduled: i32) -> bool {
    current && phase != Phase::Idle && tick > scheduled
}

fn center_display_action_valid(generation_matches: bool, phase: Phase, effects_ready: bool) -> bool {
    generation_matches && phase != Phase::Idle && effects_ready
}

fn escape_html_text(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn separate_console_output(message: String) -> String {
    format!("\n{message}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn stopped_status_puts_recovery_and_fault_before_a_large_excluded_list() {
        let mut faults = crate::fault::Faults::new(funrounds_core::Capabilities::default());
        faults.record(&crate::fault::Fault::global("created action failed after cancellation"));
        let details = format!(
            "capabilities={:?}\nexcluded={}\ncapability_reasons=",
            faults.capabilities(),
            "sample:[equipment,health,armor,gravity,speed];".repeat(40)
        );
        assert!(details.len() > 2047);
        let status = super::format_status(
            "[FunRounds] enabled=false phase=Idle current=ordinary effects_ready=false",
            &faults,
            false,
            &details,
        );
        let parts = crate::commands::status_reply_parts(&status).collect::<Vec<_>>();
        assert!(parts[0].ends_with("recovery=Stopped cleanup_pending=false"));
        assert_eq!(parts[1], "faults=Global:[created action failed after cancellation]");
        assert!(parts[2].starts_with("capabilities="));
        assert_eq!(parts[3..parts.len() - 1].concat().matches("sample:[").count(), 40);
        assert_eq!(parts.last(), Some(&"capability_reasons="));
    }

    #[test]
    fn center_mode_name_waits_for_the_next_tick_and_cancels_when_stale() {
        use funrounds_core::Phase;

        assert!(!super::center_display_due(true, Phase::Preparing, 10, 10));
        assert!(super::center_display_due(true, Phase::Preparing, 11, 10));
        assert!(!super::center_display_due(false, Phase::Preparing, 11, 10));
        assert!(!super::center_display_due(true, Phase::Idle, 11, 10));
    }

    #[test]
    fn center_mode_name_is_rendered_as_text_not_html() {
        assert_eq!(super::escape_html_text("示例 & <example>"), "示例 &amp; &lt;example&gt;");
    }

    #[test]
    fn center_action_revalidates_generation_phase_and_effect_readiness() {
        use funrounds_core::Phase;

        assert!(super::center_display_action_valid(true, Phase::Preparing, true));
        assert!(!super::center_display_action_valid(false, Phase::Preparing, true));
        assert!(!super::center_display_action_valid(true, Phase::Idle, true));
        assert!(!super::center_display_action_valid(true, Phase::Preparing, false));
    }

    #[test]
    fn console_output_starts_on_a_separate_line() {
        assert_eq!(super::separate_console_output("[FunRounds] mode".into()), "\n[FunRounds] mode");
    }

    #[test]
    fn miss_penalty_respects_native_self_damage_and_final_health_point() {
        let mut mode = funrounds_core::Config::parse(include_str!("../../../config/funrounds.example.toml"))
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
    fn team_info_includes_preparation_without_starting_the_clock() {
        assert_eq!(super::team_match_info(false, None, 0.0), "");
        assert_eq!(
            super::team_match_info(true, None, 0.0),
            "\n比分：T 0 : 0 CT\n剩余时间：计时未开始（冻结结束后开始）"
        );
        let game = funrounds_core::TeamDeathmatch::start(10.0, 60.0).unwrap();
        assert_eq!(super::team_match_info(true, Some(&game), 12.0), "\n比分：T 0 : 0 CT\n剩余时间：58s");
    }
    use super::discard_map_effects;
    use std::collections::HashMap;

    #[test]
    fn event_tick_rewinds_do_not_discard_map_state_but_frame_rewinds_do() {
        let mut last = None;
        assert!(!super::observe_lifecycle_tick(&mut last, 1294, None));
        assert!(!super::observe_lifecycle_tick(&mut last, 1293, Some("player_hurt")));
        assert!(!super::observe_lifecycle_tick(&mut last, 1295, Some("weapon_fire")));
        assert_eq!(last, Some(1294));
        assert!(!super::observe_lifecycle_tick(&mut last, 1294, None));
        assert!(!super::observe_lifecycle_tick(&mut last, 1295, None));
        assert!(super::observe_lifecycle_tick(&mut last, 0, None));
        assert_eq!(last, Some(0));
    }

    #[test]
    fn new_map_round_boundaries_before_frame_discard_old_state_and_reach_running() {
        use funrounds_core::{Capabilities, Config, Phase, RoundController};
        use rand::{SeedableRng, rngs::StdRng};

        let config = Config::parse(include_str!("../../../config/funrounds.example.toml")).unwrap();
        let capabilities = Capabilities {
            equipment: true,
            health: true,
            armor: true,
            gravity: true,
            speed: true,
            helmet: true,
            clear_weapons: true,
            give_weapons: true,
            ..Capabilities::default()
        };
        for first_event in ["round_prestart", "round_start"] {
            let mut controller = RoundController::new(Some(config.clone()), capabilities).unwrap();
            let mut rng = StdRng::seed_from_u64(1);
            let mut last = Some(10000);
            let old_key = controller.round_key(10000);
            assert!(controller.start_round(old_key, false, &mut rng));
            assert!(controller.freeze_end(old_key));

            assert!(super::observe_lifecycle_tick(&mut last, 1, Some(first_event)));
            controller.reset();
            let new_key = controller.round_key(1);
            assert_ne!(new_key, old_key);
            assert!(!super::observe_lifecycle_tick(&mut last, 1, Some("round_start")));
            assert!(controller.start_round(new_key, false, &mut rng));
            assert_eq!(controller.phase(), Phase::Preparing);
            assert!(!super::observe_lifecycle_tick(&mut last, 1, None));
            assert!(controller.freeze_end(new_key));
            assert_eq!(controller.phase(), Phase::Running);
        }
    }

    #[test]
    fn lifesteal_excludes_self_world_and_dead_attackers() {
        assert!(super::lifesteal_eligible(1, 2, true));
        assert!(!super::lifesteal_eligible(1, 1, true));
        assert!(!super::lifesteal_eligible(-1, 2, true));
        assert!(!super::lifesteal_eligible(1, -1, true));
        assert!(!super::lifesteal_eligible(1, 2, false));
    }

    #[test]
    fn team_respawn_works_during_freeze_without_starting_the_score_timer() {
        use funrounds_core::Phase;
        assert!(super::respawn_window_open(Phase::Preparing, None));
        assert!(super::respawn_window_open(Phase::Running, Some(3.0)));
        assert!(!super::respawn_window_open(Phase::Running, Some(0.0)));
        assert!(super::respawn_window_open(Phase::Running, None));
        assert!(!super::respawn_window_open(Phase::Idle, Some(30.0)));
    }

    #[test]
    fn respawn_rejects_changed_handles_team_and_external_resurrection() {
        let player = crate::game::LivePlayer {
            slot: 1,
            controller_handle: 0x4001,
            pawn_handle: 0x4002,
        };
        assert!(super::respawn_target_valid(player, Some(player), 2, 2, false));
        assert!(!super::respawn_target_valid(player, Some(player), 2, 3, false));
        assert!(!super::respawn_target_valid(player, Some(player), 2, 2, true));
        assert!(!super::respawn_target_valid(player, None, 2, 2, false));
        assert!(!super::respawn_target_valid(
            player,
            Some(crate::game::LivePlayer {
                pawn_handle: 0x8002,
                ..player
            }),
            2,
            2,
            false
        ));
        assert!(!super::respawn_target_valid(
            player,
            Some(crate::game::LivePlayer {
                controller_handle: 0x8001,
                ..player
            }),
            2,
            2,
            false
        ));
    }

    #[test]
    fn map_change_discards_active_and_retired_entity_snapshots() {
        let mut effects = HashMap::from([(1, "active")]);
        let mut retired_effects = vec!["retired"];
        discard_map_effects(&mut effects, &mut retired_effects);
        assert!(effects.is_empty());
        assert!(retired_effects.is_empty());
    }

    #[test]
    fn failed_retired_restoration_keeps_its_snapshot_for_retry() {
        let mut retained = vec!["sample color"];
        let result = super::restore_or_retain(&mut retained, 0, |_| Err("color readback failed".into()));
        assert_eq!(result, Err("color readback failed".into()));
        assert_eq!(retained, ["sample color"]);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                super::restore_or_retain(&mut retained, 0, |_| panic!("restoration panic"))
            }))
            .is_err()
        );
        assert_eq!(retained, ["sample color"]);
        super::restore_or_retain(&mut retained, 0, |_| Ok(false)).unwrap();
        assert_eq!(retained, ["sample color"]);
        super::restore_or_retain(&mut retained, 0, |_| Ok(true)).unwrap();
        assert!(retained.is_empty());
    }
}
