// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[derive(Clone)]
pub(crate) struct Shot {
    task: PlayerTask,
    player: LivePlayer,
    weapon: FiredWeapon,
    weapon_name: String,
    tick: i32,
    serial: u64,
}

#[derive(Clone)]
pub(crate) enum Action {
    Blast {
        shot: Shot,
        origin: plugify::Vec3,
        config: funrounds_core::ChickenBlastConfig,
    },
    Heal {
        shot: Shot,
        target: PlayerTask,
        player: LivePlayer,
        amount: i32,
        base: i32,
    },
}

impl Action {
    pub(super) fn involves(&self, slots: &[i32]) -> bool {
        match self {
            Self::Blast { shot, .. } => slots.contains(&shot.player.slot),
            Self::Heal { shot, player, .. } => slots.contains(&shot.player.slot) || slots.contains(&player.slot),
        }
    }
}

struct Heal {
    shot: Shot,
    target: PlayerTask,
    player: LivePlayer,
    damage: f64,
    settings: funrounds_core::FriendlyFireHealSettings,
}

#[derive(Default)]
pub(super) struct State {
    serial: u64,
    shots: HashMap<i32, Shot>,
    cooldowns: HashMap<i32, (PlayerTask, f64)>,
    heals: Vec<Heal>,
}

pub(super) fn probe(events: &EventCapabilities, caps: &mut funrounds_core::Capabilities, reasons: &mut Vec<String>) {
    let mut source = crate::capability::Probe::default();
    source.require(events.weapon_fire && events.combat_events, "events:weapon_fire/lifecycle");
    crate::capability::imports!(source; __s2sdk_GetGameTickCount, __s2sdk_GetGameTime, __s2sdk_GetClientActiveWeapon,
        __s2sdk_GetEntityOwner, __s2sdk_GetEntityClassname, __s2sdk_IsValidEntHandle);
    source.require(
        unsafe { crate::effects::__s2sdk_GetWeaponItemDefinition }.is_some(),
        "GetWeaponItemDefinition",
    );
    let mut heal = source.clone();
    heal.require(events.damage, "native:player_damage_pre");
    crate::capability::imports!(heal; __s2sdk_GetClientTeam, __s2sdk_FindConVar, __s2sdk_GetConVarValue,
        __s2sdk_GetClientHealth, __s2sdk_SetClientHealth);
    heal.require(teammates_are_friendly().is_ok(), "convar:mp_teammates_are_enemies (readable boolean)");
    caps.friendly_fire_heal = heal.record("friendly_fire_heal", reasons);
    let mut blast = source;
    blast.require(events.bullet_impact, "event:bullet_impact");
    blast.require(
        crate::KNIFE_DEATH_PRE_HOOKED.load(std::sync::atomic::Ordering::Acquire),
        "event:player_death_pre",
    );
    blast.require(events.damage && crate::damage::damage_frame_token_available(), "native:damage/frame");
    blast.require(crate::chicken_entities::available(), "env_explosion/feedback");
    crate::capability::imports!(blast; __s2sdk_GetEventFloat, __s2sdk_SetEventString, __s2sdk_AcceptEntityInput);
    caps.explosive_bullets = blast.record("explosive_bullets", reasons);
    caps.damage_multiplier = events.damage;
}

impl Runtime {
    fn shooting_active(&self) -> bool {
        self.classic_enabled() && self.classic_round() && active_combat_phase(self.controller.phase())
    }

    fn shot_current(&self, shot: &Shot) -> bool {
        self.shooting_active() && self.combat_task(shot.player) == Some(shot.task) && LivePlayer::resolve(shot.player.slot) == Some(shot.player)
    }

