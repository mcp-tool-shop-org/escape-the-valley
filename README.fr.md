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

Escape the Valley est un jeu de survie de type Oregon Trail qui se déroule dans votre terminal. Menez un groupe de colons à travers une nature sauvage générée de manière procédurale. Gérez la nourriture, l’eau, l’état du chariot et le moral tout en naviguant à travers des événements, des dangers et des choix difficiles.

Un maître de jeu IA optionnel (alimenté par Ollama) raconte votre voyage avec trois voix narratives distinctes. Un registre XRPL Testnet optionnel suit les changements de vos provisions sous forme de reçus en chaîne — preuve que vous avez survécu, ou preuve que vous l’avez essayé.

## Nouveautés

**1.2.0** — Lancement des fichiers binaires de la version sur GitHub (bibliothèque d’événements + feuille de style intégrée ; tests rapides `--help` et ≥ 200 événements). `trail ledger proof` vérifie le fichier de sauvegarde chargé. L’interface utilisateur en mode texte affiche les données initiales/les principes/les rebondissements/le moral ; `c` ajuste le rythme ; le menu du registre `R` confirme la validité de ce fichier de sauvegarde. L’affichage HUD est lisible en 80 × 24 et 120 × 30.

**1.1.1** — `pip install "escape-the-valley[voice]"` s’installe réellement (la version précédente avait épinglé un paquet non publié). Le binaire PyInstaller n’inclut plus la voix supplémentaire.

**1.1.0** — narration en continu, fins graduées, événements pouvant causer des blessures, preuve de réconciliation sur le registre, artefacts d’exécution.

Binaries attached to the v1.1.0 and v1.1.1 GitHub Releases do not launch (a relative import in the frozen entrypoint). Use `pip install escape-the-valley` or the v1.2.0+ GitHub binary / `npx` launcher.

## Démarrage rapide

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

## Comment jouer

À chaque tour, vous choisissez une action dans le camp :

| Action | Ce qu’elle fait |
|--------|-------------|
| **Travel** | Avancez vers la sortie de la vallée. Coûte de la nourriture et de l’eau. Risque de panne et d’événements. |
| **Rest** | Soignez le groupe, rétablissez le moral. Coûte des provisions mais n’offre aucun progrès. |
| **Hunt** | Dépensez des munitions pour tenter d’obtenir de la nourriture. Plus efficace dans les forêts et les plaines. |
| **Repair** | Dépensez une pièce de rechange pour réparer le chariot. Essentiel à la survie. |

**Événements** interrompent le voyage avec des choix (A/B/C). Les choix prudents sont plus sûrs mais coûtent du temps. Les choix audacieux sont plus rapides mais risqués. Il n’y a pas de réponse toujours correcte.

**Le chariot est primordial.** S’il tombe en panne sans pièces de rechange, la partie est terminée. Maintenez-le à plus de la moitié de sa capacité et effectuez des opérations d’entretien (repos puis réparation) pour une résistance temporaire aux pannes.

**Rythme** contrôle la vitesse par rapport à la sécurité. Le rythme régulier est le mode par défaut. Un rythme soutenu permet de parcourir plus de terrain, mais consomme plus de provisions et use les chariots plus rapidement.

**Solutions de secours** (rations réduites, réparations désespérées, abandon du chargement) existent pour les situations d’urgence. Elles ont des effets secondaires et des délais — ce sont des solutions de dernier recours, pas des stratégies.

Pour obtenir des conseils plus approfondis, consultez le [Guide de survie](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/).

## Profils du maître de jeu

Le narrateur IA façonne le ton, et non les mécanismes. Les trois profils jouent au même jeu.

- **Chroniqueur** — Concret, pragmatique, sobre. Folklore minimal. Raconte ce qui s’est passé.
- **Conte du feu de camp** — Narrateur sérieux autour du feu de camp. Moments subtilement étranges. Le profil par défaut.
- **Porteur de lanterne** — Étrange et liminal, mais toujours ancré dans les conséquences. Le plus bizarre.

Définir avec `--gm-profile` : `trail tui --gm-profile lantern`

## Provisions

Le jeu suit 12 types de ressources répartis en deux catégories :

**Consommables :** nourriture, eau, bois de chauffage, médicaments, sel, munitions, huile de lanterne, tissu

**Équipement :** pièces de rechange, corde, outils, bottes

Les 5 provisions principales (nourriture, eau, médicaments, munitions, pièces de rechange) sont les plus importantes. Les provisions supplémentaires telles que le bois de chauffage, le sel, l’huile de lanterne et le tissu ajoutent de la profondeur : le bois de chauffage alimente les camps nocturnes, le sel empêche la détérioration des aliments, l’huile de lanterne permet de voyager plus en sécurité la nuit et le tissu répare l’équipement et la bâche du chariot.

