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

Escape the Valley 是一款类似于《俄勒冈小径》的生存游戏，可在您的终端中运行。带领一群定居者穿越程序生成的荒野。在应对事件、危险和艰难选择时，管理食物、水、马车状况和士气。

可选的 AI 游戏主持人（由 Ollama 提供支持）以三种不同的叙事风格讲述您的旅程。可选的 XRPL 测试网络账本背包会跟踪您的物资变化，作为链上收据——证明您幸存下来，或者证明您尝试过。

## 最新内容

**1.3.1** — CLI 运行（`trail new` / `trail continue`）现在会显示记忆卡和标准死亡原因，就像 TUI 一样（`trail stats`、小径账本死亡天数和 GM 记忆摘要在两种模式下都有效）。TUI 丢弃货物报告会显示实际丢弃的货物。分叉路线提示不再消耗一回合的马车维护或逃生阀冷却时间。在相同的种子上重新开始游戏，会获得不同的 `run_id`，用于 XRPL 备忘录。GitHub 发布 / `npx` 二进制文件现在捆绑了 `xrpl-py`。Windows 管道输出（`cp1252`）不再因 Rich 框绘制字符而崩溃（#19）。

**1.3.0** — 四种您可以真正达成的结局，包括“饱经风霜”。马车是一种威胁，而不是整个游戏。初冬会减缓一个标记年份。沙漠中的浅水区会避开沙地。狩猎会记录谁受伤。`npx @mcptoolshop/escape-the-valley` 启动 v1.3.0 二进制文件（它一直停留在已死的 v1.1.1 GitHub 构件上）。

**1.2.1** — PyPI 轮子实际上会上传（Metadata-Version 2.3）。与 1.2.0 相同。

**1.2.0** — GitHub 发布二进制文件启动（事件库 + 样式表捆绑；smoke 断言 `--help` 和 ≥200 个事件）。`trail ledger proof` 审核加载的存档。TUI 显示种子/教义/转折/士气；`c` 循环速度；账本菜单 `R` 证明此存档。HUD 在 80×24 和 120×30 时可读。

**1.1.1** — `pip install "escape-the-valley[voice]"` 实际上会安装（额外的包已固定到一个未发布的包）。PyInstaller 二进制文件不再引入语音扩展。

**1.1.0** — 流式叙事、分级结局、可能导致受伤的事件、链上对账证明、运行构件。

附加到 v1.1.0 和 v1.1.1 GitHub 发布中的二进制文件无法启动（冻结的入口点中的相对导入）。使用 `pip install escape-the-valley` 或 v1.2.0+ GitHub 二进制文件 / `npx` 启动器。

## 快速开始

```bash
pip install escape-the-valley

# Zero-prerequisite npm launcher (GitHub v1.3.1 binary; v1.1.0 and v1.1.1
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

# Without AI narration (deterministic mode)
trail tui --seed 42 --gm-off

# Use a specific Ollama model
trail tui --seed 42 --model mistral
```

## 如何玩

每回合，您从营地中选择一个动作：

| 动作 | 作用 |
|--------|-------------|
| **Travel** | 向山谷出口移动。消耗食物和水。存在故障和事件的风险。 |
| **Rest** | 治疗队伍，恢复士气。消耗物资，但没有进展。 |
| **Hunt** | 消耗弹药以增加获得食物的机会。在森林和草原中效果更好。 |
| **Repair** | 消耗一个备用零件以修复马车。对于生存至关重要。 |

**事件**会中断旅程，并提供选择（A/B/C）。谨慎的选择更安全，但会消耗时间。大胆的选择更快，但风险更高。没有总是正确的答案。

**马车就是一切。**如果它在没有零件的情况下损坏，游戏就结束了。保持其状况在一半以上，并在维护期间（休息然后修理）进行维护，以获得临时的故障抵抗力。

**速度**控制速度与安全性。稳定是默认设置。快速速度可以覆盖更多的地面，但会消耗更多的物资并更快地损坏马车。

**逃生阀**（硬性配给、绝望的修理、丢弃货物）用于紧急情况。它们具有副作用和冷却时间——是最后的手段，而不是策略。

有关更深入的提示，请参阅[生存指南](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/)。

## GM 配置文件

AI 叙述者塑造的是语气，而不是机制。所有三个配置文件都玩相同的游戏。

- **编年史家** — 务实、实际、简洁。极少有民间传说。报告发生了什么。
- **篝火旁** — 严肃的篝火叙述者。微妙的怪异时刻。默认设置。
- **灯笼持有人** — 怪异且超脱，但仍然以后果为基础。最奇怪的一个。

使用 `--gm-profile` 设置：`trail tui --gm-profile lantern`

## 物资

游戏跟踪两种类别中的 12 种资源类型：

**消耗品：**食物、水、木柴、药品、盐、弹药、灯笼油、布

**装备：**零件、绳索、工具、靴子

