// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use rand::prelude::IndexedRandom;

#[derive(Clone)]
pub enum Action {
    Penalty {
        task: PlayerTask,
        player: LivePlayer,
        amount: f32,
        burn: bool,
    },
    Weapon {
        task: PlayerTask,
        player: LivePlayer,
        fired: FiredWeapon,
        switch_to: Option<i32>,
    },
}

impl Action {
    pub fn task(&self) -> PlayerTask {
        match self {
            Self::Penalty { task, .. } | Self::Weapon { task, .. } => *task,
        }
    }

    fn player(&self) -> LivePlayer {
        match self {
            Self::Penalty { player, .. } | Self::Weapon { player, .. } => *player,
        }
    }
}

#[derive(Clone, Copy)]
struct Life {
    task: PlayerTask,
    last_combat: f64,
    regeneration_at: f64,
    breath_pending: bool,
    aura_at: f64,
    stopped_since: f64,
    stopped_tick: f64,
    grounded_since: f64,
    grounded_tick: f64,
    weapon_ready: f64,
}

impl Life {
    fn new(task: PlayerTask, now: f64) -> Self {
        Self {
            task,
            last_combat: now,
            regeneration_at: now,
            breath_pending: true,
            aura_at: now,
            stopped_since: now,
            stopped_tick: now,
            grounded_since: now,
            grounded_tick: now,
            weapon_ready: now,
        }
    }
}

#[derive(Clone, Copy)]
struct Flash {
    task: PlayerTask,
    player: LivePlayer,
    tick: i32,
    pending_until: f64,
    native_until: f64,
    native_duration: f64,
    installed: Option<(f64, f64)>,
    partial_write: bool,
}

#[derive(Default)]
pub(super) struct State {
    lives: HashMap<i32, Life>,
    weapons: Vec<(i32, Action)>,
    flashes: HashMap<i32, Flash>,
    heads: HashMap<i32, f32>,
    head_round: Option<RoundGeneration>,
    head_scales: HashMap<i32, f32>,
}

impl State {
    pub fn is_clean(&self) -> bool {
        self.heads.is_empty() && self.flashes.is_empty() && self.weapons.is_empty()
    }
}

pub(super) fn probe(events: &EventCapabilities, caps: &mut funrounds_core::Capabilities, reasons: &mut Vec<String>) {
    let mut health = crate::capability::Probe::default();
    crate::capability::imports!(health; __s2sdk_GetGameTime, __s2sdk_GetClientHealth, __s2sdk_SetClientHealth);
    health.require(events.combat_events, "events:player_lifecycle");
    let mut regeneration = health.clone();
    regeneration.require(
        events.player_hurt && events.weapon_fire && events.grenade_thrown,
        "events:player_hurt/weapon_fire/grenade_thrown",
    );
    crate::capability::imports!(regeneration; __s2sdk_EmitSoundToClient);
    caps.regeneration = regeneration.record("regeneration", reasons);
    let mut aura = health;
    crate::capability::imports!(aura; __s2sdk_GetEntityAbsOrigin, __s2sdk_GetClientAbsOrigin);
    aura.require(crate::chicken_entities::available(), "ordinary_chicken_identity");
    caps.chicken_aura = aura.record("chicken_aura", reasons);
    let mut movement = crate::capability::Probe::default();
    crate::capability::imports!(movement; __s2sdk_GetClientAbsVelocity, __s2sdk_GetClientFlags, __s2sdk_GetGameTime);
    movement.require(events.damage && events.combat_events, "hooks:damage/lifecycle");
    movement.require(crate::damage::reflection_available(), "native:TakeEntityDamageWithWeapon");
    caps.dont_stop = movement.clone().record("dont_stop", reasons);
    movement.require(caps.afterburn, "managed_afterburn");
    caps.hot_floor = movement.record("hot_floor", reasons);
    let mut weapon = crate::capability::Probe::default();
    crate::capability::imports!(weapon; __s2sdk_DropWeapon, __s2sdk_SwitchWeapon, __s2sdk_GetClientWeapons,
        __s2sdk_GetClientActiveWeapon, __s2sdk_GetEntityOwner, __s2sdk_GetEntityClassname);
    weapon.require(events.weapon_fire && events.combat_events, "events:weapon_fire/lifecycle");
    caps.weapon_on_fire = weapon.record("weapon_on_fire", reasons);
    let mut flash = crate::capability::Probe::default();
    crate::capability::imports!(flash; __s2sdk_GetEntSchemaFloat2, __s2sdk_SetEntSchemaFloat2, __s2sdk_GetGameTime, __s2sdk_GetEventFloat);
    flash.require(events.player_blind && events.combat_events, "events:player_blind/lifecycle");
    flash.schema("CCSPlayerPawn", "m_blindUntilTime");
    flash.schema("CCSPlayerPawn", "m_flFlashDuration");
    caps.long_flash = flash.record("long_flash", reasons);
    let mut head = crate::capability::Probe::default();
    crate::capability::imports!(head; __s2sdk_GetClientKills, __s2sdk_GetClientDeaths);
    head.require(
        unsafe { crate::funrounds_native::__funrounds_native_HasBigHeadScaleSupport }.is_some_and(|query| unsafe { query() }),
        "native:BigHeadScaleSupport",
    );
    head.require(
        unsafe { crate::funrounds_native::__funrounds_native_SetBigHeadScaleTarget }.is_some()
            && unsafe { crate::funrounds_native::__funrounds_native_ClearBigHeadScaleTarget }.is_some(),
        "native:BigHeadScaleTarget",
    );
    caps.big_head = head.record("big_head", reasons);
}

