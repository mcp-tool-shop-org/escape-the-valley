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

Escape the Valley 是一款《俄勒冈小径》风格的生存游戏，可在您的终端中运行。带领一群定居者穿越程序生成的荒野。在应对事件、危险和艰难选择时，管理食物、水、马车状况和士气。

可选的 AI 游戏主持人（由 Ollama 提供支持）以三种不同的叙事风格讲述您的旅程。可选的 XRPL Testnet 背包记录您的物资变化，作为链上收据——证明您幸存下来，或者证明您尝试过。

## 最新内容

**1.3.2** — 每个 `event_id` 的冷却时间（`N=4`）可防止相同的事件模板在连续或几乎连续的回合中重复出现（#21；1.3.1 → 1.3.2 将事件抽取的时机调整到重复发生之后，因此在 1.3.1 中保存的游戏在升级到 1.3.2 后，游戏进程会发生变化，因为事件的抽取时机发生了变化。在同一版本中，相同的种子和相同的选择会产生相同的结果）。关闭 GM 的 CLI 结果面板会一次性显示备用摘要（#20），选择提示会删除尾部的双重标点符号，并且 `1 mile remaining.` 使用单数语法。损坏的 `.trail/secrets.json` 文件会在重写之前被隔离（#31.7）。`THIRD_PARTY_NOTICES*.txt` 和 `trail licenses` 的输出在所有平台上都标准化为 LF 格式，并且 CI 会验证已包含的许可证的完整性，与下载的 python-build-standalone 存档进行比较（#31.1、#31.2、#31.3、#31.5）。

**1.3.1** — CLI 运行（`trail new` / `trail continue`）现在会生成记忆卡和规范的死亡原因，就像 TUI 一样（`trail stats`、小径记录死亡天数和 GM 记忆摘要在两种模式下都有效）。TUI 丢弃货物报告会显示实际丢弃的货物。分叉路线提示不再消耗一回合的马车维护时间或安全阀冷却时间。在相同的种子下重新开始的游戏会获得不同的 `run_id`，用于 XRPL 备忘录。GitHub 发布 / `npx` 二进制文件现在捆绑了 `xrpl-py`。Windows 管道输出（`cp1252`）不再因 Rich 框绘制字符而崩溃（#19）。

**1.3.0** — 四种您可以真正达成的结局，包括“饱经风霜”。马车是一种威胁，而不是整个游戏。早期冬季会减缓一个标记年份的进程。沙漠中的浅滩不会出现在沙滩上。狩猎会显示谁受伤了。`npx @mcptoolshop/escape-the-valley` 启动 v1.3.0 二进制文件（此前它一直停留在已失效的 v1.1.1 GitHub 构件上）。

**1.2.1** — PyPI 轮子实际上会上传（Metadata-Version 2.3）。与 1.2.0 相同的产品。

**1.2.0** — GitHub 发布二进制文件启动（事件库 + 样式表捆绑；模拟断言 `--help` 和 ≥200 个事件）。`trail ledger proof` 审核加载的存档。TUI 显示种子/教条/转折/士气；`c` 循环播放节奏；记录菜单 `R` 证明此存档。HUD 在 80×24 和 120×30 的分辨率下可读。

**1.1.1** — `pip install "escape-the-valley[voice]"` 实际上会安装（额外的包已固定到一个未发布的包）。PyInstaller 二进制文件不再引入语音扩展包。

**1.1.0** — 流式叙事、分级结局、可能导致受伤的事件、链上对账证明、运行构件。

附加到 v1.1.0 和 v1.1.1 GitHub 发布中的二进制文件无法启动（冻结的入口点中的相对导入）。使用 `pip install escape-the-valley` 或 v1.2.0+ GitHub 二进制文件 / `npx` 启动器。

## 快速开始

```bash
pip install escape-the-valley

# Zero-prerequisite npm launcher (GitHub v1.3.2 binary; v1.1.0 and v1.1.1
# artifacts do not start):
#   npx @mcptoolshop/escape-the-valley tui --seed 42

# Launch the full-screen TUI (recommended)
trail tui --seed 42

# Resume a saved game
trail tui --continue

# Spoken voice (opt-in; needs pip install "escape-the-valley[voice]").
# AI narration is already on by default when Ollama is running;
# pass --gm-off to disable the GM. --voice does not turn the GM on.
trail tui --seed 42 --voice

# With voice pacing control
trail tui --seed 42 --voice --voice-pace slow

# Without AI narration (deterministic mode; same seed and choices give the same run in this version)
trail tui --seed 42 --gm-off

# Use a specific Ollama model
trail tui --seed 42 --model mistral
```

## 如何玩

每个回合，您都会从营地中选择一个动作：

| 动作 | 它会做什么 |
|--------|-------------|
| **Travel** | 向山谷出口移动。消耗食物和水。存在故障和事件的风险。 |
| **Rest** | 治疗队伍，恢复士气。消耗物资，但不会取得进展。 |
| **Hunt** | 消耗弹药以增加获得食物的机会。在森林和草原中效果更好。 |
| **Repair** | 使用备用零件修理马车。对于生存至关重要。 |

**事件**会中断旅程，并提供选择（A/B/C）。谨慎的选择更安全，但会消耗时间。大胆的选择更快，但风险更高。没有总是正确的答案。

**马车就是一切。**如果它在没有零件的情况下发生故障，游戏就结束了。保持其状况高于一半，并在维护期间（休息然后修理）以获得临时的故障抵抗力。

**节奏**控制速度与安全。稳定是默认设置。快速节奏可以覆盖更多的地面，但会消耗更多的物资并更快地损坏马车。

**安全阀**（硬性配给、绝望的修理、丢弃货物）用于应对紧急情况。它们具有副作用和冷却时间——是最后的手段，而不是策略。

