// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{
    capability::{Probe, imports},
    game::LivePlayer,
    s2sdk,
};
use funrounds_core::{PlayerTask, SuicideSequence};
use plugify::{Str, Vec3};
use std::collections::HashMap;

const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
pub(crate) const EXPLOSION_PARTICLE: &str = "particles/explosions_fx/explosion_c4_short.vpcf";
const EXPLOSION_SOUND: &str = "BaseGrenade.Explode";
// Use the built-in detonation warning, not the quiet C4 pickup Foley.
const START_SOUND: &str = "C4.ExplodeWarning";
pub(crate) const FEEDBACK_LIFETIME: f32 = 2.0;

pub fn c4_probe(purchase: bool) -> Probe {
    let mut probe = Probe::default();
    imports!(probe;
        __s2sdk_GetClientWeapons, __s2sdk_GetClientActiveWeapon, __s2sdk_GetEntityOwner, __s2sdk_IsValidEntHandle,
        __s2sdk_GetEntityClassname, __s2sdk_IsValidEntHandle, __s2sdk_GiveNamedItem,
        __s2sdk_RemoveWeapon, __s2sdk_RemoveEntity, __s2sdk_SelectWeapon,
    );
    if purchase {
        probe.include(crate::cash::probe());
    }
    probe
}

pub(crate) fn resource_probe() -> Probe {
    let mut probe = Probe::default();
    imports!(probe;
        __s2sdk_Precache, __s2sdk_OnBuildGameSessionManifest_Register,
        __s2sdk_OnBuildGameSessionManifest_Unregister,
    );
    probe
}

pub fn probe(hook_ready: bool) -> Probe {
    let mut probe = Probe::default();
    probe.include(resource_probe());
    probe.require(hook_ready, "hook:CanAcquire");
    imports!(probe;
        __s2sdk_GetClientWeapons, __s2sdk_GetClientActiveWeapon, __s2sdk_GetEntityOwner,
        __s2sdk_GiveNamedItem, __s2sdk_RemoveWeapon, __s2sdk_RemoveEntity, __s2sdk_SelectWeapon,
        __s2sdk_DropWeapon, __s2sdk_GetClientSpeed, __s2sdk_SetClientSpeed,
        __s2sdk_GetEntSchema2, __s2sdk_SetEntSchema2, __s2sdk_GetEntSchemaFloat2,
        __s2sdk_SetEntSchemaFloat2, __s2sdk_GetEntityAbsOrigin,
    );
    imports!(probe;
        __s2sdk_CreateEntityByName, __s2sdk_DispatchSpawn2, __s2sdk_AcceptEntityInput, __s2sdk_TeleportEntity,
        __s2sdk_EmitSoundToClient, __s2sdk_GetMaxClients, __s2sdk_IsClientInGame, __s2sdk_GetClientTeam,
    );
    for (class, field) in [
        ("CCSPlayerController", "m_pInGameMoneyServices"),
        ("CCSPlayerController_InGameMoneyServices", "m_iAccount"),
        ("CBasePlayerPawn", "m_pWeaponServices"),
        ("CCSPlayer_WeaponServices", "m_flNextAttack"),
    ] {
        probe.schema(class, field);
    }
    for name in [
        "mp_friendlyfire",
        "ff_damage_reduction_bullets",
        "ff_damage_reduction_grenade",
        "ff_damage_reduction_other",
    ] {
        probe.require(crate::parameters::has_convar(name), format!("convar:{name}"));
    }
    probe
}

fn integer(pointer: usize, class: &str, field: &str) -> i64 {
    s2sdk::GetEntSchema2(pointer, &Str::from_str(class), &Str::from_str(field), 0)
}

fn services(player: LivePlayer) -> usize {
    integer(s2sdk::EntHandleToEntPointer(player.pawn_handle), "CBasePlayerPawn", "m_pWeaponServices") as usize
}

fn attack_time(player: LivePlayer) -> f32 {
    s2sdk::GetEntSchemaFloat2(
        services(player),
        &Str::from_str("CCSPlayer_WeaponServices"),
        &Str::from_str("m_flNextAttack"),
        0,
    ) as f32
}