fn read_flash(player: LivePlayer, field: &str) -> f64 {
    s2sdk::GetEntSchemaFloat2(
        s2sdk::EntHandleToEntPointer(player.pawn_handle),
        &Str::from_str("CCSPlayerPawn"),
        &Str::from_str(field),
        0,
    )
}

fn write_flash(player: LivePlayer, field: &str, value: f64) -> bool {
    s2sdk::SetEntSchemaFloat2(
        s2sdk::EntHandleToEntPointer(player.pawn_handle),
        &Str::from_str("CCSPlayerPawn"),
        &Str::from_str(field),
        value,
        true,
        0,
    );
    read_flash(player, field) == f64::from(value as f32)
}

fn restore_flash(flash: &mut Flash) -> bool {
    if LivePlayer::resolve_present(flash.player.slot) != Some(flash.player) {
        return true;
    }
    let Some((until, duration)) = flash.installed else {
        return true;
    };
    if !flash.partial_write && (read_flash(flash.player, "m_blindUntilTime") != until || read_flash(flash.player, "m_flFlashDuration") != duration) {
        return true;
    }
    flash.partial_write = true;
    let until_ok = read_flash(flash.player, "m_blindUntilTime") != until || write_flash(flash.player, "m_blindUntilTime", flash.native_until);
    let duration_ok =
        read_flash(flash.player, "m_flFlashDuration") != duration || write_flash(flash.player, "m_flFlashDuration", flash.native_duration);
    until_ok && duration_ok
}

impl Runtime {
    fn composed_active(&self) -> bool {
        self.classic_enabled() && self.effects_ready && active_combat_phase(self.controller.phase())
    }

    pub(super) fn composed_action_valid(&self, action: &Action) -> bool {
        let player = action.player();
        self.composed_active()
            && self.combat_task(player) == Some(action.task())
            && LivePlayer::resolve(player.slot) == Some(player)
            && match action {
                Action::Penalty { .. } => !self.combat_protected(&game::DamagePlayers {
                    victim: Some(player),
                    attacker: None,
                }),
                Action::Weapon { fired, switch_to, .. } => {
                    weapon_identity_is_current(player, fired) && switch_to.is_none_or(|handle| inventory_weapon(player, handle))
                }
            }
    }

    pub(super) fn record_regeneration_combat(&mut self, slot: i32) {
        if !self.classic_enabled() || self.controller.current_mode().is_none_or(|mode| mode.regeneration.is_none()) {
            return;
        }
        let Some(player) = LivePlayer::resolve(slot) else { return };
        let Some(task) = self.combat_task(player) else { return };
        self.record_regeneration_combat_for(task, f64::from(s2sdk::GetGameTime()));
    }

