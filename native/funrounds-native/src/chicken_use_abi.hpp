// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <cstddef>
#include <cstdint>
#include <type_traits>

namespace funrounds::native::detail::abi {

// Mirrors SourceSDK public/entity2/entityclass.h's EntityUseInput_t / USEPTR.
// Keep this small ABI view independent of the SDK headers for boundary tests.
enum class UseType : std::int32_t {
    Off = 0,
    On = 1,
    Set = 2,
    Toggle = 3,
};

struct EntityUseInput_t {
    void* m_pActivator{};
    void* m_pCaller{};
    UseType m_nType{};
    float m_flValue{};
};

using CChickenChickenUse = void (*)(void* chicken, const EntityUseInput_t* input);

static_assert(std::is_standard_layout_v<EntityUseInput_t>);
static_assert(offsetof(EntityUseInput_t, m_pActivator) == 0);
static_assert(offsetof(EntityUseInput_t, m_pCaller) == sizeof(void*));
static_assert(offsetof(EntityUseInput_t, m_nType) == sizeof(void*) * 2);
static_assert(offsetof(EntityUseInput_t, m_flValue) == sizeof(void*) * 2 + sizeof(std::int32_t));
static_assert(sizeof(EntityUseInput_t) == sizeof(void*) * 2 + sizeof(std::int32_t) + sizeof(float));
static_assert(std::is_same_v<CChickenChickenUse, void (*)(void*, const EntityUseInput_t*)>);

} // namespace funrounds::native::detail::abi
