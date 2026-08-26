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

「Escape the Valley」は、オレゴン・トレイル風のサバイバルゲームで、ターミナル上で動作します。手続き的に生成された荒野を旅する開拓者のグループを率いましょう。イベント、危険、そして難しい選択に直面しながら、食料、水、馬車の状態、士気を管理します。

オプションのAIゲームマスター（Ollama製）が、3つの異なる語り口であなたの旅を実況します。オプションのXRPLテストネット台帳バックパックは、オンチェーンのレシートとして、あなたの物資の変化を追跡します。これは、あなたが生き残ったこと、または少なくとも試みたことの証です。

## 新機能

**1.3.0** — 実際に到達できる4つのエンディング（風化を含む）。馬車はゲーム全体ではなく、脅威の一つ。初期の冬は経過年を遅らせる。砂漠の浅瀬は砂地から離れる。怪我をした人物の名前を表示。`npx @mcptoolshop/escape-the-valley`がv1.3.0バイナリを起動します（以前は古いv1.1.1 GitHubアーティファクトに引っかかっていました）。

**1.2.1** — PyPIホイールが実際にアップロードされます（Metadata-Version 2.3）。1.2.0と同じ製品です。

**1.2.0** — GitHubリリースバイナリの公開（イベントライブラリとスタイルシートをバンドル；スモークテストで`--help`および≧200個のイベントを確認）。`trail ledger proof`がロードされたセーブデータを監査します。TUIにシード/ドクトリン/展開/士気が表示されます。`c`でペースを調整できます。台帳メニュー`R`でこのセーブを証明します。HUDは80×24と120×30の解像度で読みやすくなっています。

**1.1.1** — `pip install "escape-the-valley[voice]"`が実際にインストールされます（以前は、未公開のパッケージにピンが固定されていました）。PyInstallerバイナリは、音声拡張機能を読み込みません。

**1.1.0** — ストリーミング実況、段階的なエンディング、怪我を負う可能性のあるイベント、オンチェーンでの和解証明、実行可能なアーティファクト。

v1.1.0とv1.1.1のGitHubリリースに添付されているバイナリは起動しません（フリーズされたエントリポイントにおける相対インポート）。`pip install escape-the-valley`またはv1.2.0以降のGitHubバイナリ/`npx`ランチャーを使用してください。

## クイックスタート