    pub(super) fn record_regeneration_combat_for(&mut self, task: PlayerTask, now: f64) {
        if self.controller.current_mode().is_none_or(|mode| mode.regeneration.is_none()) {
            return;
        }
        let life = self.composed.lives.entry(task.player.slot).or_insert_with(|| Life::new(task, now));
        if life.task != task {
            *life = Life::new(task, now);
        }
        life.last_combat = now;
        life.regeneration_at = now;
        life.breath_pending = true;
    }

    pub(super) fn queue_weapon_on_fire(&mut self, player: LivePlayer, event_weapon: &str) {
        if !self.composed_active() {
            return;
        }
        let Some(settings) = self.controller.current_mode().and_then(|mode| mode.weapon_on_fire) else {
            return;
        };
        let Some(task) = self.combat_task(player) else { return };
        let Some(fired) = fired_weapon(player, event_weapon) else { return };
        let now = f64::from(s2sdk::GetGameTime());
        let life = self.composed.lives.entry(player.slot).or_insert_with(|| Life::new(task, now));
        if life.task != task {
            *life = Life::new(task, now);
        }
        if now < life.weapon_ready || self.rng.random::<f32>() >= settings.chance {
            return;
        }
        let switch_to = if self
            .rng
            .random_range(0..u64::from(settings.switch_weight) + u64::from(settings.drop_weight))
            < u64::from(settings.switch_weight)
        {
            let targets: Vec<_> = s2sdk::GetClientWeapons(player.slot)
                .iter()
                .copied()
                .filter(|handle| *handle != fired.handle && inventory_weapon(player, *handle))
                .collect();
            let Some(target) = targets.choose(&mut self.rng) else { return };
            Some(*target)
        } else {
            None
        };
        life.weapon_ready = now + f64::from(settings.cooldown_seconds);
        self.composed.weapons.push((
            s2sdk::GetGameTickCount(),
            Action::Weapon {
                task,
                player,
                fired,
                switch_to,
            },
        ));
    }

    pub(super) fn queue_long_flash(&mut self, slot: i32, event: usize) {
        if !self.composed_active() || self.controller.current_mode().is_none_or(|mode| mode.long_flash.is_none()) {
            return;
        }
        let Some(player) = LivePlayer::resolve(slot) else { return };
        let Some(task) = self.combat_task(player) else { return };
        let duration = f64::from(s2sdk::GetEventFloat(event, &Str::from_str("blind_duration")));
        let now = f64::from(s2sdk::GetGameTime());
        let current_until = read_flash(player, "m_blindUntilTime");
        let current_duration = read_flash(player, "m_flFlashDuration");
        let settings = self.controller.current_mode().and_then(|mode| mode.long_flash).expect("checked setting");
        let Some(pending_until) = funrounds_core::extended_flash_deadline(
            now,
            now + duration,
            current_until,
            settings.duration_multiplier,
            settings.duration_cap_seconds,
        ) else {
            return;
        };
        let previous = self.composed.flashes.get(&slot).copied().filter(|flash| flash.task == task);
        let pending_until = previous.map_or(pending_until, |previous| previous.pending_until.max(pending_until));
        let mut native_until = current_until;
        let mut native_duration = current_duration;
        let mut installed = None;
        if let Some(previous) = previous
            && let Some((until, owned_duration)) = previous.installed
        {
            if current_until == until {
                native_until = previous.native_until.max(now + duration);
            }
            if current_duration == owned_duration {
                native_duration = previous.native_duration.max(duration);
            }
            installed = previous.installed;
        }
        self.composed.flashes.insert(
            slot,
            Flash {
                task,
                player,
                native_until,
                native_duration,
                pending_until,
                tick: s2sdk::GetGameTickCount(),
                installed,
                partial_write: false,
            },
        );
    }

