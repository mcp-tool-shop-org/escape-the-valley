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

「Escape the Valley」は、ターミナルで動作する「Oregon Trail」スタイルのサバイバルゲームです。手続き的に生成された荒野を旅する開拓者のグループを率いましょう。イベント、危険、そして難しい選択を乗り越えながら、食料、水、馬車の状態、士気を管理します。

オプションのAIゲームマスター（Ollama製）が、3つの異なる語り口であなたの旅を語ります。オプションのXRPLテストネット台帳バックパックは、オンチェーンのレシート（あなたが生き残った、または試みたという証拠）として、あなたの物資の変化を追跡します。

## 新機能

**1.3.1** — CLI実行（`trail new` / `trail continue`）が、TUIと同様に、メモリカードと死因を出力するようになりました（`trail stats`、Trail Ledgerの死亡日数、およびGMのメモは両方のモードで機能します）。TUIの「Abandon Cargo」は、実際に放棄された貨物を報告します。フォークルートのプロンプトは、馬車のメンテナンスまたは緊急脱出バルブのクールダウンに1ターンを消費しなくなりました。同じシードでリプレイを実行すると、XRPLメモに異なる`run_id`が表示されます。GitHubリリース / `npx`バイナリには、`xrpl-py`が同梱されています。Windowsのパイプド出力（`cp1252`）は、リッチボックス描画文字でクラッシュしなくなりました（#19）。

**1.3.0** — 実際に到達できる4つのエンディング（風化を含む）。馬車はゲーム全体ではなく、脅威です。初期の冬は、特定の年を遅らせます。砂漠の浅瀬は、砂地から離れています。狩りの際、誰が怪我をしたかを記録します。`npx @mcptoolshop/escape-the-valley`がv1.3.0バイナリを起動します（以前は、古いv1.1.1GitHubアーティファクトに留まっていました）。

**1.2.1** — PyPIホイールが実際にアップロードされます（Metadata-Version 2.3）。1.2.0と同じ製品です。

**1.2.0** — GitHubリリースバイナリが起動します（イベントライブラリとスタイルシートが同梱。スモークアサートは`--help`と200以上のイベントを検証します）。`trail ledger proof`がロードされたセーブを監査します。TUIは、シード/ドクトリン/ひねり/士気を表示します。`c`がペースを切り替えます。台帳メニューの`R`が、このセーブを証明します。HUDは、80×24および120×30で読みやすくなっています。

**1.1.1** — `pip install "escape-the-valley[voice]"`が実際にインストールされます（追加パッケージが未公開バージョンに固定されていました）。PyInstallerバイナリは、音声追加パッケージを読み込みません。

**1.1.0** — ストリーミングナレーション、段階的なエンディング、負傷を引き起こすイベント、オンチェーンの和解証明、実行アーティファクト。

v1.1.0とv1.1.1GitHubリリースに付属するバイナリは起動しません（フリーズされたエントリポイントの相対インポート）。`pip install escape-the-valley`またはv1.2.0+GitHubバイナリ/`npx`ランチャーを使用してください。

## クイックスタート

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

## 遊び方

各ターン、キャンプからアクションを選択します。

| アクション | その効果 |
|--------|-------------|
| **Travel** | 谷の出口に向かって移動します。食料と水を消費します。故障とイベントのリスクがあります。 |
| **Rest** | パーティーを回復させ、士気を回復させます。物資を消費しますが、進行はありません。 |
| **Hunt** | 弾薬を消費して、食料を入手する機会を得ます。森林地帯と平原で効果的です。 |
| **Repair** | スペアパーツを消費して、馬車を修理します。生存に不可欠です。 |

**イベント**は、旅を中断し、選択肢（A/B/C）を提示します。慎重な選択肢はより安全ですが、時間を消費します。大胆な選択肢はより速いですが、リスクがあります。常に正しい答えはありません。

**馬車はすべてです。** 部品がなくなって故障すると、ゲームは終了します。馬車の状態を半分以上に保ち、一時的な故障耐性を得るために、メンテナンスウィンドウ（休憩してから修理）を実施してください。

**ペース**は、速度と安全性のバランスを制御します。デフォルトは「安定」です。速いペースは、より多くの距離を移動しますが、より多くの物資を消費し、馬車の故障を早めます。

**緊急脱出バルブ**（厳しい配給、必死の修理、貨物の放棄）は、緊急時に使用できます。これらには副作用とクールダウンがあり、最終手段であり、戦略ではありません。

より詳細なヒントについては、[サバイバルガイド](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/)を参照してください。

## GMプロファイル

AIナレーターは、ゲームのメカニクスではなく、トーンを決定します。3つのプロファイルはすべて同じゲームをプレイします。

- **記録者** — 落ち着いていて、実用的で、簡潔です。民話は最小限です。何が起こったかを報告します。
- **焚き火** — 真剣な焚き火の語り手。微妙な不気味な瞬間があります。デフォルトです。
- **ランタンベアラー** — 不気味で、境界線上にありますが、それでも結果に根ざしています。奇妙な存在です。

`--gm-profile`で設定：`trail tui --gm-profile lantern`

## 物資

ゲームは、2つのカテゴリにわたる12種類の資源を追跡します。

**消耗品:** 食料、水、薪、医薬品、塩、弾薬、ランタンオイル、布

**装備:** 部品、ロープ、道具、ブーツ

