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
const PRICE: i64 = 10000;

pub fn c4_probe(purchase: bool) -> Probe {
    let mut probe = Probe::default();
    imports!(probe;
        __s2sdk_GetClientWeapons, __s2sdk_GetClientActiveWeapon, __s2sdk_GetEntityOwner,
        __s2sdk_GetEntityClassname, __s2sdk_IsValidEntHandle, __s2sdk_GiveNamedItem,
        __s2sdk_RemoveWeapon, __s2sdk_RemoveEntity, __s2sdk_SelectWeapon,
    );
    if purchase {
        imports!(probe; __s2sdk_GetEntSchema2, __s2sdk_SetEntSchema2);
        probe.schema("CCSPlayerController", "m_pInGameMoneyServices");
        probe.schema("CCSPlayerController_InGameMoneyServices", "m_iAccount");
    }
    probe
}

pub fn probe(hook_ready: bool) -> Probe {
    let mut probe = Probe::default();
    probe.require(hook_ready, "hook:CanAcquire");
    imports!(probe;
        __s2sdk_GetClientWeapons, __s2sdk_GetClientActiveWeapon, __s2sdk_GetEntityOwner,
        __s2sdk_GiveNamedItem, __s2sdk_RemoveWeapon, __s2sdk_RemoveEntity, __s2sdk_SelectWeapon,
        __s2sdk_DropWeapon, __s2sdk_GetClientSpeed, __s2sdk_SetClientSpeed,
        __s2sdk_GetEntSchema2, __s2sdk_SetEntSchema2, __s2sdk_GetEntSchemaFloat2,
        __s2sdk_SetEntSchemaFloat2, __s2sdk_GetEntityAbsOrigin,
    );
    imports!(probe; __s2sdk_CreateEntityByName, __s2sdk_DispatchSpawn, __s2sdk_AcceptEntityInput, __s2sdk_TeleportEntity);
    for (class, field) in [
        ("CEnvExplosion", "m_iMagnitude"),
        ("CEnvExplosion", "m_flPlayerDamage"),
        ("CBaseEntity", "m_spawnflags"),
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
        return Err("C4发放已取消".into());
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
    Err("C4发放失败".into())
}

fn money(player: LivePlayer) -> Result<usize, String> {
    if !s2sdk::IsClientInGame(player.slot) || s2sdk::PlayerSlotToEntHandle(player.slot) != player.controller_handle {
        return Err("购买者连接已改变".into());
    }
    let pointer = integer(
        s2sdk::EntHandleToEntPointer(player.controller_handle),
        "CCSPlayerController",
        "m_pInGameMoneyServices",
    ) as usize;
    (pointer != 0).then_some(pointer).ok_or_else(|| "现金服务不可用".into())
}

fn balance(pointer: usize) -> i64 {
    integer(pointer, "CCSPlayerController_InGameMoneyServices", "m_iAccount")
}

fn write_balance(pointer: usize, value: i64) -> bool {
    s2sdk::SetEntSchema2(
        pointer,
        &Str::from_str("CCSPlayerController_InGameMoneyServices"),
        &Str::from_str("m_iAccount"),
        value,
        true,
        0,
    );
    balance(pointer) == value
}

// Engine grant/equip can synchronously dispatch events: execute without the runtime lock.
pub fn purchase(player: LivePlayer, current: impl Fn() -> bool, same_buyer: impl Fn() -> bool) -> Result<i32, String> {
    if !current() {
        return Err("C4事务已取消".into());
    }
    if s2sdk::GetClientTeam(player.slot) as i32 != 2 {
        return Err("只有T可以购买C4".into());
    }
    let pointer = money(player)?;
    let before = balance(pointer);
    if before < PRICE {
        return Err("购买C4需要10000现金".into());
    }
    let after = before - PRICE;
    if !write_balance(pointer, after) {
        return Err("现金扣款读回失败".into());
    }
    if let Ok(handle) = grant_c4(player, current) {
        return Ok(handle);
    }
    let refunded =
        same_buyer() && money(player).is_ok_and(|current| current == pointer && balance(current) == after && write_balance(current, before));
    if refunded {
        Err("C4发放失败，已退还现金".into())
    } else {
        Err("C4发放失败，现金所有权已改变，退款未完成".into())
    }
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
        if failures.is_empty() { Ok(()) } else { Err(failures.join("; ")) }
    }
}

pub fn hit(source: LivePlayer, target: LivePlayer, amount: f32) -> Result<(), String> {
    let result = crate::damage::with_internal_damage_context(
        crate::damage::InternalDamageContext {
            victim: target.pawn_handle,
            attacker: source.pawn_handle,
            inflictor: source.pawn_handle,
            weapon: -1,
            victim_slot: target.slot,
            attacker_slot: source.slot,
            headshot: false,
            suppress_lifesteal: false,
            random_heal: None,
            death_weapon: Some("c4".into()),
        },
        || {
            crate::funrounds_native::TakeEntityDamageWithWeapon(
                target.pawn_handle,
                source.pawn_handle,
                source.pawn_handle,
                &ZERO,
                &ZERO,
                amount,
                crate::damage::DMG_BLAST as i32,
                -1,
                true,
            )
        },
    );
    if result < 0 { Err("原生自爆伤害被拒绝".into()) } else { Ok(()) }
}

pub fn feedback(player: LivePlayer) -> Result<(), String> {
    let effect = s2sdk::CreateEntityByName(&Str::from_str("env_explosion"));
    if !s2sdk::IsValidEntHandle(effect) {
        return Err("原生爆炸反馈创建失败".into());
    }
    let pointer = s2sdk::EntHandleToEntPointer(effect);
    // SF_ENVEXPLOSION_NODAMAGE: the dedicated native hits own all gameplay damage.
    s2sdk::SetEntSchema2(pointer, &Str::from_str("CBaseEntity"), &Str::from_str("m_spawnflags"), 1, false, 0);
    s2sdk::SetEntSchema2(pointer, &Str::from_str("CEnvExplosion"), &Str::from_str("m_iMagnitude"), 100, false, 0);
    s2sdk::SetEntSchemaFloat2(
        pointer,
        &Str::from_str("CEnvExplosion"),
        &Str::from_str("m_flPlayerDamage"),
        0.0,
        false,
        0,
    );
    s2sdk::DispatchSpawn(effect);
    s2sdk::TeleportEntity(effect, &s2sdk::GetEntityAbsOrigin(player.pawn_handle), &ZERO, &ZERO);
    s2sdk::AcceptEntityInput(
        effect,
        &Str::from_str("Explode"),
        player.pawn_handle,
        player.pawn_handle,
        &plugify::Var::default(),
        s2sdk::FieldType::Auto,
        0,
    );
    if s2sdk::IsValidEntHandle(effect) {
        s2sdk::RemoveEntity(effect);
    }
    Ok(())
}
