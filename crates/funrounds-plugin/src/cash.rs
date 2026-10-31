// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{capability::Probe, game::LivePlayer, parameters, s2sdk};
use plugify::Str;

const MONEY_SERVICES: &str = "CCSPlayerController_InGameMoneyServices";
const MONEY_ACCOUNT: &str = "m_iAccount";
const MONEY_LIMIT: &str = "mp_maxmoney";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PurchaseItem {
    Chicken,
    Healthshot,
    C4,
}

impl PurchaseItem {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Chicken => "purchase.item.chicken",
            Self::Healthshot => "purchase.item.healthshot",
            Self::C4 => "purchase.item.c4",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RefundStatus {
    Refunded,
    OwnershipChanged,
    RefundFailed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PurchaseError {
    Insufficient { item: PurchaseItem, price: u32 },
    Debit(String),
    Rejected(&'static str),
    Delivery { detail: String, refund: RefundStatus },
}

pub fn probe() -> Probe {
    let mut probe = Probe::default();
    crate::capability::imports!(probe;
        __s2sdk_GetMaxClients, __s2sdk_IsClientInGame, __s2sdk_GetClientPawn,
        __s2sdk_PlayerSlotToEntHandle, __s2sdk_EntPointerToEntHandle, __s2sdk_IsValidEntHandle,
        __s2sdk_EntHandleToEntPointer, __s2sdk_GetEntSchema2, __s2sdk_SetEntSchema2,
    );
    probe.schema("CCSPlayerController", "m_pInGameMoneyServices");
    probe.schema(MONEY_SERVICES, MONEY_ACCOUNT);
    probe
}

pub fn reward_probe() -> Probe {
    let mut probe = probe();
    crate::capability::imports!(probe; __s2sdk_FindConVar, __s2sdk_GetConVarValue);
    let can_find_convars = unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_FindConVar).read().is_some() };
    probe.require(can_find_convars && parameters::has_convar(MONEY_LIMIT), "convar:mp_maxmoney");
    probe
}

#[derive(Clone, Copy)]
pub struct Account {
    slot: i32,
    controller_handle: i32,
    player: Option<LivePlayer>,
    services: usize,
}

pub struct CashDebit {
    account: Account,
    before: i64,
    after: i64,
}

impl CashDebit {
    /// Charge one cash-only purchase. The account retains the exact player identity for a possible refund.
    pub fn charge(player: LivePlayer, price: u32, item: PurchaseItem) -> Result<Self, PurchaseError> {
        let account = Account::resolve(player).map_err(PurchaseError::Debit)?;
        let before = account.balance().map_err(PurchaseError::Debit)?;
        if before < i64::from(price) {
            return Err(PurchaseError::Insufficient { item, price });
        }
        let after = before - i64::from(price);
        if after != before {
            account
                .write(after)
                .map_err(|error| PurchaseError::Debit(format!("cash debit failed: {error}")))?;
        }
        Ok(Self { account, before, after })
    }

    /// Refund only while the same buyer still owns the exact balance written by this purchase.
    pub fn refund_if_unchanged(self, same_buyer: impl FnOnce() -> bool) -> RefundStatus {
        if !same_buyer() {
            return RefundStatus::OwnershipChanged;
        }
        match self.account.balance() {
            Err(error) => RefundStatus::RefundFailed(error),
            Ok(balance) if balance != self.after => RefundStatus::OwnershipChanged,
            Ok(_) => match self.account.write(self.before) {
                Ok(()) => RefundStatus::Refunded,
                Err(error) => RefundStatus::RefundFailed(error),
            },
        }
    }
}

/// Reuse the exact cash ownership/refund contract for C4 and public purchases. `deliver` must
/// clean up any partially created engine item before returning an error.
pub(crate) fn purchase<T>(
    player: LivePlayer,
    price: u32,
    item: PurchaseItem,
    same_buyer: impl FnOnce() -> bool,
    deliver: impl FnOnce() -> Result<T, String>,
) -> Result<T, PurchaseError> {
    let debit = CashDebit::charge(player, price, item)?;
    match deliver() {
        Ok(value) => Ok(value),
        Err(detail) => Err(PurchaseError::Delivery {
            detail,
            refund: debit.refund_if_unchanged(same_buyer),
        }),
    }
}

impl Account {
    pub fn resolve(player: LivePlayer) -> Result<Self, String> {
        if LivePlayer::resolve_present(player.slot) != Some(player) {
            return Err("player identity changed before cash access".into());
        }
        let account = Self::resolve_slot(player.slot)?;
        if account.controller_handle != player.controller_handle || LivePlayer::resolve_present(player.slot) != Some(player) {
            return Err("player identity changed before cash access".into());
        }
        Ok(Self {
            player: Some(player),
            ..account
        })
    }

