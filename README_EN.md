<div align="center">

# FunRounds-CS2

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/Version-0.4.0--alpha-informational.svg)](Cargo.toml)
[![Rust Version](https://img.shields.io/badge/Rust-1.99.1+-orange.svg)](rust-toolchain.toml)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20x64-lightgrey.svg)]()
[![Framework](https://img.shields.io/badge/Framework-Plugify%20%2B%20S2SDK%203.3.1-success.svg)](https://github.com/untrustedmodders/plugify)

**A high-performance, safe, and modular special rounds plugin for Counter-Strike 2 dedicated servers.**

[English](README_EN.md) • [简体中文](README_ZH.md)

</div>

---

### Overview

**FunRounds-CS2** is a modern Counter-Strike 2 server plugin developed in Rust and native C++20, built atop the [Plugify](https://github.com/untrustedmodders/plugify) framework and [S2SDK](https://github.com/Mai-Ooizumi/plugify-plugin-s2sdk/tree/funrounds).

At the start of each round (or on administrative demand), it seamlessly applies multi-dimensional gameplay mutators—altering player attributes, mobility physics, dynamic loadouts, recoil/camera mechanics, combat rules, chicken ecology, and bomb systems—with zero-leak state restoration and crash isolation.

---

### Key Features

| Category | Description |
| :--- | :--- |
| **Attributes & Physics** | Health (up to 10,000 or random range), armor/helmets (armor up to 1,000), gravity scaling, speed multiplier, friction, auto-bunnyhopping, ground acceleration, ground reverse walking (`ground_reverse`), time scale (`time_scale`), and periodic speed/gravity oscillation. |
| **Movement & Mobility** | Air-strafe directional acceleration assist (`autostrafe`), wall hang upon contact (`wall_hang`), grapple hook pulling to target (`grapple`), and map-specific trampoline launch zones (`trampoline`). |
| **Visuals & Camera** | Third person camera (`third_person`), stealth ghost mode with breathing audio cues and distance awareness (`ghost`), dynamic KD-scaled big head mode (`big_head`), night vision (`fr_nvg`), crosshair/radar hiding (`hide_hud`), model RGB tinting, translucency, and scope restrictions. |
| **Recoil & Knockback** | Weapon recoil self-push knockback (`fr_knock` supports ground and air modes), recoil & spread suppression, weapon drop/switch on fire (`weapon_on_fire`), explosive bullets (`explosive_bullets`), view-punch intensity/decay, and fast switch (`fast_switch`). |
| **Loadouts & Weapons** | Canonical weapon IDs, named weapon groups (`[weapon_groups]` referenced via `@group`), team-specific weapon clearing and granting, 4 ammo modes (inherit / reserve / clip / single-shot), taser recharge intervals, kill progression (`kill_loadout`), and random draws. |
| **Grenades & Tactical** | Automatic grenade replenishment, proximity fuse detonation (`proximity_fuse`), guided HE tracking (`he_guidance`), HE blast impulse (`hegrenade_impulse`), medical smoke healing (`medical_smoke`), decoy dodgeball collisions (`dodgeball`), throwing knives, and extended flash duration (`long_flash`). |
| **Combat & Hazards** | Vampiric lifesteal, reflect damage (`reflect_damage`), dynamic HP curves, friendly fire healing (`friendly_fire_heal`), passive regeneration (`regeneration`), low-speed penalty (`dont_stop`), hot floor lava damage (`hot_floor`), death explosion (`death_explosion`), and teamkill limits. |
| **C4 Objectives & Chickens** | 3 bomb types (native, chicken, carry), custom timers, bomb bouncing, 4-color wire cutting minigame (`fr_wire`), C4 cash purchasing (`fr_c4`), and C4 suicide explosions (`fr_suicide`); chicken population scaling, eating chickens with digestion risk, chicken healing aura (`chicken_aura`), and healthshots. |

<details>
<summary><b>Click to expand full gameplay feature details</b></summary>

- **Canonical Equipment & Weapon Groups**
  - **Canonical Weapon IDs**: Fully transitions to standard IDs without the legacy `weapon_` prefix (e.g. `famas`, `p250`, `hegrenade`, `flashbang`, `smokegrenade`, `decoy`, `molotov`, `incgrenade`).
  - **Named Weapon Groups**: Supports top-level `[weapon_groups]` definitions referenced succinctly as `@group_name` across modes.
  - **Expanded Armor Cap**: Initial armor can be configured up to `1000` points.

- **Movement & Mobility**
  - **Air-Strafe Assist (`autostrafe`)**: Automatically aligns horizontal velocity with viewing angles to provide smooth aerial acceleration.
  - **Wall Hang (`wall_hang`)**: Suspends players on vertical surfaces while touching walls and holding the jump key.
  - **Grapple Hook (`grapple`)**: Shoots a grappling tether towards surfaces upon pressing the use key, pulling the player at configured speeds.
  - **Map Trampolines (`trampoline`)**: Designated geographic trigger areas (e.g. Dust2, Mirage) launching players upwards with high vertical velocity.

- **Weaponry & Combat Mutators**
  - **Explosive Bullets (`explosive_bullets`)**: Bullet impacts detonate with configurable cooldowns, radius, and damage ranges.
  - **Weapon Fire Mishap (`weapon_on_fire`)**: Firing carries a randomized chance to forcefully switch to the next weapon or drop the active weapon.
  - **Big Head (`big_head`)**: Scales head hitbox dimensions dynamically according to the player's live kill-to-death ratio (KD).
  - **Friendly Fire Healing (`friendly_fire_heal`)**: Converts friendly fire into healing via fixed values or proportional damage ratios.
  - **Passive Regeneration (`regeneration`)**: Passively regenerates health after a period of staying out of combat.

- **Advanced Grenades**
  - **Proximity Fuse (`proximity_fuse`)**: Grenades detonate early upon detecting nearby enemy players within a specified trigger radius.
  - **Guided HE Tracking (`he_guidance`)**: High explosive grenades steer toward nearby enemies in flight at a configured turn rate.
  - **HE Blast Impulse (`hegrenade_impulse`)**: High explosive grenades apply customizable directional physical impulse and knockback.
  - **Medical Smoke (`medical_smoke`)**: Smoke clouds grant periodic healing to allied players inside the active cloud.
  - **Extended Flash (`long_flash`)**: Multiplies flashbang blind durations up to a configured ceiling cap.

- **Environmental Hazards & Rules**
  - **Low-Speed Hazard (`dont_stop`)**: Inflicts periodic health penalties if a player's horizontal speed stays below a minimum threshold past the grace period.
  - **Hot Floor Lava (`hot_floor`)**: Standing continuously on the ground burns the player with damage, eventually igniting them.
  - **Death Explosion (`death_explosion`)**: Designates a quota of players per team to trigger a massive explosion upon dying.
  - **Teamkill Limit (`teamkill_limit`)**: Enforces a strict threshold on teamkills per round before punishing the offender.
  - **Periodic Parameter Waves (`periodic_speed` / `periodic_gravity`)**: Randomly mutates gravity and speed multipliers at periodic intervals using discrete values or continuous ranges.

</details>

<details>
<summary><b>Click to expand Architecture & Reliability</b></summary>

- **Three-Tier Architecture**:
  - `funrounds-core`: Pure, engine-independent Rust library handling state machines, round queues, math, rules, `[defaults]` template inheritance, and strictly validated TOML configurations.
  - `funrounds-plugin`: Native Plugify Rust plugin interacting safely with the CS2 engine on the game thread via S2SDK.
  - `funrounds-native`: Native C++20 companion module handling low-level engine hooks, entity collision, projectile touch detection, chicken interactions, ghost radar occlusion, and gamedata offsets.
- **Strict Fault Isolation**:
  - **FFI Panic Containment**: All engine callbacks wrap in `catch_unwind` at the C ABI boundary to prevent server crashes.
  - **Transactional State Restoration**: Tracks player/round generations to guarantee only hijacked properties and ConVars are restored without corrupting native server settings.
  - **Dynamic Capability Quarantine**: Automatically excludes modes requiring missing SDK hooks or engine features without impacting other modes.
</details>

---

### Project Layout

```text
├── bindings/             # Plugify S2SDK provider bindings (164 methods)
├── config/               # Reference configurations and i18n catalogs
│   ├── funrounds.example.toml # Reference configuration
│   └── i18n/             # Localization files (zh-CN.toml, en.toml)
├── crates/
│   ├── funrounds-core/   # Engine-independent gameplay logic, rules & config parser
│   └── funrounds-plugin/ # Native Plugify runtime plugin implementation
├── native/
│   └── funrounds-native/ # Native C++ companion (engine hooks, collision, gamedata)
├── packaging/            # Plugin package manifest (funrounds.pplugin)
├── scripts/              # Native build and CI automation scripts
└── rust-toolchain.toml   # Fixed toolchain configuration (Rust 1.99.1)
```

---

### Installation

#### Prerequisites

- Counter-Strike 2 Dedicated Server (Windows x64 or Linux x64)
- [Plugify](https://github.com/untrustedmodders/plugify) installed and active on your CS2 server
- [S2SDK](https://github.com/Mai-Ooizumi/plugify-plugin-s2sdk/tree/funrounds) plugin (version `=3.3.1`) active in Plugify

#### Deployment

Copy compiled binaries and configurations to the server's `game/csgo/addons/plugify/` directory:

```text
game/csgo/addons/plugify/
├── configs/
│   ├── funrounds.toml                              # Copied from config/funrounds.example.toml
│   └── funrounds/
│       └── i18n/                                   # Copy catalogs from config/i18n/
│           ├── zh-CN.toml
│           └── en.toml
└── envs/
    ├── funrounds/                                  # Main Rust Plugin
    │   ├── bin/
    │   │   └── funrounds_plugin.dll (or .so)       # Plugin binary
    │   └── funrounds.pplugin                       # Manifest
    └── funrounds_native/                           # Native C++ Companion
        ├── bin/
        │   └── funrounds-native.dll (or lib*.so)   # Native binary
        ├── funrounds-native.pplugin                # Native manifest
        └── gamedata/
            └── funrounds-native.jsonc              # Gamedata offsets
```

---

### Commands Reference

#### Server & Administrator Commands (Console / RCON)

| Command | Arguments | Description |
| :--- | :--- | :--- |
| `fr_status` | None | Displays current mode, phase, capability readiness, and fault diagnoses. |
| `fr_list` | None | Lists all loaded and enabled round modes in the pool. |
| `fr_next` | `<id \| random \| id <id>>` | Queues a specific mode ID (`fr_next <id>` or `fr_next id <id>`) or random selection (`random`) for the next round. |
| `fr_reload` | None | Validates and schedules configuration and i18n reload for the next round. |
| `fr_enable` | `<0 \| 1>` | Master switch: disables (`0`) or enables (`1`) the plugin. |

#### Player Commands (Console & Chat)

| Console | Chat Alias | Arguments | Description |
| :--- | :--- | :--- | :--- |
| `fr_info` | `!info` / `!frinfo` | None | Displays current round mode name, ID, and description in chat. |
| `fr_help` | `!help` / `!frhelp` | None | Displays available player commands and usage instructions. |
| `fr_knock` | `!knock` | `[0\|1\|2]` | Configures recoil self-push (`0`: off, `1`: ground & air, `2`: airborne only [default]; cycle without args). |
| `fr_c4` | `!c4` | None | T players purchase extra C4 at the configured price when allowed. |
| `fr_suicide` | `!suicide` | None | Triggers suicide explosion with held or purchased C4 when allowed. |
| `fr_chicken` | `!chicken` | None | Purchases an ordinary chicken at the configured price. |
| `fr_healthshot` | `!healthshot` | None | Purchases a healthshot at the configured price. |
| `fr_nvg` | `!nvg` | None | Toggles night vision. |
| `fr_wire` | `!wire` | `<color>` | Selects planting or quick-defusing wire color (blue, yellow, red, green). |

---

### Configuration

Configuration uses strict TOML syntax with a global `[defaults]` template for field inheritance:

```toml
# Global settings
diagnostics = false # Diagnostic logging

# i18n
[i18n]
default_language = "zh-CN" # Default language ("zh-CN" or "en")

# Optional named weapon groups
# [weapon_groups]
# basic = ["famas", "p250"]

# Global defaults
[defaults]
gravity = 1.0
bunnyhop = false

[[modes]]
id = "example"              # Unique mode identifier
enabled = true              # Whether mode enters the pool
name_key = "mode.example.name" # Localized name key
description_key = "mode.example.description" # Localized description key
weapons = ["famas", "p250"] # Canonical weapon IDs

# Team equipment distribution
clear_weapons = "all"       # Teams to strip (none / all / terrorists / counter_terrorists)
give_weapons = "all"        # Teams to give candidates
give_knife = "all"          # Teams to give knives

# Attributes & movement
health = 120                # Initial health (1–10000 or range { min = 100, max = 150 })
armor = 50                  # Initial armor (0–1000 or range { min = 0, max = 100 })
gravity = 1.0               # Gravity multiplier ((0, 10], 1.0 is normal)
speed = 1.0                 # Speed multiplier ((0, 10])
self_push = true            # Recoil self-push

# Shooting & view dynamics
ammo = "reserve"            # Ammo rule (inherit / reserve / clip / single_shot)
headshots = "normal"        # Headshot rule (normal / only / disabled)
fast_switch = true          # Fast switch

# Optional mutator examples
# autostrafe = true         # Air-strafe assist
# explosive_bullets = { cooldown_seconds = 1.5, damage = 30, radius = 120 } # Explosive bullets
# wall_hang = true          # Wall hang
# grapple = { max_distance = 1000.0, max_speed = 600.0, pull_gain = 4.0 } # Grapple hook
# lifesteal = true          # Vampiric lifesteal
# third_person = { distance = 64.0, follow_eyes = true, return_strength = 2.0 } # Third-person
```

<details>
<summary><b>View Full Configuration Reference</b></summary>

Refer to [config/funrounds.example.toml](config/funrounds.example.toml) for full documentation of chicken ecology, health curves, ghost stealth, custom bombs, and economy rules.
</details>

---

### Building from Source

#### 1. Build Rust Plugin

Requires Rust `1.99.1` (automatically handled by `rust-toolchain.toml`):

```bash
# Formatting & Clippy lint checks
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# Execute full test suite
cargo test --workspace --locked

# Build Release binaries
cargo build --release --workspace --locked
```

#### 2. Build Native C++ Companion

Requires Python 3.10+, CMake 3.25+, and a C++20 compiler (MSVC 2022+ or GCC 11+):

```bash
# Build native companion into dist/native
python scripts/build-native.py --output-dir dist/native
```

---

### License & Author

- **Author**: Mai Ooizumi ([@Mai-Ooizumi](https://github.com/Mai-Ooizumi))
- **License**: GNU Affero General Public License v3.0 or later ([AGPL-3.0-or-later](LICENSE))
