<p align="center">
  <a href="README.ja.md">日本語</a> | <a href="README.zh.md">中文</a> | <a href="README.es.md">Español</a> | <a href="README.md">English</a> | <a href="README.hi.md">हिन्दी</a> | <a href="README.it.md">Italiano</a> | <a href="README.pt-BR.md">Português (BR)</a>
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

## Qu'est-ce que c'est ?

Escape the Valley est un jeu de survie de type Oregon Trail qui fonctionne dans votre terminal. Guidez un groupe de colons à travers une nature générée aléatoirement. Gérez la nourriture, l'eau, l'état du chariot et le moral, tout en gérant les événements, les dangers et les choix difficiles.

Un Maître du Jeu IA optionnel (alimenté par Ollama) raconte votre aventure avec trois voix narratives distinctes. Un "sac à dos" de registre XRPL optionnel suit les changements de vos provisions sous forme de reçus sur la blockchain, prouvant que vous avez survécu, ou que vous avez essayé.

## Démarrage rapide

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

## Comment jouer

À chaque tour, vous choisissez une action au camp :

| Action | Ce que cela fait |
|--------|-------------|
| **Travel** | Avancez vers la sortie de la vallée. Coûte de la nourriture et de l'eau. Risque de panne et d'événements. |
| **Rest** | Soignez le groupe, améliorez le moral. Coûte des provisions, mais ne fait pas progresser. |
| **Hunt** | Utilisez des munitions pour avoir une chance d'obtenir de la nourriture. Plus efficace dans les forêts et les plaines. |
| **Repair** | Utilisez une pièce de rechange pour réparer le chariot. Essentiel pour la survie. |

**Les événements** interrompent le voyage et proposent des choix (A/B/C). Les choix prudents sont plus sûrs, mais coûtent du temps. Les choix audacieux sont plus rapides, mais risqués. Il n'y a pas toujours de bonne réponse.

**Le chariot est essentiel.** S'il tombe en panne et que vous n'avez plus de pièces, la partie est terminée. Maintenez-le en bon état (plus de la moitié) et effectuez des périodes de maintenance (repos puis réparation) pour une résistance temporaire aux pannes.

**Le rythme** contrôle la vitesse par rapport à la sécurité. Le rythme normal est le réglage par défaut. Un rythme rapide couvre plus de terrain, mais consomme plus de provisions et abîme les chariots plus rapidement.

Des **solutions de secours** (rations minimales, réparation désespérée, abandon de la cargaison) existent en cas d'urgence. Elles ont des effets secondaires et des temps de recharge ; ce sont des solutions de dernier recours, pas des stratégies.

Pour obtenir des conseils plus approfondis, consultez le [Guide de survie](docs/survival-guide.md).

## Profils du Maître du Jeu

Le narrateur IA façonne le ton, pas les mécanismes. Les trois profils jouent le même jeu.

- **Chronicler** — Pragmatique, concis, terre-à-terre. Folklore minimal. Décrit ce qui s'est passé.
- **Fireside** — Narrateur de conte de feu sérieux. Moments subtils et étranges. Le réglage par défaut.
- **Lantern-Bearer** — Étrange et liminal, mais toujours ancré dans les conséquences. Le plus bizarre.

Définissez avec `--gm-profile` : `trail tui --gm-profile lantern`

## Sac à dos de registre (Optionnel)

Le sac à dos de registre suit vos 5 principales provisions (nourriture, eau, médicaments, munitions, pièces) sous forme de jetons sur le réseau de test XRPL. Chaque point de contrôle de la ville enregistre un reçu de transaction sur la blockchain. À la fin de votre partie, votre registre de parcours comprend les identifiants de transaction que toute personne peut vérifier.

Entièrement optionnel. Le jeu se déroule de la même manière avec ou sans. Activez-le depuis le menu L dans l'interface utilisateur en texte ou via la ligne de commande :

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
```

Nécessite `pip install -e ".[xrpl]"` pour la dépendance `xrpl-py`.

## Commandes

| Commande | Description |
|---------|-------------|
| `trail tui` | Lance l'interface utilisateur en texte en plein écran |
| `trail new` | Démarre une nouvelle partie (mode CLI classique) |
| `trail play` | Continue une partie sauvegardée (mode CLI classique) |
| `trail status` | Affiche le groupe, le chariot et les provisions |
| `trail journal` | Affiche les entrées récentes du journal |
| `trail self-check` | Vérifie l'état de l'environnement du jeu |
| `trail version` | Affiche la version |
| `trail ledger status` | Affiche l'état du sac à dos |
| `trail ledger enable` | Active le sac à dos XRPL |
| `trail ledger disable` | Désactive le sac à dos XRPL |
| `trail ledger settle` | Effectue manuellement un enregistrement à un point de contrôle |
| `trail ledger reconcile` | Réessaie les enregistrements infructueux |
| `trail ledger wallet` | Affiche les détails du portefeuille |
| `trail parcel list` | Liste les colis reçus |
| `trail parcel accept <id>` | Accepte un colis en attente |

## Avertissements

Par défaut, le jeu affiche des avertissements détaillés pour aider les nouveaux joueurs à repérer les dangers dès le début. Les joueurs expérimentés peuvent passer en mode minimal, qui n'affiche que les avertissements de seuil critique (menaces de dernière minute) :

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Exigences

- Python 3.11+
- Ollama (facultatif, pour la narration par IA)
- xrpl-py (facultatif, pour le "ledger backpack")

## Sécurité

Aucune télémétrie. Aucun compte. Toutes les fonctionnalités réseau (Ollama, XRPL) sont facultatives et désactivées par défaut. Les opérations XRPL utilisent uniquement le réseau de test. Consultez le fichier [SECURITY.md](SECURITY.md) pour l'analyse complète des risques.

## Licence

MIT

Développé par <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