```bash
pip install escape-the-valley

# Zero-prerequisite npm launcher (GitHub v1.3.0 binary; v1.1.0 and v1.1.1
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

各ターンで、キャンプからアクションを選択します。

| アクション | その効果 |
|--------|-------------|
| **Travel** | 谷の出口に向かって移動します。食料と水を消費します。故障やイベントのリスクがあります。 |
| **Rest** | パーティーを回復させ、士気を回復させます。物資は消費しますが、進行はありません。 |
| **Hunt** | 弾薬を消費して、食料を入手するチャンスを得ます。森や平原で効果的です。 |
| **Repair** | スペアパーツを使用して馬車を修理します。生存に不可欠です。 |

**イベント**は、選択肢（A/B/C）によって旅を中断させます。慎重な選択肢はより安全ですが、時間がかかります。大胆な選択肢はより速いですが、リスクがあります。常に正しい答えがあるわけではありません。

**馬車がすべてです。** 部品がなく故障した場合、ゲームオーバーになります。状態を半分以上に保ち、一時的な故障に対する耐性を高めるために、メンテナンスの時間を設けてください（休憩してから修理）。

**ペース**は、速度と安全性のバランスを制御します。デフォルトは「安定」です。速いペースではより多くの距離を進むことができますが、物資の消費量が増え、馬車の故障も早くなります。

**緊急時の脱出策**（厳しい配給、必死の修理、貨物の放棄）があります。これらには副作用とクールダウンがあり、戦略ではなく最後の手段です。

より詳細なヒントについては、「[Survival Guide](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/)」をご覧ください。

## GMプロファイル

AI実況者は、ゲームのメカニクスではなく、トーンを決定します。3つのプロファイルすべてで同じゲームがプレイされます。

- **Chronicler（記録者）** — 落ち着いており、実践的で、簡潔です。民話は最小限に抑えられています。何が起こったかを報告します。
- **Fireside（焚き火のそば）** — 真剣なキャンプファイヤーの実況者。微妙な不気味さがあります。デフォルト設定です。
- **Lantern-Bearer（提灯を持つ人）** — 不気味で、境界線上にありますが、それでも結果に根ざしています。少し変わった設定です。

`--gm-profile`で設定：`trail tui --gm-profile lantern`

## 物資

ゲームは、2つのカテゴリに分類された12種類の資源を追跡します。

**消耗品:** 食料、水、薪、医薬品、塩、弾薬、ランタンオイル、布

**装備:** 部品、ロープ、道具、ブーツ

5つの主要な物資（食料、水、医薬品、弾薬、部品）が最も重要です。薪、塩、ランタンオイル、布などの追加の物資は、ゲームに深みを与えます。薪は夜間のキャンプを可能にし、塩は食品の腐敗を防ぎ、ランタンオイルはより安全な夜間移動を可能にし、布は装備や馬車のカバーを修復します。

## 台帳バックパック（オプション）

台帳バックパックは、5つの主要な物資（食料、水、医薬品、弾薬、部品）をXRPLテストネット上のトークンとして追跡します。すべての町でチェックポイントに到達すると、オンチェーンの決済レシートが記録されます。ゲーム終了時に、あなたの旅の台帳には、誰でも検証できるトランザクションIDが含まれます。

完全にオプションです。オフにしてもゲームは同じようにプレイできます（デフォルト）。TUIのLメニューから、またはCLI経由で有効にします。

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

TUIでは、**L**を押すと台帳メニューが開きます。バックパックがオンになっている場合、**R**を押すとこのセーブを証明します（同じ結果が得られます）。休憩は行いません。

`pip install -e ".[xrpl]"`と、その依存関係である`xrpl-py`が必要です。

## コマンド

| コマンド | 説明 |
|---------|-------------|
| `trail tui` | フルスクリーンテキストUIを起動します |
| `trail new` | 新しいゲームを開始します（従来のCLIモード） |
| `trail play` | 保存されたゲームを続行します（従来のCLIモード） |
| `trail status` | パーティー、馬車、物資を表示します |
| `trail journal` | 最近のジャーナルエントリを表示します |
| `trail self-check` | ゲーム環境の状態を確認します |
| `trail version` | バージョンを表示します |
| `trail ledger status` | 台帳バックパックの状態を表示します |
| `trail ledger enable` | XRPL台帳バックパックを有効にします |
| `trail ledger disable` | XRPL台帳バックパックを無効にします |
| `trail ledger settle` | チェックポイントを手動で決済します |
| `trail ledger reconcile` | 失敗した決済を再試行します |
| `trail ledger proof` | ロードされたセーブを証明します（PASS/FAIL/INCONCLUSIVE） |
| `trail ledger wallet` | ウォレットの詳細を表示します |
| `trail stats` | ゲームの統計情報を表示します（`--json`をサポート） |
| `trail parcel send <addr> <supply> <amount>` | 他のプレイヤーに物資を送ります |
| `trail parcel list` | 受信した荷物を一覧表示します |
| `trail parcel accept <id>` | 保留中の荷物を受け取ります |
| `trail parcel sent` | 送信した荷物を一覧表示します |
| `trail wallet share` | 取引のためにウォレットアドレスを表示します |

## 警告アラート

デフォルトでは、ゲームは詳細な警告を表示し、新しいプレイヤーが危険を早期に察知できるようにします。経験豊富なプレイヤーは、最小限のモードに切り替えることができます。このモードでは、崖っぷちでの警告（最後の瞬間、重大な脅威）のみが表示されます。

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## トラブルシューティング

**もし何か問題があるように見えたら、まず`trail self-check`を実行してください。** これにより、Ollamaにアクセスできるかどうか、セーブデータが正常に読み込まれるかどうか、およびどのモデルがインストールされているかが報告されます。 問題が発生する主な原因は次の3つです。

| 症状 | 原因 | 解決策 |
|---------|-------|-----|
| **Generic / no narration** | Ollamaが実行されていない（GMはオプションであり、代替手段として機能し、システムを完全に停止させることはない） | Ollama（`ollama serve`）を開始するか、または`--gm-off`を使用して決定的な方法でプレイします。確認のために`trail self-check`を実行してください。 |
| **トランザクションの保留／決済失敗** | XRPLテストネットはパブリックなテストネットワークであり、時々不安定になることがあります。 | `trail ledger reconcile`は、失敗したトランザクションを再試行します。ネットワークが回復したら、再度実行してください。いずれの場合も、ローカルでは必要なデータは正しく保存されます。 |
| **Save won't resume** | `run.json`の書き込み中に途中で中断または破損が発生しました。 | エンジンは、それを拒否する前に`run.json.corrupt-<timestamp>`として隔離するため、次のセーブで証拠が上書きされることはありません。そのバックアップから復元するか、新しいシードからゲームを再開してください。 |

最初のナレーションターンではモデルが読み込まれ、10〜30秒かかる場合があります。これは正常であり、フリーズではありません。詳細については、「トラブルシューティングハンドブック」をご覧ください：[https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/)。

## 要件

- Python 3.11+
- Ollama（オプション、AIによるナレーション用）
- xrpl-py（オプション、トランザクション履歴のバックアップ用）

## セキュリティ

テレメトリーは行いません。アカウントも使用しません。デフォルトではGMによるナレーションが有効になっています（ローカルOllamaへのHTTP接続）。無効にするには、`--gm-off`を渡してください。XRPLは、`trail ledger enable`になるまでオフのままです（テストネットのみ）。音声はオプションで有効にできます（`--voice`）。完全な脅威モデルについては、[SECURITY.md](SECURITY.md)を参照してください。

## ライセンス

MIT

MCP Tool Shopによって作成されました。
