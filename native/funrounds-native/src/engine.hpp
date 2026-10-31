// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <cstddef>
#include <cstdint>
#include <string_view>

#include <schemasystem/schemasystem.h>

#include <s2sdk/s2sdk.hpp>

namespace funrounds::native::detail {

struct SchemaField {
    std::int32_t offset;
    std::size_t size;
};

bool EnsureEngine();
void ShutdownEngine() noexcept;
s2sdk::ConfigId GameConfig() noexcept;
ISchemaSystemTypeScope* ServerSchema() noexcept;
bool ResolveSchemaField(const char* className, const char* fieldName, std::size_t expectedSize, SchemaField& field) noexcept;

void* EntityFromHandle(std::int32_t handle) noexcept;
std::int32_t HandleFromEntity(void* entity) noexcept;
bool IsPlayerPawn(std::int32_t handle) noexcept;
bool IsPlayerController(std::int32_t handle) noexcept;

} // namespace funrounds::native::detail
