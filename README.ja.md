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

「Escape the Valley」は、ターミナルで動作するオレゴン・トレイル風のサバイバルゲームです。手続き的に生成された荒野を旅しながら、入植者のグループを率いましょう。イベント、危険、そして難しい選択に直面しながら、食料、水、馬車の状態、士気を管理します。

オプションのAIゲームマスター（Ollamaによって提供）が、3つの異なる語り口であなたの旅を語ります。オプションのXRPLテストネット台帳バックパックは、オンチェーンのレシートとしてサプライの変化を追跡します。これは、あなたが生き残ったこと、または少なくとも試みたことの証拠です。

## 1.1.0の新機能

- **ストリーミングナレーション** - GMはトークンごとに書き込み、一時停止後に完成したブロックをドロップするのではなく、各シーンをリアルタイムで構成します。
- **段階評価されたエンディング** - ゲームの終わりに、生存者、経過時間、そして旅があなたに何をもたらしたかによって、評価されたエピローグ（勝利、苦難、ピュロス的勝利、または破滅）が表示されます。単なる死因ではありません。
- **真剣な試練** - イベントは現在、パーティーを負傷させたり殺したりする可能性があります。間違った選択は命を奪う可能性があり、その死は実際の原因に帰せられます。
- **オンチェーン台帳による照合証明** - ゲームのレシートを再生し、XRPLテストネットと照合して検証する監査モード。これにより、サプライ履歴を独立して確認できます。
- **ゲーム終了時の記念品** - 完了した各ゲームは、思い出の品を残します：XRPLポストカード、あなたの統計情報、そしてエクスポート/共有パス。

## クイックスタート

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

## 遊び方

各ターンで、キャンプからアクションを選択します。

| アクション | その効果 |
|--------|-------------|
| **Travel** | 谷の出口に向かって移動します。食料と水を消費します。故障やイベントのリスクがあります。 |
| **Rest** | パーティーを回復させ、士気を回復させます。物資は消費されますが、進捗はありません。 |
| **Hunt** | 弾薬を消費して、食料を入手するチャンスを得ます。森や平原でより効果的です。 |
| **Repair** | スペアパーツを使用して馬車を修理します。生存に不可欠です。 |

**イベント**は、旅中に選択肢（A/B/C）を提供して中断します。慎重な選択はより安全ですが、時間がかかります。大胆な選択はより速いですが、リスクがあります。常に正しい答えがあるわけではありません。

**馬車がすべてです。** 部品がない状態で故障すると、ゲームオーバーになります。状態を半分以上に保ち、一時的な故障耐性を得るためにメンテナンス（休憩と修理）を行います。

**ペース**は、速度と安全性のバランスを制御します。デフォルトは「安定」です。速いペースではより多くの距離を進むことができますが、物資の消費量が増え、馬車の故障も早くなります。

**緊急時の対応策**（厳しい配給、必死の修理、貨物の放棄）があります。これらには副作用とクールダウンがあり、最後の手段であり、戦略ではありません。

より詳細なヒントについては、「[サバイバルガイド](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/)」を参照してください。

## GMプロファイル

AIナレーターは、ゲームのメカニクスではなく、トーンを形作ります。3つのプロファイルすべてで同じゲームがプレイされます。

- **記録者** - 地に足がつき、実用的で、簡潔です。民話は最小限です。何が起こったかを報告します。
- **焚き火の語り手** - 真剣なキャンプファイヤーのナレーター。微妙な不気味さがあります。デフォルト設定です。
- **ランタンベアラー** - 不気味で境界線上にありますが、それでも結果に根ざしています。少し変わった存在です。

`--gm-profile`を使用して設定します：`trail tui --gm-profile lantern`

## 物資

ゲームは、2つのカテゴリに分類された12種類の資源を追跡します。

**消耗品:** 食料、水、薪、医薬品、塩、弾薬、ランタンオイル、布

**装備:** 部品、ロープ、道具、ブーツ

5つの主要な物資（食料、水、医薬品、弾薬、部品）が最も重要です。薪、塩、ランタンオイル、布などの追加の物資は、深みを加えます：薪は夜間のキャンプを可能にし、塩は食品の腐敗を防ぎ、ランタンオイルはより安全な夜間移動を可能にし、布は装備や馬車のカバーを修復します。

## 台帳バックパック（オプション）

