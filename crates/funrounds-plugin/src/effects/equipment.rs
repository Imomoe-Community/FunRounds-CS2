// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::__s2sdk_GetWeaponItemDefinition;
use super::recoil::is_firearm;
use crate::{
    capability::{Probe, imports},
    fault::{Fault, Mechanism},
    game::LivePlayer,
    s2sdk,
};
use funrounds_core::{Mode, PlayerTask};
use plugify::Str;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrenadeKind {
    He,
    Decoy,
}

impl GrenadeKind {
    fn classname(self) -> &'static str {
        match self {
            Self::He => "weapon_hegrenade",
            Self::Decoy => "weapon_decoy",
        }
    }

    pub fn enabled(self, mode: &Mode) -> bool {
        match self {
            Self::He => mode.replenish_he,
            Self::Decoy => mode.replenish_decoy,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refill {
    id: u64,
    action_id: u64,
    map_epoch: u64,
    pub task: PlayerTask,
    pub player: LivePlayer,
    pub kind: GrenadeKind,
    spent: Option<i32>,
    diagnostics: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Refill(Refill),
    Remove { map_epoch: u64, action_id: u64, handle: i32 },
}

struct Transaction {
    refill: Refill,
    in_flight: bool,
    cancelled: bool,
}

#[derive(Default)]
pub struct Refills {
    next_id: u64,
    next_action_id: u64,
    map_epoch: u64,
    transactions: HashMap<u64, Transaction>,
    retiring: HashSet<i32>,
    removing: HashMap<i32, u64>,
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Waiting,
    Completed,
    Cancelled,
    Failed(crate::fault::Scope),
}

pub struct RefillResult {
    outcome: Outcome,
    created: Option<i32>,
    diagnostic: String,
}

pub enum ActionResult {
    Refilled(RefillResult),
    Removed(bool),
}

#[derive(Default)]
pub struct Completion {
    pub retry: Option<(PlayerTask, u64)>,
    pub diagnostic: Option<String>,
}

impl Refills {
    pub fn queue(&mut self, task: PlayerTask, player: LivePlayer, kind: GrenadeKind, diagnostics: bool) -> (u64, Option<i32>) {
        let spent = s2sdk::GetClientWeapons(player.slot)
            .iter()
            .copied()
            .find(|handle| s2sdk::IsValidEntHandle(*handle) && s2sdk::GetEntityClassname(*handle).to_string() == kind.classname());
        (self.queue_recorded(task, player, kind, spent, diagnostics), spent)
    }

    fn queue_recorded(&mut self, task: PlayerTask, player: LivePlayer, kind: GrenadeKind, spent: Option<i32>, diagnostics: bool) -> u64 {
        if let Some(transaction) = self.transactions.values().find(|transaction| {
            !transaction.cancelled
                && transaction.refill.task == task
                && transaction.refill.player == player
                && transaction.refill.kind == kind
                && transaction.refill.spent == spent
        }) {
            return transaction.refill.id;
        }
        self.next_id = self.next_id.checked_add(1).expect("grenade refill sequence exhausted");
        let refill = Refill {
            id: self.next_id,
            action_id: 0,
            map_epoch: self.map_epoch,
            task,
            player,
            kind,
            spent,
            diagnostics,
        };
        self.transactions.insert(
            refill.id,
            Transaction {
                refill,
                in_flight: false,
                cancelled: false,
            },
        );
        refill.id
    }

    pub fn start(&mut self, id: u64) -> Option<Action> {
        let transaction = self.transactions.get_mut(&id)?;
        if transaction.cancelled || transaction.in_flight {
            return None;
        }
        self.next_action_id = self.next_action_id.checked_add(1).expect("equipment action sequence exhausted");
        transaction.refill.action_id = self.next_action_id;
        transaction.in_flight = true;
        Some(Action::Refill(transaction.refill))
    }

    pub fn current(&self, action: Action) -> bool {
        match action {
            Action::Refill(refill) => {
                refill.map_epoch == self.map_epoch
                    && self
                        .transactions
                        .get(&refill.id)
                        .is_some_and(|transaction| transaction.refill == refill && transaction.in_flight && !transaction.cancelled)
            }
            Action::Remove {
                map_epoch,
                action_id,
                handle,
            } => map_epoch == self.map_epoch && self.retiring.contains(&handle) && self.removing.get(&handle) == Some(&action_id),
        }
    }

    pub fn cancel(&mut self, id: u64) {
        if let Some(transaction) = self.transactions.get_mut(&id) {
            if transaction.in_flight {
                transaction.cancelled = true;
            } else {
                self.transactions.remove(&id);
            }
        }
    }

    pub fn cancel_action(&mut self, action: Action) {
        if let Action::Refill(refill) = action
            && self
                .transactions
                .get(&refill.id)
                .is_some_and(|transaction| transaction.refill == refill && transaction.in_flight)
        {
            self.cancel(refill.id);
        }
    }

    pub fn cancel_player(&mut self, slot: i32) {
        self.cancel_matching(|refill| refill.task.player.slot == slot);
    }

    pub fn cancel_all(&mut self) {
        self.cancel_matching(|_| true);
    }

    fn cancel_matching(&mut self, matches: impl Fn(&Refill) -> bool) {
        self.transactions.retain(|_, transaction| {
            if !matches(&transaction.refill) {
                return true;
            }
            transaction.cancelled = true;
            transaction.in_flight
        });
    }

    pub fn discard_map_state(&mut self) {
        self.map_epoch = self.map_epoch.checked_add(1).expect("grenade refill map epoch exhausted");
        self.transactions.clear();
        self.retiring.clear();
        self.removing.clear();
    }

    pub fn finish(&mut self, action: Action, result: ActionResult) -> Result<Completion, Fault> {
        match (action, result) {
            (Action::Refill(refill), ActionResult::Refilled(mut result)) => {
                if refill.map_epoch != self.map_epoch {
                    return Ok(Completion::default());
                }
                if !self
                    .transactions
                    .get(&refill.id)
                    .is_some_and(|transaction| transaction.refill == refill && transaction.in_flight)
                {
                    return Ok(Completion::default());
                }
                let mut transaction = self.transactions.remove(&refill.id).expect("matching refill exists");
                if transaction.cancelled && !matches!(result.outcome, Outcome::Cancelled | Outcome::Failed(crate::fault::Scope::Global)) {
                    result.outcome = Outcome::Cancelled;
                    result.diagnostic = "transaction cancelled during native action".into();
                }
                if result.outcome == Outcome::Waiting {
                    transaction.in_flight = false;
                    self.transactions.insert(refill.id, transaction);
                    return Ok(Completion {
                        retry: Some((refill.task, refill.id)),
                        diagnostic: None,
                    });
                }
                if result.outcome != Outcome::Completed {
                    self.retiring.extend(result.created);
                    result.diagnostic = format!(
                        "grenade refill {:?} slot={} weapon={} spent_handle={:?} transaction={} action={}: {}",
                        result.outcome,
                        refill.player.slot,
                        refill.kind.classname(),
                        refill.spent,
                        refill.id,
                        refill.action_id,
                        result.diagnostic,
                    );
                }
                if let Outcome::Failed(scope) = result.outcome {
                    return Err(match scope {
                        crate::fault::Scope::Mechanism(mechanism) => Fault::mechanism(mechanism, result.diagnostic),
                        _ => Fault::global(result.diagnostic),
                    });
                }
                Ok(Completion {
                    retry: None,
                    diagnostic: (!result.diagnostic.is_empty()).then_some(result.diagnostic),
                })
            }
            (
                Action::Remove {
                    map_epoch,
                    action_id,
                    handle,
                },
                ActionResult::Removed(gone),
            ) => {
                if map_epoch == self.map_epoch && self.removing.get(&handle) == Some(&action_id) {
                    self.removing.remove(&handle);
                    if gone {
                        self.retiring.remove(&handle);
                    }
                }
                Ok(Completion::default())
            }
            _ => Err("equipment engine action returned an unexpected result".into()),
        }
    }

    pub fn cleanup_actions(&mut self) -> Vec<Action> {
        self.cleanup_actions_with(s2sdk::IsValidEntHandle)
    }

    fn cleanup_actions_with(&mut self, mut valid: impl FnMut(i32) -> bool) -> Vec<Action> {
        self.retiring.retain(|handle| valid(*handle));
        self.removing.retain(|handle, _| self.retiring.contains(handle));
        let mut actions = Vec::new();
        for handle in self.retiring.iter().copied() {
            if self.removing.contains_key(&handle) {
                continue;
            }
            self.next_action_id = self.next_action_id.checked_add(1).expect("equipment action sequence exhausted");
            self.removing.insert(handle, self.next_action_id);
            actions.push(Action::Remove {
                map_epoch: self.map_epoch,
                action_id: self.next_action_id,
                handle,
            });
        }
        actions
    }

    pub fn is_clean(&self) -> bool {
        self.transactions.is_empty() && self.retiring.is_empty() && self.removing.is_empty()
    }
}

pub fn refill_probe(grenade_event: bool) -> Probe {
    let mut probe = Probe::default();
    probe.require(grenade_event, "event:grenade_thrown");
    imports!(probe;
        __s2sdk_GetClientWeapons, __s2sdk_GetClientActiveWeapon, __s2sdk_GetEntityClassname,
        __s2sdk_IsValidEntHandle, __s2sdk_RemoveWeapon, __s2sdk_RemoveEntity, __s2sdk_GiveNamedItem,
        __s2sdk_SelectWeapon, __s2sdk_EntHandleToEntPointer, __s2sdk_GetEntSchema2,
    );
    probe.schema("CBaseCSGrenade", "m_bThrowAnimating");
    probe.schema("CBaseCSGrenade", "m_bRedraw");
    probe
}

pub fn execute(action: Action, mut current: impl FnMut() -> bool) -> ActionResult {
    match action {
        Action::Refill(refill) => ActionResult::Refilled(execute_refill(refill, current)),
        Action::Remove { handle, .. } => {
            if current() && s2sdk::IsValidEntHandle(handle) {
                s2sdk::RemoveEntity(handle);
            }
            ActionResult::Removed(!s2sdk::IsValidEntHandle(handle))
        }
    }
}

fn execute_refill(refill: Refill, mut current: impl FnMut() -> bool) -> RefillResult {
    let Refill { player, spent, kind, .. } = refill;
    let classname = kind.classname();
    let result = |outcome, created, diagnostic: &str| RefillResult {
        outcome,
        created,
        diagnostic: diagnostic.into(),
    };
    let mut valid = || current() && LivePlayer::resolve(player.slot) == Some(player);
    if !valid() {
        return result(Outcome::Cancelled, None, "player or task changed before refill");
    }
    let Some(handle) = spent_grenade_owned(spent, s2sdk::GetClientWeapons(player.slot).iter().copied())
        .filter(|handle| s2sdk::IsValidEntHandle(*handle) && s2sdk::GetEntityClassname(*handle).to_string() == classname)
    else {
        return result(Outcome::Cancelled, None, "spent weapon unavailable");
    };
    let weapon = s2sdk::EntHandleToEntPointer(handle);
    if weapon == 0 {
        return result(Outcome::Cancelled, None, "spent weapon entity unavailable");
    }
    let throw_animating = s2sdk::GetEntSchema2(weapon, &Str::from_str("CBaseCSGrenade"), &Str::from_str("m_bThrowAnimating"), 0);
    let redraw = s2sdk::GetEntSchema2(weapon, &Str::from_str("CBaseCSGrenade"), &Str::from_str("m_bRedraw"), 0);
    match grenade_redraw_ready(throw_animating, redraw) {
        Some(false) => return result(Outcome::Waiting, None, ""),
        None => {
            return result(
                Outcome::Failed(crate::fault::Scope::Mechanism(Mechanism::GrenadeRefill)),
                None,
                &format!("invalid native throw-redraw state throw_animating={throw_animating} redraw={redraw}"),
            );
        }
        Some(true) => {}
    }
    if !valid() {
        return result(Outcome::Cancelled, None, "task changed before removing spent weapon");
    }
    let active_before = if refill.diagnostics {
        s2sdk::GetClientActiveWeapon(player.slot)
    } else {
        -1
    };
    s2sdk::RemoveWeapon(player.slot, handle);
    if !valid() {
        return result(Outcome::Cancelled, None, "task changed during spent weapon removal");
    }
    if s2sdk::GetClientWeapons(player.slot).iter().any(|current| *current == handle) {
        return result(
            Outcome::Failed(crate::fault::Scope::Global),
            None,
            "spent item remained in inventory after removal",
        );
    }
    let inventory_before = s2sdk::GetClientWeapons(player.slot).iter().copied().collect::<Vec<_>>();
    let replacement = s2sdk::GiveNamedItem(player.slot, &Str::from_str(classname));
    let replacement_valid = s2sdk::IsValidEntHandle(replacement);
    let returned_class = replacement_valid.then(|| s2sdk::GetEntityClassname(replacement).to_string());
    let mut created = Vec::new();
    track_created_weapon(
        &mut created,
        &[(handle, classname.into())],
        &inventory_before,
        replacement,
        returned_class.as_deref(),
    );
    let created = created.first().map(|(handle, _)| *handle);
    if !valid() {
        return result(Outcome::Cancelled, created, "task changed during replacement grant");
    }
    if created != Some(replacement)
        || !s2sdk::GetClientWeapons(player.slot).iter().any(|current| *current == replacement)
        || returned_class.as_deref() != Some(classname)
    {
        return result(
            Outcome::Failed(crate::fault::Scope::Global),
            created,
            &format!("GiveNamedItem returned unusable handle={replacement}"),
        );
    }
    let active_after_give = s2sdk::GetClientActiveWeapon(player.slot);
    if active_after_give != replacement {
        s2sdk::SelectWeapon(player.slot, replacement);
    }
    if !valid() {
        return result(Outcome::Cancelled, created, "task changed during replacement selection");
    }
    let active_after = s2sdk::GetClientActiveWeapon(player.slot);
    if active_after != replacement {
        return result(
            Outcome::Failed(crate::fault::Scope::Mechanism(Mechanism::GrenadeRefill)),
            created,
            &format!("active weapon remained {active_after}"),
        );
    }
    if s2sdk::GetClientWeapons(player.slot).iter().any(|current| *current == handle) {
        return result(Outcome::Failed(crate::fault::Scope::Global), created, "spent item remained in inventory");
    }
    let replacement_weapon = s2sdk::EntHandleToEntPointer(replacement);
    if replacement_weapon == 0 {
        return result(Outcome::Failed(crate::fault::Scope::Global), created, "replacement entity unavailable");
    }
    if !refill.diagnostics {
        return result(Outcome::Completed, created, "");
    }
    let new_throw_animating = s2sdk::GetEntSchema2(
        replacement_weapon,
        &Str::from_str("CBaseCSGrenade"),
        &Str::from_str("m_bThrowAnimating"),
        0,
    );
    let new_redraw = s2sdk::GetEntSchema2(replacement_weapon, &Str::from_str("CBaseCSGrenade"), &Str::from_str("m_bRedraw"), 0);
    result(
        Outcome::Completed,
        created,
        &format!(
            "grenade replenished slot={} weapon={classname} old_handle={handle} new_handle={replacement} active_before={active_before} active_after_give={active_after_give} active_after={active_after} completed_throw_animating={throw_animating} completed_redraw={redraw} new_throw_animating={new_throw_animating} new_redraw={new_redraw} transaction={} action={}",
            player.slot, refill.id, refill.action_id,
        ),
    )
}

fn spent_grenade_owned(thrown: Option<i32>, mut inventory: impl Iterator<Item = i32>) -> Option<i32> {
    thrown.filter(|handle| inventory.any(|current| current == *handle))
}

fn grenade_redraw_ready(throw_animating: i64, redraw: i64) -> Option<bool> {
    match (throw_animating, redraw) {
        (0 | 1, 0 | 1) => Some(throw_animating == 0 && redraw == 1),
        _ => None,
    }
}

fn shared_item_definition(requested: &str) -> Option<u16> {
    match requested {
        "weapon_m4a1_silencer" => Some(60),
        "weapon_m4a1" => Some(16),
        "weapon_revolver" => Some(64),
        "weapon_deagle" => Some(1),
        "weapon_usp_silencer" => Some(61),
        "weapon_hkp2000" => Some(32),
        "weapon_mp5sd" => Some(23),
        "weapon_mp7" => Some(33),
        _ => None,
    }
}

fn item_identity_matches(requested: &str, returned_class: Option<&str>, actual: u16) -> bool {
    match shared_item_definition(requested) {
        Some(expected) => expected == actual,
        None => returned_class == Some(requested),
    }
}

pub(super) struct EquipmentReplacement {
    previous: Vec<(i32, String)>,
    created: Vec<(i32, String)>,
    retiring: Vec<i32>,
    pending_grant: Option<(i32, Vec<i32>)>,
}

#[derive(Debug, PartialEq, Eq)]
enum EquipmentGrantOutcome {
    InInventory,
    Duplicate,
    NotInInventory,
}

impl super::Effect {
    pub(super) fn replace_equipment_inner(&mut self, equipment: &[String], mode: &Mode) -> Result<(), Fault> {
        if LivePlayer::resolve(self.player.slot) != Some(self.player) {
            return Err(Fault::cancelled("player identity changed before replacing equipment"));
        }
        let previous = replacement_targets(
            s2sdk::GetClientWeapons(self.player.slot)
                .iter()
                .copied()
                .filter(|handle| s2sdk::IsValidEntHandle(*handle))
                .map(|handle| (handle, s2sdk::GetEntityClassname(handle).to_string())),
        );
        if self.replacement.is_some() {
            return Err(Fault::global("previous loadout replacement remains unsettled"));
        }
        self.replacement = Some(EquipmentReplacement {
            previous,
            created: Vec::new(),
            retiring: Vec::new(),
            pending_grant: None,
        });
        let previous = self.replacement.as_ref().expect("loadout transaction exists").previous.clone();
        for (handle, _) in previous {
            s2sdk::RemoveWeapon(self.player.slot, handle);
            if LivePlayer::resolve(self.player.slot) != Some(self.player) {
                return Err(Fault::cancelled("player changed during loadout removal"));
            }
            if s2sdk::GetClientWeapons(self.player.slot).iter().any(|current| *current == handle) {
                return Err("engine did not remove the previous loadout".into());
            }
        }
        for (equipment_index, requested) in equipment.iter().enumerate() {
            let name = team_weapon(requested, s2sdk::GetClientTeam(self.player.slot));
            let is_random_draw = mode.random_loadout && equipment_index < 2;
            let expected_definition = shared_item_definition(name).unwrap_or(0);
            let definition_reader = if is_random_draw && expected_definition != 0 {
                Some(
                    unsafe { __s2sdk_GetWeaponItemDefinition }
                        .ok_or_else(|| Fault::mechanism(Mechanism::RandomLoadout, "GetWeaponItemDefinition unavailable"))?,
                )
            } else {
                None
            };
            let inventory_before = s2sdk::GetClientWeapons(self.player.slot).iter().copied().collect::<Vec<_>>();
            let handle = s2sdk::GiveNamedItem(self.player.slot, &Str::from_str(name));
            let replacement = self.replacement.as_mut().expect("loadout transaction exists");
            replacement.pending_grant = Some((handle, inventory_before));
            let returned_class = replacement.account_grant();
            let handle_valid = returned_class.is_some();
            if LivePlayer::resolve(self.player.slot) != Some(self.player) {
                return Err(Fault::cancelled("player changed during loadout grant"));
            }
            let actual_definition = if handle_valid {
                definition_reader.map_or(0, |read| unsafe { read(handle) })
            } else {
                0
            };
            let grant_name = if is_random_draw {
                item_identity_matches(name, returned_class.as_deref(), actual_definition).then_some(name)
            } else {
                returned_class.as_deref()
            };
            let inventory = s2sdk::GetClientWeapons(self.player.slot);
            let handle_in_inventory = handle_valid && inventory.iter().any(|current| *current == handle);
            let matching_inventory_handle = inventory.iter().copied().find(|current| {
                s2sdk::IsValidEntHandle(*current)
                    && if is_random_draw {
                        let definition = definition_reader.map_or(0, |read| unsafe { read(*current) });
                        item_identity_matches(name, Some(&s2sdk::GetEntityClassname(*current).to_string()), definition)
                    } else {
                        s2sdk::GetEntityClassname(*current).to_string() == name
                    }
            });
            let outcome = equipment_grant_outcome(
                    is_random_draw,
                    handle_valid,
                    grant_name,
                    name,
                    handle_in_inventory,
                    matching_inventory_handle.is_some(),
                )
                .map_err(|reason| {
                    let current_weapons = inventory
                        .iter()
                        .copied()
                        .filter(|current| s2sdk::IsValidEntHandle(*current))
                        .map(|current| (current, s2sdk::GetEntityClassname(current).to_string()))
                        .collect::<Vec<_>>();
                    format!(
                        "loadout grant failed slot={} mode={} equipment[{equipment_index}] requested={name} handle={handle} returned_class={} expected_definition={expected_definition} actual_definition={actual_definition} current_weapons={current_weapons:?}: {reason}",
                        self.player.slot,
                        mode.id,
                        returned_class.as_deref().unwrap_or("<invalid-handle>"),
                    )
                })?;
            if self.diagnostics && (outcome != EquipmentGrantOutcome::InInventory || (is_random_draw && returned_class.as_deref() != Some(name))) {
                let current_weapons = inventory
                    .iter()
                    .copied()
                    .filter(|current| s2sdk::IsValidEntHandle(*current))
                    .map(|current| (current, s2sdk::GetEntityClassname(current).to_string()))
                    .collect::<Vec<_>>();
                s2sdk::PrintToServer(&Str::from_str(&format!(
                    "[FunRounds] random loadout grant slot={} mode={} draw={} requested={name} handle={handle} returned_class={} expected_definition={expected_definition} actual_definition={actual_definition} outcome={outcome:?} inventory={current_weapons:?}",
                    self.player.slot,
                    mode.id,
                    equipment_index + 1,
                    returned_class.as_deref().unwrap_or("<invalid-handle>"),
                )));
            }
            if mode.no_spread_recoil
                && is_firearm(name)
                && let Some(recoil_handle) = matching_inventory_handle.or_else(|| handle_in_inventory.then_some(handle))
            {
                self.capture_weapon_recoil(recoil_handle)?;
            }
        }
        Ok(())
    }

    pub(super) fn restore_failed_equipment(&mut self, refills: &mut Refills, restore_inventory: bool) -> Result<(), String> {
        let Some(replacement) = self.replacement.as_mut() else { return Ok(()) };
        replacement.account_grant();
        replacement.retire_created(refills);
        if !replacement.removals_complete(refills) {
            return Ok(());
        }
        if !restore_inventory || LivePlayer::resolve(self.player.slot) != Some(self.player) {
            self.replacement = None;
            return Ok(());
        }
        let result = replacement.restore_previous(self.player);
        replacement.retire_created(refills);
        if result.is_ok() && replacement.retiring.is_empty() {
            self.replacement = None;
        }
        result
    }
}

fn replaces_weapon(name: &str) -> bool {
    name != "weapon_c4"
}

fn replacement_targets(weapons: impl IntoIterator<Item = (i32, String)>) -> Vec<(i32, String)> {
    weapons.into_iter().filter(|(_, name)| replaces_weapon(name)).collect()
}

fn track_created_weapon(
    created: &mut Vec<(i32, String)>,
    previous: &[(i32, String)],
    inventory_before: &[i32],
    handle: i32,
    returned_class: Option<&str>,
) -> bool {
    let Some(classname) = returned_class else {
        return false;
    };
    if !classname.starts_with("weapon_")
        || !replaces_weapon(classname)
        || inventory_before.contains(&handle)
        || previous.iter().any(|(known, _)| *known == handle)
        || created.iter().any(|(known, _)| *known == handle)
    {
        return false;
    }
    created.push((handle, classname.to_string()));
    true
}

fn equipment_grant_outcome(
    random_draw: bool,
    handle_valid: bool,
    returned_class: Option<&str>,
    requested: &str,
    handle_in_inventory: bool,
    matching_inventory_item: bool,
) -> Result<EquipmentGrantOutcome, &'static str> {
    if random_draw {
        if !handle_valid {
            return if matching_inventory_item {
                Ok(EquipmentGrantOutcome::Duplicate)
            } else {
                Err("GiveNamedItem returned an invalid handle without a matching owned item")
            };
        }
        if returned_class != Some(requested) {
            return Err("GiveNamedItem returned a valid handle for a different item");
        }
        if handle_in_inventory {
            Ok(EquipmentGrantOutcome::InInventory)
        } else if matching_inventory_item {
            Ok(EquipmentGrantOutcome::Duplicate)
        } else {
            Ok(EquipmentGrantOutcome::NotInInventory)
        }
    } else if !handle_valid {
        Err("GiveNamedItem returned an invalid handle")
    } else if handle_in_inventory {
        Ok(EquipmentGrantOutcome::InInventory)
    } else {
        Err("engine did not immediately equip the returned item")
    }
}

impl EquipmentReplacement {
    fn account_grant(&mut self) -> Option<String> {
        self.account_grant_with(|handle| s2sdk::IsValidEntHandle(handle).then(|| s2sdk::GetEntityClassname(handle).to_string()))
    }

    fn account_grant_with(&mut self, classname: impl FnOnce(i32) -> Option<String>) -> Option<String> {
        let (handle, inventory_before) = self.pending_grant.as_ref()?;
        let returned_class = classname(*handle);
        track_created_weapon(&mut self.created, &self.previous, inventory_before, *handle, returned_class.as_deref());
        self.pending_grant = None;
        returned_class
    }

    fn retire_created(&mut self, refills: &mut Refills) {
        refills.retiring.extend(self.created.iter().map(|(handle, _)| *handle));
        self.retiring.extend(self.created.iter().map(|(handle, _)| *handle));
        self.created.clear();
    }

    fn removals_complete(&mut self, refills: &Refills) -> bool {
        self.retiring.retain(|handle| refills.retiring.contains(handle));
        self.retiring.is_empty()
    }

    fn restore_previous(&mut self, player: LivePlayer) -> Result<(), String> {
        while let Some((handle, name)) = self.previous.last().cloned() {
            if !s2sdk::GetClientWeapons(player.slot).iter().any(|current| *current == handle) {
                let inventory_before = s2sdk::GetClientWeapons(player.slot).iter().copied().collect::<Vec<_>>();
                let restored = s2sdk::GiveNamedItem(player.slot, &Str::from_str(&name));
                self.pending_grant = Some((restored, inventory_before));
                let classname = self.account_grant();
                if LivePlayer::resolve(player.slot) != Some(player) {
                    self.previous.clear();
                    return Ok(());
                }
                if classname.as_deref() != Some(&name) || !s2sdk::GetClientWeapons(player.slot).iter().any(|current| *current == restored) {
                    return Err(format!("loadout rollback failed for {name}"));
                }
                self.created.retain(|(created, _)| *created != restored);
            }
            self.previous.pop();
        }
        Ok(())
    }
}

fn team_weapon(name: &str, team: s2sdk::CSTeam) -> &str {
    if name == "weapon_knife" && team == s2sdk::CSTeam::T {
        "weapon_knife_t"
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use funrounds_core::{Capabilities, PlayerTracker, RoundController};

    #[test]
    fn failed_loadout_keeps_unverified_and_created_grants_through_panic_and_delayed_deletion() {
        let handle = 0x81003040u32 as i32;
        let mut replacement = EquipmentReplacement {
            previous: vec![(12, "weapon_ak47".into())],
            created: Vec::new(),
            retiring: Vec::new(),
            pending_grant: Some((handle, vec![11])),
        };
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                replacement.account_grant_with(|_| panic!("readback panicked after GiveNamedItem"));
            }))
            .is_err()
        );
        assert_eq!(replacement.pending_grant.as_ref().map(|grant| grant.0), Some(handle));
        replacement.account_grant_with(|_| Some("weapon_mag7".into()));
        assert_eq!(replacement.created, [(handle, "weapon_mag7".into())]);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                assert_eq!(replacement.created.len(), 1);
                panic!("later property application panicked");
            }))
            .is_err()
        );

        let mut refills = Refills::default();
        replacement.retire_created(&mut refills);
        assert!(!replacement.removals_complete(&refills));
        assert!(!refills.is_clean());
        let remove = refills.cleanup_actions_with(|candidate| candidate == handle)[0];
        refills.finish(remove, ActionResult::Removed(false)).unwrap();
        assert!(!replacement.removals_complete(&refills));
        assert_eq!(replacement.previous, [(12, "weapon_ak47".into())]);
        let retry = refills.cleanup_actions_with(|_| true)[0];
        refills.finish(retry, ActionResult::Removed(true)).unwrap();
        assert!(replacement.removals_complete(&refills));
        assert!(refills.is_clean());
    }

    #[test]
    fn failed_loadout_never_claims_c4_or_a_preexisting_grant() {
        let mut replacement = EquipmentReplacement {
            previous: vec![(12, "weapon_ak47".into())],
            created: Vec::new(),
            retiring: Vec::new(),
            pending_grant: Some((11, vec![11])),
        };
        replacement.account_grant_with(|_| Some("weapon_mag7".into()));
        replacement.pending_grant = Some((13, vec![11]));
        replacement.account_grant_with(|_| Some("weapon_c4".into()));
        let mut refills = Refills::default();
        replacement.retire_created(&mut refills);
        assert!(replacement.removals_complete(&refills));
        assert!(refills.is_clean());
    }

    #[test]
    fn cancelled_current_refill_preserves_global_failure_and_cleanup() {
        let (mut refills, _, _, id) = sample_refill();
        let action = refills.start(id).unwrap();
        refills.cancel_all();
        let fault = refills
            .finish(action, result(Outcome::Failed(crate::fault::Scope::Global), Some(1024)))
            .err()
            .unwrap();
        assert_eq!(fault.scope, crate::fault::Scope::Global);
        let mut faults = crate::fault::Faults::new(Capabilities::default());
        faults.record(&fault);
        assert_eq!(faults.recovery, crate::fault::Recovery::Stopped);
        assert_eq!(refills.retiring, HashSet::from([1024]));
        refills.cleanup_actions_with(|_| false);
        faults.finish_cleanup(refills.is_clean());
        assert_eq!(faults.recovery, crate::fault::Recovery::Stopped);
    }

    fn sample_refill() -> (Refills, PlayerTask, LivePlayer, u64) {
        let rounds = RoundController::new(None, Capabilities::default()).unwrap();
        let mut players = PlayerTracker::default();
        assert!(players.connect(1));
        let player = players.spawn(1, 1, 0x80001100).unwrap();
        let task = PlayerTask {
            round: rounds.generation(),
            player,
        };
        let live = LivePlayer {
            slot: 1,
            controller_handle: 0x2101,
            pawn_handle: player.pawn_handle as i32,
        };
        let mut refills = Refills::default();
        let id = refills.queue_recorded(task, live, GrenadeKind::He, Some(0x81001010_u32 as i32), true);
        (refills, task, live, id)
    }

    fn result(outcome: Outcome, created: Option<i32>) -> ActionResult {
        ActionResult::Refilled(RefillResult {
            outcome,
            created,
            diagnostic: "native result".into(),
        })
    }

    #[test]
    fn quiet_refills_commit_without_samples_and_still_report_failures() {
        let (_, task, live, _) = sample_refill();
        let mut refills = Refills::default();
        let id = refills.queue_recorded(task, live, GrenadeKind::He, Some(1024), false);
        let action = refills.start(id).unwrap();
        assert!(matches!(action, Action::Refill(Refill { diagnostics: false, .. })));
        let completion = refills
            .finish(
                action,
                ActionResult::Refilled(RefillResult {
                    outcome: Outcome::Completed,
                    created: Some(2048),
                    diagnostic: String::new(),
                }),
            )
            .unwrap();
        assert!(completion.diagnostic.is_none());
        assert!(refills.is_clean());
        let id = refills.queue_recorded(task, live, GrenadeKind::He, Some(2048), false);
        let action = refills.start(id).unwrap();
        let fault = refills
            .finish(action, result(Outcome::Failed(crate::fault::Scope::Global), None))
            .err()
            .unwrap();
        assert_eq!(fault.scope, crate::fault::Scope::Global);
        assert!(fault.to_string().contains("grenade refill"));
    }

    #[test]
    fn grenade_refill_waits_for_native_redraw_only() {
        for (throw_animating, redraw, expected) in [(0, 0, false), (1, 0, false), (1, 1, false), (0, 1, true)] {
            assert_eq!(grenade_redraw_ready(throw_animating, redraw), Some(expected));
        }
        for state in [(-1, 0), (0, 2), (2, 1), (1, -1)] {
            assert_eq!(grenade_redraw_ready(state.0, state.1), None);
        }
    }

    #[test]
    fn spent_grenade_requires_the_captured_full_handle() {
        let spent = 0x81001010_u32 as i32;
        assert_eq!(spent_grenade_owned(Some(spent), [19, spent, 23].into_iter()), Some(spent));
        assert_eq!(spent_grenade_owned(Some(spent), [0x01001010, 0x81009010_u32 as i32].into_iter()), None);
        assert_eq!(spent_grenade_owned(None, [spent].into_iter()), None);
    }

    #[test]
    fn repeated_events_and_waiting_results_preserve_one_transaction() {
        let (mut refills, task, live, id) = sample_refill();
        assert_eq!(refills.queue_recorded(task, live, GrenadeKind::He, Some(0x81001010_u32 as i32), true), id);
        let first = refills.start(id).unwrap();
        assert!(refills.start(id).is_none());
        let waiting = refills.finish(first, result(Outcome::Waiting, None)).unwrap();
        assert_eq!(waiting.retry, Some((task, id)));
        let next = refills.start(id).unwrap();
        assert_ne!(first, next);
        refills.cancel_action(first);
        assert!(refills.current(next));
        assert!(refills.finish(first, result(Outcome::Completed, Some(901))).unwrap().diagnostic.is_none());
        assert!(refills.current(next));
        assert!(refills.retiring.is_empty());
        refills.finish(next, result(Outcome::Completed, Some(902))).unwrap();
        assert!(refills.is_clean());
    }

    #[test]
    fn cancellation_before_dispatch_performs_no_native_work() {
        let (mut refills, _, _, id) = sample_refill();
        let action = refills.start(id).unwrap();
        refills.cancel(id);
        assert!(!refills.current(action));
        let result = execute(action, || false);
        let completion = refills.finish(action, result).unwrap();
        assert!(completion.diagnostic.unwrap().contains("player or task changed before refill"));
        assert!(refills.is_clean());
    }

    #[test]
    fn cancel_keeps_in_flight_grants_until_full_handle_invalidation() {
        let (mut refills, _, _, id) = sample_refill();
        let action = refills.start(id).unwrap();
        refills.cancel_all();
        assert!(!refills.is_clean());
        assert!(!refills.current(action));
        let created = 0x80001234_u32 as i32;
        refills.finish(action, result(Outcome::Completed, Some(created))).unwrap();
        assert!(!refills.is_clean());
        let removal = refills.cleanup_actions_with(|handle| handle == created);
        assert_eq!(removal.len(), 1);
        assert!(refills.cleanup_actions_with(|_| true).is_empty());
        refills.finish(removal[0], ActionResult::Removed(false)).unwrap();
        assert!(!refills.is_clean());
        let retry = refills.cleanup_actions_with(|_| true);
        assert_eq!(retry.len(), 1);
        assert_ne!(retry[0], removal[0]);
        refills.finish(removal[0], ActionResult::Removed(true)).unwrap();
        assert!(refills.current(retry[0]));
        assert!(!refills.is_clean());
        refills.finish(retry[0], ActionResult::Removed(true)).unwrap();
        assert!(refills.is_clean());
    }

    #[test]
    fn failed_or_cancelled_partial_grants_remain_owned() {
        for outcome in [Outcome::Failed(crate::fault::Scope::Global), Outcome::Cancelled] {
            let (mut refills, _, _, id) = sample_refill();
            let action = refills.start(id).unwrap();
            let failed = matches!(outcome, Outcome::Failed(_));
            assert_eq!(refills.finish(action, result(outcome, Some(1024))).is_err(), failed);
            assert_eq!(refills.retiring, HashSet::from([1024]));
            assert!(!refills.is_clean());
            assert!(refills.cleanup_actions_with(|_| false).is_empty());
            assert!(refills.is_clean());
            refills
                .finish(action, result(Outcome::Failed(crate::fault::Scope::Global), Some(2048)))
                .unwrap();
            assert!(refills.is_clean());
        }
    }

    #[test]
    fn successful_grants_are_committed_and_not_removed_by_round_cleanup() {
        let (mut refills, _, _, id) = sample_refill();
        let action = refills.start(id).unwrap();
        refills.finish(action, result(Outcome::Completed, Some(1024))).unwrap();
        refills.cancel_all();
        assert!(refills.cleanup_actions_with(|_| true).is_empty());
        assert!(refills.is_clean());
    }

    #[test]
    fn map_change_ignores_late_results_even_when_new_map_reuses_handle() {
        let (mut refills, task, live, id) = sample_refill();
        let old = refills.start(id).unwrap();
        refills
            .finish(old, result(Outcome::Failed(crate::fault::Scope::Global), Some(1024)))
            .err()
            .expect("current mechanism failure is reported");
        let old_removal = refills.cleanup_actions_with(|_| true)[0];
        refills.discard_map_state();
        assert!(refills.is_clean());
        let next_id = refills.queue_recorded(task, live, GrenadeKind::Decoy, Some(1024), true);
        let next = refills.start(next_id).unwrap();
        refills
            .finish(old, result(Outcome::Failed(crate::fault::Scope::Global), Some(1024)))
            .unwrap();
        refills.finish(old_removal, ActionResult::Removed(true)).unwrap();
        assert!(refills.current(next));
        assert!(refills.retiring.is_empty());
        refills
            .finish(next, result(Outcome::Failed(crate::fault::Scope::Global), Some(1024)))
            .err()
            .expect("current mechanism failure is reported");
        refills.finish(old_removal, ActionResult::Removed(true)).unwrap();
        assert_eq!(refills.retiring, HashSet::from([1024]));
    }

    #[test]
    fn player_cancellation_drops_waiting_work_and_keeps_in_flight_accounting() {
        let (mut refills, task, live, id) = sample_refill();
        let waiting = refills.queue_recorded(task, live, GrenadeKind::Decoy, Some(1024), true);
        let action = refills.start(id).unwrap();
        refills.cancel_player(live.slot);
        assert!(refills.start(waiting).is_none());
        assert!(!refills.current(action));
        assert!(!refills.is_clean());
        refills.finish(action, result(Outcome::Waiting, None)).unwrap();
        assert!(refills.is_clean());
    }

    #[test]
    fn generic_knife_follows_the_player_team_without_changing_other_equipment() {
        assert_eq!(super::team_weapon("weapon_knife", crate::s2sdk::CSTeam::T), "weapon_knife_t");
        assert_eq!(super::team_weapon("weapon_knife", crate::s2sdk::CSTeam::CT), "weapon_knife");
        assert_eq!(super::team_weapon("weapon_mag7", crate::s2sdk::CSTeam::T), "weapon_mag7");
    }
    #[test]
    fn rollback_attempts_equipment_steps_after_property_failure() {
        let mut attempted = Vec::new();
        let result = crate::effects::finish_rollback(
            ["properties", "remove new weapon", "restore old weapon", "restore another weapon"]
                .into_iter()
                .map(|step| {
                    attempted.push(step);
                    if step == "properties" || step == "restore old weapon" {
                        Err(step.to_owned())
                    } else {
                        Ok(())
                    }
                }),
        );
        assert_eq!(attempted.len(), 4);
        assert_eq!(result, Err("properties; restore old weapon".into()));
    }
    #[test]
    fn loadout_replacement_preserves_the_standard_bomb_objective() {
        let inventory = [
            (1, "weapon_c4".to_owned()),
            (2, "weapon_knife".to_owned()),
            (3, "weapon_ak47".to_owned()),
            (4, "weapon_glock".to_owned()),
            (5, "weapon_hegrenade".to_owned()),
        ];
        let removed = super::replacement_targets(inventory.clone());
        assert_eq!(removed, inventory[1..]);
        assert!(
            inventory
                .iter()
                .filter(|(handle, _)| !removed.iter().any(|(removed, _)| removed == handle))
                .any(|(_, name)| name == "weapon_c4")
        );
    }

    #[test]
    fn rollback_tracks_only_new_matching_non_c4_full_handles() {
        let previous = [(20, "weapon_ak47".to_owned())];
        let mut created = Vec::new();
        assert!(super::track_created_weapon(&mut created, &previous, &[30], 100, Some("weapon_mag7"),));
        assert!(!super::track_created_weapon(&mut created, &previous, &[30], 100, Some("weapon_mag7"),));
        assert!(!super::track_created_weapon(&mut created, &previous, &[30], 20, Some("weapon_ak47"),));
        assert!(!super::track_created_weapon(&mut created, &previous, &[30], 30, Some("weapon_mag7"),));
        assert!(super::track_created_weapon(&mut created, &previous, &[], 101, Some("weapon_negev"),));
        assert!(!super::track_created_weapon(&mut created, &previous, &[], 102, Some("weapon_c4"),));
        assert!(!super::track_created_weapon(&mut created, &previous, &[], 103, None));
        assert_eq!(created, [(100, "weapon_mag7".to_owned()), (101, "weapon_negev".to_owned())]);
    }

    #[test]
    fn random_item_identity_distinguishes_shared_weapon_classnames() {
        for (requested, classname, own, other) in [
            ("weapon_m4a1_silencer", "weapon_m4a1", 60, 16),
            ("weapon_revolver", "weapon_deagle", 64, 1),
            ("weapon_usp_silencer", "weapon_hkp2000", 61, 32),
            ("weapon_mp5sd", "weapon_mp7", 23, 33),
        ] {
            assert!(super::item_identity_matches(requested, Some(classname), own));
            assert!(!super::item_identity_matches(requested, Some(classname), other));
            assert!(!super::item_identity_matches(classname, Some(classname), own));
            assert!(!super::item_identity_matches(requested, Some(classname), 0));
        }
        assert!(super::item_identity_matches("weapon_ak47", Some("weapon_ak47"), 0));
        assert!(!super::item_identity_matches("weapon_ak47", Some("weapon_mag7"), 0));
    }

    #[test]
    fn random_draws_accept_only_valid_items_or_observable_duplicate_results() {
        assert_eq!(
            super::equipment_grant_outcome(true, true, Some("weapon_mag7"), "weapon_mag7", true, true),
            Ok(super::EquipmentGrantOutcome::InInventory),
        );
        assert_eq!(
            super::equipment_grant_outcome(true, true, Some("weapon_mag7"), "weapon_mag7", false, false),
            Ok(super::EquipmentGrantOutcome::NotInInventory),
        );
        assert_eq!(
            super::equipment_grant_outcome(true, false, None, "weapon_mag7", false, true),
            Ok(super::EquipmentGrantOutcome::Duplicate),
        );
        assert!(super::equipment_grant_outcome(true, false, None, "weapon_mag7", false, false).is_err());
        assert_eq!(
            super::equipment_grant_outcome(false, true, Some("weapon_mag7"), "weapon_mag7", true, true),
            Ok(super::EquipmentGrantOutcome::InInventory),
        );
        assert!(super::equipment_grant_outcome(false, false, None, "weapon_mag7", false, true).is_err());
        assert!(super::equipment_grant_outcome(false, true, Some("weapon_mag7"), "weapon_mag7", false, false).is_err());
        assert!(super::equipment_grant_outcome(true, true, Some("weapon_negev"), "weapon_mag7", false, true).is_err());
    }

    #[test]
    fn strict_loadouts_keep_the_legacy_t_knife_alias_contract() {
        assert_eq!(
            super::equipment_grant_outcome(false, true, Some("weapon_knife"), "weapon_knife_t", true, true),
            Ok(super::EquipmentGrantOutcome::InInventory),
        );
        assert!(super::equipment_grant_outcome(true, true, Some("weapon_knife"), "weapon_knife_t", true, true).is_err());
    }
}