    pub(super) fn update_composed(&mut self, now: f64, tick: i32) {
        self.update_big_heads();
        if !self.composed_active() {
            // Hurt events can create a life during freeze. Keep their healing delay,
            // but never carry frozen time into a movement penalty at freeze_end.
            for life in self.composed.lives.values_mut() {
                life.stopped_since = now;
                life.stopped_tick = now;
                life.grounded_since = now;
                life.grounded_tick = now;
            }
            return;
        }
        let Some(mode) = self.controller.current_mode() else { return };
        let pending = std::mem::take(&mut self.composed.weapons);
        for (queued, action) in pending {
            if tick > queued {
                self.engine_actions.push(EngineAction::Composed(action));
            } else {
                self.composed.weapons.push((queued, action));
            }
        }
        let mut flashes = std::mem::take(&mut self.composed.flashes).into_iter();
        while let Some((slot, mut flash)) = flashes.next() {
            if self.combat_task(flash.player) != Some(flash.task) || LivePlayer::resolve(slot) != Some(flash.player) {
                if !restore_flash(&mut flash) {
                    self.composed.flashes.insert(slot, flash);
                }
                continue;
            }
            if flash.pending_until > 0.0 && tick > flash.tick {
                let current = read_flash(flash.player, "m_blindUntilTime");
                let target = flash.pending_until;
                flash.pending_until = 0.0;
                if target > current {
                    let duration = read_flash(flash.player, "m_flFlashDuration") + target - current;
                    let until = f64::from(target as f32);
                    let duration = f64::from(duration as f32);
                    let previous_installed = flash.installed;
                    flash.installed = Some((until, duration));
                    flash.partial_write = true;
                    if !write_flash(flash.player, "m_blindUntilTime", until) || !write_flash(flash.player, "m_flFlashDuration", duration) {
                        // A failed extension can leave either field at our
                        // previous installed value; keep that ownership too.
                        if let Some((previous_until, previous_duration)) = previous_installed {
                            flash.installed = Some((
                                if read_flash(flash.player, "m_blindUntilTime") == previous_until {
                                    previous_until
                                } else {
                                    until
                                },
                                if read_flash(flash.player, "m_flFlashDuration") == previous_duration {
                                    previous_duration
                                } else {
                                    duration
                                },
                            ));
                        }
                        self.composed.flashes.insert(slot, flash);
                        self.composed.flashes.extend(flashes);
                        self.handle_fault(Fault::mechanism(Mechanism::LongFlash, "long_flash schema write/readback failed"));
                        return;
                    }
                    flash.partial_write = false;
                }
            }
            if flash.pending_until > 0.0 || flash.installed.is_some_and(|(until, _)| until > now) {
                self.composed.flashes.insert(slot, flash);
            }
        }
        let chicken_positions: Vec<_> = if mode.chicken_aura.is_some() {
            self.ordinary_chickens
                .entities
                .active_handles(self.controller.generation())
                .into_iter()
                .filter(|handle| crate::chicken_entities::health(*handle).is_some_and(|(health, state)| health > 0 && state == 0))
                .filter_map(crate::chicken_entities::position)
                .collect()
        } else {
            Vec::new()
        };
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            let Some(task) = self.combat_task(player) else { continue };
            let Some((expected, combat)) = self.combat.lives.get(&slot).filter(|(expected, _)| *expected == task) else {
                continue;
            };
            let base = i32::from(combat.attributes().health);
            let protected = combat.protected(now as f32);
            let task = *expected;
            if self.effects.get(&slot).is_none_or(|effect| effect.has_pending_loadout()) {
                continue;
            }
            let life = self.composed.lives.entry(slot).or_insert_with(|| Life::new(task, now));
            if life.task != task {
                *life = Life::new(task, now);
            }
            if let Some(settings) = mode.chicken_aura
                && now >= life.aura_at
            {
                life.aura_at = now + f64::from(settings.interval_seconds);
                let position = s2sdk::GetClientAbsOrigin(slot);
                if chicken_positions.iter().any(|chicken| {
                    (chicken.x - position.x).powi(2) + (chicken.y - position.y).powi(2) + (chicken.z - position.z).powi(2) <= settings.radius.powi(2)
                }) && let Err(error) = heal(slot, base, settings.heal, mode.healing_cap_base_multiple, Mechanism::ChickenAura)
                {
                    self.handle_fault(error);
                    return;
                }
            }
            if let Some(settings) = mode.dont_stop {
                let velocity = s2sdk::GetClientAbsVelocity(slot);
                if protected
                    || !matches!(self.controller.phase(), Phase::Running | Phase::Warmup)
                    || velocity.x.hypot(velocity.y) >= settings.minimum_horizontal_speed
                {
                    life.stopped_since = now;
                    life.stopped_tick = now;
                } else if now >= life.stopped_since + f64::from(settings.grace_seconds) && now >= life.stopped_tick {
                    life.stopped_tick = now + f64::from(settings.damage_interval_seconds);
                    self.engine_actions.push(EngineAction::Composed(Action::Penalty {
                        task,
                        player,
                        amount: settings.damage as f32,
                        burn: false,
                    }));
                }
            }
            if let Some(settings) = mode.hot_floor {
                if protected || !matches!(self.controller.phase(), Phase::Running | Phase::Warmup) || s2sdk::GetClientFlags(slot) & 1 == 0 {
                    life.grounded_since = now;
                    life.grounded_tick = now;
                } else {
                    let elapsed = now - life.grounded_since;
                    if elapsed >= f64::from(settings.grounded_seconds) && now >= life.grounded_tick {
                        life.grounded_tick = now + f64::from(settings.damage_interval_seconds);
                        self.engine_actions.push(EngineAction::Composed(Action::Penalty {
                            task,
                            player,
                            amount: settings.damage as f32,
                            burn: true,
                        }));
                    }
                    if elapsed >= f64::from(settings.ignite_after_seconds) {
                        self.afterburns.refresh_source(
                            task,
                            player,
                            now,
                            funrounds_core::AfterburnSettings {
                                duration_seconds: settings.burn_duration_seconds,
                                damage_fraction: settings.burn_damage_fraction,
                                minimum_damage: settings.burn_minimum_damage,
                            },
                            true,
                        );
                        if self.afterburns.queue_ignite(task, player) {
                            self.engine_actions
                                .push(EngineAction::Afterburn(super::afterburn::Action::Ignite { task, player }));
                        }
                    }
                }
            }
        }
        self.composed
            .lives
            .retain(|slot, life| self.players.identity(*slot) == Some(life.task.player));
    }

    pub(crate) fn settle_regeneration(&mut self, now: f64) {
        if !self.composed_active() {
            return;
        }
        let Some(mode) = self.controller.current_mode() else { return };
        let Some(settings) = mode.regeneration else { return };
        let cap = mode.healing_cap_base_multiple;
        // Native penalty and delayed damage actions dispatch hurt synchronously.
        // Check the latest combat time only after those actions have settled.
        for (&slot, life) in &mut self.composed.lives {
            if now < life.last_combat + f64::from(settings.delay_seconds) || now < life.regeneration_at {
                continue;
            }
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            let Some((expected, combat)) = self.combat.lives.get(&slot) else {
                continue;
            };
            if *expected != life.task
                || !self.players.validates(life.task, self.controller.generation(), player.pawn_handle as u32)
                || self.controllers.get(&slot) != Some(&player.controller_handle)
                || self.effects.get(&slot).is_none_or(|effect| effect.has_pending_loadout())
            {
                continue;
            }
            life.regeneration_at = now + f64::from(settings.interval_seconds);
            match heal(slot, i32::from(combat.attributes().health), settings.heal, cap, Mechanism::Regeneration) {
                Ok(true) if life.breath_pending => {
                    life.breath_pending = false;
                    // Short native gasp; the looping Hostage.Breath track is reserved for Ghost.
                    s2sdk::EmitSoundToClient(
                        player.pawn_handle,
                        &plugify::Arr::from(vec![slot]),
                        &Str::from_str("Hostage.Pain"),
                        0.6,
                        100.0,
                    );
                }
                Ok(_) => {}
                Err(error) => {
                    self.handle_fault(error);
                    return;
                }
            }
        }
    }

    pub(super) fn update_big_heads(&mut self) {
        if !self.classic_enabled() || self.controller.phase() == Phase::Idle {
            return;
        }
        let Some(mode) = self.controller.current_mode() else { return };
        if mode.big_head.is_some()
            && !unsafe { crate::funrounds_native::__funrounds_native_HasBigHeadScaleSupport }.is_some_and(|query| unsafe { query() })
        {
            self.handle_fault(Fault::mechanism(Mechanism::BigHead, "native Big Head support or owned state lost"));
            return;
        }
        if self.composed.head_round != Some(self.controller.generation()) {
            self.composed.head_round = Some(self.controller.generation());
            self.composed.head_scales.clear();
            if let Some(settings) = mode.big_head {
                for slot in 0..s2sdk::GetMaxClients() {
                    if s2sdk::IsClientInGame(slot) {
                        self.composed.head_scales.insert(
                            s2sdk::PlayerSlotToEntHandle(slot),
                            funrounds_core::big_head_scale(
                                s2sdk::GetClientKills(slot).max(0) as u32,
                                s2sdk::GetClientDeaths(slot).max(0) as u32,
                                settings.min_scale,
                                settings.max_scale,
                            ),
                        );
                    }
                }
            }
        }
        for slot in 0..s2sdk::GetMaxClients() {
            let Some(player) = LivePlayer::resolve(slot) else { continue };
            if self.combat_task(player).is_none() {
                continue;
            }
            if let Some(settings) = mode.big_head {
                // A respawn or takeover reuses this round's controller snapshot; no mid-round K/D resampling.
                let scale = self.composed.heads.get(&player.pawn_handle).copied().unwrap_or_else(|| {
                    *self.composed.head_scales.entry(player.controller_handle).or_insert_with(|| {
                        funrounds_core::big_head_scale(
                            s2sdk::GetClientKills(slot).max(0) as u32,
                            s2sdk::GetClientDeaths(slot).max(0) as u32,
                            settings.min_scale,
                            settings.max_scale,
                        )
                    })
                });
                if self.composed.heads.get(&player.pawn_handle) != Some(&scale) {
                    self.composed.heads.insert(player.pawn_handle, scale);
                    if !unsafe { crate::funrounds_native::__funrounds_native_SetBigHeadScaleTarget }
                        .is_some_and(|set| unsafe { set(player.pawn_handle, scale) })
                    {
                        self.handle_fault(Fault::mechanism(Mechanism::BigHead, "Big Head target registration failed"));
                        return;
                    }
                }
            }
        }
    }

    pub(super) fn clear_composed_life(&mut self, identity: PlayerIdentity) {
        if self.composed.lives.get(&identity.slot).is_some_and(|life| life.task.player == identity) {
            self.composed.lives.remove(&identity.slot);
        }
        self.composed.weapons.retain(|(_, action)| action.task().player != identity);
        if let Some(mut flash) = self.composed.flashes.remove(&identity.slot)
            && (flash.task.player != identity || !restore_flash(&mut flash))
        {
            self.composed.flashes.insert(identity.slot, flash);
            if flash.task.player == identity {
                self.record_fault(Fault::mechanism(Mechanism::LongFlash, "long_flash life restoration pending"));
            }
        }
        let handle = identity.pawn_handle as i32;
        if self.composed.heads.contains_key(&handle) {
            if unsafe { crate::funrounds_native::__funrounds_native_ClearBigHeadScaleTarget }.is_some_and(|clear| unsafe { clear(handle) }) {
                self.composed.heads.remove(&handle);
            } else {
                self.record_fault(Fault::mechanism(Mechanism::BigHead, "Big Head life restoration pending"));
            }
        }
    }

    pub(super) fn takeover_composed(&mut self, task: PlayerTask, player: LivePlayer) {
        if let Some(slot) = self
            .composed
            .lives
            .iter()
            .find_map(|(slot, life)| (life.task.round == task.round && life.task.player.pawn_handle == task.player.pawn_handle).then_some(*slot))
            && let Some(mut life) = self.composed.lives.remove(&slot)
        {
            life.task = task;
            self.composed.lives.insert(player.slot, life);
        }
        if let Some(slot) = self
            .composed
            .flashes
            .iter()
            .find_map(|(slot, flash)| (flash.task.round == task.round && flash.player.pawn_handle == player.pawn_handle).then_some(*slot))
            && let Some(mut flash) = self.composed.flashes.remove(&slot)
            && flash.installed.is_some()
        {
            flash.task = task;
            flash.player = player;
            flash.pending_until = 0.0;
            self.composed.flashes.insert(player.slot, flash);
        }
    }

    pub(super) fn cleanup_composed(&mut self) {
        self.composed.lives.clear();
        self.composed.weapons.clear();
        for (slot, mut flash) in std::mem::take(&mut self.composed.flashes) {
            if !restore_flash(&mut flash) {
                self.composed.flashes.insert(slot, flash);
            }
        }
        let clear = unsafe { crate::funrounds_native::__funrounds_native_ClearBigHeadScaleTarget };
        self.composed
            .heads
            .retain(|handle, _| !clear.is_some_and(|clear| unsafe { clear(*handle) }));
        if !self.composed.flashes.is_empty() {
            self.record_fault(Fault::mechanism(Mechanism::LongFlash, "long_flash restoration pending"));
        }
        if !self.composed.heads.is_empty() {
            self.record_fault(Fault::mechanism(Mechanism::BigHead, "Big Head restoration pending"));
        }
    }
}