    pub(super) fn capture_shot(&mut self, slot: i32, event_weapon: &str) {
        self.shooting.shots.remove(&slot);
        let Some(player) = LivePlayer::resolve(slot) else { return };
        if !self.shooting_active()
            || self
                .controller
                .current_mode()
                .is_none_or(|m| m.explosive_bullets.is_none() && m.friendly_fire_heal.is_none())
        {
            return;
        }
        let Some(task) = self.combat_task(player) else { return };
        let Some(weapon) = fired_weapon(player, event_weapon) else { return };
        let weapon_name =
            crate::effects::equipment::give_name_for_original(&weapon.classname, weapon.item_definition).expect("verified firearm definition");
        self.shooting.serial = self.shooting.serial.wrapping_add(1);
        self.shooting.shots.insert(
            player.slot,
            Shot {
                task,
                player,
                weapon,
                weapon_name,
                tick: s2sdk::GetGameTickCount(),
                serial: self.shooting.serial,
            },
        );
    }

    pub(super) fn bullet_impact(&mut self, event: usize) {
        let slot = s2sdk::GetEventPlayerSlot(event, &Str::from_str("userid"));
        let Some(shot) = self.shooting.shots.get(&slot).cloned() else { return };
        // Impacts are synchronous with the matching weapon_fire batch. Never
        // carry a recent shot into a later tick or resolve a replacement weapon.
        if shot.tick != s2sdk::GetGameTickCount() || !self.shot_current(&shot) {
            return;
        }
        let Some(settings) = self.controller.current_mode().and_then(|m| m.explosive_bullets) else {
            return;
        };
        let now = f64::from(s2sdk::GetGameTime());
        if self
            .shooting
            .cooldowns
            .get(&slot)
            .is_some_and(|(task, ready)| *task == shot.task && now < *ready)
        {
            return;
        }
        let origin = plugify::Vec3 {
            x: s2sdk::GetEventFloat(event, &Str::from_str("x")),
            y: s2sdk::GetEventFloat(event, &Str::from_str("y")),
            z: s2sdk::GetEventFloat(event, &Str::from_str("z")),
        };
        if ![origin.x, origin.y, origin.z].iter().all(|v| v.is_finite()) {
            return;
        }
        let config = funrounds_core::ChickenBlastConfig {
            damage: f32::from(settings.damage.sample(&mut self.rng)),
            radius: f32::from(settings.radius.sample(&mut self.rng)),
        };
        self.engine_actions.push(EngineAction::Shooting(Action::Blast {
            shot: shot.clone(),
            origin,
            config,
        }));
        self.shooting
            .cooldowns
            .insert(slot, (shot.task, now + f64::from(settings.cooldown_seconds)));
    }

    pub(crate) fn convert_friendly_shot(&mut self, damage: &crate::damage::Damage, players: &game::DamagePlayers) -> bool {
        if !self.shooting_active()
            || damage.skip_plugin_rules
            || damage.weapon_kind != 1
            || damage.damage_type & (crate::damage::DMG_BULLET | 0x1000000) == 0
            || damage.damage_type & (crate::damage::DMG_BLAST | crate::damage::DMG_BURN | crate::damage::DMG_SLASH) != 0
            || !damage.amount.is_finite()
            || damage.amount <= 0.0
        {
            return false;
        }
        let Some(settings) = self.controller.current_mode().and_then(|m| m.friendly_fire_heal) else {
            return false;
        };
        let (Some(attacker), Some(victim)) = (players.attacker, players.victim) else {
            return false;
        };
        match friendly(attacker, victim) {
            Ok(true) => {}
            Ok(false) => return false,
            Err(error) => {
                self.handle_fault(Fault::mechanism(Mechanism::FriendlyFireHeal, error));
                return false;
            }
        }
        let Some(shot) = self.shooting.shots.get(&attacker.slot).cloned() else {
            return false;
        };
        if !self.shot_current(&shot)
            || shot.tick != s2sdk::GetGameTickCount()
            || damage.weapon != shot.weapon.handle
            || damage.inflictor != attacker.pawn_handle
        {
            return false;
        }
        let Some(target) = self.combat_task(victim) else { return false };
        if let Some(heal) = self
            .shooting
            .heals
            .iter_mut()
            .find(|h| h.shot.serial == shot.serial && h.shot.task == shot.task && h.target == target)
        {
            heal.damage += f64::from(damage.amount);
        } else {
            self.shooting.heals.push(Heal {
                shot,
                target,
                player: victim,
                damage: f64::from(damage.amount),
                settings,
            });
        }
        true
    }

