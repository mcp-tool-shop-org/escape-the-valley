<p align="center">
  <a href="README.ja.md">日本語</a> | <a href="README.zh.md">中文</a> | <a href="README.es.md">Español</a> | <a href="README.md">English</a> | <a href="README.hi.md">हिन्दी</a> | <a href="README.it.md">Italiano</a> | <a href="README.pt-BR.md">Português (BR)</a>
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

## Qu'est-ce que c'est ?

Escape the Valley est un jeu de survie de type Oregon Trail qui se déroule dans votre terminal. Menez un groupe de colons à travers une nature sauvage générée de manière procédurale. Gérez la nourriture, l'eau, l'état du chariot et le moral tout en naviguant à travers des événements, des dangers et des choix difficiles.

Un maître de jeu IA optionnel (alimenté par Ollama) raconte votre voyage avec trois voix narratives distinctes. Un registre XRPL Testnet optionnel suit les changements de vos provisions sous forme de reçus en chaîne — preuve que vous avez survécu, ou preuve que vous l'avez essayé.

## Nouveautés

**1.3.0** — Quatre fins que vous pouvez réellement atteindre, y compris une fin où les personnages sont marqués par les épreuves. Le chariot est une menace, et non le jeu entier. Un début d'hiver précoce ralentit l'année en cours. Les gués du désert ne se trouvent plus dans le sable. La chasse indique qui a été blessé. `npx @mcptoolshop/escape-the-valley` lance le fichier binaire v1.3.0 (il était bloqué sur l'ancien artefact GitHub v1.1.1).

**1.2.1** — Le paquet PyPI est réellement téléchargé (Version des métadonnées : 2.3). Identique à la version 1.2.0.

**1.2.0** — Les fichiers binaires de publication GitHub sont lancés (bibliothèque d'événements + feuille de style intégrées ; assertions de test `--help` et ≥ 200 événements). `trail ledger proof` vérifie la sauvegarde chargée. L'interface TUI affiche les valeurs initiales/doctrine/imprévus/moral ; `c` fait varier le rythme ; le menu du registre `R` prouve cette sauvegarde. L'affichage HUD est lisible en 80×24 et 120×30.

**1.1.1** — `pip install "escape-the-valley[voice]"` s'installe réellement (l'extra avait épinglé un paquet non publié). Le fichier binaire PyInstaller n'inclut plus l'extra vocal.

**1.1.0** — narration en continu, fins graduées, événements pouvant blesser, preuve de réconciliation sur le registre, fichiers d'exécution.

Les fichiers binaires joints aux publications GitHub v1.1.0 et v1.1.1 ne se lancent pas (une importation relative dans le point d'entrée gelé). Utilisez `pip install escape-the-valley` ou le fichier binaire GitHub v1.2.0+ / lanceur `npx`.

## Démarrage rapide

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

## Comment jouer

À chaque tour, vous choisissez une action dans le camp :

| Action | Ce qu'elle fait |
|--------|-------------|
| **Travel** | Avancez vers la sortie de la vallée. Coûte de la nourriture et de l'eau. Risque de panne et d'événements. |
| **Rest** | Soignez le groupe, restaurez le moral. Coûte des provisions mais n'offre aucun progrès. |
| **Hunt** | Dépensez des munitions pour tenter de trouver de la nourriture. Plus efficace dans les forêts et les plaines. |
| **Repair** | Dépensez une pièce de rechange pour réparer le chariot. Essentiel à la survie. |

Les **événements** interrompent le voyage avec des choix (A/B/C). Les choix prudents sont plus sûrs mais coûtent du temps. Les choix audacieux sont plus rapides mais risqués. Il n'y a pas de réponse toujours correcte.

**Le chariot est primordial.** S'il tombe en panne sans pièces de rechange, la partie est terminée. Maintenez-le à plus de la moitié de sa capacité et effectuez des opérations d'entretien (repos puis réparation) pour une résistance temporaire aux pannes.

Le **rythme** contrôle la vitesse par rapport à la sécurité. Le rythme normal est le réglage par défaut. Un rythme soutenu permet de parcourir plus de terrain, mais consomme plus de provisions et use les chariots plus rapidement.

Les **valves d'échappement** (rations réduites, réparations désespérées, abandon du chargement) existent pour les situations d'urgence. Elles ont des effets secondaires et des temps de recharge — ce sont des solutions de dernier recours, pas des stratégies.

Pour obtenir des conseils plus approfondis, consultez le [Guide de survie](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/).

## Profils du maître de jeu

Le narrateur IA façonne le ton, et non la mécanique. Les trois profils jouent au même jeu.

- **Chroniqueur** — Concret, pragmatique, sobre. Folklore minimal. Raconte ce qui s'est passé.
- **Conte du feu de camp** — Narrateur sérieux autour du feu de camp. Moments subtilement étranges. Le profil par défaut.
- **Porte-lanterne** — Étrange et liminal, mais toujours ancré dans les conséquences. Le plus bizarre.

Définir avec `--gm-profile` : `trail tui --gm-profile lantern`

## Provisions

Le jeu suit 12 types de ressources répartis en deux catégories :

**Consommables :** nourriture, eau, bois de chauffage, médicaments, sel, munitions, huile pour lanterne, tissu

**Équipement :** pièces de rechange, corde, outils, bottes

Les 5 provisions essentielles (nourriture, eau, médicaments, munitions, pièces de rechange) sont les plus importantes. Les provisions supplémentaires telles que le bois de chauffage, le sel, l'huile pour lanterne et le tissu ajoutent de la profondeur : le bois de chauffage alimente les camps nocturnes, le sel empêche la détérioration des aliments, l'huile pour lanterne permet de voyager plus en sécurité la nuit et le tissu répare l'équipement et la bâche du chariot.

## Sac à dos du registre (facultatif)

Le sac à dos du registre suit vos 5 provisions essentielles (nourriture, eau, médicaments, munitions, pièces de rechange) sous forme de jetons sur le XRPL Testnet. Chaque point de contrôle enregistre un reçu de règlement en chaîne. À la fin de votre partie, votre historique inclut des ID de transaction que n'importe qui peut vérifier.

Entièrement facultatif. Le jeu se joue de la même manière avec ou sans (c'est le réglage par défaut). Activez-le à partir du menu L dans l'interface TUI ou via la ligne de commande :

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

Dans l'interface TUI, **L** ouvre le menu du registre ; lorsque le sac à dos est activé, **R** prouve cette sauvegarde (mêmes verdicts). Il ne permet pas de se reposer.

Nécessite `pip install -e ".[xrpl]"` pour la dépendance `xrpl-py`.

## Commandes

| Commande | Description |
|---------|-------------|
| `trail tui` | Lancez l'interface utilisateur textuelle en plein écran |
| `trail new` | Démarrez une nouvelle partie (mode CLI classique) |
| `trail play` | Reprenez une partie sauvegardée (mode CLI classique) |
| `trail status` | Affichez le groupe, le chariot et les provisions |
| `trail journal` | Affichez les entrées de journal récentes |
| `trail self-check` | Vérifiez l'état de l'environnement du jeu |
| `trail version` | Affichez la version |
| `trail ledger status` | Affichez l'état du sac à dos |
| `trail ledger enable` | Activez le sac à dos XRPL |
| `trail ledger disable` | Désactivez le sac à dos XRPL |
| `trail ledger settle` | Réglez manuellement un point de contrôle |
| `trail ledger reconcile` | Réessayez les règlements ayant échoué |
| `trail ledger proof` | Prouvez la sauvegarde chargée (PASSÉ / ÉCHOUÉ / INCONCLUSIF) |
| `trail ledger wallet` | Affichez les détails du portefeuille |
| `trail stats` | Affichez les statistiques de la partie (prend en charge `--json`) |
| `trail parcel send <addr> <supply> <amount>` | Envoyez des provisions à un autre voyageur |
| `trail parcel list` | Affichez les colis reçus |
| `trail parcel accept <id>` | Acceptez un colis en attente |
| `trail parcel sent` | Affichez les colis que vous avez envoyés |
| `trail wallet share` | Affichez l'adresse de votre portefeuille pour le commerce |

## Alertes

Par défaut, le jeu affiche des avertissements détaillés pour aider les nouveaux joueurs à identifier rapidement les dangers. Les joueurs expérimentés peuvent passer en mode minimal, qui n’affiche que les avertissements concernant les bords de falaises (menaces critiques et imminentes) :

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Dépannage

**Si quelque chose ne va pas, exécutez d’abord `trail self-check`.** Il indique si Ollama est accessible, si votre sauvegarde se charge correctement et quel modèle est installé. Voici les trois problèmes possibles :

| Symptôme | Cause | Solution |
|---------|-------|-----|
| **Generic / no narration** | Ollama n’est pas en cours d’exécution (le GM est facultatif et assure une sauvegarde, il ne cause jamais de problèmes). | Démarrez Ollama (`ollama serve`) ou jouez de manière déterministe avec `--gm-off`. Exécutez `trail self-check` pour confirmer. |
| **Le registre est en attente / le règlement a échoué.** | XRPL Testnet est un réseau de test public et peut parfois être instable. | `trail ledger reconcile` réessaie les règlements ayant échoué ; exécutez-le à nouveau lorsque le réseau sera rétabli. Les données sont correctes localement dans tous les cas. |
| **Save won't resume** | `run.json` a été tronqué ou corrompu pendant l’écriture. | Le moteur le met en quarantaine sous le nom de `run.json.corrupt-<timestamp>` avant de le rejeter, afin que votre prochaine sauvegarde ne puisse pas altérer les preuves. Restaurez à partir de cette sauvegarde ou démarrez une nouvelle partie à partir d’une graine. |

Le premier tour narratif charge le modèle et peut prendre 10 à 30 secondes, ce qui est normal et ne signifie pas que le jeu est bloqué. Pour plus de détails : [Manuel de dépannage](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/).

## Configuration requise

- Python 3.11+
- Ollama (facultatif, pour la narration par IA)
- xrpl-py (facultatif, pour le sac à dos du registre)

## Sécurité

Aucune télémétrie. Aucun compte. La narration par GM est activée par défaut (HTTP vers Ollama local) ; passez `--gm-off` pour la désactiver. XRPL est désactivé jusqu’à `trail ledger enable` (uniquement Testnet). L’utilisation de la voix est facultative (`--voice`). Consultez [SECURITY.md](SECURITY.md) pour obtenir le modèle complet des menaces.

## Licence

MIT

Créé par <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