fn set_attack_time(player: LivePlayer, value: f32) {
    s2sdk::SetEntSchemaFloat2(
        services(player),
        &Str::from_str("CCSPlayer_WeaponServices"),
        &Str::from_str("m_flNextAttack"),
        f64::from(value),
        true,
        0,
    );
}

fn owned_item(player: LivePlayer, weapon: i32) -> bool {
    s2sdk::IsValidEntHandle(weapon)
        && s2sdk::GetEntityOwner(weapon) == player.pawn_handle
        && s2sdk::GetClientWeapons(player.slot).iter().any(|handle| *handle == weapon)
}

pub fn real_c4(player: LivePlayer, weapon: i32) -> bool {
    owned_item(player, weapon) && s2sdk::GetEntityClassname(weapon).as_str() == "weapon_c4"
}

pub fn grant_c4(player: LivePlayer, current: impl Fn() -> bool) -> Result<i32, String> {
    if !current() {
        return Err("C4 delivery cancelled".into());
    }
    let inventory_before = s2sdk::GetClientWeapons(player.slot).iter().copied().collect::<Vec<_>>();
    let handle = s2sdk::GiveNamedItem(player.slot, &Str::from_str("weapon_c4"));
    let granted = current() && real_c4(player, handle) && !inventory_before.contains(&handle);
    if granted {
        s2sdk::SelectWeapon(player.slot, handle);
    }
    if granted && current() && s2sdk::GetClientActiveWeapon(player.slot) == handle {
        return Ok(handle);
    }
    if !inventory_before.contains(&handle)
        && s2sdk::IsValidEntHandle(handle)
        && matches!(s2sdk::GetEntityOwner(handle), owner if owner == -1 || owner == player.pawn_handle)
    {
        if LivePlayer::resolve_present(player.slot) == Some(player) && owned_item(player, handle) {
            s2sdk::RemoveWeapon(player.slot, handle);
        }
        if s2sdk::IsValidEntHandle(handle) {
            s2sdk::RemoveEntity(handle);
        }
    }
    Err("C4 delivery failed".into())
}

// Engine grant/equip can synchronously dispatch events: execute without the runtime lock.
pub(crate) fn purchase(
    player: LivePlayer,
    price: u32,
    current: impl Fn() -> bool,
    same_buyer: impl Fn() -> bool,
) -> Result<i32, crate::cash::PurchaseError> {
    if !current() {
        return Err(crate::cash::PurchaseError::Rejected("purchase.cancelled"));
    }
    if s2sdk::GetClientTeam(player.slot) as i32 != 2 {
        return Err(crate::cash::PurchaseError::Rejected("purchase.c4_t_only"));
    }
    crate::cash::purchase(player, price, crate::cash::PurchaseItem::C4, same_buyer, || {
        grant_c4(player, current).map_err(|_| "C4 delivery failed".to_owned())
    })
}

#[derive(Clone, Copy)]
pub struct Owned {
    pub player: LivePlayer,
    speed_before: f32,
    speed_written: f32,
    attack_before: f32,
    attack_written: f32,
}

impl Owned {
    fn capture(player: LivePlayer, now: f32, seconds: f32) -> Result<Self, String> {
        if services(player) == 0 {
            return Err("武器服务不可用".into());
        }
        let speed = s2sdk::GetClientSpeed(player.slot);
        let attack = attack_time(player);
        if !speed.is_finite() || !attack.is_finite() || !now.is_finite() {
            return Err("自爆属性无效".into());
        }
        Ok(Self {
            player,
            speed_before: speed,
            speed_written: speed * 1.6,
            attack_before: attack,
            attack_written: now + seconds + 1.0,
        })
    }

    pub fn restore(self) -> Result<(), String> {
        if LivePlayer::resolve_present(self.player.slot) != Some(self.player) {
            return Ok(());
        }
        if s2sdk::GetClientSpeed(self.player.slot) == self.speed_written {
            s2sdk::SetClientSpeed(self.player.slot, self.speed_before);
            if s2sdk::GetClientSpeed(self.player.slot) != self.speed_before {
                return Err("自爆速度恢复失败".into());
            }
        }
        if services(self.player) != 0 && attack_time(self.player) == self.attack_written {
            set_attack_time(self.player, self.attack_before);
            if attack_time(self.player) != self.attack_before {
                return Err("自爆攻击锁恢复失败".into());
            }
        }
        Ok(())
    }

