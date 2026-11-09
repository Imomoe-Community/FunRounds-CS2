// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#include <cstddef>
#include <cstdint>
#include <cstring>

#include <const.h>

#include <plugin_export.h>
#include <s2sdk/s2sdk.hpp>

#include "collision.hpp"
#include "engine.hpp"

namespace funrounds::native::detail {
namespace {

struct CollisionSchema {
    SchemaField entityCollision{};
    SchemaField collisionAttribute{};
    SchemaField attributeGroup{};
    SchemaField collisionGroup{};
    bool ready{};
};

CollisionSchema g_collisionSchema;

bool ResolveCollisionSchema() noexcept {
    if (g_collisionSchema.ready) {
        return true;
    }

    CollisionSchema resolved{};
    if (!ResolveSchemaField("CBaseEntity", "m_pCollision", sizeof(void*), resolved.entityCollision) ||
        !ResolveSchemaField("CCollisionProperty", "m_collisionAttribute", 0, resolved.collisionAttribute) ||
        !ResolveSchemaField("VPhysicsCollisionAttribute_t", "m_nCollisionGroup", sizeof(std::uint8_t), resolved.attributeGroup) ||
        !ResolveSchemaField("CCollisionProperty", "m_CollisionGroup", sizeof(std::uint8_t), resolved.collisionGroup)) {
        return false;
    }

    resolved.ready = true;
    g_collisionSchema = resolved;
    return true;
}

template <typename T>
void WriteField(void* destination, std::int32_t offset, const T& value) noexcept {
    std::memcpy(static_cast<std::byte*>(destination) + offset, &value, sizeof(value));
}

} // namespace

void ResetCollisionSchema() noexcept {
    g_collisionSchema = {};
}

} // namespace funrounds::native::detail

extern "C" PLUGIN_API void SetEntityDefaultCollisionGroup(std::int32_t entityHandle) {
    try {
        using namespace funrounds::native::detail;
        if (entityHandle == -1 || !EnsureEngine()) {
            return;
        }
        void* entity = EntityFromHandle(entityHandle);
        if (!entity || HandleFromEntity(entity) != entityHandle) {
            return;
        }

        if (!ResolveCollisionSchema()) {
            return;
        }

        const auto methodOffset = s2sdk::GetGameConfigOffset(GameConfig(), "CBaseEntity::CollisionRulesChanged");
        if (methodOffset < 0) {
            return;
        }
        auto** vtable = *reinterpret_cast<void***>(entity);
        if (!vtable || !vtable[methodOffset]) {
            return;
        }

        void* property = nullptr;
        std::memcpy(&property, static_cast<std::byte*>(entity) + g_collisionSchema.entityCollision.offset, sizeof(property));
        if (!property) {
            return;
        }

        constexpr auto defaultGroup = static_cast<std::uint8_t>(COLLISION_GROUP_DEFAULT);
        WriteField(static_cast<std::byte*>(property) + g_collisionSchema.collisionAttribute.offset,
                   g_collisionSchema.attributeGroup.offset,
                   defaultGroup);
        WriteField(property, g_collisionSchema.collisionGroup.offset, defaultGroup);
        reinterpret_cast<void (*)(void*)>(vtable[methodOffset])(entity);
    } catch (...) {
    }
}
