<div align="center">

# FunRounds-CS2

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/Version-0.3.0--alpha-informational.svg)](Cargo.toml)
[![Rust Version](https://img.shields.io/badge/Rust-1.99.1+-orange.svg)](rust-toolchain.toml)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20x64-lightgrey.svg)]()
[![Framework](https://img.shields.io/badge/Framework-Plugify%20%2B%20S2SDK%203.0.6-success.svg)](https://github.com/untrustedmodders/plugify)

**A high-performance, safe, and modular special rounds plugin for Counter-Strike 2 dedicated servers.**

[English](README_EN.md) • [简体中文](README_ZH.md)

</div>

---

### Overview

**FunRounds-CS2** is a modern Counter-Strike 2 server plugin developed in Rust and native C++20, built atop the [Plugify](https://github.com/untrustedmodders/plugify) framework and [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk).

At the start of each round (or on administrative demand), it seamlessly applies multi-dimensional gameplay mutators—altering player attributes, physics, dynamic loadouts, recoil/camera mechanics, combat rules, chicken ecology, and bomb systems—with zero-leak state restoration and crash isolation.

---

### Key Features

| Category | Description |
| :--- | :--- |
| **Attributes & Physics** | Health (1–10,000 or random range), armor/helmets, gravity scaling, speed multiplier, friction, auto-bunnyhopping, ground acceleration, ground reverse walking (`ground_reverse`), time scale (`time_scale`), and periodic speed/gravity oscillation. |
| **Visuals & Camera** | Over-the-shoulder third person (`third_person`), stealth ghost mode with breathing audio cues and distance awareness (`ghost`), night vision (`fr_nvg`), crosshair/radar hiding (`hide_hud`), model RGB tinting, translucency, and scope restrictions. |
| **Recoil & Knockback** | Weapon recoil self-push knockback (`fr_knock` supports ground and air modes), recoil & spread suppression, view-punch intensity/decay (`recoil_view` / `view_punch_decay`), fast switch (`fast_switch`), and shooting dropped grenades. |
| **Loadouts & Ammunition** | Team-specific weapon clearing and granting, 4 ammo modes (inherit / reserve / clip / single-shot), taser recharge intervals, automatic grenade replenishment (HE/flash/smoke/decoy/molotov), kill progression (`kill_loadout`), and random draws. |
| **Combat Modifiers** | Vampiric lifesteal, reflect damage (`reflect_damage`), HP-scaled damage/speed/decay, energy absorption shield, miss penalty self-damage, decoy dodgeball collisions (`dodgeball`), throwing knives with bouncing, and afterburn damage. |
| **C4 Objectives & Suicide** | 3 bomb types (native, chicken, carry), custom timers, bomb bouncing, 4-color wire cutting minigame (`fr_wire`), C4 cash purchasing (`fr_c4`), and C4 suicide explosions (`fr_suicide`). |
| **Chicken & Healthshots** | Chicken population scaling, eating chickens with digestion risk & chain explosions; custom healthshot healing ratios with cash purchase (`fr_healthshot`). |
| **Economy & i18n** | Buy anywhere, team purchase access, round money floor, passive income; built-in bilingual i18n (`zh-CN` / `en`) automatically matching client language. |

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
├── bindings/             # Plugify S2SDK provider bindings (123 methods)
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
- [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk) plugin (version `3.0.6`) active in Plugify

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
| `fr_status` | None | Displays current mode, phase, engine capabilities, and fault diagnoses. |
| `fr_list` | None | Lists all loaded and enabled round modes in the pool. |
| `fr_next` | `<id \| random>` | Queues a specific mode ID or random mode for the next round. |
| `fr_reload` | None | Validates and schedules configuration and i18n reload for the next round. |
| `fr_enable` | `<0 \| 1>` | Master switch: disables (`0`) or enables (`1`) the plugin. |

#### Player Commands (Console & Chat)

| Console Command | Chat Alias | Arguments | Description |
| :--- | :--- | :--- | :--- |
| `fr_info` | `!info` / `!frinfo` | None | Displays the current round mode, ID, and description in chat. |
| `fr_help` | `!help` / `!frhelp` | None | Displays available player commands and usage instructions. |
| `fr_knock` | `!knock` | `[0\|1\|2]` | Sets or cycles shooting self-push (`0`: off, `1`: ground & air, `2`: airborne only [default]; cycle without args). |
| `fr_c4` | `!c4` | None | Purchases an extra C4 at the configured price when allowed. |
| `fr_suicide` | `!suicide` | None | Triggers a suicide explosion with held or purchased C4 when allowed. |
| `fr_chicken` | `!chicken` | None | Buys an ordinary chicken at the configured price. |
| `fr_healthshot` | `!healthshot` | None | Buys a healthshot at the configured price. |
| `fr_nvg` | `!nvg` | None | Toggles night vision. |
| `fr_wire` | `!wire` | `<color>` | Selects planting or quick-defusing wire color (`blue`, `yellow`, `red`, `green`). |

---

### Configuration

The configuration uses strict TOML format with support for global `[defaults]` template inheritance:

```toml
# Top-level settings
diagnostics = false # Diagnostic logging (default: false)

# Internationalization
[i18n]
default_language = "en" # Server default language ("zh-CN" or "en")

# Global default template (mode-level fields take precedence)
[defaults]
gravity = 1.0
bunnyhop = false

[[modes]]
id = "example"              # Unique mode identifier (ASCII, alphanumeric, '_' or '-')
enabled = true              # Whether this mode enters the round pool
name_key = "mode.example.name" # Localized name key
description_key = "mode.example.description" # Localized description key
weapons = ["weapon_famas", "weapon_p250"]

# Team loadout control
clear_weapons = "all"       # Team to clear weapons before granting (none / all / terrorists / counter_terrorists)
give_weapons = "all"        # Team to receive candidates
give_knife = "all"          # Team to receive knife

# Attributes & movement
health = 120                # Initial health (1-10000, or random range { min = 100, max = 150 })
armor = 50                  # Initial armor (0-255, or random range { min = 0, max = 100 })
gravity = 1.0               # Gravity multiplier ((0, 10], 1.0 is normal gravity)
speed = 1.0                 # Speed multiplier ((0, 10])
self_push = true            # Weapon recoil self-push

# Recoil & view
ammo = "reserve"            # Ammo mode (inherit / reserve / clip / single_shot)
headshots = "normal"        # Headshot rule (normal / only / disabled)
fast_switch = true          # Fast weapon switch

# Optional mutators
# lifesteal = true          # Vampiric lifesteal
# suicide = true            # Suicide blast gameplay
# third_person = { distance = 64.0, follow_eyes = true, return_strength = 2.0 }
# bomb = { kind = "native", plant_time = 3.0, explode_time = 45.0, bounce = false }
```

<details>
<summary><b>View Full Configuration Reference</b></summary>

Refer to [config/funrounds.example.toml](config/funrounds.example.toml) for full documentation of chicken ecology, health curves, ghost stealth, custom bombs, and economy rules.
</details>

---

### Building from Source

#### 1. Build Rust Plugin

Requires Rust `1.99.1` (automatically managed by `rust-toolchain.toml`):

```bash
# Code formatting check and Clippy analysis
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# Execute test suite
cargo test --workspace --locked

# Build optimized release binaries
cargo build --release --workspace --locked
```

#### 2. Build Native C++ Companion

Requires Python 3.10+, CMake 3.25+, and a C++20 compiler (MSVC 2022+ or GCC 11+):

```bash
# Build native module to output directory
python scripts/build-native.py --output-dir dist/native
```

---

### License & Author

- **Author**: Mai Ooizumi ([@Mai-Ooizumi](https://github.com/Mai-Ooizumi))
- **License**: GNU Affero General Public License v3.0 or later ([AGPL-3.0-or-later](LICENSE))
