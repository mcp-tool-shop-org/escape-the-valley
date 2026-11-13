<p align="center">
  <a href="README.md">English</a> | <a href="README.zh.md">中文</a> | <a href="README.es.md">Español</a> | <a href="README.fr.md">Français</a> | <a href="README.hi.md">हिन्दी</a> | <a href="README.it.md">Italiano</a> | <a href="README.pt-BR.md">Português (BR)</a>
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

## これは何ですか？

Escape the Valleyは、ターミナルで動作する、オレゴントレイル風のサバイバルゲームです。手続き的に生成された荒野を旅する開拓者のグループを率いましょう。イベント、危険、そして難しい選択を乗り越えながら、食料、水、馬車の状態、士気を管理します。

オプションのAIゲームマスター（Ollamaによって提供）が、3つの異なる語り口であなたの旅を語ります。オプションのXRPL Testnet台帳バックパックは、オンチェーンのレシート（あなたが生き残った、または試みたことの証明）として、あなたの物資の変化を追跡します。

## 新機能

**1.3.3** — 分岐点でセーブデータをロードすると、中断した場所からキャンプに戻るので、ロードされたゲームは、一度も閉じなかったゲームと同じように進行します（#41）。すべての死亡は、休息中、狩り中、修理中、食料の配給中、またはイベントによる負傷など、その原因とともに記録され、トレイル台帳、統計、およびゲームマスターの記憶に記録されます（#42）。

**1.3.2** — `event_id`ごとのクールダウン（`N=4`）により、同じイベントの基本構造が連続したターンまたはほぼ連続したターンで繰り返されるのを防ぎます（#21。1.3.1 → 1.3.2では、繰り返しの後にイベントの抽選が行われるように変更されているため、1.3.1で保存されたゲームは、1.3.2にアップグレードした後、異なるように進行します。同じバージョン内では、同じシードと選択を行うと、同じゲームが進行します）。GMをオフにしたCLIの「結果」パネルは、デフォルトの概要を一度だけ表示し、選択肢のヒントは末尾の二重句読点を削除し、`1 mile remaining.`は単数形を使用します。破損した`.trail/secrets.json`ファイルは、書き換え前に隔離されます（#31.7）。`THIRD_PARTY_NOTICES*.txt`と`trail licenses`の出力は、すべてのプラットフォームでLFに正規化され、CIはダウンロードされたpython-build-standaloneアーカイブに対して、バンドルされたライセンスの整合性を検証します（#31.1、#31.2、#31.3、#31.5）。

**1.3.1** — CLIで実行するゲーム（`trail new` / `trail continue`）では、TUIと同様に、メモリカードと死亡原因が表示されます（`trail stats`、トレイル台帳の死亡日数、およびGMのメモは両方のモードで機能します）。TUIの「荷物を放棄」機能では、実際に放棄された荷物が表示されます。分岐点のルートのプロンプトでは、馬車のメンテナンスターンや緊急バルブのクールダウンは消費されなくなりました。同じシードでリプレイを実行すると、異なる`run_id`が生成され、XRPLのメモに記録されます。GitHubリリース/`npx`バイナリには、`xrpl-py`がバンドルされています。Windowsでパイプされた出力（`cp1252`）を実行しても、Richのボックス描画文字でクラッシュしなくなりました（#19）。

**1.3.0** — 実際に到達できる4つのエンディング（耐え忍ぶエンディングを含む）。馬車はゲーム全体ではなく、脅威です。初期の冬は、指定された年を遅らせます。砂漠の浅瀬は、砂の上から離れます。狩りの際、誰が怪我をしたかを表示します。`npx @mcptoolshop/escape-the-valley`は、v1.3.0バイナリを起動します（以前は、死んだv1.1.1 GitHubアーティファクトに引っかかっていた）。

**1.2.1** — PyPIホイールが実際にアップロードされます（Metadata-Version 2.3）。1.2.0と同じ製品です。

**1.2.0** — GitHubリリースバイナリが起動します（イベントライブラリとスタイルシートがバンドルされています。スモークテストで、`--help`と200以上のイベントが確認されます）。`trail ledger proof`がロードされたセーブデータを監査します。TUIには、シード/ドクトリン/ひねり/士気が表示されます。`c`がペースをサイクルします。台帳メニューの`R`が、このセーブを証明します。HUDは、80×24および120×30の解像度で読みやすくなっています。