## Sac à dos du registre (facultatif)

Le sac à dos du registre suit vos 5 provisions principales (nourriture, eau, médicaments, munitions, pièces de rechange) sous forme de jetons sur le XRPL Testnet. Chaque point de contrôle enregistre un reçu de règlement en chaîne. À la fin de votre partie, votre historique inclut les ID des transactions que n’importe qui peut vérifier.

Entièrement facultatif. Le jeu se joue de la même manière sans (le mode par défaut). Activez-le à partir du menu L dans l’interface utilisateur ou via la ligne de commande :

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

Dans l’interface utilisateur, **L** ouvre le menu du registre ; avec le sac à dos activé, **R** valide cette sauvegarde (mêmes résultats). Il ne permet pas de se reposer.

Nécessite `pip install -e ".[xrpl]"` pour la dépendance `xrpl-py`.

## Commandes

| Commande | Description |
|---------|-------------|
| `trail tui` | Lance l’interface utilisateur textuelle en plein écran |
| `trail new` | Démarre une nouvelle partie (mode CLI classique) |
| `trail play` | Continue une partie sauvegardée (mode CLI classique) |
| `trail status` | Affiche le groupe, le chariot et les provisions |
| `trail journal` | Affiche les entrées de journal récentes |
| `trail self-check` | Vérifie l’état de l’environnement du jeu |
| `trail version` | Affiche la version |
| `trail ledger status` | Affiche l’état du sac à dos |
| `trail ledger enable` | Active le sac à dos XRPL |
| `trail ledger disable` | Désactive le sac à dos XRPL |
| `trail ledger settle` | Règle manuellement un point de contrôle |
| `trail ledger reconcile` | Réessaie les règlements ayant échoué |
| `trail ledger proof` | Valide la sauvegarde chargée (PASSÉ/ÉCHOUÉ/INCONCLUSIF) |
| `trail ledger wallet` | Affiche les détails du portefeuille |
| `trail stats` | Affiche les statistiques de la partie (prend en charge `--json`) |
| `trail parcel send <addr> <supply> <amount>` | Envoie des provisions à un autre voyageur |
| `trail parcel list` | Liste les colis reçus |
| `trail parcel accept <id>` | Accepte un colis en attente |
| `trail parcel sent` | Affiche la liste des colis que vous avez envoyés |
| `trail wallet share` | Imprime l’adresse de votre portefeuille pour le commerce |

## Alertes

Par défaut, le jeu affiche des alertes détaillées pour aider les nouveaux joueurs à identifier rapidement les dangers. Les joueurs expérimentés peuvent passer au mode minimal, qui n’affiche que les alertes critiques (en dernier recours) :

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Dépannage

**Si quelque chose ne va pas, exécutez d’abord `trail self-check`.** Il indique si Ollama est accessible, si votre sauvegarde se charge et quel modèle est installé. Les trois problèmes courants :

| Symptôme | Cause | Solution |
|---------|-------|-----|
| **Generic / no narration** | Ollama ne fonctionne pas (le maître de jeu est facultatif et revient à une option par défaut, sans jamais bloquer le jeu) | Démarrez Ollama (`ollama serve`) ou utilisez `--gm-off` pour une exécution déterministe. Exécutez `trail self-check` pour confirmer. |
| **Grand livre en attente / règlement échoué** | Le réseau de test XRPL est un réseau de test public et peut parfois être instable. | `trail ledger reconcile` tente à nouveau les règlements ayant échoué ; exécutez-le à nouveau lorsque le réseau sera rétabli. Les données locales sont correctes dans tous les cas. |
| **Save won't resume** | `run.json` a été tronqué ou corrompu pendant l’écriture. | Le moteur le met en quarantaine sous la forme `run.json.corrupt-<timestamp>` avant de le rejeter, afin que votre prochaine sauvegarde ne puisse pas altérer les preuves. Restaurez à partir de cette sauvegarde ou démarrez une nouvelle exécution à partir d’une graine. |

Le premier tour narratif charge le modèle et peut prendre 10 à 30 secondes ; c’est normal, il ne s’agit pas d’un blocage. Pour plus de détails : [Manuel de résolution des problèmes](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/).

## Prérequis

- Python 3.11+
- Ollama (facultatif, pour la narration par IA)
- xrpl-py (facultatif, pour le sac à dos du grand livre)

## Sécurité

Aucune télémétrie. Aucun compte. La narration GM est activée par défaut (HTTP vers Ollama local) ; passez `--gm-off` pour la désactiver. XRPL est désactivé jusqu’à `trail ledger enable` (uniquement le réseau de test). L’utilisation de la voix est facultative (`--voice`). Consultez [SECURITY.md](SECURITY.md) pour obtenir le modèle complet des menaces.

## Licence

MIT

Créé par <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