fn inventory_weapon(player: LivePlayer, handle: i32) -> bool {
    s2sdk::IsValidEntHandle(handle)
        && s2sdk::GetEntityOwner(handle) == player.pawn_handle
        && s2sdk::GetClientWeapons(player.slot).iter().any(|current| *current == handle)
        && {
            let class = s2sdk::GetEntityClassname(handle);
            class.as_str().starts_with("weapon_")
        }
}

pub(super) fn heal(slot: i32, base: i32, amount: funrounds_core::HealingAmount, cap: f32, mechanism: Mechanism) -> Result<bool, Fault> {
    if let Some(amount) = amount.amount(base)
        && let Some(target) = funrounds_core::healing_target(base, s2sdk::GetClientHealth(slot), amount, cap)
    {
        s2sdk::SetClientHealth(slot, target);
        if s2sdk::GetClientHealth(slot) != target {
            return Err(Fault::mechanism(mechanism, "shared healing write/readback failed"));
        }
        return Ok(true);
    }
    Ok(false)
}

pub(crate) fn execute(action: Action) -> Result<(), Fault> {
    match action {
        Action::Weapon {
            player, fired, switch_to, ..
        } => {
            if let Some(target) = switch_to {
                s2sdk::SwitchWeapon(player.slot, target);
            } else {
                let zero = plugify::Vec3 { x: 0.0, y: 0.0, z: 0.0 };
                s2sdk::DropWeapon(player.slot, fired.handle, &zero, &zero);
            }
        }
        Action::Penalty { player, amount, burn, .. } => {
            let zero = plugify::Vec3 { x: 0.0, y: 0.0, z: 0.0 };
            crate::damage::with_internal_damage_context(
                crate::damage::InternalDamageContext {
                    victim: player.pawn_handle,
                    attacker: player.pawn_handle,
                    inflictor: player.pawn_handle,
                    weapon: -1,
                    victim_slot: player.slot,
                    attacker_slot: player.slot,
                    headshot: false,
                    check_protection: true,
                    suppress_lifesteal: true,
                    random_heal: None,
                    death_weapon: Some(if burn { "hot_floor" } else { "dont_stop" }.into()),
                },
                || {
                    crate::damage::apply_native_damage(
                        crate::damage::NativeDamageCall {
                            victim: player.pawn_handle,
                            attacker: player.pawn_handle,
                            inflictor: player.pawn_handle,
                            force: &zero,
                            position: &zero,
                            amount,
                            damage_type: if burn { crate::damage::DMG_BURN } else { 0 },
                            weapon: -1,
                            skip_plugin_rules: true,
                        },
                        if burn { Mechanism::HotFloor } else { Mechanism::DontStop },
                        "movement penalty native damage rejected",
                    )
                },
            )?;
        }
    }
    Ok(())
}