    pub fn start(self, current: impl Fn() -> bool) -> Result<(), String> {
        if !current() {
            return Err("自爆生命已改变".into());
        }
        for weapon in s2sdk::GetClientWeapons(self.player.slot).iter().copied().collect::<Vec<_>>() {
            if !current() {
                return Err("自爆装备事务已取消".into());
            }
            if !s2sdk::IsValidEntHandle(weapon) || s2sdk::GetEntityOwner(weapon) != self.player.pawn_handle {
                continue;
            }
            if real_c4(self.player, weapon) {
                s2sdk::RemoveWeapon(self.player.slot, weapon);
                if s2sdk::IsValidEntHandle(weapon) {
                    s2sdk::RemoveEntity(weapon);
                }
            } else {
                s2sdk::DropWeapon(self.player.slot, weapon, &ZERO, &ZERO);
            }
        }
        if !current() {
            return Err("自爆装备事务中生命已改变".into());
        }
        s2sdk::SetClientSpeed(self.player.slot, self.speed_written);
        set_attack_time(self.player, self.attack_written);
        if !s2sdk::GetClientWeapons(self.player.slot).is_empty()
            || s2sdk::GetClientSpeed(self.player.slot) != self.speed_written
            || attack_time(self.player) != self.attack_written
        {
            return Err("自爆空手／速度／攻击锁读回失败".into());
        }
        Ok(())
    }
}

pub struct Entry {
    pub task: PlayerTask,
    pub player: LivePlayer,
    pub c4: i32,
    pub look_down: bool,
    pub sequence: Option<SuicideSequence>,
    pub owned: Option<Owned>,
}

#[derive(Clone, Copy)]
pub enum Action {
    Purchase {
        task: PlayerTask,
        player: LivePlayer,
        start: bool,
        price: u32,
    },
    Start {
        task: PlayerTask,
        owned: Owned,
    },
    Blast {
        task: PlayerTask,
        player: LivePlayer,
    },
    Finish {
        task: PlayerTask,
        player: LivePlayer,
        amount: f32,
        tick: i32,
        after_round_end: bool,
    },
}

impl Action {
    pub fn task(self) -> PlayerTask {
        match self {
            Self::Purchase { task, .. } | Self::Start { task, .. } | Self::Blast { task, .. } | Self::Finish { task, .. } => task,
        }
    }
    pub fn player(self) -> LivePlayer {
        match self {
            Self::Start { owned, .. } => owned.player,
            Self::Purchase { player, .. } | Self::Blast { player, .. } | Self::Finish { player, .. } => player,
        }
    }
}

#[derive(Default)]
pub struct Suicides {
    pub entries: HashMap<i32, Entry>,
    pub pending: Vec<Action>,
    pub buying: HashMap<i32, PlayerTask>,
    pub ff: crate::parameters::Parameters,
}

impl Suicides {
    pub fn qualify(&mut self, task: PlayerTask, player: LivePlayer, c4: i32) {
        if self
            .entries
            .get(&player.slot)
            .is_some_and(|entry| entry.task == task && (entry.c4 == c4 || entry.sequence.is_some()))
        {
            return;
        }
        self.entries.insert(
            player.slot,
            Entry {
                task,
                player,
                c4,
                look_down: crate::funrounds_native::GetLookAtWeaponButtonState(player.pawn_handle) == 1,
                sequence: None,
                owned: None,
            },
        );
    }

    pub fn active(&self, player: LivePlayer) -> bool {
        self.entries
            .get(&player.slot)
            .is_some_and(|entry| entry.player == player && entry.sequence.is_some())
    }