5つの主要な物資（食料、水、医薬品、弾薬、部品）が最も重要です。薪、塩、ランタンオイル、布などの追加の物資は、深みを加えています。薪は夜のキャンプを暖め、塩は食料の腐敗を防ぎ、ランタンオイルは夜間の安全な移動を可能にし、布は装備や馬車のカバーを修復します。

## 台帳バックパック（オプション）

台帳バックパックは、5つの主要な物資（食料、水、医薬品、弾薬、部品）をXRPLテストネット上のトークンとして追跡します。すべての町チェックポイントで、オンチェーンに決済レシートが記録されます。ゲームの最後に、あなたのトレイル台帳には、誰でも検証できるトランザクションIDが含まれます。

完全にオプションです。オフにしてもゲームは同じようにプレイできます（デフォルト）。TUIのLメニューから、またはCLIから有効にします。

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

TUIでは、**L**を押すと台帳メニューが開きます。バックパックがオンになっている場合は、**R**を押すと、このセーブが証明されます（同じ結果になります）。休憩は行いません。

`npx` / GitHubリリースバイナリに同梱されています。`pip`インストールの場合、`pip install "escape-the-valley[xrpl]"`（または`pip install -e ".[xrpl]"`）を実行して、`xrpl-py`依存関係をインストールしてください。

## コマンド

| コマンド | 説明 |
|---------|-------------|
| `trail tui` | フルスクリーンテキストUIを起動します |
| `trail new` | 新しいゲームを開始します（クラシックCLIモード） |
| `trail play` | 保存されたゲームを続行します（クラシックCLIモード） |
| `trail status` | パーティー、馬車、物資を表示します |
| `trail journal` | 最近のジャーナルエントリを表示します |
| `trail self-check` | ゲーム環境の健全性を確認します |
| `trail version` | バージョンを表示します |
| `trail licenses` | ライセンスとサードパーティの通知を表示します（または`trail --licenses`） |
| `trail ledger status` | バックパックの状態を表示します |
| `trail ledger enable` | XRPLバックパックを有効にします |
| `trail ledger disable` | XRPLバックパックを無効にします |
| `trail ledger settle` | チェックポイントを手動で決済します |
| `trail ledger reconcile` | 決済の再試行 |
| `trail ledger proof` | 読み込まれたセーブデータの検証（PASS/FAIL/INCONCLUSIVE） |
| `trail ledger wallet` | ウォレットの詳細を表示 |
| `trail stats` | ゲームの統計情報とメモリカードを表示（`--json`をサポート） |
| `trail parcel send <addr> <supply> <amount>` | 他のプレイヤーに物資を送る |
| `trail parcel list` | 受信した荷物のリストを表示 |
| `trail parcel accept <id>` | 保留中の荷物を受け取る |
| `trail parcel sent` | 送信した荷物のリストを表示 |
| `trail wallet share` | 取引用のウォレットアドレスを表示 |

## 警告表示

デフォルトでは、ゲームは詳細な警告を表示し、新しいプレイヤーが危険を早期に察知できるようにします。経験豊富なプレイヤーは、最小限のモードに切り替えることができます。このモードでは、危険が差し迫った状況（最後の瞬間、重大な脅威）でのみ警告が表示されます。

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## トラブルシューティング

**何か問題がある場合は、まず`trail self-check`を実行してください。** これにより、Ollamaが利用可能かどうか、セーブデータが読み込まれるかどうか、およびどのモデルがインストールされているかが報告されます。問題が発生する主な原因は次の3つです。

| 症状 | 原因 | 解決策 |
|---------|-------|-----|
| **Generic / no narration** | Ollamaが実行されていない（GMはオプションであり、問題が発生してもゲームが完全に動作不能になることはありません） | Ollama（`ollama serve`）を起動するか、`--gm-off`を使用して決定的にプレイします。`trail self-check`を実行して確認します。 |
| **トランザクションの保留／決済の失敗** | XRPLテストネットはパブリックなテストネットワークであり、時々不安定になることがあります。 | `trail ledger reconcile`は、失敗した決済を再試行します。ネットワークが回復したら、再度実行してください。いずれの場合も、物資はローカルで正しく処理されます。 |
| **Save won't resume** | `run.json`が書き込み中に途中で切り捨てられたか、破損した | エンジンは、それを`run.json.corrupt-<timestamp>`として隔離してから拒否するため、次のセーブデータで証拠が上書きされることはありません。そのバックアップから復元するか、新しいセーブデータからゲームを開始してください。 |

最初のナレーションターンではモデルが読み込まれ、10〜30秒かかることがあります。これは正常であり、ゲームがフリーズしているわけではありません。詳細については、[トラブルシューティングハンドブック](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/)を参照してください。

## 要件

- Python 3.11+
- Ollama（オプション、AIナレーション用）
- xrpl-py（オプション、`pip`にインストールされた場合の台帳バックパック用。`npx` / GitHubのリリースバイナリに同梱）

## セキュリティ

テレメトリーはありません。アカウントもありません。GMナレーションはデフォルトで有効になっています（ローカルのOllamaへのHTTP）。無効にするには、`--gm-off`を渡してください。XRPLは、`trail ledger enable`になるまでオフになっています（テストネットのみ）。音声はオプトイン（`--voice`）です。完全な脅威モデルについては、[SECURITY.md](SECURITY.md)を参照してください。

## ライセンス

MIT — `trail licenses`（または`trail --licenses`）を実行して、ゲームのライセンスとサードパーティの依存関係に関する情報を表示します（各GitHubリリースにも添付され、プラットフォームごとに1つのファイルがあります：`THIRD_PARTY_NOTICES-<os>-<arch>.txt`）。

作成：<a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
