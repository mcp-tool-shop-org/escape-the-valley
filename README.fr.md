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

Un maître de jeu IA optionnel (alimenté par Ollama) raconte votre voyage avec trois voix distinctes pour raconter l’histoire. Un registre XRPL Testnet optionnel suit les changements de vos provisions sous forme de reçus en chaîne, ce qui prouve que vous avez survécu ou que vous avez essayé.

## Nouveautés dans la version 1.1.0

- **Narration en continu** : le maître du jeu écrit mot par mot, composant chaque étape en direct au lieu de publier un bloc terminé après une pause.
- **Finalités graduées** : les parties se terminent par un épilogue avec une note (triomphante, éprouvée, pyrrhique ou perdue), basé sur qui a survécu, combien de temps cela a pris et quel a été le coût du voyage – et non pas une simple cause de décès.
- **Enjeux réels** : les événements peuvent désormais blesser ou tuer des membres du groupe. Un mauvais choix peut coûter la vie, et la mort est attribuée à sa cause réelle.
- **Preuve de rapprochement sur le registre** : un mode d’audit qui rejoue les reçus de règlement d’une partie et les vérifie par rapport au XRPL Testnet, afin que l’historique des provisions puisse être vérifié indépendamment.
- **Artefacts de la partie** : chaque partie terminée laisse un souvenir : une carte postale XRPL, vos statistiques et un chemin d’exportation/de partage.

## Démarrage rapide

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

## Comment jouer

À chaque tour, vous choisissez une action dans le camp :

| Action | Ce qu’elle fait |
|--------|-------------|
| **Travel** | Avancez vers la sortie de la vallée. Coûte de la nourriture et de l’eau. Risque de panne et d’événements. |
| **Rest** | Soignez le groupe, rétablissez le moral. Coûte des provisions mais n’offre aucun progrès. |
| **Hunt** | Dépensez des munitions pour tenter d’obtenir de la nourriture. Plus efficace dans les forêts et les plaines. |
| **Repair** | Dépensez une pièce de rechange pour réparer le chariot. Essentiel à la survie. |

Les **événements** interrompent le voyage avec des choix (A/B/C). Les choix prudents sont plus sûrs mais coûtent du temps. Les choix audacieux sont plus rapides mais risqués. Il n’y a pas de réponse toujours correcte.

**Le chariot est primordial.** S’il tombe en panne sans pièces, la partie est terminée. Maintenez-le à plus de la moitié de sa capacité et effectuez des opérations d’entretien (repos puis réparation) pour une résistance temporaire aux pannes.

Le **rythme** contrôle la vitesse par rapport à la sécurité. Le rythme normal est le réglage par défaut. Un rythme soutenu permet de parcourir plus de terrain, mais consomme plus de provisions et use les chariots plus rapidement.

Les **soupapes d’échappement** (rations réduites, réparations désespérées, abandon du chargement) existent pour les situations d’urgence. Elles ont des effets secondaires et des temps de recharge – ce sont des solutions de dernier recours, pas des stratégies.

Pour obtenir des conseils plus approfondis, consultez le [Guide de survie](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/).

## Profils du maître du jeu

Le narrateur IA façonne le ton, et non la mécanique. Les trois profils jouent au même jeu.

- **Chroniqueur** : sobre, pragmatique, concis. Folklore minimal. Raconte ce qui s’est passé.
- **Conte du feu de camp** : narrateur sérieux autour du feu de camp. Moments subtilement étranges. Le profil par défaut.
- **Porteur de lanterne** : étrange et liminal, mais toujours ancré dans les conséquences. Le plus bizarre.

Définissez avec `--gm-profile` : `trail tui --gm-profile lantern`

## Provisions

Le jeu suit 12 types de ressources répartis en deux catégories :

**Consommables** : nourriture, eau, bois de chauffage, médicaments, sel, munitions, huile pour lanterne, tissu

**Équipement** : pièces, corde, outils, bottes

Les 5 provisions principales (nourriture, eau, médicaments, munitions, pièces) sont les plus importantes. Les provisions supplémentaires comme le bois de chauffage, le sel, l’huile pour lanterne et le tissu ajoutent de la profondeur : le bois de chauffage alimente les camps nocturnes, le sel empêche la détérioration des aliments, l’huile pour lanterne permet de voyager en toute sécurité la nuit et le tissu répare l’équipement et la bâche du chariot.

