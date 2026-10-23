# FunRounds-CS2

<div align="center">

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/Version-0.2.0--alpha-informational.svg)](Cargo.toml)
[![Rust Version](https://img.shields.io/badge/Rust-1.99.1+-orange.svg)](rust-toolchain.toml)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20x64-lightgrey.svg)]()
[![Framework](https://img.shields.io/badge/Framework-Plugify%20%2B%20S2SDK%203.0.1-success.svg)](https://github.com/untrustedmodders/plugify)

**A high-performance, safe, and modular special rounds plugin for Counter-Strike 2 dedicated servers.**

[Bilingual (README.md)](README.md) • [English Only (README_EN.md)](README_EN.md) • [简体中文 (README_ZH.md)](README_ZH.md)

</div>

---

### Overview

**FunRounds-CS2** is a modern Counter-Strike 2 server plugin developed in Rust and native C++, built atop the [Plugify](https://github.com/untrustedmodders/plugify) framework and [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk).

It revitalizes the classic "Fun Rounds" gameplay concept for Source 2. Each round (or upon administrative selection), the plugin seamlessly applies distinctive gameplay mutators—altering player attributes, loadouts, physics, combat rules, and bomb mechanics—while maintaining strict engine stability and zero-leak state restoration.

### Key Features

- **Three-Tier Modular Architecture**:
  - `funrounds-core`: Pure, engine-agnostic Rust library handling state machines, round queues, math, rules, `[defaults]` inheritance, and strictly validated TOML configurations.
  - `funrounds-plugin`: Native Plugify Rust plugin interacting safely with the CS2 engine on the game thread via S2SDK.
  - `funrounds-native`: Native C++ companion module handling low-level engine hooks, entity collision, projectile touch detection, and gamedata offsets.
- **Rich Round Mutators**:
  - **Player Physics & Attributes**: Configurable health (up to 10,000), armor/helmets, gravity scaling, speed multiplier, player RGB tint, auto-bunnyhop, dynamic over-health decay, and periodic gravity/speed oscillation.
  - **Recoil & Knockback**: Custom weapon recoil view-punch, view-punch decay, recoil/spread suppression, scope restriction, custom FOV, delayed crosshair hiding, and shooting self-push / knockback physics (`fr_knock`).
  - **Weapons & Dynamic Loadouts**: Whitelisted CS2 weapons, infinite reserve/clip/single-shot ammo, taser recharge timers, automatic grenade replenishment (HE & Decoy), team-based weapon distribution/clearing, independent random loadouts, and kill-streak weapon progression.
  - **Combat Modifiers**: Vampiric lifesteal, headshot restrictions, damage/speed scaling by player HP, decoy dodgeball physics, and throwing knives with bouncing mechanics.
  - **Custom C4 / Bomb & Suicide Mechanics**: Support for native bombs, chicken-carried bombs, portable carry bombs, custom plant/explode/defuse timers, bouncing bomb physics, C4 purchasing (`fr_c4`), and C4 suicide explosions (`fr_suicide`).
  - **Respawns & Server Rule Overrides**: Round-start and delayed team respawns, plus per-mode overrides for buy time, cash limits, friendly fire scaling, and random spawn points.
- **Fail-Safe & Fault-Tolerant Design**:
  - **FFI Panic Containment**: All engine callbacks wrap in `catch_unwind` at the C ABI boundary to prevent server crashes.
  - **Transactional State Restoration**: Tracks player/round generations to guarantee only hijacked properties and ConVars are restored without corrupting native server settings.
  - **Dynamic Capability Quarantine**: Automatically excludes modes requiring missing SDK hooks or engine features without impacting other modes.

### Project Layout

```text
├── bindings/             # Plugify S2SDK provider bindings (123 methods)
├── config/               # Reference configurations (funrounds.example.toml)
├── crates/
│   ├── funrounds-core/   # Engine-independent gameplay logic, rules & config parser
│   └── funrounds-plugin/ # Native Plugify runtime plugin implementation
├── native/
│   └── funrounds-native/ # Native C++ companion (engine hooks, collision, gamedata)
├── packaging/            # Plugin package manifest (funrounds.pplugin)
├── scripts/              # Native build and CI automation scripts
└── rust-toolchain.toml   # Fixed toolchain configuration (Rust 1.99.1)
```

### Installation

#### Prerequisites

- Counter-Strike 2 Dedicated Server (Windows x64 or Linux x64)
- [Plugify](https://github.com/untrustedmodders/plugify) installed on your CS2 server
- [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk) plugin (version `3.0.1`) active in Plugify

#### Deployment

Copy the built artifacts to the server's `game/csgo/addons/plugify/` directory:

```text
game/csgo/addons/plugify/
├── configs/
│   └── funrounds.toml                              # Copied from config/funrounds.example.toml
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

### Configuration

The configuration uses strict TOML format with support for global `[defaults]` template inheritance:

```toml
# Top-level settings
diagnostics = false # Diagnostic logging (default: false)

# Global default template (mode-level fields take precedence)
[defaults]
gravity = 1.0
bunnyhop = false

[[modes]]
id = "example"              # Unique mode identifier (ASCII, alphanumeric, '_' or '-')
enabled = true              # Whether this mode enters the round pool
name = "Scout Jump"         # Display name
description = "Low gravity sniper battle"
weapons = ["weapon_ssg08", "weapon_glock"]

# Team loadout control
clear_weapons = "all"       # Team to clear weapons before granting (none / all / terrorists / counter_terrorists)
give_weapons = "all"        # Team to receive candidates
give_knife = "all"          # Team to receive knife

# Attributes & movement
health = 100                # Initial health (1-10000)
armor = 100                 # Initial armor (0-255)
gravity = 0.5               # Gravity multiplier (0.0 < g <= 10.0)
speed = 1.0                 # Speed multiplier
bunnyhop = true             # Auto-bunnyhopping
self_push = true            # Weapon recoil self-push

# Recoil & view
no_spread_recoil = false    # Suppress spread and recoil
recoil_view = 2.0           # Recoil view-punch intensity
view_punch_decay = 5.0      # View-punch decay

# Combat & ammo
ammo = "reserve"            # Ammo mode (inherit / reserve / clip / single_shot)
lifesteal = false           # Vampiric lifesteal
health_damage = false       # Scale damage with current attacker HP

# Optional Bomb Rules (Native, Chicken, or Carry)
# bomb = { kind = "native", plant_time = 3.0, explode_time = 45.0, defuse_kit_time = 5.0, defuse_time = 10.0, bounce = false }
```

### Commands Reference

#### Server & Administrator Commands (Console / RCON)

| Command | Arguments | Description |
| :--- | :--- | :--- |
| `fr_status` | None | Displays current mode, phase, engine capabilities, and fault diagnoses. |
| `fr_list` | None | Lists all loaded and enabled round modes in the pool. |
| `fr_next` | `<id \| random>` | Queues a specific mode ID or random mode for the next round. |
| `fr_reload` | None | Validates and schedules configuration reload for the next round. |
| `fr_enable` | `<0 \| 1>` | Master switch: disables (`0`) or enables (`1`) the plugin. |

#### Player Commands (Console & Chat)

| Console Command | Chat Alias | Arguments | Description |
| :--- | :--- | :--- | :--- |
| `fr_info` | `!frinfo` | None | Displays the current round mode, ID, and description in chat. |
| `fr_help` | `!frhelp` | None | Displays available player commands and usage instructions. |
| `fr_knock` | `!knock` | `[0\|1\|2]` | Sets or cycles shooting self-push (`0`: off, `1`: ground & air, `2`: airborne only [default]; cycle without args). |
| `fr_c4` | `!c4` | None | Purchases an extra C4 for $10,000 when allowed by mode (`c4_buy`). |
| `fr_suicide` | `!suicide` | None | Triggers a suicide explosion with held or purchased C4 when allowed. |

### Building from Source

#### 1. Build Rust Plugin

Requires Rust `1.99.1` (automatically managed by `rust-toolchain.toml`):

```bash
# Code formatting check and Clippy analysis
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# Execute test suite (119+ unit tests)
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
