<p align="center">
  <a href="README.md">English</a> | <a href="README.zh.md">中文</a> | <a href="README.es.md">Español</a> | <a href="README.fr.md">Français</a> | <a href="README.hi.md">हिन्दी</a> | <a href="README.it.md">Italiano</a> | <a href="README.pt-BR.md">Português (BR)</a>
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

## これは何ですか？

Escape the Valleyは、ターミナル上で動作する、オレゴン・トレイル風のサバイバルゲームです。手続き的に生成された荒野を、入植者のグループと共に進んでください。食料、水、荷車の状態、士気を管理しながら、イベント、危険、そして難しい選択肢に直面します。

オプションのAIゲームマスター（Ollama搭載）が、3つの異なる語り口であなたの冒険を語ります。オプションのXRPLテストネットのレジャーバックは、あなたの物資の変化をオンチェーンの記録として追跡し、あなたが生き残った証拠、または試した証拠となります。

## クイックスタート

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

## 遊び方

各ターンで、キャンプからアクションを選択します。

| アクション | 効果 |
|--------|-------------|
| **Travel** | 谷の出口に向かって移動します。食料と水を消費します。故障やイベントのリスクがあります。 |
| **Rest** | パーティーを回復させ、士気を回復させます。物資は消費しますが、進行はありません。 |
| **Hunt** | 弾薬を消費して、食料を得るチャンスがあります。森林や平原で効果的です。 |
| **Repair** | 予備部品を消費して、荷車を修理します。生存には不可欠です。 |

**イベント**は、選択肢（A/B/C）と共に旅を中断します。慎重な選択肢は安全ですが、時間がかかります。大胆な選択肢は速いですが、危険です。常に正しい答えはありません。

**荷車はすべてです。** 部品がない状態で故障すると、ゲームオーバーです。荷車の状態を半分以上に保ち、定期的にメンテナンス（休息後に修理）を行うことで、一時的に故障への耐性を高めることができます。

**ペース**は、速度と安全性のバランスを制御します。デフォルトは「一定」です。速いペースはより多くの距離を進めますが、物資をより多く消費し、荷車の故障も早くなります。

**緊急用手段**（非常食、応急処置、積荷の放棄）は、緊急時に利用できます。これらは副作用とクールダウン時間があり、最終手段であり、戦略ではありません。

より詳しいヒントは、[サバイバルガイド](docs/survival-guide.md)を参照してください。

## ゲームマスターのプロファイル

AIナレーターは、ゲームの仕組みではなく、雰囲気を形作ります。すべてのプロファイルは同じゲームをプレイします。

- **Chronicler（記録者）:** 冷静で、現実的で、簡潔。フォークロアは最小限。何が起こったかを報告します。
- **Fireside（暖炉のそば）:** 真面目なキャンプファイヤーの語り手。さりげない不気味な瞬間があります。デフォルトのプロファイルです。
- **Lantern-Bearer（ランタンを持つ者）:** 不気味で、曖昧ですが、それでも結果には忠実です。少し変わったプロファイルです。

`--gm-profile`オプションで設定します。例：`trail tui --gm-profile lantern`

## レジャーバック（オプション）

レジャーバックは、5つの主要な物資（食料、水、薬、弾薬、部品）を、XRPLテストネット上のトークンとして追跡します。各町でのチェックポイントでは、オンチェーンで入植記録が作成されます。ゲーム終了時には、あなたのトレイルの記録には、誰でも検証できるトランザクションIDが含まれます。

完全にオプションです。オフにすると（デフォルト）、ゲームは全く同じように動作します。TUIのLメニューまたはCLIから有効にしてください。

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
```

`xrpl-py`の依存関係をインストールするために、`pip install -e ".[xrpl]"`が必要です。

## コマンド

| コマンド | 説明 |
|---------|-------------|
| `trail tui` | フルスクリーンのテキストUIを起動します。 |
| `trail new` | 新しいゲームを開始します（クラシックCLIモード）。 |
| `trail play` | 保存されたゲームを再開します（クラシックCLIモード）。 |
| `trail status` | パーティー、荷車、物資を表示します。 |
| `trail journal` | 最近のジャーナルエントリを表示します。 |
| `trail self-check` | ゲーム環境の状態を確認します。 |
| `trail version` | バージョンを表示します。 |
| `trail ledger status` | レジャーバックの状態を表示します。 |
| `trail ledger enable` | XRPLレジャーバックを有効にします。 |
| `trail ledger disable` | XRPLレジャーバックを無効にします。 |
| `trail ledger settle` | チェックポイントを強制的にクリアします。 |
| `trail ledger reconcile` | クリアに失敗したチェックポイントを再試行します。 |
| `trail ledger wallet` | ウォレットの詳細を表示します。 |
| `trail parcel list` | 受信した荷物のリストを表示します。 |
| `trail parcel accept <id>` | 保留中の荷物を受け入れます。 |

## 警告

デフォルトでは、ゲームは詳細な警告を表示し、新しいプレイヤーが危険を早期に発見できるようにします。経験豊富なプレイヤーは、最小モードに切り替えることができます。最小モードでは、最後の瞬間、重要な脅威（クリティカルな危険）に関する警告のみが表示されます。

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## 要件

- Python 3.11 以降
- Ollama (オプション、AIによるナレーション機能用)
- xrpl-py (オプション、Ledger Backpack機能用)

## セキュリティ

テレメトリー機能はありません。アカウント機能もありません。ネットワーク機能（Ollama、XRPL）は、オプションであり、デフォルトでは無効になっています。XRPLの操作は、テストネットのみを使用します。詳細については、[SECURITY.md](SECURITY.md) を参照してください。

## ライセンス

MIT

開発者: <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
