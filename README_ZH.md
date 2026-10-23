# FunRounds-CS2 (简体中文)

<div align="center">

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/Version-0.2.0--alpha-informational.svg)](Cargo.toml)
[![Rust Version](https://img.shields.io/badge/Rust-1.99.1+-orange.svg)](rust-toolchain.toml)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20x64-lightgrey.svg)]()
[![Framework](https://img.shields.io/badge/Framework-Plugify%20%2B%20S2SDK%203.0.1-success.svg)](https://github.com/untrustedmodders/plugify)

**高性能、高安全性的 Counter-Strike 2 专用服务端特殊娱乐回合插件**

[中英交替 (README.md)](README.md) • [English Only (README_EN.md)](README_EN.md) • [简体中文 (README_ZH.md)](README_ZH.md)

</div>

---

### 项目简介

**FunRounds-CS2** 是基于 Rust 与 C++ 原生扩展开发、面向 Counter-Strike 2 专用服务端的特殊娱乐回合（Fun Rounds）插件。本项目构建于 [Plugify](https://github.com/untrustedmodders/plugify) 插件框架和 [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk) 扩展之上。

插件在回合开始时（或由管理员实时指定）无缝应用多维度的玩法突变：修改玩家属性、物理参数、动态配装、后坐与镜头机制、特殊战斗规则及 C4 玩法，同时保证严格的崩溃隔离与无残留状态恢复。

### 核心特性

- **三层模块化架构**：
  - `funrounds-core`：纯 Rust 编写、独立于游戏引擎的核心库。负责状态机流转、回合随机池、战斗数学规则、`[defaults]` 继承模版以及严格校验的 TOML 配置解析。
  - `funrounds-plugin`：Plugify Rust 原生适配插件。在游戏主线程通过 S2SDK 与 CS2 引擎交互，处理实体句柄追踪与生命周期管理。
  - `funrounds-native`：基于 C++20 的原生伴侣模块。提供底层引擎挂钩、实体碰撞拦截、投射物 Touch 判定与 Gamedata 偏移管理。
- **丰富的玩法修改机制**：
  - **基础属性与物理参数**：支持自定义生命值（1～10000）、护甲头盔、重力倍率（`gravity`）、移速倍率（`speed`）、渲染颜色（`color`）、连跳（`bunnyhop`）以及周期性重力/移速动态波动。
  - **后坐与镜头反馈**：支持自定义开火镜头震动（`recoil_view`）、冲击衰减（`view_punch_decay`）、散布与后坐抑制（`no_spread_recoil`）、限制开镜（`no_scope`）、视野调整（`fov`）及延时隐藏准星。
  - **武器与动态配装**：支持 CS2 官方武器白名单、无限备弹/无限弹匣/单发弹药、电击枪充能间隔、投掷物自动补充、阵营配发与清理、随机抽枪及击杀升级换枪（`kill_loadout`）。
  - **特殊战斗规则**：吸血恢复（`lifesteal`）、爆头规则限制、按自身生命缩放伤害/速度/衰减、飞刀弹射判定、诱饵弹躲避球碰撞伤害（`dodgeball`）、开火自推反冲（`fr_knock` 支持地面与空中配置）。
  - **C4 特殊机制与自爆**：支持原生 C4（`native`）、鸡背 C4（`chicken`）、可携带 C4（`carry`），支持自定义安放/拆除/爆炸时间、炸弹弹跳物理、购买 C4（`fr_c4`）及引爆 C4 自爆（`fr_suicide`）。
  - **复活与服务端规则覆写**：支持回合开局复活与延时按队复活，支持单模式覆盖购买时间、金钱上限、队友伤害倍率、随机出生点等 ConVar。
- **高可靠性与故障隔离**：
  - **FFI Panic 拦截**：在 C ABI 边界全面捕获 Rust panic（`catch_unwind`），防止单次回调异常波及专用服务端进程。
  - **事务性状态恢复**：严格维护代次（Generation）与实体句柄，在回合结束或停用时仅还原由本插件修改过的属性和 ConVar，不污染服务端原有配置。
  - **能力动态隔离**：根据服务端运行环境的 SDK 钩子支持情况，自动隔离缺失底层能力的模式，其余模式不受影响正常运行。

### 目录结构

```text
├── bindings/             # Plugify S2SDK 接口清单 (123 项接口定义)
├── config/               # 示例配置文件 (funrounds.example.toml)
├── crates/
│   ├── funrounds-core/   # 独立于引擎的玩法核心、规则校验与配置解析
│   └── funrounds-plugin/ # 游戏线程 Plugify 插件运行时实现
├── native/
│   └── funrounds-native/ # 原生 C++ 伴侣扩展 (底层挂钩、碰撞拦截、Gamedata 偏移)
├── packaging/            # Rust 插件包清单 (funrounds.pplugin)
├── scripts/              # 原生构建与 CI 自动化脚本
└── rust-toolchain.toml   # 固定编译器工具链版本 (Rust 1.99.1)
```

### 安装与部署

#### 前置要求

- Counter-Strike 2 专用服务端（Windows x64 或 Linux x64）
- 服务端已安装配置 [Plugify](https://github.com/untrustedmodders/plugify) 插件框架
- 服务端已加载并运行 [S2SDK](https://github.com/untrustedmodders/plugify-plugin-s2sdk) 插件（版本 `3.0.1`）

#### 部署步骤

将构建产物复制到服务端 `game/csgo/addons/plugify/` 对应路径：

```text
game/csgo/addons/plugify/
├── configs/
│   └── funrounds.toml                              # 从 config/funrounds.example.toml 复制
└── envs/
    ├── funrounds/                                  # Rust 主插件
    │   ├── bin/
    │   │   └── funrounds_plugin.dll (或 .so)       # 插件动态库
    │   └── funrounds.pplugin                       # 插件清单
    └── funrounds_native/                           # 原生 C++ 伴侣扩展
        ├── bin/
        │   └── funrounds-native.dll (或 lib*.so)   # 原生动态库
        ├── funrounds-native.pplugin                # 原生插件清单
        └── gamedata/
            └── funrounds-native.jsonc              # Gamedata 特征与偏移文件
```

### 配置说明

配置文件采用严格 TOML 语法解析。支持 `[defaults]` 全局继承模版：

```toml
# 顶层全局设置
diagnostics = false # 是否开启诊断日志（默认 false）

# 全局默认模版（模式自身字段优先）
[defaults]
gravity = 1.0
bunnyhop = false

[[modes]]
id = "example"              # 模式唯一标识（仅允许字母、数字、'_' 或 '-'）
enabled = true              # 是否启用该模式并加入随机池
name = "鸟狙低重力"          # 游戏中显示的模式名称
description = "低重力狙击对抗"
weapons = ["weapon_ssg08", "weapon_glock"]

# 阵营发放与清理
clear_weapons = "all"       # 发放前清理武器队伍 (none / all / terrorists / counter_terrorists)
give_weapons = "all"        # 发放候选武器队伍
give_knife = "all"          # 发放刀具队伍

# 基础属性与移动
health = 100                # 初始生命值 (1～10000)
armor = 100                 # 初始护甲值 (0～255)
gravity = 0.5               # 重力倍率 (0.0 < g <= 10.0，1.0 为正常)
speed = 1.0                 # 移动速度倍率
bunnyhop = true             # 是否开启自动连跳
self_push = true            # 是否开启开火自推反冲

# 射击与镜头反馈
no_spread_recoil = false    # 是否消除武器后坐力与散布
recoil_view = 2.0           # 开火镜头震动强度
view_punch_decay = 5.0      # 镜头冲击衰减值

# 战斗与弹药
ammo = "reserve"            # 弹药模式 (inherit / reserve / clip / single_shot)
lifesteal = false           # 是否开启造成伤害吸血
health_damage = false       # 是否根据攻击者生命缩放伤害

# 特殊 C4 规则（可选原生 native、鸡背 chicken 或可携带 carry）
# bomb = { kind = "native", plant_time = 3.0, explode_time = 45.0, defuse_kit_time = 5.0, defuse_time = 10.0, bounce = false }
```

### 指令列表

#### 管理员与服务端控制台指令（Console / RCON）

| 控制台指令 | 参数 | 说明 |
| :--- | :--- | :--- |
| `fr_status` | 无 | 查看当前回合状态、运行阶段、底层能力就绪状态及故障排查信息。 |
| `fr_list` | 无 | 列出当前配置池中所有已启用的特殊模式列表。 |
| `fr_next` | `<id \| random>` | 指定下一回合的模式 ID，或恢复随机抽取 (`random`)。 |
| `fr_reload` | 无 | 校验并重载配置文件，重载成功于下一回合生效。 |
| `fr_enable` | `<0 \| 1>` | 插件总启停开关：`1` 为启用，`0` 为停用。 |

#### 玩家控制台与聊天指令

| 控制台指令 | 聊天快捷指令 | 参数 | 说明 |
| :--- | :--- | :--- | :--- |
| `fr_info` | `!frinfo` | 无 | 在聊天栏中查看当前回合模式名称、编号与玩法说明。 |
| `fr_help` | `!frhelp` | 无 | 查看插件帮助信息及玩家可用指令。 |
| `fr_knock` | `!knock` | `[0\|1\|2]` | 设置或切换开火自推反冲（`0` 关闭，`1` 地面及空中，`2` 仅空中 [默认]，无参数依次切换）。 |
| `fr_c4` | `!c4` | 无 | 当模式允许普通购买时，花费 $10,000 现金购买 C4。 |
| `fr_suicide` | `!suicide` | 无 | 当模式允许自爆时，使用已有 C4 或购买后引爆自爆。 |

### 源码编译

#### 1. 编译 Rust 插件

编译前请确保安装了 Rust `1.99.1`（可通过仓库内的 `rust-toolchain.toml` 自动匹配）。

```bash
# 代码格式化检查与 Clippy 静态检查
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# 运行自动化单元测试 (包含 119+ 完整测试用例)
cargo test --workspace --locked

# 编译 Release 发布版本
cargo build --release --workspace --locked
```

#### 2. 编译原生 C++ 扩展

需要 Python 3.10+、CMake 3.25+ 以及对应平台的 C++20 编译器（MSVC 2022+ 或 GCC 11+）：

```bash
# 构建原生扩展并输出至 output 目录
python scripts/build-native.py --output-dir dist/native
```

---

### 开源协议与作者

- **作者**: Mai Ooizumi ([@Mai-Ooizumi](https://github.com/Mai-Ooizumi))
- **开源协议**: GNU Affero General Public License v3.0 or later ([AGPL-3.0-or-later](LICENSE))
