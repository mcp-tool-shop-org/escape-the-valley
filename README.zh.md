<p align="center">
  <a href="README.ja.md">日本語</a> | <a href="README.md">English</a> | <a href="README.es.md">Español</a> | <a href="README.fr.md">Français</a> | <a href="README.hi.md">हिन्दी</a> | <a href="README.it.md">Italiano</a> | <a href="README.pt-BR.md">Português (BR)</a>
</p>

<p align="center">
  <img src="https://raw.githubusercontent.com/mcp-tool-shop-org/brand/main/logos/escape-the-valley/readme.png" width="400" alt="Ledger Trail: Escape the Valley">
</p>

<p align="center">
  <a href="https://github.com/mcp-tool-shop-org/escape-the-valley/actions"><img src="https://github.com/mcp-tool-shop-org/escape-the-valley/workflows/CI/badge.svg" alt="CI"></a>
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT License">
  <img src="https://img.shields.io/badge/version-1.0.0-green" alt="Version">
  <a href="https://mcp-tool-shop-org.github.io/escape-the-valley/"><img src="https://img.shields.io/badge/Landing_Page-live-blue" alt="Landing Page"></a>
</p>

<p align="center">
  <em>A survival game where the trail is the teacher and the ledger keeps you honest.</em>
</p>

---

## 什么是这个？

Escape the Valley 是一款类似于 Oregon Trail 的生存游戏，可在您的终端中运行。带领一群拓荒者穿越一个程序生成的荒野。在探索过程中，管理食物、水、马车状况和士气，并应对事件、危险和艰难的选择。

一个可选的 AI 游戏主持人（由 Ollama 提供支持）会用三种不同的叙述方式讲述您的旅程。一个可选的 XRPL 测试网账本会跟踪您的物资变化，作为链上记录——证明您生存了下来，或者证明您尝试过。

## 快速开始

```bash
pip install -e ".[dev]"

# Launch the full-screen TUI (recommended)
trail tui --seed 42

# Resume a saved game
trail tui --continue

# With AI narration (requires Ollama running locally)
trail tui --seed 42 --voice

# Without AI narration (deterministic mode)
trail tui --seed 42 --gm-off
```

## 如何玩

每回合，您从营地选择一个动作：

| 动作 | 作用 |
|--------|-------------|
| **Travel** | 向山谷出口移动。消耗食物和水。存在马车故障和事件的风险。 |
| **Rest** | 恢复队伍的健康，提高士气。消耗物资，但不前进。 |
| **Hunt** | 消耗弹药，有机会获得食物。在森林和草原中效果更好。 |
| **Repair** | 消耗备用零件来修复马车。对于生存至关重要。 |

**事件**会通过选择（A/B/C）中断旅行。谨慎的选择更安全，但会花费时间。大胆的选择更快，但风险更高。没有总是正确的答案。

**马车是关键。** 如果马车在没有备用零件的情况下发生故障，游戏结束。保持马车状况在半以上，并定期进行维护（休息后修复），以获得临时的抗故障能力。

**节奏**控制速度与安全之间的平衡。默认值为稳定。快速节奏可以覆盖更多距离，但会消耗更多物资，并更快地损坏马车。

**紧急出口**（如：严格配给、绝望的修复、放弃货物）用于紧急情况。它们会产生副作用和冷却时间——是最后的手段，而不是策略。

要了解更多技巧，请参阅[生存指南](docs/survival-guide.md)。

## 游戏主持人配置文件

AI 叙述者会影响语调，而不会改变游戏机制。所有三个配置文件都玩相同的游戏。

- **Chronicler（记录者）**：务实、简洁、朴实。很少使用民间传说。报告发生了什么。
- **Fireside（壁炉旁）**：一个严肃的篝火旁边的叙述者。有一些微妙的怪异时刻。默认选项。
- **Lantern-Bearer（灯笼守护者）**：怪异且介于两者之间，但仍然基于后果。这个比较奇怪。

使用 `--gm-profile` 参数设置：`trail tui --gm-profile lantern`

## 账本背包（可选）

账本背包会跟踪您 5 种核心物资（食物、水、药品、弹药、零件）在 XRPL 测试网上的状态，并将它们作为令牌记录。每个城镇检查点都会在链上记录一个结算记录。在您的游戏结束时，您的账本会包含任何人都可验证的交易 ID。

完全可选。如果禁用它，游戏会以完全相同的形式运行（默认状态）。您可以在 TUI 中的 L 菜单或通过 CLI 启用它：

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
```

需要安装 `pip install -e ".[xrpl]"`，以获取 `xrpl-py` 依赖项。

## 命令

| 命令 | 描述 |
|---------|-------------|
| `trail tui` | 启动全屏文本界面 |
| `trail new` | 开始新的游戏（经典 CLI 模式） |
| `trail play` | 继续已保存的游戏（经典 CLI 模式） |
| `trail status` | 显示队伍、马车和物资 |
| `trail journal` | 显示最近的日记条目 |
| `trail self-check` | 检查游戏环境的健康状况 |
| `trail version` | 显示版本号 |
| `trail ledger status` | 显示背包状态 |
| `trail ledger enable` | 启用 XRPL 背包 |
| `trail ledger disable` | 禁用 XRPL 背包 |
| `trail ledger settle` | 手动结算一个检查点 |
| `trail ledger reconcile` | 重试失败的结算 |
| `trail ledger wallet` | 显示钱包详细信息 |
| `trail parcel list` | 列出收到的包裹 |
| `trail parcel accept <id>` | 接受待处理的包裹 |

## 警告提示

默认情况下，游戏会显示详细的警告信息，以帮助新手玩家尽早发现危险。有经验的玩家可以切换到精简模式，该模式仅显示临界警告（最后一刻的、关键的威胁）：

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## 要求

- Python 3.11 及以上版本
- Ollama（可选，用于 AI 语音解说）
- xrpl-py（可选，用于账本备份）

## 安全性

无任何数据收集。无任何账户。所有网络功能（Ollama、XRPL）均为可选功能，默认禁用。XRPL 操作仅使用测试网络。有关完整的安全威胁模型，请参阅 [SECURITY.md](SECURITY.md)。

## 许可证

MIT

由 <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a> 构建。