如需更深入的提示，请参阅[生存指南](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/)。

## GM 配置文件

AI 叙述者决定了语气，而不是机制。所有三个配置文件都玩相同的游戏。

- **Chronicler** — 务实、实际、简洁。极少有民间传说。报告发生了什么。
- **Fireside** — 严肃的篝火叙述者。微妙的怪异时刻。默认设置。
- **Lantern-Bearer** — 怪异且超脱，但仍然以后果为基础。最奇怪的一个。

使用 `--gm-profile` 设置：`trail tui --gm-profile lantern`

## 物资

游戏跟踪两种类别中的 12 种资源类型：

**消耗品：**食物、水、木柴、药品、盐、弹药、油灯油、布

**装备：**零件、绳索、工具、靴子

5 种核心物资（食物、水、药品、弹药、零件）是最重要的。扩展物资，如木柴、盐、油灯油和布，增加了深度：木柴为夜间营地提供燃料，盐可以防止食物腐烂，油灯油可以实现更安全的夜间旅行，布可以修补装备和马车罩。

## Ledger Backpack（可选）

Ledger Backpack 跟踪您的 5 种核心物资（食物、水、药品、弹药、零件），作为 XRPL Testnet 上的令牌。每个城镇检查点都会在链上记录一个结算收据。在您的游戏结束时，您的 trail 记录将包含任何人都可以验证的交易 ID。

完全可选。关闭它时，游戏玩法完全相同（默认设置）。从 TUI 中的 L 菜单或通过 CLI 启用它：

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

在 TUI 中，**L** 会打开记录菜单；启用背包后，**R** 会证明此存档（结果相同）。它不会休息。

捆绑在 `npx` / GitHub 发布版本的二进制文件中；对于 `pip` 安装，运行 `pip install "escape-the-valley[xrpl]"`（或 `pip install -e ".[xrpl]"`）以获取 `xrpl-py` 依赖项。

## 命令

| 命令 | 描述 |
|---------|-------------|
| `trail tui` | 启动全屏 Textual UI |
| `trail new` | 开始新的游戏（经典 CLI 模式） |
| `trail play` | 继续已保存的游戏（经典 CLI 模式） |
| `trail status` | 显示队伍、车厢和补给 |
| `trail journal` | 显示最近的日志条目 |
| `trail self-check` | 检查游戏环境的运行状况 |
| `trail version` | 显示版本 |
| `trail licenses` | 显示许可协议和第三方声明（或 `trail --licenses`） |
| `trail ledger status` | 显示背包状态 |
| `trail ledger enable` | 启用 XRPL 背包 |
| `trail ledger disable` | 禁用 XRPL 背包 |
| `trail ledger settle` | 手动确认一个检查点 |
| `trail ledger reconcile` | 重试失败的确认 |
| `trail ledger proof` | 验证加载的存档（PASS/FAIL/INCONCLUSIVE） |
| `trail ledger wallet` | 显示钱包详情 |
| `trail stats` | 显示游戏统计信息和记忆卡（支持 `--json`） |
| `trail parcel send <addr> <supply> <amount>` | 将补给发送给其他旅行者 |
| `trail parcel list` | 列出收到的包裹 |
| `trail parcel accept <id>` | 接受待处理的包裹 |
| `trail parcel sent` | 列出您已发送的包裹 |
| `trail wallet share` | 打印您的钱包地址以进行交易 |

## 警告提示

默认情况下，游戏会显示详细的警告，以帮助新玩家尽早发现危险。经验丰富的玩家可以切换到最小模式，该模式仅显示临界警告（最后一刻、关键威胁）：

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## 故障排除

**如果出现任何问题，请首先运行 `trail self-check`。** 它会报告 Ollama 是否可访问、您的存档是否加载以及安装了哪个模型。 可能出现三种问题：

| 症状 | 原因 | 解决方法 |
|---------|-------|-----|
| **Generic / no narration** | Ollama 尚未运行（GM 是可选的，并且会回退，不会导致游戏崩溃） | 启动 Ollama（`ollama serve`），或者以确定性方式使用 `--gm-off`。 运行 `trail self-check` 以确认。 |
| **账本待处理/确认失败** | XRPL Testnet 是一个公共测试网络，有时不稳定 | `trail ledger reconcile` 会重试失败的确认；当网络恢复时，再次运行它。 无论如何，本地的补给都是正确的。 |
| **Save won't resume** | `run.json` 在写入过程中被截断或损坏 | 引擎会在拒绝之前将其隔离为 `run.json.corrupt-<timestamp>`，因此您的下一个存档无法覆盖证据。 从备份中恢复，或者从种子开始新的游戏。 |

第一个叙述回合会加载模型，可能需要 10-30 秒——这是正常的，不是卡顿。 完整详细信息：[故障排除手册](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/)。

## 要求

- Python 3.11+
- Ollama（可选，用于 AI 叙述）
- xrpl-py（可选，用于 `pip` 安装上的账本背包；捆绑在 `npx` / GitHub 发布版本的二进制文件中）

## 安全性

没有遥测数据。 没有帐户。 默认情况下，GM 叙述已启用（HTTP 连接到本地 Ollama）；传递 `--gm-off` 以禁用。 除非 `trail ledger enable`（仅限 Testnet），否则 XRPL 处于关闭状态。 语音是可选的（`--voice`）。 请参阅 [SECURITY.md](SECURITY.md) 以获取完整的威胁模型。

## 许可

MIT — 运行 `trail licenses`（或 `trail --licenses`）以查看游戏的许可协议和第三方依赖项声明（也附加到每个 GitHub 发布版本，每个平台一个文件：`THIRD_PARTY_NOTICES-<os>-<arch>.txt`）。

由 <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a> 构建