5 种核心物资（食物、水、药品、弹药、零件）是最重要的。扩展物资，如木柴、盐、灯笼油和布，增加了深度：木柴为夜间营地提供燃料，盐可以防止食物腐烂，灯笼油可以实现更安全的夜间旅行，布可以修补装备和马车罩。

## 账本背包（可选）

账本背包跟踪您的 5 种核心物资（食物、水、药品、弹药、零件），作为 XRPL 测试网络上的令牌。每个城镇检查点都会在链上记录一个结算收据。在您的游戏结束时，您的旅程账本将包含任何人都可以验证的交易 ID。

完全可选。关闭时，游戏玩法完全相同（默认设置）。从 TUI 中的 L 菜单或通过 CLI 启用它：

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

在 TUI 中，**L** 打开账本菜单；启用背包后，**R** 证明此存档（相同的结果）。它不会休息。

捆绑在 `npx` / GitHub 发布二进制文件中；对于 `pip` 安装，运行 `pip install "escape-the-valley[xrpl]"`（或 `pip install -e ".[xrpl]"`）以获取 `xrpl-py` 依赖项。

## 命令

| 命令 | 描述 |
|---------|-------------|
| `trail tui` | 启动全屏文本 UI |
| `trail new` | 开始新的游戏（经典 CLI 模式） |
| `trail play` | 继续已保存的游戏（经典 CLI 模式） |
| `trail status` | 显示队伍、马车和物资 |
| `trail journal` | 显示最近的日志条目 |
| `trail self-check` | 检查游戏环境的健康状况 |
| `trail version` | 显示版本 |
| `trail licenses` | 显示许可和第三方通知（或 `trail --licenses`） |
| `trail ledger status` | 显示背包状态 |
| `trail ledger enable` | 启用 XRPL 背包 |
| `trail ledger disable` | 禁用 XRPL 背包 |
| `trail ledger settle` | 手动结算检查点 |
| `trail ledger reconcile` | 重试失败的交易 |
| `trail ledger proof` | 验证加载的存档（通过/失败/无法确定） |
| `trail ledger wallet` | 显示钱包详情 |
| `trail stats` | 显示运行统计数据和记忆卡（支持 `--json`） |
| `trail parcel send <addr> <supply> <amount>` | 向其他旅行者发送补给 |
| `trail parcel list` | 列出收到的包裹 |
| `trail parcel accept <id>` | 接受待处理的包裹 |
| `trail parcel sent` | 列出您已发送的包裹 |
| `trail wallet share` | 打印您的钱包地址以用于交易 |

## 警告提示

默认情况下，游戏会显示详细的警告，以帮助新玩家尽早发现危险。经验丰富的玩家可以切换到精简模式，该模式仅显示临界警告（最后一刻的关键威胁）：

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## 故障排除

**如果出现任何问题，请首先运行 `trail self-check`。** 它会报告 Ollama 是否可访问、您的存档是否加载以及安装了哪个模型。 可能出现以下三种问题：

| 症状 | 原因 | 解决方法 |
|---------|-------|-----|
| **Generic / no narration** | Ollama 未运行（GM 是可选的，并且会回退，不会导致程序崩溃） | 启动 Ollama（`ollama serve`），或以确定性方式使用 `--gm-off`。运行 `trail self-check` 以确认。 |
| **账本待处理/交易失败** | XRPL 测试网络是一个公共测试网络，有时会不稳定 | `trail ledger reconcile` 会重试失败的交易；当网络恢复时，再次运行它。无论如何，本地的补给都是正确的。 |
| **Save won't resume** | `run.json` 在写入过程中被截断或损坏 | 引擎会将其隔离为 `run.json.corrupt-<timestamp>`，然后再拒绝它，因此您的下一个存档无法覆盖证据。从该备份中恢复，或从种子开始新的游戏。 |

第一个叙述回合会加载模型，可能需要 10-30 秒——这是正常的，不是卡顿。完整详情：[故障排除手册](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/)。

## 要求

- Python 3.11+
- Ollama（可选，用于 AI 叙述）
- xrpl-py（可选，用于 `pip` 安装上的账本背包；包含在 `npx` / GitHub 发布二进制文件中）

## 安全性

没有遥测数据。没有账户。默认情况下，GM 叙述已启用（HTTP 连接到本地 Ollama）；传递 `--gm-off` 以禁用。XRPL 默认关闭，直到 `trail ledger enable`（仅限测试网络）。语音是可选的（`--voice`）。有关完整的威胁模型，请参阅 [SECURITY.md](SECURITY.md)。

## 许可证

MIT — 运行 `trail licenses`（或 `trail --licenses`）以查看游戏的许可证和第三方依赖项通知（也附加到每个 GitHub 发布版本中，每个平台一个文件：`THIRD_PARTY_NOTICES-<os>-<arch>.txt`）。

由 <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a> 构建