    pub(super) fn settle_shooting(&mut self, tick: i32) {
        for heal in std::mem::take(&mut self.shooting.heals) {
            if tick == heal.shot.tick {
                self.shooting.heals.push(heal);
                continue;
            }
            if tick <= heal.shot.tick
                || !self.shot_current(&heal.shot)
                || self.combat_task(heal.player) != Some(heal.target)
                || LivePlayer::resolve(heal.player.slot) != Some(heal.player)
            {
                continue;
            }
            let is_friendly = match friendly(heal.shot.player, heal.player) {
                Ok(is_friendly) => is_friendly,
                Err(error) => {
                    self.handle_fault(Fault::mechanism(Mechanism::FriendlyFireHeal, error));
                    return;
                }
            };
            if is_friendly
                && let Some(effect) = self
                    .effects
                    .get(&heal.player.slot)
                    .filter(|e| e.identity == heal.target.player && !e.has_pending_loadout())
            {
                self.engine_actions.push(EngineAction::Shooting(Action::Heal {
                    amount: heal.settings.healing(heal.damage),
                    base: i32::from(effect.attributes.health),
                    shot: heal.shot,
                    target: heal.target,
                    player: heal.player,
                }));
            }
        }
        self.shooting.shots.retain(|_, shot| shot.tick == tick);
    }

    pub(super) fn shooting_action_valid(&self, action: &Action) -> bool {
        match action {
            Action::Blast { shot, .. } => self.shot_current(shot) && self.controller.current_mode().is_some_and(|m| m.explosive_bullets.is_some()),
            Action::Heal { shot, target, player, .. } => {
                self.shot_current(shot)
                    && self.combat_task(*player) == Some(*target)
                    && LivePlayer::resolve(player.slot) == Some(*player)
                    && friendly(shot.player, *player).is_ok_and(|friendly| friendly)
                    && self.controller.current_mode().is_some_and(|m| m.friendly_fire_heal.is_some())
            }
        }
    }
}

pub(super) fn teammates_are_friendly() -> Result<bool, &'static str> {
    let find = unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_FindConVar).read() }.ok_or("FindConVar import unavailable")?;
    let handle = unsafe { find(&Str::from_str("mp_teammates_are_enemies")) };
    if !crate::parameters::valid_handle(handle) {
        return Err("mp_teammates_are_enemies is unavailable");
    }
    let get = unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_GetConVarValue).read() }.ok_or("GetConVarValue import unavailable")?;
    let value = unsafe { get(handle) }.to_string();
    match crate::parameters::parsed_value(&value) {
        Some(0.0) => Ok(true),
        Some(1.0) => Ok(false),
        _ => Err("mp_teammates_are_enemies has an invalid boolean value"),
    }
}

fn friendly(attacker: LivePlayer, victim: LivePlayer) -> Result<bool, &'static str> {
    let team = s2sdk::GetClientTeam(attacker.slot) as i32;
    if attacker == victim || !matches!(team, 2 | 3) || s2sdk::GetClientTeam(victim.slot) as i32 != team {
        return Ok(false);
    }
    teammates_are_friendly()
}

