// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <cstdint>

#include <plugin_export.h>

// Ignites one exact live player pawn for `duration` seconds and returns the
// newly attached entityflame full handle. Returns -1 when validation or the
// engine effect check fails. This is a game-thread-only engine operation.
extern "C" PLUGIN_API std::int32_t TryIgnitePlayer(std::int32_t pawnHandle, float duration);