    pub(crate) fn takeover(&mut self, source_slots: &[i32], player: LivePlayer, task: PlayerTask) -> Result<Option<i32>, String> {
        let mut sources = source_slots.to_vec();
        sources.sort_unstable();
        sources.dedup();
        let mut slots = sources.clone();
        slots.push(player.slot);
        slots.sort_unstable();
        slots.dedup();
        let matches_pawn = |entry: &Entry| {
            entry.task.round == task.round
                && entry.task.player.pawn_handle == task.player.pawn_handle
                && entry.player.pawn_handle == player.pawn_handle
        };
        let pending_round_end_finish = |slot: i32| {
            self.pending.iter().any(|action| {
                matches!(
                    action,
                    Action::Finish {
                        task: queued,
                        player: queued_player,
                        after_round_end: true,
                        ..
                    } if queued.player.slot == slot
                        && queued.player.pawn_handle == task.player.pawn_handle
                        && queued_player.pawn_handle == player.pawn_handle
                )
            })
        };
        let matching_sources = sources
            .iter()
            .copied()
            .filter(|slot| self.entries.get(slot).is_some_and(matches_pawn) || pending_round_end_finish(*slot))
            .collect::<Vec<_>>();
        let source_slot = matching_sources
            .iter()
            .copied()
            .find(|slot| {
                *slot == player.slot
                    && (self
                        .entries
                        .get(slot)
                        .is_some_and(|entry| entry.sequence.is_some() || entry.owned.is_some())
                        || pending_round_end_finish(*slot))
            })
            .or_else(|| {
                matching_sources.iter().copied().find(|slot| {
                    self.entries
                        .get(slot)
                        .is_some_and(|entry| entry.sequence.is_some() || entry.owned.is_some())
                        || pending_round_end_finish(*slot)
                })
            })
            .or_else(|| matching_sources.iter().copied().find(|slot| *slot == player.slot))
            .or_else(|| matching_sources.first().copied());
        self.buying.retain(|slot, _| !slots.contains(slot));
        self.pending.retain_mut(|action| {
            let queued = action.task();
            if !slots.contains(&queued.player.slot) {
                return true;
            }
            if matches!(action, Action::Purchase { .. }) || source_slot != Some(queued.player.slot) {
                return false;
            }
            match action {
                Action::Start { task: queued, owned } if queued.round == task.round && queued.player.pawn_handle == task.player.pawn_handle => {
                    *queued = task;
                    owned.player = player;
                    true
                }
                Action::Blast {
                    task: queued,
                    player: queued_player,
                } if queued.round == task.round && queued.player.pawn_handle == task.player.pawn_handle => {
                    *queued = task;
                    *queued_player = player;
                    true
                }
                Action::Finish {
                    task: queued,
                    player: queued_player,
                    after_round_end,
                    ..
                } if (queued.round == task.round || *after_round_end) && queued.player.pawn_handle == task.player.pawn_handle => {
                    *queued = task;
                    *queued_player = player;
                    true
                }
                _ => false,
            }
        });

        let Some(source_slot) = source_slot else {
            if self.entries.contains_key(&player.slot) {
                self.forget(player.slot)?;
            }
            return Ok(None);
        };
        if source_slot != player.slot && self.entries.contains_key(&player.slot) && !sources.contains(&player.slot) {
            // A target-slot entry that is not backed by a current same-pawn tracker
            // identity is stale, even when its old handle happens to match. Avoid
            // restoring a duplicate snapshot onto the pawn retained by its live owner.
            if self
                .entries
                .get(&player.slot)
                .is_some_and(|entry| entry.player.pawn_handle == player.pawn_handle)
            {
                self.entries.remove(&player.slot);
            } else {
                self.forget(player.slot)?;
            }
        }
        let Some(mut entry) = self.entries.remove(&source_slot) else {
            return Ok(Some(source_slot));
        };
        if entry.task.round != task.round
            || entry.task.player.pawn_handle != task.player.pawn_handle
            || entry.player.pawn_handle != player.pawn_handle
        {
            self.entries.insert(source_slot, entry);
            return Err("suicide owner changed during bot takeover".into());
        }

        if let Some(sequence) = &mut entry.sequence
            && !sequence.retarget(task)
        {
            self.entries.insert(source_slot, entry);
            return Err("active suicide sequence changed during bot takeover".into());
        }
        entry.task = task;
        entry.player = player;
        if let Some(owned) = &mut entry.owned {
            owned.player = player;
        }
        for slot in slots {
            if slot != source_slot && slot != player.slot {
                self.entries.remove(&slot);
            }
        }
        self.entries.insert(player.slot, entry);
        Ok(Some(source_slot))
    }