pub(crate) fn execute(action: Action) -> Result<(), Fault> {
    match action {
        Action::Heal { player, amount, base, .. } => super::composed::heal(
            player.slot,
            base,
            funrounds_core::HealingAmount::Fixed(amount),
            1.0,
            Mechanism::FriendlyFireHeal,
        )
        .map(|_| ()),
        Action::Blast { shot, origin, config } => {
            let (effect, _) = crate::chicken_entities::prepare_explosion(shot.player.pawn_handle, Some(shot.player.pawn_handle), origin, config)
                .map_err(|e| Fault::mechanism(Mechanism::ExplosiveBullets, e))?;
            // Each explosion uses its own inflictor; it never enters chicken exceptions.
            let pointer = s2sdk::EntHandleToEntPointer(effect);
            s2sdk::SetEntSchemaEnt2(pointer, &Str::from_str("CEnvExplosion"), &Str::from_str("m_hInflictor"), effect, false, 0);
            if s2sdk::GetEntSchemaEnt2(pointer, &Str::from_str("CEnvExplosion"), &Str::from_str("m_hInflictor"), 0) != effect {
                crate::chicken_entities::remove(effect);
                return Err(Fault::mechanism(Mechanism::ExplosiveBullets, "explosion inflictor readback failed"));
            }
            let _entity = BlastEntity(effect);
            let _scope = BlastScope::enter(effect, shot.clone());
            let action = EngineAction::Shooting(Action::Blast {
                shot: shot.clone(),
                origin,
                config,
            });
            if !crate::engine_action_current(&action) {
                return Ok(());
            }
            let feedback = crate::suicide::spawn_feedback_at(shot.player.pawn_handle, origin, crate::chicken_entities::EXPLOSION_PARTICLE)
                .map_err(|e| Fault::mechanism(Mechanism::ExplosiveBullets, e))?;
            let tracked = crate::RUNTIME
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_mut()
                .is_some_and(|runtime| runtime.track_suicide_feedback(feedback, s2sdk::GetGameTime()));
            if !tracked {
                crate::suicide::remove_feedback(feedback);
                return Ok(());
            }
            crate::chicken_entities::emit_blast_sound(effect, origin, config.radius);
            crate::chicken_entities::detonate(effect, shot.player.pawn_handle);
            Ok(())
        }
    }
}

struct BlastContext {
    effect: i32,
    shot: Shot,
    receipt: Option<(i32, u64)>,
}
thread_local! { static BLAST: std::cell::RefCell<Option<BlastContext>> = const { std::cell::RefCell::new(None) }; }
struct BlastScope(Option<BlastContext>);
impl BlastScope {
    fn enter(effect: i32, shot: Shot) -> Self {
        Self(BLAST.with(|b| b.replace(Some(BlastContext { effect, shot, receipt: None }))))
    }
}
impl Drop for BlastScope {
    fn drop(&mut self) {
        BLAST.with(|b| b.replace(self.0.take()));
    }
}

pub(crate) fn attribute_damage(damage: &mut crate::damage::Damage) {
    BLAST.with(|b| {
        let mut b = b.borrow_mut();
        let Some(source) = b
            .as_mut()
            .filter(|b| damage.inflictor == b.effect && damage.attacker == b.shot.player.pawn_handle)
        else {
            return;
        };
        damage.weapon_class = Some(source.shot.weapon_name.clone());
        let weapon = &source.shot.weapon;
        damage.weapon = if s2sdk::IsValidEntHandle(weapon.handle)
            && s2sdk::GetEntityClassname(weapon.handle).as_str() == weapon.classname
            && unsafe { crate::effects::__s2sdk_GetWeaponItemDefinition }.is_some_and(|read| unsafe { read(weapon.handle) } == weapon.item_definition)
        {
            weapon.handle
        } else {
            -1
        };
        source.receipt = crate::damage::current_damage_frame_token()
            .filter(|t| *t != 0)
            .map(|t| (damage.victim, t));
    });
}

pub(crate) fn death_weapon(victim: i32, attacker: i32) -> Option<String> {
    BLAST.with(|b| {
        let b = b.borrow();
        let source = b.as_ref()?;
        let token = crate::damage::current_damage_frame_token()?;
        (attacker == source.shot.player.slot && source.receipt == Some((s2sdk::EntPointerToEntHandle(s2sdk::GetClientPawn(victim)), token))).then(
            || {
                source
                    .shot
                    .weapon_name
                    .strip_prefix("weapon_")
                    .unwrap_or(&source.shot.weapon_name)
                    .to_owned()
            },
        )
    })
}

struct BlastEntity(i32);
impl Drop for BlastEntity {
    fn drop(&mut self) {
        crate::chicken_entities::remove(self.0);
    }
}