**1.1.1** — `pip install "escape-the-valley[voice]"`が実際にインストールされます（追加パッケージが、未公開のパッケージに固定されていました）。PyInstallerバイナリは、音声の追加パッケージを読み込みません。

**1.1.0** — ストリーミングナレーション、段階的なエンディング、負傷を引き起こすイベント、台帳上の和解の証明、ゲームのアーティファクト。

v1.1.0とv1.1.1のGitHubリリースに添付されているバイナリは起動しません（フリーズされたエントリポイントにある相対インポート）。`pip install escape-the-valley`またはv1.2.0+GitHubバイナリ/`npx`ランチャーを使用してください。

## クイックスタート

```bash
pip install escape-the-valley

# Zero-prerequisite npm launcher (GitHub v1.3.3 binary; v1.1.0 and v1.1.1
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

## 遊び方

各ターン、キャンプからアクションを選択します。

| アクション | その効果 |
|--------|-------------|
| **Travel** | 谷の出口に向かって移動します。食料と水を消費します。故障とイベントのリスクがあります。 |
| **Rest** | パーティーを回復させ、士気を回復させます。物資を消費しますが、進行はありません。 |
| **Hunt** | 弾薬を消費して、食料を入手する機会を得ます。森や平原で効果的です。 |
| **Repair** | スペアパーツを消費して、馬車を修理します。生存に不可欠です。 |

**イベント**は、旅を中断し、選択肢（A/B/C）を提示します。慎重な選択肢はより安全ですが、時間がかかります。大胆な選択肢はより速いですが、リスクがあります。常に正しい答えはありません。

**馬車はすべてです。** 部品がなくなって故障した場合、ゲームは終了します。馬車の状態を半分以上に保ち、一時的な故障に対する耐性を得るために、メンテナンスの時間を設けて（休息してから修理）ください。

**ペース**は、速度と安全性のバランスを制御します。デフォルトは「安定」です。速いペースは、より多くの距離を移動しますが、より多くの物資を消費し、馬車の故障を早めます。

**緊急バルブ**（食料の配給、緊急修理、荷物の放棄）は、緊急時に使用できます。これらには副作用とクールダウンがあり、最終手段であり、戦略ではありません。

より詳細なヒントについては、[サバイバルガイド](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/)を参照してください。

## GMプロファイル

AIナレーターは、ゲームのメカニクスではなく、トーンを決定します。3つのプロファイルはすべて同じゲームをプレイします。

- **Chronicler** — 落ち着いていて、実用的で、控えめです。民話は最小限です。何が起こったかを報告します。
- **Fireside** — 深刻なキャンプファイヤーの語り手。微妙な不気味な瞬間があります。デフォルトです。
- **Lantern-Bearer** — 不気味で、境界線上にありますが、それでも結果に根ざしています。奇妙なプロファイルです。

`--gm-profile`で設定：`trail tui --gm-profile lantern`

## 物資

ゲームは、2つのカテゴリに分類された12種類の資源を追跡します。

**消耗品：** 食料、水、薪、医薬品、塩、弾薬、ランタンオイル、布

**装備：** 部品、ロープ、道具、ブーツ

5つの主要な物資（食料、水、医薬品、弾薬、部品）が最も重要です。薪、塩、ランタンオイル、布などの追加の物資は、深みを加えています。薪は夜のキャンプを暖め、塩は食料の腐敗を防ぎ、ランタンオイルは夜間の安全な移動を可能にし、布は装備や馬車のカバーを修復します。

## Ledger Backpack（オプション）

Ledger Backpackは、5つの主要な物資（食料、水、医薬品、弾薬、部品）を、XRPL Testnet上のトークンとして追跡します。すべての町のチェックポイントで、入植地のレシートがオンチェーンに記録されます。ゲームの最後に、あなたのtrail台帳には、誰でも検証できるトランザクションIDが含まれます。

完全にオプションです。機能をオフにすると、ゲームのプレイは同じになります（デフォルト）。TUIのLメニューから、またはCLI経由で機能を有効にします。

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

TUIでは、**L**を押すと台帳メニューが開きます。バックパックがオンになっている場合、**R**を押すと、現在のセーブデータが検証されます（結果は同じ）。休憩は行いません。

`npx` / GitHub リリースバイナリに同梱されています。`pip`にインストールする場合は、`pip install "escape-the-valley[xrpl]"`（または`pip install -e ".[xrpl]"`）を実行して、`xrpl-py`の依存関係を解決してください。

## コマンド

| コマンド | 説明 |
|---------|-------------|
| `trail tui` | フルスクリーン Textual UIを起動します |
| `trail new` | 新しいゲームを開始します（従来のCLIモード） |
| `trail play` | 保存されたゲームを続行します（従来のCLIモード） |
| `trail status` | パーティー、ワゴン、および物資を表示します |
| `trail journal` | 最近のジャーナルエントリを表示します |
| `trail self-check` | ゲーム環境の状態を確認します |
| `trail version` | バージョンを表示します |
| `trail licenses` | ライセンスとサードパーティの通知を表示します（または`trail --licenses`） |
| `trail ledger status` | バックパックの状態を表示します |
| `trail ledger enable` | XRPLバックパックを有効にします |
| `trail ledger disable` | XRPLバックパックを無効にします |
| `trail ledger settle` | チェックポイントを手動で確定します |
| `trail ledger reconcile` | 失敗した確定処理を再試行します |
| `trail ledger proof` | ロードされたセーブデータを検証します（PASS/FAIL/INCONCLUSIVE） |
| `trail ledger wallet` | ウォレットの詳細を表示します |
| `trail stats` | ゲームの統計情報とメモリカードを表示します（`--json`をサポート） |
| `trail parcel send <addr> <supply> <amount>` | 別のプレイヤーに物資を送ります |
| `trail parcel list` | 受信した荷物のリストを表示します |
| `trail parcel accept <id>` | 保留中の荷物を受け取ります |
| `trail parcel sent` | 送信した荷物のリストを表示します |
| `trail wallet share` | 取引のためにウォレットアドレスを表示します |

## 警告表示

デフォルトでは、ゲームは詳細な警告を表示し、新しいプレイヤーが危険を早期に発見できるようにします。経験豊富なプレイヤーは、最小限のモードに切り替えることができます。このモードでは、危険が差し迫った場合にのみ警告が表示されます（最後の瞬間、重大な脅威）。

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## トラブルシューティング

**問題が発生した場合は、最初に`trail self-check`を実行してください。** これにより、Ollamaにアクセスできるかどうか、セーブデータがロードできるかどうか、およびどのモデルがインストールされているかが報告されます。問題が発生する主な原因は3つです。

| 症状 | 原因 | 解決策 |
|---------|-------|-----|
| **Generic / no narration** | Ollamaが実行されていません（GMはオプションであり、デフォルトで動作し、ゲームを完全に動作不能にすることはありません） | Ollama（`ollama serve`）を開始するか、`--gm-off`を使用して決定的にプレイします。`trail self-check`を実行して確認します。 |
| **台帳の確定処理が保留中/失敗** | XRPL Testnetはパブリックテストネットワークであり、不安定になる場合があります | `trail ledger reconcile`は、失敗した確定処理を再試行します。ネットワークが回復したら、再度実行してください。いずれの場合も、物資はローカルで正しく処理されます。 |
| **Save won't resume** | `run.json`が書き込み中に切り捨てられたか、破損しました | エンジンは、これを拒否する前に、`run.json.corrupt-<timestamp>`として隔離します。そのため、次のセーブデータで証拠が上書きされることはありません。バックアップから復元するか、シードから新しいゲームを開始してください。 |

最初にナレーションされたターンでは、モデルがロードされ、10〜30秒かかる場合があります。これは正常であり、フリーズではありません。詳細については、[トラブルシューティングハンドブック](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/)を参照してください。

## 要件

- Python 3.11+
- Ollama（オプション、AIナレーション用）
- xrpl-py（オプション、`pip`にインストールされた場合の台帳バックパック用。`npx` / GitHub リリースバイナリに同梱）

## セキュリティ

テレメトリーはありません。アカウントもありません。GMナレーションはデフォルトでオンになっています（ローカルのOllamaへのHTTP）。機能を無効にするには、`--gm-off`を渡します。XRPLは、`trail ledger enable`（Testnetのみ）になるまでオフになっています。音声はオプトインです（`--voice`）。完全な脅威モデルについては、[SECURITY.md](SECURITY.md)を参照してください。

## ライセンス

MIT — `trail licenses`（または`trail --licenses`）を実行して、ゲームのライセンスとサードパーティの依存関係の通知を表示します（各GitHubリリースにも添付され、プラットフォームごとに1つのファイルがあります：`THIRD_PARTY_NOTICES-<os>-<arch>.txt`）。

作成者：<a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
