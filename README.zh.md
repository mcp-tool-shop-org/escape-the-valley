<p align="center">
  <a href="README.ja.md">日本語</a> | <a href="README.md">English</a> | <a href="README.es.md">Español</a> | <a href="README.fr.md">Français</a> | <a href="README.hi.md">हिन्दी</a> | <a href="README.it.md">Italiano</a> | <a href="README.pt-BR.md">Português (BR)</a>
</p>

<p align="center">
  <img src="https://raw.githubusercontent.com/mcp-tool-shop-org/brand/main/logos/escape-the-valley/readme.png" width="400" alt="Ledger Trail: Escape the Valley">
</p>

<p align="center">
  <a href="https://github.com/mcp-tool-shop-org/escape-the-valley/actions"><img src="https://github.com/mcp-tool-shop-org/escape-the-valley/workflows/CI/badge.svg" alt="CI"></a>
  <a href="https://pypi.org/project/escape-the-valley/"><img src="https://img.shields.io/pypi/v/escape-the-valley" alt="PyPI"></a>
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT License">
  <a href="https://mcp-tool-shop-org.github.io/escape-the-valley/"><img src="https://img.shields.io/badge/Landing_Page-live-blue" alt="Landing Page"></a>
</p>

<p align="center">
  <em>A survival game where the trail is the teacher and the ledger keeps you honest.</em>
</p>

---

## 这是什么？

《逃离山谷》是一款类似于俄勒冈小径的生存游戏，可在您的终端中运行。带领一群定居者穿越程序生成的荒野。在应对事件、危险和艰难选择时，管理食物、水、马车状况和士气。

可选的人工智能游戏主持人（由 Ollama 提供支持）以三种不同的叙事风格讲述您的旅程。一个可选的 XRPL 测试网络账本背包会跟踪您的物资变化，并将其记录为链上收据——证明您幸存下来，或者证明您尝试过。

## 1.1.0 版本的新内容

- **流式叙述**——游戏主持人逐个生成令牌，实时创作每个情节，而不是在暂停后输出一个完整的片段。
- **分级结局**——游戏结束时会有一个分级后的尾声（胜利、饱经风霜、惨胜或失败），根据幸存者、花费的时间以及旅程给您带来的损失来决定，而不仅仅是简单的死亡原因。
- **真正的风险**——事件现在可以伤害或杀死队伍成员。一个糟糕的选择可能会导致人员伤亡，并且死亡的原因会归结于其真实原因。
- **链上对账证明**——一种审计模式，可重播游戏中的结算收据，并将其与 XRPL 测试网络进行验证，以便独立检查物资历史记录。
- **游戏文物**——每次完成游戏后都会留下一个纪念品：一张 XRPL 明信片、您的统计数据以及导出/共享路径。

## 快速入门

```bash
pip install escape-the-valley

# Or, zero-prerequisite (no Python setup) via the npm launcher — downloads a
# verified binary and runs it:
#   npx @mcptoolshop/escape-the-valley tui --seed 42

# Launch the full-screen TUI (recommended)
trail tui --seed 42

# Resume a saved game
trail tui --continue

# With AI narration (requires Ollama running locally)
trail tui --seed 42 --voice

# Spoken voice narration needs the voice extra:
#   pip install "escape-the-valley[voice]"

# With voice pacing control
trail tui --seed 42 --voice --voice-pace slow

# Without AI narration (deterministic mode)
trail tui --seed 42 --gm-off

# Use a specific Ollama model
trail tui --seed 42 --model mistral
```

## 如何玩

在每个回合中，您都可以从营地中选择一项行动：

| 行动 | 作用 |
|--------|-------------|
| **Travel** | 向山谷出口移动。消耗食物和水。存在故障和事件的风险。 |
| **Rest** | 治疗队伍，恢复士气。消耗物资但不会推进游戏进程。 |
| **Hunt** | 消耗弹药以增加获得食物的机会。在森林和平原中效果更好。 |
| **Repair** | 使用备用零件修理马车。对于生存至关重要。 |

**事件**会通过选择（A/B/C）中断旅程。谨慎的选择更安全，但会消耗时间。大胆的选择更快，但也更有风险。没有总是正确的答案。

**马车至关重要。**如果它在没有零件的情况下损坏，游戏就结束了。保持其状况高于一半，并进行维护（休息然后修理），以获得临时的抗故障能力。

**节奏**控制速度与安全性。默认设置为稳定。快速节奏可以覆盖更多的距离，但会消耗更多的物资并更快地损坏马车。

**紧急措施**（减少配给、绝望的修理、放弃货物）可用于应对紧急情况。它们具有副作用和冷却时间——是最后的手段，而不是策略。

有关更深入的提示，请参阅[生存指南](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/)。

