# FunRounds-CS2

<div align="center">

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/Version-0.2.0--alpha-informational.svg)](Cargo.toml)
[![Rust Version](https://img.shields.io/badge/Rust-1.99.1+-orange.svg)](rust-toolchain.toml)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20x64-lightgrey.svg)]()
[![Framework](https://img.shields.io/badge/Framework-Plugify%20%2B%20S2SDK%203.0.1-success.svg)](https://github.com/untrustedmodders/plugify)

**高性能、高安全性的 Counter-Strike 2 专用服务端特殊娱乐回合插件**<br>
<small>A high-performance, safe, and modular special rounds plugin for Counter-Strike 2 dedicated servers.</small>

[中英交替 (README.md)](README.md) • [English Only (README_EN.md)](README_EN.md) • [中文独立文档 (README_ZH.md)](README_ZH.md)

</div>

---

### 项目简介 / <small>Overview</small>

**FunRounds-CS2** 是基于 Rust 与 C++ 原生扩展开发、面向 Counter-Strike 2 专用服务端的特殊娱乐回合（Fun Rounds）插件，运行于 [Plugify](https://github.com/untrustedmodders/plugify) 插件框架和 [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk) 之上。
<small>**FunRounds-CS2** is a modern special rounds plugin for Counter-Strike 2 dedicated servers developed in Rust and native C++, running atop the [Plugify](https://github.com/untrustedmodders/plugify) framework and [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk).</small>

插件在回合开始时（或由管理员实时指定）无缝应用多维度的玩法突变：修改玩家属性、物理参数、动态配装、后坐与镜头机制、特殊战斗规则及 C4 玩法，同时保证严格的崩溃隔离与无残留状态恢复。
<small>At the start of each round (or on administrative demand), it seamlessly applies multi-dimensional gameplay mutators: altering player attributes, physics, dynamic loadouts, recoil/view mechanics, combat rules, and bomb systems—with zero-leak state restoration and crash isolation.</small>

---

### 核心特性 / <small>Key Features</small>

- **三层模块化架构**：纯 Rust 核心负责规则与配置、Rust 插件适配游戏线程事件、原生 C++ 模块处理底层挂钩与碰撞。
  <small>**Three-Tier Modular Architecture**: Pure Rust core for logic & configuration, Rust plugin for game-thread events, and a native C++ companion for low-level hooks & collision.</small>
- **基础属性与物理参数**：支持自定义生命（1～10000）、护甲、重力（`gravity`）、移速（`speed`）、渲染颜色（`color`）、连跳（`bunnyhop`）及周期性变重力/变移速。
  <small>**Attributes & Physics**: Custom health (1–10,000), armor, gravity, speed multiplier, player RGB tint, auto-bunnyhopping, and periodic speed/gravity oscillation.</small>
- **镜头与射击反馈**：支持开火镜头震动（`recoil_view`）、冲击衰减（`view_punch_decay`）、无后坐散布（`no_spread_recoil`）、禁用开镜（`no_scope`）、视野调整（`fov`）及延时隐藏准星。
  <small>**Recoil & Visual Dynamics**: Weapon recoil view-punch, view-punch decay, recoil/spread suppression, scope restriction, custom FOV, and delayed crosshair hiding.</small>
- **动态配装与弹药机制**：支持无限备弹/无限弹匣/单发弹药、电击枪充能、投掷物自动补充、阵营配发与清理、随机抽枪及击杀升级（`kill_loadout`）。
  <small>**Loadouts & Ammo Modes**: Infinite reserve/clip/single-shot ammo, taser recharging, grenade refills, team-based weapon distribution/clearing, random draws, and kill-streak weapon progression.</small>
- **战斗规则与自推反冲**：吸血恢复（`lifesteal`）、爆头规则限制、按生命缩放伤害/速度/衰减、飞刀弹射、诱饵弹躲避球碰撞、开火自推反冲（`fr_knock` 支持地面/空中设置）。
  <small>**Combat Rules & Knockback**: Vampiric lifesteal, headshot restrictions, HP-scaled damage/speed/decay, throwing knives, decoy dodgeball, and customizable shooting self-push knockback (`fr_knock`).</small>
- **C4 炸弹与自爆机制**：支持原生 C4、鸡背 C4、可携带 C4，支持自定义安放/拆除/爆炸时间、炸弹弹跳、购买 C4（`fr_c4`）及 C4 自爆（`fr_suicide`）。
  <small>**C4 Objectives & Suicide**: Native, chicken-carried, and portable carry bombs with custom timers, bomb bouncing, C4 purchasing (`fr_c4`), and C4 suicide explosions (`fr_suicide`).</small>
- **复活与服务端规则覆写**：支持回合开局复活与延时按队复活，支持单模式覆盖购买时间、金钱上限、队友伤害倍率、随机出生点等 ConVar。
  <small>**Respawns & Server Rule Overrides**: Round-start and delayed team respawns, plus per-mode overrides for buy time, cash limits, friendly fire scaling, and random spawn points.</small>
- **严格的安全隔离**：C ABI 边界 `catch_unwind` 异常拦截，按代次（Generation）严格回滚受管属性，底层能力缺失时自动降级隔离。
  <small>**Strict Fault Isolation**: FFI panic containment across C ABI boundaries, generation-tracked transactional rollbacks, and automatic capability quarantine.</small>

---

### 项目目录结构 / <small>Project Layout</small>

```text
├── bindings/             # Plugify S2SDK 接口清单 (123 项接口定义) / Provider manifest
├── config/               # 示例配置文件 (funrounds.example.toml) / Reference configuration
├── crates/
│   ├── funrounds-core/   # 独立于引擎的玩法核心、规则校验与配置解析 / Core rules & config parser
│   └── funrounds-plugin/ # 游戏线程 Plugify 插件运行时 / Plugify game runtime plugin
├── native/
│   └── funrounds-native/ # 原生 C++ 伴侣扩展 (底层挂钩、碰撞拦截、Gamedata) / Native C++ companion
├── packaging/            # Rust 插件包清单 (funrounds.pplugin) / Plugify package manifest
├── scripts/              # 原生构建与 CI 脚本 / Build & CI automation scripts
└── rust-toolchain.toml   # 固定编译器工具链版本 (Rust 1.99.1) / Pinned Rust toolchain
```

---

### 安装与部署 / <small>Installation</small>

#### 前置要求 / <small>Prerequisites</small>

- Counter-Strike 2 专用服务端（Windows x64 或 Linux x64）<br><small>Counter-Strike 2 Dedicated Server (Windows x64 or Linux x64)</small>
- [Plugify](https://github.com/untrustedmodders/plugify) 插件框架已安装运行<br><small>[Plugify](https://github.com/untrustedmodders/plugify) framework installed and active</small>
- [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk) 插件（版本 `3.0.1`）已配置<br><small>[S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk) plugin (version `3.0.1`) configured</small>

#### 目录部署 / <small>Deployment Structure</small>

将构建产物复制到服务端 `game/csgo/addons/plugify/` 对应路径：
<small>Copy the build outputs into the respective directories under `game/csgo/addons/plugify/`:</small>

```text
game/csgo/addons/plugify/
├── configs/
│   └── funrounds.toml                              # 从 config/funrounds.example.toml 复制 / Config file
└── envs/
    ├── funrounds/                                  # Rust 主插件 / Main Rust Plugin
    │   ├── bin/
    │   │   └── funrounds_plugin.dll (或 .so)       # 动态库 / Binary
    │   └── funrounds.pplugin                       # 插件清单 / Manifest
    └── funrounds_native/                           # 原生 C++ 扩展 / Native C++ Companion
        ├── bin/
        │   └── funrounds-native.dll (或 lib*.so)   # 原生动态库 / Native binary
        ├── funrounds-native.pplugin                # 原生插件清单 / Native manifest
        └── gamedata/
            └── funrounds-native.jsonc              # Gamedata 特征与偏移 / Gamedata offsets
```

---

### 配置说明 / <small>Configuration</small>

配置文件采用严格 TOML 语法解析。支持 `[defaults]` 全局继承模版：
<small>The configuration uses strict TOML syntax with support for a global `[defaults]` inheritance template:</small>

```toml
# 全局设置 / Global settings
diagnostics = false # 是否开启诊断日志 / Diagnostic logging

# 全局默认模版（模式自身字段优先） / Global defaults (mode-level fields take precedence)
[defaults]
gravity = 1.0
bunnyhop = false

[[modes]]
id = "example"              # 唯一标识 / Unique mode identifier
enabled = true              # 是否启用该模式 / Whether mode enters the pool
name = "鸟狙低重力"          # 显示名称 / Display name
description = "低重力狙击对抗" # 模式说明 / Description
weapons = ["weapon_ssg08", "weapon_glock"]

# 阵营发放与清理 / Team loadout control
clear_weapons = "all"       # 发放前清理武器队伍 (none / all / terrorists / counter_terrorists)
give_weapons = "all"        # 发放候选武器队伍
give_knife = "all"          # 发放刀具队伍

# 基础属性与移动 / Attributes & movement
health = 100                # 初始生命值 (1～10000) / Initial health
armor = 100                 # 初始护甲值 (0～255) / Initial armor
gravity = 0.5               # 重力倍率 / Gravity scale (0.0 < g <= 10.0)
speed = 1.0                 # 移速倍率 / Speed multiplier
bunnyhop = true             # 开启自动连跳 / Auto-bunnyhopping
self_push = true            # 开启开火自推反冲 / Weapon recoil self-push

# 射击与镜头反馈 / Recoil & view
no_spread_recoil = false    # 消除散布与后坐 / Suppress spread & recoil
recoil_view = 2.0           # 开火镜头震动强度 / Recoil view-punch intensity
view_punch_decay = 5.0      # 镜头冲击衰减 / View-punch decay

# 战斗与弹药 / Combat & ammo
ammo = "reserve"            # 弹药模式 (inherit / reserve / clip / single_shot)
lifesteal = false           # 伤害吸血 / Vampiric lifesteal
health_damage = false       # 造成的伤害随自身生命缩放 / Scale damage with current HP

# 特殊 C4 规则（原生 native、鸡背 chicken 或可携带 carry） / Custom Bomb
# bomb = { kind = "native", plant_time = 3.0, explode_time = 45.0, defuse_kit_time = 5.0, defuse_time = 10.0, bounce = false }
```

---

### 指令列表 / <small>Commands Reference</small>

#### 管理员与服务端控制台指令（Console / RCON）/ <small>Server & Administrator Commands</small>

| 控制台指令 <br><small>Command</small> | 参数 <br><small>Arguments</small> | 说明 <br><small>Description</small> |
| :--- | :--- | :--- |
| `fr_status` | 无 <br><small>None</small> | 查看当前回合状态、运行阶段、底层能力就绪状态及故障排查信息。<br><small>Displays current mode, phase, capability readiness, and fault diagnoses.</small> |
| `fr_list` | 无 <br><small>None</small> | 列出配置池中所有已启用的特殊模式列表。<br><small>Lists all loaded and enabled round modes in the pool.</small> |
| `fr_next` | `<id \| random>` | 指定下一回合的模式 ID，或恢复随机抽取 (`random`)。<br><small>Queues a specific mode ID or random selection for the next round.</small> |
| `fr_reload` | 无 <br><small>None</small> | 校验并重载配置文件，重载成功于下一回合生效。<br><small>Validates and schedules configuration reload for the next round.</small> |
| `fr_enable` | `<0 \| 1>` | 插件总启停开关：`1` 为启用，`0` 为停用。<br><small>Master switch: disables (`0`) or enables (`1`) the plugin.</small> |

#### 玩家控制台与聊天指令 / <small>Player Commands (Console & Chat)</small>

| 控制台指令 <br><small>Console</small> | 聊天指令 <br><small>Chat Alias</small> | 参数 <br><small>Arguments</small> | 说明 <br><small>Description</small> |
| :--- | :--- | :--- | :--- |
| `fr_info` | `!frinfo` | 无 <br><small>None</small> | 在聊天栏中查看当前回合模式名称、编号与玩法说明。<br><small>Displays current round mode name, ID, and description in chat.</small> |
| `fr_help` | `!frhelp` | 无 <br><small>None</small> | 查看插件帮助信息及玩家可用指令。<br><small>Displays available player commands and usage instructions.</small> |
| `fr_knock` | `!knock` | `[0\|1\|2]` | 设置或切换开火自推反冲（`0` 关闭，`1` 地面及空中，`2` 仅空中 [默认]，无参数依次切换）。<br><small>Configures weapon recoil self-push (`0`: off, `1`: ground & air, `2`: airborne only [default]; cycle without args).</small> |
| `fr_c4` | `!c4` | 无 <br><small>None</small> | 当模式允许时，花费 $10,000 购买额外 C4。<br><small>Purchases an extra C4 for $10,000 when allowed by the mode (`c4_buy`).</small> |
| `fr_suicide` | `!suicide` | 无 <br><small>None</small> | 当模式允许自爆时，使用已有或购买的 C4 引爆炸药。<br><small>Triggers a suicide explosion with held or purchased C4 when allowed (`suicide`).</small> |

---

### 源码编译 / <small>Building from Source</small>

#### 1. 编译 Rust 插件 / <small>Build Rust Plugin</small>

工具链要求：Rust `1.99.1`（由 `rust-toolchain.toml` 自动匹配）：
<small>Toolchain requirement: Rust `1.99.1` (automatically managed by `rust-toolchain.toml`):</small>

```bash
# 格式化检查与 Clippy 严格静态检查 / Format and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# 执行全量单元测试 / Run unit tests (119+ tests)
cargo test --workspace --locked

# 编译 Release 发布产物 / Build release binaries
cargo build --release --workspace --locked
```

#### 2. 编译原生 C++ 扩展 / <small>Build Native C++ Companion</small>

工具链要求：Python 3.10+、CMake 3.25+ 及对应平台 C++20 编译器（MSVC 2022+ 或 GCC 11+）：
<small>Toolchain requirement: Python 3.10+, CMake 3.25+, and a C++20 compiler (MSVC 2022+ or GCC 11+):</small>

```bash
# 自动抓取对齐 SDK 输入并编译原生扩展 / Fetch pinned SDK inputs and build native module
python scripts/build-native.py --output-dir dist/native
```

---

### 开源协议与作者 / <small>License & Author</small>

- **作者 / Author**: Mai Ooizumi ([@Mai-Ooizumi](https://github.com/Mai-Ooizumi))
- **开源协议 / License**: GNU Affero General Public License v3.0 or later ([AGPL-3.0-or-later](LICENSE))
