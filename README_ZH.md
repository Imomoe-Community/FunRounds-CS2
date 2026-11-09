<div align="center">

# FunRounds-CS2

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/Version-0.4.0--alpha-informational.svg)](Cargo.toml)
[![Rust Version](https://img.shields.io/badge/Rust-1.99.1+-orange.svg)](rust-toolchain.toml)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20x64-lightgrey.svg)]()
[![Framework](https://img.shields.io/badge/Framework-Plugify%20%2B%20S2SDK%203.3.1-success.svg)](https://github.com/untrustedmodders/plugify)

**高性能、高安全性的 Counter-Strike 2 专用服务端特殊娱乐回合插件**

[English](README_EN.md) • [简体中文](README_ZH.md)

</div>

---

### 项目简介

**FunRounds-CS2** 是基于 Rust 与 C++20 原生扩展开发、面向 Counter-Strike 2 专用服务端的特殊娱乐回合（Fun Rounds）插件。本项目构建于 [Plugify](https://github.com/untrustedmodders/plugify) 插件框架和 [S2SDK](https://github.com/Mai-Ooizumi/plugify-plugin-s2sdk/tree/funrounds) 扩展之上。

插件在每个回合开始时（或由管理员实时指定）无缝应用多维度的玩法突变：修改玩家属性、机动物理、动态配装、后坐与视角机制、特殊战斗规则、小鸡生态及 C4 玩法，同时保证严格的崩溃隔离与无残留状态恢复。

---

### 核心特性

| 玩法维度 | 特性机制概要 |
| :--- | :--- |
| **基础属性与物理** | 生命值（1～10000 或区间随机）、护甲头盔（护甲最高 1000）、重力倍率、移速倍率、地面摩擦力、自动连跳（Bunnyhop）、地面加速度与反向行走（`ground_reverse`）、全局时间流速（`time_scale`）及周期性变重力/变移速。 |
| **动作与机动能力** | 空中辅助转向加速（`autostrafe`）、空中贴墙挂墙（`wall_hang`）、准星瞄准点抓钩牵引（`grapple`）及地图特定区域蹦床弹射（`trampoline`）。 |
| **视觉与相机视角** | 第三人称视角（`third_person`）、幽灵按行动隐身（`ghost`，支持呼吸心跳音效与距离感知）、大头模式（`big_head`，依据 KD 动态缩放头部）、夜视仪（`fr_nvg`）、准星/雷达隐藏（`hide_hud`）、模型 RGB 着色、半透明外观与开镜限制。 |
| **射击与自推反冲** | 开火自身反冲自推（`fr_knock` 支持地面与空中切换）、后坐力与散布消除、开火概率换枪/掉枪（`weapon_on_fire`）、子弹命中爆炸（`explosive_bullets`）、开枪镜头震动冲击与衰减及快速切枪（`fast_switch`）。 |
| **动态配装与武器** | 规范化物品 ID、命名武器组（`[weapon_groups]` 支持 `@group` 引用）、阵营清理与定向配发、4 种弹药模式（原生 / 无限备弹 / 无限弹匣 / 单发换装）、电击枪充能、击杀升级换枪（`kill_loadout`）与独立随机配装。 |
| **投掷物与特殊战术** | 投掷手雷自动补充、接近敌人近炸引信（`proximity_fuse`）、HE 手雷寻的追踪飞行（`he_guidance`）、HE 爆风推力、烟雾区域治疗（`medical_smoke`）、诱饵弹躲避球碰撞杀伤（`dodgeball`）、右键飞刀及长效闪光（`long_flash`）。 |
| **特殊战斗与生存惩罚** | 造成伤害吸血、伤害反弹（`reflect_damage`）、动态生命曲线、友军射击转治疗（`friendly_fire_heal`）、脱战自愈（`regeneration`）、低速扣血惩罚（`dont_stop`）、熔岩地面（`hot_floor`）、死亡殉爆（`death_explosion`）与队杀限制。 |
| **C4 目标与小鸡生态** | 3 种炸弹模式（原生 C4 / 鸡背 C4 / 便携可携带 C4）、自定义耗时与弹跳、4 色引线拆包剪线小游戏（`fr_wire`）、现金购买 C4（`fr_c4`）与自爆（`fr_suicide`）；普通小鸡生态刷新、吃鸡回血与自爆、小鸡回血光环（`chicken_aura`）及医疗针。 |

<details>
<summary><b>点击展开查看全部详细玩法特性与机制列表</b></summary>

- **规范化装备系统**
  - **规范物品 ID**：全面采用无 `weapon_` 前缀的标准物品标识符（如 `famas`, `p250`, `hegrenade`, `flashbang`, `smokegrenade`, `decoy`, `molotov`, `incgrenade` 等）。
  - **命名武器组**：支持在配置顶层定义 `[weapon_groups]`，模式中通过 `@group_name` 语法引用与组合复用。
  - **扩展护甲上限**：初始护甲支持设定至最高 `1000` 点。

- **动作与机动能力**
  - **空中辅助加速 (`autostrafe`)**：空中自动对齐偏航速度并施加辅助加速，带来顺滑的空速增益体验。
  - **空中挂墙 (`wall_hang`)**：在空中贴靠墙面且按住跳跃键时进入挂壁悬停状态。
  - **抓钩绳索 (`grapple`)**：瞄准任意场景表面并按下使用键，以配置的最大速度与拉力牵引玩家快速位移。
  - **区域蹦床 (`trampoline`)**：在指定竞技地图（如 Dust2、Mirage）内置点位踩入即触发高空垂直弹射。

- **射击与战斗突变**
  - **爆炸子弹 (`explosive_bullets`)**：开火命中产生范围爆炸、伤害与击退效果，支持独立冷却、伤害与半径区间。
  - **开火失控 (`weapon_on_fire`)**：射击时有几率触发强制切换下一武器或直接脱手掉落当前武器。
  - **动态大头 (`big_head`)**：根据玩家当场击杀/死亡比率（KD）动态缩放头部模型大小与爆头判定框。
  - **友伤转治疗 (`friendly_fire_heal`)**：命中友军不仅免除误伤，还可按固定数值或伤害比例为队友恢复生命。
  - **脱战自愈 (`regeneration`)**：脱离交火经过指定冷却延迟后，按固定周期自动恢复生命值。

- **高阶投掷物机制**
  - **近炸引信 (`proximity_fuse`)**：手雷飞行至敌方玩家感知半径内时触发近炸，直接缩短延时提前起爆。
  - **寻的追踪手雷 (`he_guidance`)**：高爆手雷出膛后自动以配置转向角速度寻的并追踪飞行轨迹中的敌人。
  - **爆风推力 (`hegrenade_impulse`)**：高爆手雷爆炸产生额外物理冲量，推开爆心周围的目标。
  - **医疗烟雾 (`medical_smoke`)**：友方烟雾弹扩散范围内为处于其中的队友持续恢复生命。
  - **长效致盲 (`long_flash`)**：按倍率延长闪光弹全白致盲时间，并受最大保护上限时间限制。

- **环境惩罚与规则机制**
  - **止步惩罚 (`dont_stop`)**：玩家水平移速低于安全阈值并超过宽限时间后，将持续受到失速扣血惩罚。
  - **熔岩地面 (`hot_floor`)**：持续接触地面超过安全秒数后持续扣血，久留不离开则直接被火焰点燃。
  - **死亡殉爆 (`death_explosion`)**：每回合随机从双方阵营赋予特定玩家死亡自爆装药，阵亡瞬间引发致命爆炸。
  - **队杀限制 (`teamkill_limit`)**：严格限制单回合队杀次数，超额直接触发惩罚。
  - **周期性参数波动 (`periodic_speed` / `periodic_gravity`)**：支持配置随机时间间隔，按自定义离散数值或连续区间周期刷新重力与移速。

</details>

<details>
<summary><b>点击展开架构与可靠性设计</b></summary>

- **三层模块化架构**：
  - `funrounds-core`：纯 Rust 编写、独立于引擎的核心库。负责玩法规则、状态机流转、`[defaults]` 模板继承、数学判定及严格校验的 TOML 配置解析。
  - `funrounds-plugin`：Plugify Rust 原生适配插件。在游戏主线程响应 S2SDK 实体事件，管理实体生命周期与多语言反馈。
  - `funrounds-native`：C++20 原生伴侣模块。提供底层伤害桥接、投射物 Touch、实体碰撞拦截、小鸡互动、幽灵雷达遮蔽及 Gamedata 偏移管理。
- **严格的安全隔离**：
  - **FFI Panic 全边界捕获**：所有跨 C ABI 边界的回调均由 `catch_unwind` 包裹，防止单次回调异常波及专用服务端进程。
  - **事务性状态回滚**：严格记录代次（Generation）与实体快照，在回合结束或插件停用时仅还原受管属性与 ConVar，绝不残留脏状态。
  - **能力动态隔离**：根据服务端运行环境的 SDK 钩子支持情况，自动隔离缺失底层能力的模式，其余模式不受影响正常运行。
</details>

---

### 项目目录结构

```text
├── bindings/             # Plugify S2SDK 接口清单 (164 项接口定义)
├── config/               # 示例配置文件及多语言包
│   ├── funrounds.example.toml # 示例配置
│   └── i18n/             # 本地化翻译文件 (zh-CN.toml, en.toml)
├── crates/
│   ├── funrounds-core/   # 独立于引擎的玩法核心、规则校验与配置解析
│   └── funrounds-plugin/ # 游戏线程 Plugify 插件运行时实现
├── native/
│   └── funrounds-native/ # 原生 C++ 伴侣扩展 (底层挂钩、碰撞拦截、Gamedata 偏移)
├── packaging/            # Rust 插件包清单 (funrounds.pplugin)
├── scripts/              # 原生构建与 CI 自动化脚本
└── rust-toolchain.toml   # 固定编译器工具链版本 (Rust 1.99.1)
```

---

### 安装与部署

#### 前置要求

- Counter-Strike 2 专用服务端（Windows x64 或 Linux x64）
- 服务端已安装配置 [Plugify](https://github.com/untrustedmodders/plugify) 插件框架
- 服务端已加载并运行 [S2SDK](https://github.com/Mai-Ooizumi/plugify-plugin-s2sdk/tree/funrounds) 插件（版本 `=3.3.1`）

#### 部署步骤

将编译产物与配置文件复制到服务端 `game/csgo/addons/plugify/` 对应路径：

```text
game/csgo/addons/plugify/
├── configs/
│   ├── funrounds.toml                              # 从 config/funrounds.example.toml 复制
│   └── funrounds/
│       └── i18n/                                   # 复制 config/i18n/ 下的翻译文件
│           ├── zh-CN.toml
│           └── en.toml
└── envs/
    ├── funrounds/                                  # Rust 主插件
    │   ├── bin/
    │   │   └── funrounds_plugin.dll (或 .so)       # 动态库
    │   └── funrounds.pplugin                       # 插件清单
    └── funrounds_native/                           # 原生 C++ 伴侣扩展
        ├── bin/
        │   └── funrounds-native.dll (或 lib*.so)   # 原生动态库
        ├── funrounds-native.pplugin                # 原生插件清单
        └── gamedata/
            └── funrounds-native.jsonc              # Gamedata 特征与偏移
```

---

### 指令列表

#### 管理员与控制台指令（Console / RCON）

| 控制台指令 | 参数 | 说明 |
| :--- | :--- | :--- |
| `fr_status` | 无 | 查看当前回合状态、运行阶段、底层能力就绪状态及故障排查信息。 |
| `fr_list` | 无 | 列出配置池中所有已启用的特殊模式列表。 |
| `fr_next` | `<id \| random \| id <id>>` | 指定下一回合的模式 ID（支持直接传 ID 或 `id <id>`），或恢复随机抽取 (`random`)。 |
| `fr_reload` | 无 | 校验并重载配置文件与多语言文本，重载成功于下一回合生效。 |
| `fr_enable` | `<0 \| 1>` | 插件总启停开关：`1` 为启用，`0` 为停用。 |

#### 玩家控制台与聊天快捷指令

| 控制台指令 | 聊天指令 | 参数 | 说明 |
| :--- | :--- | :--- | :--- |
| `fr_info` | `!info` / `!frinfo` | 无 | 在聊天栏中查看当前回合模式名称、编号与玩法说明。 |
| `fr_help` | `!help` / `!frhelp` | 无 | 查看插件帮助信息及玩家可用指令列表。 |
| `fr_knock` | `!knock` | `[0\|1\|2]` | 设置或切换开火自推反冲（`0` 关闭，`1` 地空全开，`2` 仅空中 [默认]，无参依次切换）。 |
| `fr_c4` | `!c4` | 无 | 进攻方在模式允许时按配置价格购买额外 C4。 |
| `fr_suicide` | `!suicide` | 无 | 允许自爆时使用所持或购买的 C4 启动自爆。 |
| `fr_chicken` | `!chicken` | 无 | 按配置价格在当前位置购买一只小鸡。 |
| `fr_healthshot` | `!healthshot` | 无 | 按配置价格购买一支医疗针。 |
| `fr_nvg` | `!nvg` | 无 | 开启或关闭夜视仪。 |
| `fr_wire` | `!wire` | `<color>` | 选择安包引线或拆包剪线颜色（`blue` / `yellow` / `red` / `green`）。 |

---

### 配置说明

配置文件采用严格 TOML 语法解析。支持 `[defaults]` 模板为所有模式提供基础属性继承：

```toml
# 全局设置
diagnostics = false # 诊断日志开关

# 多语言配置
[i18n]
default_language = "zh-CN" # 服务端缺省语言（"zh-CN" 或 "en"）

# 可选命名武器组
# [weapon_groups]
# basic = ["famas", "p250"]

# 全局默认模板（模式内未设置的字段将继承此处）
[defaults]
gravity = 1.0
bunnyhop = false

[[modes]]
id = "example"              # 模式唯一标识
enabled = true              # 是否启用该模式
name_key = "mode.example.name" # 多语言名称键
description_key = "mode.example.description" # 多语言说明键
weapons = ["famas", "p250"] # 规范物品 ID

# 阵营装备配发与清理
clear_weapons = "all"       # 配发前清理武器的队伍 (none / all / terrorists / counter_terrorists)
give_weapons = "all"        # 配发候选武器的队伍
give_knife = "all"          # 配发刀具的队伍

# 基础属性与移动
health = 120                # 初始生命值 (1～10000 或随机范围 { min = 100, max = 150 })
armor = 50                  # 初始护甲值 (0～1000 或随机范围 { min = 0, max = 100 })
gravity = 1.0               # 重力倍率 ((0, 10]，1.0 为正常重力)
speed = 1.0                 # 移速倍率 ((0, 10])
self_push = true            # 开启开火自推反冲

# 射击与视角控制
ammo = "reserve"            # 弹药规则 (inherit / reserve / clip / single_shot)
headshots = "normal"        # 爆头规则 (normal / only / disabled)
fast_switch = true          # 快速切枪开关

# 可选玩法突变示例
# autostrafe = true         # 空中辅助转向加速
# explosive_bullets = { cooldown_seconds = 1.5, damage = 30, radius = 120 } # 爆炸子弹
# wall_hang = true          # 空中贴墙挂墙
# grapple = { max_distance = 1000.0, max_speed = 600.0, pull_gain = 4.0 } # 抓钩绳索
# lifesteal = true          # 吸血恢复
# third_person = { distance = 64.0, follow_eyes = true, return_strength = 2.0 } # 第三人称
```

<details>
<summary><b>查看完整配置选项注释</b></summary>

请直接查阅 [config/funrounds.example.toml](config/funrounds.example.toml) 获取完整的字段说明，涵盖小鸡生态、伤害缩放曲线、幽灵隐身、特殊 C4、购买与经济等全部选项。
</details>

---

### 源码编译

#### 1. 编译 Rust 插件

工具链要求：Rust `1.99.1`（由 `rust-toolchain.toml` 自动匹配）：

```bash
# 代码格式化与 Clippy 严格静态检查
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# 执行全量自动化单元测试
cargo test --workspace --locked

# 编译 Release 发布产物
cargo build --release --workspace --locked
```

#### 2. 编译原生 C++ 伴侣扩展

要求 Python 3.10+、CMake 3.25+ 以及对应平台的 C++20 编译器（MSVC 2022+ 或 GCC 11+）：

```bash
# 构建原生扩展并输出至 dist/native
python scripts/build-native.py --output-dir dist/native
```

---

### 开源协议与作者

- **作者**: Mai Ooizumi ([@Mai-Ooizumi](https://github.com/Mai-Ooizumi))
- **开源协议**: GNU Affero General Public License v3.0 or later ([AGPL-3.0-or-later](LICENSE))
