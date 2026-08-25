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

「Escape the Valley」は、オレゴン・トレイル風のサバイバルゲームで、ターミナル上で動作します。手続き的に生成された荒野を旅する入植者のグループを率いましょう。イベント、危険、そして難しい選択を乗り越えながら、食料、水、馬車の状態、士気を管理します。

オプションのAIゲームマスター（Ollama製）が、3つの異なる語り口であなたの旅を実況します。オプションのXRPLテストネット台帳バックパックは、オンチェーンのレシートとして、あなたの物資の変化を追跡します。これは、あなたが生き残ったこと、または少なくとも試みたことの証です。

## 新機能

**1.2.0** — GitHub Release binaries launch (event library + stylesheet bundled; smoke asserts `--help` and ≥200 events). `trail ledger proof` audits the loaded save. TUI shows seed/doctrine/twists/morale; `c` cycles pace; ledger menu `R` proofs this save. HUD readable at 80×24 and 120×30.

**1.1.1** — `pip install "escape-the-valley[voice]"`が正しくインストールされるようになりました（以前は未公開パッケージにピン留めされていました）。PyInstallerバイナリは、音声関連の追加ファイルを読み込みません。

**1.1.0** — ストリーミングによる実況、段階的なエンディング、負傷を引き起こすイベント、オンチェーンでの整合性確認、実行時のアーティファクト。

v1.1.0およびv1.1.1のGitHubリリース版に添付されているバイナリは起動しない（フリーズされたエントリーポイントにおける相対インポートの問題）。`pip install escape-the-valley`またはv1.2.0以降のGitHubバイナリ/`npx`ランチャーを使用してください。

## クイックスタート

