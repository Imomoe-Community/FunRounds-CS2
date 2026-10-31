// Copyright (C) 2026 Mai Ooizumi
// SPDX-License-Identifier: AGPL-3.0-or-later

#pragma once

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <string_view>

namespace funrounds::native::ghost_passive_sound {

struct SoundEventName {
    std::string_view name;
    bool suppressForHiddenGhost{};
};

// Current Source 2 passive movement event family from the paired 1.41.9.3
// sound-event assets. Hashes are resolved from SoundOpSystem001 at runtime;
// names are retained here so a missing or changed event disables this feature
// instead of silently leaving a partially filtered family.
inline constexpr std::array<SoundEventName, 158> kSoundEventNames{{
    SoundEventName{"Base.Footstep", true},
    SoundEventName{"Base.Land", true},
    SoundEventName{"CT_Concrete.StepLeft", true},
    SoundEventName{"Land_Concrete.StepLeft", true},
    SoundEventName{"CT_Ceiling_Tile.StepLeft", true},
    SoundEventName{"Land_Ceiling_Tile.StepLeft", true},
    SoundEventName{"CT_Default.StepLeft", true},
    SoundEventName{"Land_Default.StepLeft", true},
    SoundEventName{"CT_Wood.StepLeft", true},
    SoundEventName{"Land_Wood.StepLeft", true},
    SoundEventName{"CT_Wood_Solid.StepLeft", true},
    SoundEventName{"Land_Wood_Solid.StepLeft", true},
    SoundEventName{"CT_SolidMetal.StepLeft", true},
    SoundEventName{"Land_SolidMetal.StepLeft", true},
    SoundEventName{"CT_ChainLink.StepLeft", true},
    SoundEventName{"Land_ChainLink.StepLeft", true},
    SoundEventName{"CT_MetalGrate.StepLeft", true},
    SoundEventName{"Land_MetalGrate.StepLeft", true},
    SoundEventName{"CT_Ladder.StepLeft", true},
    SoundEventName{"Land_Ladder.StepLeft", true},
    SoundEventName{"CT_MetalVent.StepLeft", true},
    SoundEventName{"Land_MetalVent.StepLeft", true},
    SoundEventName{"CT_MetalVehicle.StepLeft", true},
    SoundEventName{"Land_MetalVehicle.StepLeft", true},
    SoundEventName{"CT_MetalBarrel.StepLeft", true},
    SoundEventName{"Land_MetalBarrel.StepLeft", true},
    SoundEventName{"CT_Snow.StepLeft", true},
    SoundEventName{"Land_Snow.StepLeft", true},
    SoundEventName{"CT_Dirt.StepLeft", true},
    SoundEventName{"Land_Dirt.StepLeft", true},
    SoundEventName{"CT_Mud.StepLeft", true},
    SoundEventName{"Land_Mud.StepLeft", true},
    SoundEventName{"CT_Slime.StepLeft", true},
    SoundEventName{"Land_Slime.StepLeft", true},
    SoundEventName{"CT_Grass.StepLeft", true},
    SoundEventName{"Land_Grass.StepLeft", true},
    SoundEventName{"CT_Tile.StepLeft", true},
    SoundEventName{"Land_Tile.StepLeft", true},
    SoundEventName{"CT_Water.StepLeft", true},
    SoundEventName{"ct_wade.StepLeft", true},
    SoundEventName{"t_wade.StepLeft", true},
    SoundEventName{"CT_Wood_Basket.StepLeft", true},
    SoundEventName{"Land_Wood_Basket.StepLeft", true},
    SoundEventName{"CT_Glass.StepLeft", true},
    SoundEventName{"Land_Glass.StepLeft", true},
    SoundEventName{"CT_Gravel.StepLeft", true},
    SoundEventName{"Land_Gravel.StepLeft", true},
    SoundEventName{"CT_Flesh.StepLeft", true},
    SoundEventName{"Land_Flesh.StepLeft", true},
    SoundEventName{"CT_Sand.StepLeft", true},
    SoundEventName{"Land_Sand.StepLeft", true},
    SoundEventName{"CT_GlassBottle.StepLeft", true},
    SoundEventName{"Land_GlassBottle.StepLeft", true},
    SoundEventName{"CT_Drywall.StepLeft", true},
    SoundEventName{"Land_Drywall.StepLeft", true},
    SoundEventName{"CT_Cardboard.StepLeft", true},
    SoundEventName{"Land_Cardboard.StepLeft", true},
    SoundEventName{"CT_Plastic_Barrel.StepLeft", true},
    SoundEventName{"CT_Plastic_Box.StepLeft", true},
    SoundEventName{"Land_Plastic_Box.StepLeft", true},
    SoundEventName{"CT_Rubber.StepLeft", true},
    SoundEventName{"Land_Rubber.StepLeft", true},
    SoundEventName{"CT_Grenade.StepLeft", true},
    SoundEventName{"Land_Grenade.StepLeft", true},
    SoundEventName{"CT_WoodBox.StepLeft", true},
    SoundEventName{"Land_WoodBox.StepLeft", true},
    SoundEventName{"CT_Wood_Crate.StepLeft", true},
    SoundEventName{"Land_Wood_Crate.StepLeft", true},
    SoundEventName{"CT_Wood_Panel.StepLeft", true},
    SoundEventName{"Land_Wood_Panel.StepLeft", true},
    SoundEventName{"CT_Weapon.StepLeft", true},
    SoundEventName{"Land_Weapon.StepLeft", true},
    SoundEventName{"CT_Foliage.StepLeft", true},
    SoundEventName{"Land_Foliage.StepLeft", true},
    SoundEventName{"CT_Carpet.StepLeft", true},
    SoundEventName{"Land_Carpet.StepLeft", true},
    SoundEventName{"Heavy.Step", true},
    SoundEventName{"CT_SlipperySlime.StepLeft", true},
    SoundEventName{"Land_SlipperySlime.StepLeft", true},
    SoundEventName{"CT_Silent.StepLeft", true},
    SoundEventName{"Land_Silent.StepLeft", true},
    SoundEventName{"T_Cardboard.StepLeft", true},
    SoundEventName{"T_Carpet.StepLeft", true},
    SoundEventName{"T_Ceiling_Tile.StepLeft", true},
    SoundEventName{"T_ChainLink.StepLeft", true},
    SoundEventName{"T_Concrete.StepLeft", true},
    SoundEventName{"T_Default.StepLeft", true},
    SoundEventName{"T_Dirt.StepLeft", true},
    SoundEventName{"T_Drywall.StepLeft", true},
    SoundEventName{"T_Flesh.StepLeft", true},
    SoundEventName{"T_Foliage.StepLeft", true},
    SoundEventName{"T_Glass.StepLeft", true},
    SoundEventName{"T_GlassBottle.StepLeft", true},
    SoundEventName{"T_Grass.StepLeft", true},
    SoundEventName{"T_Gravel.StepLeft", true},
    SoundEventName{"T_Grenade.StepLeft", true},
    SoundEventName{"T_Ladder.StepLeft", true},
    SoundEventName{"T_MetalBarrel.StepLeft", true},
    SoundEventName{"T_MetalGrate.StepLeft", true},
    SoundEventName{"T_MetalVehicle.StepLeft", true},
    SoundEventName{"T_MetalVent.StepLeft", true},
    SoundEventName{"T_Mud.StepLeft", true},
    SoundEventName{"T_Plastic_Barrel.StepLeft", true},
    SoundEventName{"T_Plastic_Box.StepLeft", true},
    SoundEventName{"T_Rubber.StepLeft", true},
    SoundEventName{"T_Sand.StepLeft", true},
    SoundEventName{"T_Silent.StepLeft", true},
    SoundEventName{"T_Slime.StepLeft", true},
    SoundEventName{"T_SlipperySlime.StepLeft", true},
    SoundEventName{"T_Snow.StepLeft", true},
    SoundEventName{"T_SolidMetal.StepLeft", true},
    SoundEventName{"T_Tile.StepLeft", true},
    SoundEventName{"T_Water.StepLeft", true},
    SoundEventName{"T_Weapon.StepLeft", true},
    SoundEventName{"T_Wood.StepLeft", true},
    SoundEventName{"T_WoodBox.StepLeft", true},
    SoundEventName{"T_Wood_Basket.StepLeft", true},
    SoundEventName{"T_Wood_Crate.StepLeft", true},
    SoundEventName{"T_Wood_Panel.StepLeft", true},
    SoundEventName{"T_Wood_Solid.StepLeft", true},
    SoundEventName{"CT_WetTile.StepLeft", true},
    SoundEventName{"T_WetTile.StepLeft", true},
    SoundEventName{"Land_Wet.Splash", true},
    SoundEventName{"WetTile.Splash", true},
    SoundEventName{"Land_WetTile.StepLeft", true},
    SoundEventName{"Player.Wade", true},
    SoundEventName{"Land_PlayerFlesh.StepLeft", true},
    SoundEventName{"T_Wood_Ladder.StepLeft", true},
    SoundEventName{"Land_Wood_Ladder.StepLeft", true},
    SoundEventName{"CT_Wood_Ladder.StepLeft", true},
    SoundEventName{"Land_WaterVol.StepLeft", true},
    SoundEventName{"T_Wet_Sand.StepLeft", true},
    SoundEventName{"CT_Wet_Sand.StepLeft", true},
    SoundEventName{"Land_Plastic_Barrel.StepLeft", true},
    SoundEventName{"CT_Plastic_milkCrate.StepLeft", true},
    SoundEventName{"T_Plastic_milkCrate.StepLeft", true},
    SoundEventName{"Land_Plastic_milkCrate.StepLeft", true},
    SoundEventName{"CT_Plastic_autoCover.StepLeft", true},
    SoundEventName{"Land_Plastic_autoCover.StepLeft", true},
    SoundEventName{"T_Plastic_autoCover.StepLeft", true},
    SoundEventName{"Land_MetalRailing.StepLeft", true},
    SoundEventName{"CT_MetalRailing.StepLeft", true},
    SoundEventName{"T_MetalRailing.StepLeft", true},
    SoundEventName{"Gear.CT", true},
    SoundEventName{"Land.Thud", true},
    SoundEventName{"Gear.JumpLand.CT", true},
    SoundEventName{"Gear.T", true},
    SoundEventName{"Gear.JumpLand.T", true},
    SoundEventName{"CT_WetConcrete.StepLeft", true},
    SoundEventName{"T_WetConcrete.StepLeft", true},
    SoundEventName{"CT_WetMud.StepLeft", true},
    SoundEventName{"T_WetMud.StepLeft", true},
    SoundEventName{"Land_WetConcrete.StepLeft", true},
    SoundEventName{"Land_WetMud.StepLeft", true},
    SoundEventName{"WetPuddle.Splash", true},
    SoundEventName{"Player.GhostIdle", true},
    SoundEventName{"Default.WalkJump", true},
    SoundEventName{"Hostage.Breath", false},
}};

inline constexpr std::size_t kPassiveSoundEventCount = 157;

constexpr const SoundEventName* FindSoundEventName(std::string_view name) noexcept {
    const auto found = std::find_if(kSoundEventNames.begin(), kSoundEventNames.end(), [name](const auto& event) {
        return event.name == name;
    });
    return found == kSoundEventNames.end() ? nullptr : &*found;
}

constexpr bool IsPassiveSoundEventName(std::string_view name) noexcept {
    const auto* const event = FindSoundEventName(name);
    return event != nullptr && event->suppressForHiddenGhost;
}

inline bool IsKnownPassiveSoundEvent(std::uint32_t soundEventHash,
                                     std::span<const std::uint32_t> passiveSoundEventHashes) noexcept {
    return std::binary_search(passiveSoundEventHashes.begin(), passiveSoundEventHashes.end(), soundEventHash);
}

} // namespace funrounds::native::ghost_passive_sound
