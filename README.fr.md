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

Escape the Valley est un jeu de survie de type Oregon Trail qui s'exécute dans votre terminal. Menez un groupe de colons à travers une nature sauvage générée de manière procédurale. Gérez la nourriture, l'eau, l'état du chariot et le moral tout en naviguant à travers des événements, des dangers et des choix difficiles.

Un maître de jeu IA optionnel (alimenté par Ollama) raconte votre voyage avec trois voix narratives distinctes. Un XRPL Testnet de registre optionnel suit les changements de vos provisions sous forme de reçus enregistrés sur la chaîne de blocs, prouvant que vous avez survécu, ou que vous avez essayé.

## Nouveautés

**1.3.2** — Délai de refroidissement par `event_id` (`N=4`) empêche la répétition du même événement lors de tours consécutifs ou presque consécutifs (#21 ; 1.3.1 → 1.3.2 déplace les tirages d'événements après qu'une répétition se serait produite, de sorte qu'une partie enregistrée dans 1.3.1 se poursuit différemment après la mise à niveau vers 1.3.2, car les tirages d'événements sont déplacés. Dans une même version, la même séquence et les mêmes choix donnent la même partie). Le panneau des résultats CLI sans maître de jeu (GM-off) affiche le résumé par défaut une fois (#20), les indices de choix suppriment la double ponctuation finale, et `1 mile remaining.` utilise une grammaire singulière. Les fichiers `.trail/secrets.json` corrompus sont mis en quarantaine avant d'être réécrits (#31.7). Les sorties `THIRD_PARTY_NOTICES*.txt` et `trail licenses` sont normalisées en LF sur toutes les plateformes, et CI vérifie l'intégrité de la licence fournie par rapport à l'archive python-build-standalone téléchargée (#31.1, #31.2, #31.3, #31.5).

**1.3.1** — Les exécutions CLI (`trail new` / `trail continue`) émettent désormais des cartes mémoire et une cause de décès canonique, comme dans l'interface TUI (`trail stats`, les jours de décès du registre Trail, et les résumés de mémoire du GM fonctionnent dans les deux modes). L'option Abandonner le chargement de la TUI signale le chargement réel abandonné. Les invites de la route de bifurcation ne consomment plus un tour d'entretien du chariot ou de délai de refroidissement de la soupape d'échappement. Les parties rejouées avec la même séquence obtiennent des `run_id` distincts pour les XRPL mémos. Les binaires GitHub Release / `npx` incluent désormais `xrpl-py`. La sortie Windows en mode pipe (`cp1252`) ne plante plus sur les caractères de dessin de boîte Rich (#19).

**1.3.0** — Quatre fins que vous pouvez réellement atteindre, y compris celle où vous êtes endurci par les épreuves. Le chariot est une menace, et non le jeu entier. Le début de l'hiver ralentit une année marquée. Les gués du désert restent hors du sable. La chasse indique qui a été blessé. `npx @mcptoolshop/escape-the-valley` lance le binaire v1.3.0 (il était bloqué sur l'artefact v1.1.1 GitHub obsolète).

**1.2.1** — La roue PyPI télécharge réellement (Version des métadonnées 2.3). Même produit que 1.2.0.

**1.2.0** — Les binaires GitHub Release sont lancés (bibliothèque d'événements + feuille de style incluse ; les assertions de test de base vérifient `--help` et ≥200 événements). `trail ledger proof` audite la sauvegarde chargée. La TUI affiche la séquence/doctrine/imprévus/moral ; `c` fait défiler le rythme ; le menu du registre `R` prouve cette sauvegarde. L'interface utilisateur est lisible en 80×24 et 120×30.

**1.1.1** — `pip install "escape-the-valley[voice]"` s'installe réellement (le paquet supplémentaire avait épinglé un paquet non publié). Le binaire PyInstaller n'inclut plus le paquet supplémentaire de voix.

**1.1.0** — narration en continu, fins graduées, événements pouvant blesser, preuve de réconciliation sur le registre, artefacts de partie.

Les binaires joints aux v1.1.0 et v1.1.1 GitHub Releases ne se lancent pas (un import relatif dans le point d'entrée figé). Utilisez `pip install escape-the-valley` ou le binaire v1.2.0+ GitHub / le lanceur `npx`.

## Démarrage rapide

```bash
pip install escape-the-valley

# Zero-prerequisite npm launcher (GitHub v1.3.2 binary; v1.1.0 and v1.1.1
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

## Comment jouer

À chaque tour, vous choisissez une action dans le camp :

| Action | Ce qu'elle fait |
|--------|-------------|
| **Travel** | Avancez vers la sortie de la vallée. Coûte de la nourriture et de l'eau. Risque de panne et d'événements. |
| **Rest** | Soignez le groupe, rétablissez le moral. Coûte des provisions, mais n'offre aucun progrès. |
| **Hunt** | Dépensez des munitions pour tenter d'obtenir de la nourriture. Mieux dans les forêts et les plaines. |
| **Repair** | Dépensez une pièce de rechange pour réparer le chariot. Essentiel pour la survie. |

Les **événements** interrompent le voyage avec des choix (A/B/C). Les choix prudents sont plus sûrs, mais coûtent du temps. Les choix audacieux sont plus rapides, mais risqués. Il n'y a pas de réponse toujours correcte.

**Le chariot est primordial.** S'il tombe en panne sans pièces de rechange, la partie est terminée. Maintenez-le à plus de la moitié de son état et effectuez des périodes d'entretien (repos puis réparation) pour une résistance temporaire aux pannes.

Le **rythme** contrôle la vitesse par rapport à la sécurité. Le rythme régulier est le mode par défaut. Un rythme rapide couvre plus de terrain, mais consomme plus de provisions et use plus rapidement les chariots.

Les **soupapes d'échappement** (rations forcées, réparations désespérées, abandon du chargement) existent pour les situations d'urgence. Elles ont des effets secondaires et des délais de refroidissement : ce sont des solutions de dernier recours, et non des stratégies.

Pour obtenir des conseils plus approfondis, consultez le [Guide de survie](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/).

## Profils du GM

Le narrateur IA façonne le ton, et non la mécanique. Les trois profils jouent au même jeu.

- **Chronicler** — Concret, pragmatique, sobre. Folklore minimal. Raconte ce qui s'est passé.
- **Fireside** — Narrateur de feu de camp sérieux. Moments subtilement étranges. Le profil par défaut.
- **Lantern-Bearer** — Étrange et liminal, mais toujours ancré dans les conséquences. Le profil le plus étrange.

Définir avec `--gm-profile` : `trail tui --gm-profile lantern`

## Provisions

Le jeu suit 12 types de ressources répartis en deux catégories :

**Consommables :** nourriture, eau, bois de chauffage, médicaments, sel, munitions, huile de lampe, tissu

**Équipement :** pièces de rechange, corde, outils, bottes

Les 5 provisions de base (nourriture, eau, médicaments, munitions, pièces de rechange) sont les plus importantes. Les provisions supplémentaires, telles que le bois de chauffage, le sel, l'huile de lampe et le tissu, ajoutent de la profondeur : le bois de chauffage alimente les camps de nuit, le sel empêche la détérioration des aliments, l'huile de lampe permet des voyages de nuit plus sûrs et le tissu répare l'équipement et la bâche du chariot.

## Ledger Backpack (Facultatif)

Le Ledger Backpack suit vos 5 provisions de base (nourriture, eau, médicaments, munitions, pièces de rechange) sous forme de jetons sur le XRPL Testnet. Chaque point de contrôle de la ville enregistre un reçu de règlement sur la chaîne de blocs. À la fin de votre partie, votre registre trail comprend des ID de transaction que n'importe qui peut vérifier.

Entièrement facultatif. Le jeu se joue de la même manière avec cette option désactivée (la valeur par défaut). Activez-le à partir du menu L dans la TUI ou via la ligne de commande :

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

Dans la TUI, **L** ouvre le menu du registre ; avec le sac à dos activé, **R** prouve cette sauvegarde (mêmes résultats). Il ne se repose pas.

Inclus dans le fichier binaire de la version `npx` / GitHub ; pour les installations `pip`, exécutez `pip install "escape-the-valley[xrpl]"` (ou `pip install -e ".[xrpl]"`) pour la dépendance `xrpl-py`.

## Commandes

| Commande | Description |
|---------|-------------|
| `trail tui` | Lancer l’interface utilisateur Textual en plein écran |
| `trail new` | Démarrer une nouvelle partie (mode CLI classique) |
| `trail play` | Reprendre une partie enregistrée (mode CLI classique) |
| `trail status` | Afficher le groupe, le chariot et les provisions |
| `trail journal` | Afficher les dernières entrées du journal |
| `trail self-check` | Vérifier l’état de l’environnement du jeu |
| `trail version` | Afficher la version |
| `trail licenses` | Afficher la licence et les mentions relatives aux logiciels tiers (ou `trail --licenses`) |
| `trail ledger status` | Afficher l’état du sac à dos |
| `trail ledger enable` | Activer le sac à dos XRPL |
| `trail ledger disable` | Désactiver le sac à dos XRPL |
| `trail ledger settle` | Valider manuellement un point de contrôle |
| `trail ledger reconcile` | Réessayer les validations ayant échoué |
| `trail ledger proof` | Vérifier la sauvegarde chargée (PASS/FAIL/INCONCLUSIVE) |
| `trail ledger wallet` | Afficher les détails du portefeuille |
| `trail stats` | Afficher les statistiques de la partie et les cartes mémoire (prend en charge `--json`) |
| `trail parcel send <addr> <supply> <amount>` | Envoyer des provisions à un autre voyageur |
| `trail parcel list` | Afficher la liste des colis reçus |
| `trail parcel accept <id>` | Accepter un colis en attente |
| `trail parcel sent` | Afficher la liste des colis que vous avez envoyés |
| `trail wallet share` | Afficher l’adresse de votre portefeuille pour les échanges |

## Alertes

Par défaut, le jeu affiche des alertes détaillées pour aider les nouveaux joueurs à identifier rapidement les dangers. Les joueurs expérimentés peuvent passer en mode minimal, qui n’affiche que les alertes critiques (menaces de dernière minute) :

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Dépannage

**Si quelque chose ne va pas, exécutez d’abord `trail self-check`.** Il indique si Ollama est accessible, si votre sauvegarde se charge et quel modèle est installé. Les trois problèmes courants :

| Symptôme | Cause | Solution |
|---------|-------|-----|
| **Generic / no narration** | Ollama ne fonctionne pas (le GM est facultatif et se désactive, il ne cause jamais de problèmes) | Démarrez Ollama (`ollama serve`), ou jouez en mode déterministe avec `--gm-off`. Exécutez `trail self-check` pour confirmer. |
| **Registre en attente / validation ayant échoué** | XRPL Testnet est un réseau de test public et peut parfois être instable | `trail ledger reconcile` réessaie les validations ayant échoué ; exécutez-le à nouveau lorsque le réseau se rétablira. Les provisions sont correctes localement dans tous les cas. |
| **Save won't resume** | `run.json` a été tronqué ou corrompu pendant l’écriture | Le moteur le met en quarantaine en tant que `run.json.corrupt-<timestamp>` avant de le refuser, de sorte que votre prochaine sauvegarde ne puisse pas altérer les preuves. Restaurez-le à partir de cette sauvegarde ou démarrez une nouvelle partie à partir d’une graine. |

Le premier tour narratif charge le modèle et peut prendre 10 à 30 secondes, ce qui est normal et ne signifie pas que le jeu est bloqué. Pour plus de détails : [Manuel de dépannage](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/).

## Configuration requise

- Python 3.11+
- Ollama (facultatif, pour la narration par IA)
- xrpl-py (facultatif, pour le sac à dos du registre sur les installations `pip` ; inclus dans le fichier binaire de la version `npx` / GitHub)

## Sécurité

Aucune télémétrie. Pas de comptes. La narration du GM est activée par défaut (HTTP vers Ollama local ; passez `--gm-off` pour la désactiver). XRPL est désactivé jusqu’à `trail ledger enable` (uniquement Testnet). La voix est activée par l’utilisateur (`--voice`). Consultez [SECURITY.md](SECURITY.md) pour obtenir le modèle de menace complet.

## Licence

MIT : exécutez `trail licenses` (ou `trail --licenses`) pour afficher la licence du jeu et les mentions relatives aux logiciels tiers (également jointes à chaque version GitHub, un fichier par plateforme : `THIRD_PARTY_NOTICES-<os>-<arch>.txt`).

Créé par <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