```bash
pip install escape-the-valley

# Zero-prerequisite npm launcher (v1.2.0+ GitHub binaries; v1.1.0 and v1.1.1
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
| **Rest** | パーティーを回復させ、士気を高めます。物資は消費しますが、進捗はありません。 |
| **Hunt** | 弾薬を使い、食料を入手するチャンスを得ます。森林地帯と平原で効果的です。 |
| **Repair** | 予備の部品を使って馬車を修理します。生存に不可欠です。 |

**イベント**は、旅中に選択肢（A/B/C）を提供して中断します。慎重な選択はより安全ですが、時間がかかります。大胆な選択はより速いですが、リスクがあります。常に正しい答えがあるわけではありません。

**馬車がすべてです。** 部品がなく故障した場合、ゲームオーバーになります。状態を半分以上に保ち、一時的な故障に対する耐性を高めるために、定期的なメンテナンス（休憩と修理）を行いましょう。

**ペース**は、速度と安全性のバランスを制御します。デフォルトは「安定」です。速いペースではより多くの距離を進むことができますが、物資の消費量が増え、馬車の故障も早くなります。

**緊急時の対応策**（食料の節約、必死の修理、貨物の放棄）があります。これらには副作用とクールダウンがあり、最後の手段であり、戦略ではありません。

より詳細なヒントについては、「[Survival Guide](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/)」をご覧ください。

## GMプロファイル

AIナレーターは、ゲームのメカニクスではなく、トーンを決定します。3つのプロファイルすべてで同じゲームがプレイされます。

- **Chronicler** — 落ち着いており、実用的で、簡潔です。民話は最小限に抑えられています。何が起こったかを報告します。
- **Fireside** — 真剣な焚き火の語り手。微妙な不気味さがあります。デフォルト設定です。
- **Lantern-Bearer** — 不気味で、境界線上にありますが、それでも結果に基づいています。少し変わったプロファイルです。

コマンド：`--gm-profile`、設定：`trail tui --gm-profile lantern`

## 物資

ゲームでは、2つのカテゴリに分類された12種類の資源を追跡します。

**消耗品:** 食料、水、薪、医薬品、塩、弾薬、ランタンオイル、布

**装備:** 部品、ロープ、道具、ブーツ

5つの主要な物資（食料、水、医薬品、弾薬、部品）が最も重要です。薪、塩、ランタンオイル、布などの追加の物資は、ゲームに深みを与えます。薪は夜間のキャンプを暖め、塩は食料の腐敗を防ぎ、ランタンオイルは夜間の安全な移動を可能にし、布は装備や馬車のカバーを修理します。

## 台帳バックパック（オプション）

台帳バックパックは、5つの主要な物資（食料、水、医薬品、弾薬、部品）をXRPLテストネット上のトークンとして追跡します。すべての町でチェックポイントに到達すると、オンチェーンに決済レシートが記録されます。ゲームの最後に、あなたの旅の台帳には、誰でも検証できるトランザクションIDが含まれます。

完全にオプションです。オフにしてもゲームは同じようにプレイできます（デフォルト設定）。TUIまたはCLIから有効にします。

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

TUIでは、**L**を押すと台帳メニューが開きます。バックパックがオンになっている場合、**R**を押すと現在のセーブの状態を確認できます（結果はPASS/FAIL/INCONCLUSIVEのいずれかになります）。休憩は行いません。

`xrpl-py`依存関係のために、`pip install -e ".[xrpl]"`が必要です。

## コマンド

| コマンド | 説明 |
|---------|-------------|
| `trail tui` | フルスクリーンテキストUIを起動します。 |
| `trail new` | 新しいゲームを開始します（従来のCLIモード）。 |
| `trail play` | 保存されたゲームを続行します（従来のCLIモード）。 |
| `trail status` | パーティー、馬車、物資を表示します。 |
| `trail journal` | 最近のジャーナルエントリを表示します。 |
| `trail self-check` | ゲーム環境の状態を確認します。 |
| `trail version` | バージョンを表示します。 |
| `trail ledger status` | バックパックの状態を表示します。 |
| `trail ledger enable` | XRPLバックパックを有効にします。 |
| `trail ledger disable` | XRPLバックパックを無効にします。 |
| `trail ledger settle` | チェックポイントを手動で決済します。 |
| `trail ledger reconcile` | 決済に失敗したものを再試行します。 |
| `trail ledger proof` | ロードされたセーブの状態を確認します（PASS/FAIL/INCONCLUSIVE）。 |
| `trail ledger wallet` | ウォレットの詳細を表示します。 |
| `trail stats` | ゲームの統計情報を表示します（`--json`をサポート）。 |
| `trail parcel send <addr> <supply> <amount>` | 他のプレイヤーに物資を送ります。 |
| `trail parcel list` | 受信した荷物を一覧表示します。 |
| `trail parcel accept <id>` | 保留中の荷物を受け取ります。 |
| `trail parcel sent` | 送信した荷物を一覧表示します。 |
| `trail wallet share` | 取引のためにウォレットアドレスを表示します。 |

## 警告表示

デフォルトでは、ゲームは詳細な警告を表示して、新しいプレイヤーが危険を早期に発見できるようにします。経験豊富なプレイヤーは、最小限のモードに切り替えることができます。このモードでは、崖っぷちの警告（最後の瞬間で重大な脅威）のみが表示されます。

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## トラブルシューティング

**問題が発生した場合は、最初に`trail self-check`を実行してください。** これにより、Ollamaが利用可能かどうか、セーブデータがロードされるかどうか、およびどのモデルがインストールされているかが報告されます。通常、次の3つのいずれかの問題が発生します。

| 症状 | 原因 | 解決策 |
|---------|-------|-----|
| **Generic / no narration** | Ollamaが実行されていません（GMはオプションであり、デフォルトに戻りますが、ゲームを完全に停止させることはありません）。 | Ollama（`ollama serve`）を起動するか、`--gm-off`を使って確実に実行します。確認のため、`trail self-check`を実行してください。 |
| **未処理のトランザクション／決済失敗** | XRPLテストネットは公開テストネットワークであり、時々不安定になることがあります。 | `trail ledger reconcile`は、失敗した決済を再試行します。ネットワークが回復したら、再度実行してください。いずれの場合も、ローカルではデータに問題ありません。 |
| **Save won't resume** | `run.json`の書き込み中に途中で中断または破損が発生しました。 | エンジンは、それを拒否する前に`run.json.corrupt-<timestamp>`として隔離するため、次の保存時に証拠が上書きされることはありません。そのバックアップから復元するか、新しいシードから実行を開始してください。 |

最初のナレーションターンではモデルが読み込まれ、10〜30秒かかる場合があります。これは正常であり、フリーズではありません。詳細については、[トラブルシューティングハンドブック](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/)をご覧ください。

## 必要なもの

- Python 3.11以上
- Ollama（オプション、AIナレーション用）
- xrpl-py（オプション、台帳バックパック用）

## セキュリティ

テレメトリーは行いません。アカウントも使用しません。GMナレーションはデフォルトで有効になっています（ローカルのOllamaへのHTTP接続）。無効にするには、`--gm-off`を渡してください。XRPLは、`trail ledger enable`になるまでオフになっています（テストネットのみ）。音声はオプトイン方式です（`--voice`）。完全な脅威モデルについては、[SECURITY.md](SECURITY.md)をご覧ください。

## ライセンス

MIT

<a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>によって作成されました