## Sac à dos du registre (facultatif)

Le sac à dos du registre suit vos 5 provisions principales (nourriture, eau, médicaments, munitions, pièces) sous forme de jetons sur le XRPL Testnet. Chaque point de contrôle enregistre un reçu de règlement en chaîne. À la fin de votre partie, votre registre inclut les ID de transaction que n’importe qui peut vérifier.

Entièrement facultatif. Le jeu se joue de la même manière sans (le réglage par défaut). Activez-le à partir du menu L dans l’interface utilisateur ou via la ligne de commande :

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
```

Nécessite `pip install -e ".[xrpl]"` pour la dépendance `xrpl-py`.

## Commandes

| Commande | Description |
|---------|-------------|
| `trail tui` | Lancez l’interface utilisateur textuelle en plein écran |
| `trail new` | Démarrez une nouvelle partie (mode CLI classique) |
| `trail play` | Reprenez une partie enregistrée (mode CLI classique) |
| `trail status` | Affichez le groupe, le chariot et les provisions |
| `trail journal` | Affichez les entrées de journal récentes |
| `trail self-check` | Vérifiez l’état de l’environnement du jeu |
| `trail version` | Affichez la version |
| `trail ledger status` | Affichez l’état du sac à dos |
| `trail ledger enable` | Activez le sac à dos XRPL |
| `trail ledger disable` | Désactivez le sac à dos XRPL |
| `trail ledger settle` | Réglez manuellement un point de contrôle |
| `trail ledger reconcile` | Réessayez les règlements ayant échoué |
| `trail ledger wallet` | Affichez les détails du portefeuille |
| `trail stats` | Affichez les statistiques de la partie (prend en charge `--json`) |
| `trail parcel send <addr> <supply> <amount>` | Envoyez des provisions à un autre voyageur |
| `trail parcel list` | Affichez les colis reçus |
| `trail parcel accept <id>` | Acceptez un colis en attente |
| `trail parcel sent` | Affichez les colis que vous avez envoyés |
| `trail wallet share` | Affichez l’adresse de votre portefeuille pour le commerce |

## Alertes

Par défaut, le jeu affiche des alertes détaillées pour aider les nouveaux joueurs à détecter rapidement les dangers. Les joueurs expérimentés peuvent passer au mode minimal, qui n’affiche que les alertes de dernière minute (menaces critiques) :

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Dépannage

**Si quelque chose ne va pas, exécutez d’abord `trail self-check`.** Il indique si Ollama est accessible, si votre sauvegarde se charge et quel modèle est installé. Les trois problèmes courants :

| Symptôme | Cause | Solution |
|---------|-------|-----|
| **Generic / no narration** | Ollama n’est pas en cours d’exécution (le GM est facultatif et revient à une configuration par défaut, il ne cause jamais de problèmes). | Démarrez Ollama (`ollama serve`) ou utilisez l’option `--gm-off` pour un fonctionnement déterministe. Exécutez `trail self-check` pour confirmer. |
| **Grand livre en attente / règlement échoué** | Le réseau de test XRPL est un réseau de test public et peut parfois être instable. | `trail ledger reconcile` tente à nouveau les règlements ayant échoué ; exécutez-le à nouveau lorsque le réseau sera rétabli. Les données locales sont correctes dans tous les cas. |
| **Save won't resume** | Le fichier `run.json` a été tronqué ou corrompu pendant son écriture. | Le moteur le met en quarantaine sous le nom `run.json.corrupt-<horodatage>` avant de le rejeter, afin que votre prochaine sauvegarde ne puisse pas remplacer les données existantes. Restaurez-le à partir de cette sauvegarde ou démarrez une nouvelle exécution à partir d’une graine. |

Le premier tour narré charge le modèle et peut prendre 10 à 30 secondes ; c’est normal, ce n’est pas un blocage. Pour plus de détails : [Manuel de résolution des problèmes](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/).

## Prérequis

- Python 3.11+
- Ollama (facultatif, pour la narration par IA)
- xrpl-py (facultatif, pour le sac à dos du grand livre)

## Sécurité

Aucune télémétrie. Aucun compte. Toutes les fonctionnalités réseau (Ollama, XRPL) sont facultatives et désactivées par défaut. Les opérations XRPL utilisent uniquement le Testnet. Consultez [SECURITY.md](SECURITY.md) pour obtenir le modèle de menace complet.

## Licence

MIT

Créé par <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