    pub fn resolve_slot(slot: i32) -> Result<Self, String> {
        if slot < 0 || slot >= s2sdk::GetMaxClients() || !s2sdk::IsClientInGame(slot) {
            return Err("player is not connected".into());
        }
        let controller_handle = s2sdk::PlayerSlotToEntHandle(slot);
        if !s2sdk::IsValidEntHandle(controller_handle) {
            return Err("player controller is unavailable".into());
        }
        let controller = s2sdk::EntHandleToEntPointer(controller_handle);
        if controller == 0 {
            return Err("player controller is unavailable".into());
        }
        let services = s2sdk::GetEntSchema2(
            controller,
            &Str::from_str("CCSPlayerController"),
            &Str::from_str("m_pInGameMoneyServices"),
            0,
        ) as usize;
        if services == 0 || !s2sdk::IsClientInGame(slot) || s2sdk::PlayerSlotToEntHandle(slot) != controller_handle {
            return Err("player cash service is unavailable".into());
        }
        Ok(Self {
            slot,
            controller_handle,
            player: None,
            services,
        })
    }

    fn current(self) -> bool {
        if self.slot < 0
            || self.slot >= s2sdk::GetMaxClients()
            || !s2sdk::IsClientInGame(self.slot)
            || s2sdk::PlayerSlotToEntHandle(self.slot) != self.controller_handle
            || self.player.is_some_and(|player| LivePlayer::resolve_present(self.slot) != Some(player))
        {
            return false;
        }
        let controller = s2sdk::EntHandleToEntPointer(self.controller_handle);
        controller != 0
            && s2sdk::GetEntSchema2(
                controller,
                &Str::from_str("CCSPlayerController"),
                &Str::from_str("m_pInGameMoneyServices"),
                0,
            ) as usize
                == self.services
    }

    pub fn balance(self) -> Result<i64, String> {
        if !self.current() {
            return Err("player identity changed during cash access".into());
        }
        let balance = s2sdk::GetEntSchema2(self.services, &Str::from_str(MONEY_SERVICES), &Str::from_str(MONEY_ACCOUNT), 0);
        self.current()
            .then_some(balance)
            .ok_or_else(|| "player identity changed during cash access".into())
    }

    pub fn write(self, amount: i64) -> Result<(), String> {
        if !self.current() {
            return Err("player identity changed before cash update".into());
        }
        s2sdk::SetEntSchema2(
            self.services,
            &Str::from_str(MONEY_SERVICES),
            &Str::from_str(MONEY_ACCOUNT),
            amount,
            true,
            0,
        );
        if self.current() && self.balance()? == amount {
            Ok(())
        } else {
            Err("cash update readback failed".into())
        }
    }
}

fn limit() -> Result<i64, String> {
    let handle = s2sdk::FindConVar(&Str::from_str(MONEY_LIMIT));
    if handle as u16 == u16::MAX {
        return Err("mp_maxmoney is unavailable".into());
    }
    parse_limit(&s2sdk::GetConVarValue(handle).to_string()).map_err(str::to_owned)
}

fn parse_limit(value: &str) -> Result<i64, &'static str> {
    let value = value.parse::<f64>().map_err(|_| "mp_maxmoney is not numeric")?;
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > i64::MAX as f64 {
        return Err("mp_maxmoney must be a nonnegative integer");
    }
    Ok(value as i64)
}

pub fn raise_to_floor(slot: i32, floor: u32) -> Result<(), String> {
    let account = Account::resolve_slot(slot)?;
    let current = account.balance()?;
    let next = funrounds_core::money_floor(current, floor, limit()?);
    if next != current {
        account.write(next)?;
    }
    Ok(())
}

pub fn grant_income(player: LivePlayer, amount: u32) -> Result<(), String> {
    let account = Account::resolve(player)?;
    let current = account.balance()?;
    let next = funrounds_core::money_income_credit(current, amount, limit()?);
    if next != current {
        account.write(next)?;
    }
    Ok(())
}
