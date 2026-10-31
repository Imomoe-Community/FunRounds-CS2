// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::s2sdk;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TransmitPair {
    pub recipient_slot: i32,
    pub entity_handle: i32,
}

impl TransmitPair {
    pub(crate) const fn new(recipient_slot: i32, entity_handle: i32) -> Self {
        Self {
            recipient_slot,
            entity_handle,
        }
    }
}

#[derive(Default, Debug)]
pub(crate) struct TransmitLease {
    owned: BTreeSet<TransmitPair>,
}

impl TransmitLease {
    // S2SDK exposes a shared hidden-pair set without owner counts. Claim only pairs
    // absent before this lease hides them; pre-existing entries stay untouched.
    pub(crate) fn sync(&mut self, desired: impl IntoIterator<Item = TransmitPair>) -> Result<(), String> {
        self.sync_with_changes(desired).map(|_| ())
    }

    // Return actual SDK hide/show requests so the Ghost caller can log transitions
    // (including SDK round-reset reclamation), without logging every frame.
    pub(crate) fn sync_with_changes(
        &mut self,
        desired: impl IntoIterator<Item = TransmitPair>,
    ) -> Result<(Vec<TransmitPair>, Vec<TransmitPair>), String> {
        let desired = desired.into_iter().collect::<BTreeSet<_>>();
        if desired.is_empty() && self.owned.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        let Some(get_hidden) = (unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_GetHiddenTransmitEntities).read() }) else {
            return Err("GetHiddenTransmitEntities import is unavailable".into());
        };
        let Some(hide) = (unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_HideTransmitEntity).read() }) else {
            return Err("HideTransmitEntity import is unavailable".into());
        };
        let Some(show) = (unsafe { std::ptr::addr_of!(s2sdk::__s2sdk_ShowTransmitEntity).read() }) else {
            return Err("ShowTransmitEntity import is unavailable".into());
        };

        let slots = desired
            .iter()
            .chain(self.owned.iter())
            .map(|pair| pair.recipient_slot)
            .collect::<BTreeSet<_>>();
        let mut hidden = BTreeMap::new();
        for slot in slots {
            hidden.insert(slot, unsafe { get_hidden(slot) }.iter().copied().collect::<BTreeSet<_>>());
        }

        let (to_hide, to_show) = reconcile(&mut self.owned, &desired, &mut hidden);
        for pair in &to_hide {
            unsafe { hide(pair.recipient_slot, pair.entity_handle) };
        }
        for pair in &to_show {
            unsafe { show(pair.recipient_slot, pair.entity_handle) };
        }
        Ok((to_hide, to_show))
    }

    pub(crate) fn clear(&mut self) -> Result<(), String> {
        self.sync(std::iter::empty())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.owned.is_empty()
    }
}

fn reconcile(
    owned: &mut BTreeSet<TransmitPair>,
    desired: &BTreeSet<TransmitPair>,
    hidden: &mut BTreeMap<i32, BTreeSet<i32>>,
) -> (Vec<TransmitPair>, Vec<TransmitPair>) {
    // The SDK clears its transmit list at round start. Reclaim our desired pairs
    // after that reset, but never claim a pair already hidden by another consumer.
    owned.retain(|pair| {
        hidden
            .get(&pair.recipient_slot)
            .is_some_and(|entities| entities.contains(&pair.entity_handle))
    });

    let to_show = owned.difference(desired).copied().collect::<Vec<_>>();
    for pair in &to_show {
        if let Some(entities) = hidden.get_mut(&pair.recipient_slot) {
            entities.remove(&pair.entity_handle);
        }
        owned.remove(pair);
    }

    let mut to_hide = Vec::new();
    let candidates = desired.difference(owned).copied().collect::<Vec<_>>();
    for pair in candidates {
        let was_hidden = hidden
            .get(&pair.recipient_slot)
            .is_some_and(|entities| entities.contains(&pair.entity_handle));
        if was_hidden {
            continue;
        }
        to_hide.push(pair);
        owned.insert(pair);
        hidden.entry(pair.recipient_slot).or_default().insert(pair.entity_handle);
    }
    (to_hide, to_show)
}