台帳バックパックは、5つの主要な物資（食料、水、医薬品、弾薬、部品）をXRPLテストネット上のトークンとして追跡します。各町のチェックポイントで、オンチェーンに決済レシートが記録されます。ゲームの終わりに、あなたの旅の台帳には、誰でも検証できるトランザクションIDが含まれます。

完全にオプションです。オフ（デフォルト）の場合でも、ゲームは同じようにプレイできます。TUI内のLメニューまたはCLIから有効にします。

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
```

`pip install -e ".[xrpl]"`を実行して、`xrpl-py`依存関係をインストールする必要があります。

## コマンド

| コマンド | 説明 |
|---------|-------------|
| `trail tui` | フルスクリーンTextual UIを起動します |
| `trail new` | 新しいゲームを開始します（クラシックCLIモード） |
| `trail play` | 保存されたゲームを続行します（クラシックCLIモード） |
| `trail status` | パーティー、馬車、物資を表示します |
| `trail journal` | 最近のジャーナルエントリを表示します |
| `trail self-check` | ゲーム環境の状態を確認します |
| `trail version` | バージョンを表示します |
| `trail ledger status` | バックパックの状態を表示します |
| `trail ledger enable` | XRPLバックパックを有効にします |
| `trail ledger disable` | XRPLバックパックを無効にします |
| `trail ledger settle` | チェックポイントを手動で決済します |
| `trail ledger reconcile` | 失敗した決済を再試行します |
| `trail ledger wallet` | ウォレットの詳細を表示します |
| `trail stats` | ゲームの統計情報を表示します（`--json`もサポート） |
| `trail parcel send <addr> <supply> <amount>` | 他のプレイヤーに物資を送ります |
| `trail parcel list` | 受信した荷物を一覧表示します |
| `trail parcel accept <id>` | 保留中の荷物を受け取ります |
| `trail parcel sent` | 送信した荷物を一覧表示します |
| `trail wallet share` | 取引用のウォレットアドレスを表示します |

## 警告アラート

デフォルトでは、ゲームは詳細な警告を表示して、新しいプレイヤーが危険を早期に発見できるようにします。経験豊富なプレイヤーは、最小限のモードに切り替えることができます。このモードでは、崖っぷちの警告（最後の瞬間、重大な脅威）のみが表示されます。

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## トラブルシューティング

**問題が発生した場合は、最初に`trail self-check`を実行してください。** これにより、Ollamaが到達可能かどうか、保存データがロードされるかどうか、およびどのモデルがインストールされているかが報告されます。 3つの一般的な問題は次のとおりです。

| 症状 | 原因 | 修正方法 |
|---------|-------|-----|
| **Generic / no narration** | Ollamaが実行されていません（GMはオプションであり、問題が発生してもシステムが完全に停止することはありません）。 | Ollamaを起動します (`ollama serve`)。または、`--gm-off` を使用して、確実に動作するように設定します。`trail self-check` を実行して確認してください。 |
| **未処理のトランザクション / 決済失敗** | XRPLテストネットは公開テストネットワークであり、時々不安定になることがあります。 | `trail ledger reconcile` は、失敗した決済を再試行します。ネットワークが回復したら、再度実行してください。いずれの場合も、ローカルではデータが正しく保存されます。 |
| **Save won't resume** | `run.json` の書き込み中にファイルが途中で切り捨てられたか、破損しました。 | エンジンは、それを `run.json.corrupt-<タイムスタンプ>` として隔離し、拒否します。これにより、次の保存時に証拠データが上書きされるのを防ぎます。そのバックアップから復元するか、新しいシードから実行を開始してください。 |

最初のナレーションターンではモデルが読み込まれ、10〜30秒かかる場合があります。これは正常であり、システムがフリーズしているわけではありません。詳細については、[トラブルシューティングハンドブック](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/) を参照してください。

## 必要なもの

- Python 3.11 以降
- Ollama（オプション、AIナレーション用）
- xrpl-py（オプション、台帳バックパック用）

## セキュリティ

テレメトリーは行いません。アカウントも使用しません。すべてのネットワーク機能（Ollama、XRPL）はオプトインであり、デフォルトでは無効になっています。XRPL操作にはテストネットのみを使用します。完全な脅威モデルについては、[SECURITY.md](SECURITY.md) を参照してください。

## ライセンス

MIT

<a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a> によって作成されました