    pub fn begin(&mut self, slot: i32, tick: i32, now: f32) -> Result<(), String> {
        let entry = self.entries.get_mut(&slot).ok_or("没有有效C4自爆资格")?;
        if entry.sequence.is_some() {
            return Err("已启动自爆".into());
        }
        if !real_c4(entry.player, entry.c4) {
            return Err("真实C4已不在身上".into());
        }
        let seconds = if s2sdk::GetClientTeam(slot) as i32 == 2 { 5.0 } else { 0.0 };
        let owned = Owned::capture(entry.player, now, seconds)?;
        entry.sequence = SuicideSequence::begin(entry.task, now + seconds, tick);
        entry.owned = Some(owned);
        self.pending.push(Action::Start { task: entry.task, owned });
        Ok(())
    }

    pub fn forget(&mut self, slot: i32) -> Result<(), String> {
        self.pending.retain(|action| action.task().player.slot != slot);
        self.buying.remove(&slot);
        if let Some(entry) = self.entries.get(&slot)
            && let Some(owned) = entry.owned
        {
            owned.restore()?;
        }
        self.entries.remove(&slot);
        Ok(())
    }

    pub fn cleanup(&mut self) -> Result<(), String> {
        self.pending.clear();
        self.cleanup_entries()
    }

    pub fn cleanup_after_round_end(&mut self) -> Result<(), String> {
        self.pending.retain_mut(|action| match action {
            Action::Finish { after_round_end, .. } => {
                *after_round_end = true;
                true
            }
            _ => false,
        });
        self.cleanup_entries()
    }

    pub fn cancel_round_end_finishes(&mut self) {
        self.pending
            .retain(|action| !matches!(action, Action::Finish { after_round_end: true, .. }));
    }

    fn cleanup_entries(&mut self) -> Result<(), String> {
        let pending = std::mem::take(&mut self.pending);
        self.buying.clear();
        let mut failures = Vec::new();
        for slot in self.entries.keys().copied().collect::<Vec<_>>() {
            if let Err(error) = self.forget(slot) {
                failures.push(error);
            }
        }
        if let Err(error) = self.ff.restore() {
            failures.push(error);
        }
        self.pending = pending;
        if failures.is_empty() { Ok(()) } else { Err(failures.join("; ")) }
    }
}

pub fn hit(source: LivePlayer, target: LivePlayer, amount: f32, force: bool) -> Result<(), String> {
    let result = crate::damage::with_internal_damage_context(
        crate::damage::InternalDamageContext {
            victim: target.pawn_handle,
            attacker: source.pawn_handle,
            inflictor: source.pawn_handle,
            weapon: -1,
            victim_slot: target.slot,
            attacker_slot: source.slot,
            headshot: false,
            check_protection: !force,
            suppress_lifesteal: false,
            random_heal: None,
            death_weapon: Some("c4".into()),
        },
        || {
            crate::damage::apply_native_damage(
                crate::damage::NativeDamageCall {
                    victim: target.pawn_handle,
                    inflictor: source.pawn_handle,
                    attacker: source.pawn_handle,
                    force: &ZERO,
                    position: &ZERO,
                    amount,
                    damage_type: crate::damage::DMG_BLAST,
                    weapon: -1,
                    skip_plugin_rules: true,
                },
                crate::fault::Mechanism::Suicide,
                "原生自爆伤害被拒绝",
            )
        },
    );
    result.map(|_| ()).map_err(|error| error.to_string())
}