## 游戏主持人配置

人工智能叙述者会影响游戏的基调，而不是机制。所有三种配置都玩相同的游戏。

- **编年史家**——务实、实际、简洁。极少有民间传说。报告发生了什么。
- **篝火旁的人**——严肃的篝火叙述者。微妙的不祥时刻。默认设置。
- **灯笼持有人**——怪诞且超脱，但仍然以后果为基础。最奇怪的一个。

使用 `--gm-profile` 设置：`trail tui --gm-profile lantern`

## 物资

游戏会跟踪两种类别中的 12 种资源类型：

**消耗品：**食物、水、木柴、药品、盐、弹药、灯笼油、布料

**装备：**零件、绳索、工具、靴子

5 种核心物资（食物、水、药品、弹药、零件）是最重要的。扩展物资，如木柴、盐、灯笼油和布料，增加了游戏的深度：木柴为夜间营地提供燃料，盐可以防止食物腐烂，灯笼油可以在夜间更安全地旅行，而布料可以修补装备和马车罩。

## 账本背包（可选）

账本背包会将您的 5 种核心物资（食物、水、药品、弹药、零件）作为令牌记录在 XRPL 测试网络上。每个城镇检查点都会将结算收据记录到链上。在游戏结束时，您的旅程账本会包含任何人都可以验证的交易 ID。

完全可选。即使禁用它（默认设置），游戏也以相同的方式进行。可以通过 TUI 中的 L 菜单或 CLI 来启用它：

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
```

需要 `pip install -e ".[xrpl]"` 以安装 `xrpl-py` 依赖项。

## 命令

| 命令 | 描述 |
|---------|-------------|
| `trail tui` | 启动全屏文本用户界面 |
| `trail new` | 开始新的游戏（经典 CLI 模式） |
| `trail play` | 继续已保存的游戏（经典 CLI 模式） |
| `trail status` | 显示队伍、马车和物资 |
| `trail journal` | 显示最近的日志条目 |
| `trail self-check` | 检查游戏环境健康状况 |
| `trail version` | 显示版本 |
| `trail ledger status` | 显示背包状态 |
| `trail ledger enable` | 启用 XRPL 背包 |
| `trail ledger disable` | 禁用 XRPL 背包 |
| `trail ledger settle` | 手动结算检查点 |
| `trail ledger reconcile` | 重试失败的结算 |
| `trail ledger wallet` | 显示钱包详细信息 |
| `trail stats` | 显示游戏统计数据（支持 `--json`） |
| `trail parcel send <addr> <supply> <amount>` | 将物资发送给其他旅行者 |
| `trail parcel list` | 列出收到的包裹 |
| `trail parcel accept <id>` | 接受待处理的包裹 |
| `trail parcel sent` | 列出您已发送的包裹 |
| `trail wallet share` | 打印您的钱包地址以进行交易 |

## 警告提示

默认情况下，游戏会显示详细的警告，以帮助新玩家尽早发现危险。经验丰富的玩家可以切换到最小模式，该模式只会显示临界警告（最后一刻、关键威胁）：

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## 故障排除

**如果任何内容看起来不正确，请首先运行 `trail self-check`。**它会报告 Ollama 是否可访问、您的存档是否可以加载以及安装了哪个模型。以下是可能出现的三种问题：

| 症状 | 原因 | 解决方法 |
|---------|-------|-----|
| **Generic / no narration** | Ollama 未运行（GM 是可选的，并且会回退，不会导致程序崩溃） | 启动 Ollama (`ollama serve`)，或者使用 `--gm-off` 参数进行确定性操作。运行 `trail self-check` 以确认。 |
| **账本待处理/结算失败** | XRPL 测试网络是一个公共测试网络，有时会不稳定。 | `trail ledger reconcile` 会重试失败的结算；当网络恢复时，再次运行它。无论如何，本地供应量都是正确的。 |
| **Save won't resume** | `run.json` 在写入过程中被截断或损坏。 | 引擎会在拒绝该文件之前将其隔离为 `run.json.corrupt-<时间戳>`，这样你的下一次保存操作就不会覆盖证据。从备份中恢复，或者从种子开始新的运行。 |

第一个叙述回合会加载模型，这可能需要 10-30 秒——这是正常的，不是程序卡死。完整详情：[故障排除手册](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/)。

## 要求

- Python 3.11+
- Ollama（可选，用于 AI 叙述）
- xrpl-py（可选，用于账本背包）

## 安全性

不收集遥测数据。没有账户。所有网络功能（Ollama、XRPL）都是选择加入的，并且默认情况下已禁用。XRPL 操作仅使用测试网。有关完整的威胁模型，请参阅 [SECURITY.md](SECURITY.md)。

## 许可证

MIT

由 <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a> 构建