pub fn spawn_feedback(player: LivePlayer) -> Result<i32, String> {
    if !s2sdk::IsValidEntHandle(player.pawn_handle) {
        return Err("自爆反馈玩家实体失效".into());
    }
    let effect = s2sdk::CreateEntityByName(&Str::from_str("info_particle_system"));
    if !s2sdk::IsValidEntHandle(effect) {
        return Err("自爆粒子反馈创建失败".into());
    }
    let origin = s2sdk::GetEntityAbsOrigin(player.pawn_handle);
    let result = (|| {
        let keys = plugify::Arr::from(vec!["effect_name".to_owned()]);
        let values = plugify::Arr::from(vec![plugify::Any::from(EXPLOSION_PARTICLE)]);
        s2sdk::DispatchSpawn2(effect, &keys, &values);
        if !s2sdk::IsValidEntHandle(effect) {
            return Err("自爆粒子实体在生成时消失".into());
        }
        s2sdk::TeleportEntity(effect, &origin, &ZERO, &ZERO);
        if !s2sdk::IsValidEntHandle(effect) || !same_position(s2sdk::GetEntityAbsOrigin(effect), origin) {
            return Err("自爆粒子爆点定位失败".into());
        }
        s2sdk::AcceptEntityInput(
            effect,
            &Str::from_str("Start"),
            player.pawn_handle,
            player.pawn_handle,
            &plugify::Var::default(),
            s2sdk::FieldType::Auto,
            0,
        );
        if !s2sdk::IsValidEntHandle(effect) {
            return Err("自爆粒子实体在启动时消失".into());
        }
        Ok(effect)
    })();
    if result.is_err() && s2sdk::IsValidEntHandle(effect) {
        s2sdk::RemoveEntity(effect);
    }
    result
}

pub fn precache_feedback_resource() {
    s2sdk::Precache(&Str::from_str(EXPLOSION_PARTICLE));
}

pub fn play_start_sound(player: LivePlayer) -> Result<(), String> {
    play_sound(player.pawn_handle, START_SOUND)
}

pub fn play_explosion_sound(source: i32) -> Result<(), String> {
    play_sound(source, EXPLOSION_SOUND)
}

pub fn remove_feedback(handle: i32) {
    if s2sdk::IsValidEntHandle(handle) {
        s2sdk::RemoveEntity(handle);
    }
}

fn play_sound(source: i32, sound: &str) -> Result<(), String> {
    if !s2sdk::IsValidEntHandle(source) {
        return Err("自爆声音来源实体失效".into());
    }
    let recipients = (0..s2sdk::GetMaxClients())
        .filter(|slot| s2sdk::IsClientInGame(*slot) && matches!(s2sdk::GetClientTeam(*slot) as i32, 1..=3))
        .collect::<Vec<_>>();
    if !recipients.is_empty() {
        s2sdk::EmitSoundToClient(source, &plugify::Arr::from(recipients), &Str::from_str(sound), 1.0, 100.0);
    }
    Ok(())
}

fn same_position(left: Vec3, right: Vec3) -> bool {
    left.x.is_finite()
        && left.y.is_finite()
        && left.z.is_finite()
        && (left.x - right.x).abs() < 0.1
        && (left.y - right.y).abs() < 0.1
        && (left.z - right.z).abs() < 0.1
}

#[derive(Default)]
pub struct FeedbackEntities {
    active: HashMap<i32, f32>,
}

impl FeedbackEntities {
    pub fn track(&mut self, handle: i32, expires_at: f32) -> bool {
        if handle == -1 || !expires_at.is_finite() {
            return false;
        }
        self.active.insert(handle, expires_at);
        true
    }

    pub fn take_expired(&mut self, now: f32) -> Vec<i32> {
        if !now.is_finite() {
            return Vec::new();
        }
        let mut expired = Vec::new();
        self.active.retain(|handle, expires_at| {
            if *expires_at <= now {
                expired.push(*handle);
                false
            } else {
                true
            }
        });
        expired
    }

    pub fn take_all(&mut self) -> Vec<i32> {
        self.active.drain().map(|(handle, _)| handle).collect()
    }

    pub fn discard_map_state(&mut self) {
        self.active.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    pub fn has_only_unexpired(&self, now: f32) -> bool {
        now.is_finite() && self.active.values().all(|expires_at| *expires_at > now)
    }
}
