<p align="center">
  <a href="README.ja.md">日本語</a> | <a href="README.zh.md">中文</a> | <a href="README.es.md">Español</a> | <a href="README.fr.md">Français</a> | <a href="README.hi.md">हिन्दी</a> | <a href="README.it.md">Italiano</a> | <a href="README.md">English</a>
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

## O que é isso?

Escape the Valley é um jogo de sobrevivência no estilo de Oregon Trail que roda no seu terminal. Lidere um grupo de colonos através de uma área selvagem gerada aleatoriamente. Gerencie comida, água, condição da carroça e moral, enquanto lida com eventos, perigos e escolhas difíceis.

Um Narrador de IA opcional (powered by Ollama) narra sua jornada com três estilos de narração distintos. Um "mochilo" de ledger XRPL opcional rastreia suas mudanças de suprimentos como recibos na blockchain — uma prova de que você sobreviveu, ou uma prova de que você tentou.

## Início Rápido

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

## Como Jogar

A cada turno, você escolhe uma ação no acampamento:

| Ação | O que ela faz |
|--------|-------------|
| **Travel** | Mova-se em direção à saída do vale. Custa comida e água. Risco de pane e eventos. |
| **Rest** | Restaure a saúde do grupo, recupere o moral. Custa suprimentos, mas não há progresso. |
| **Hunt** | Use munição para ter uma chance de encontrar comida. Melhor em florestas e planícies. |
| **Repair** | Use uma peça sobressalente para consertar a carroça. Essencial para a sobrevivência. |

**Eventos** interrompem a viagem com opções (A/B/C). Escolhas cautelosas são mais seguras, mas custam tempo. Escolhas ousadas são mais rápidas, mas arriscadas. Não existe uma resposta sempre correta.

**A carroça é tudo.** Se ela quebrar sem peças, o jogo termina. Mantenha-a com mais da metade da condição e faça pausas para manutenção (descanse e depois repare) para aumentar temporariamente a resistência a quebras.

**Ritmo** controla a velocidade versus a segurança. Constante é o padrão. Um ritmo mais rápido cobre mais terreno, mas consome mais suprimentos e danifica as carroças mais rapidamente.

**Válvulas de escape** (ração extrema, reparo desesperado, abandonar carga) existem para emergências. Elas têm efeitos colaterais e tempos de recarga — são recursos de último caso, não estratégias.

Para dicas mais detalhadas, consulte o [Guia de Sobrevivência](docs/survival-guide.md).

## Perfis do Narrador (GM)

O narrador de IA molda o tom, não a mecânica. Todos os três perfis jogam o mesmo jogo.

- **Chronicler** — Prático, direto, conciso. Mínimo de folclore. Relata o que aconteceu.
- **Fireside** — Narrador de fogueira, sério. Momentos sutis e estranhos. O padrão.
- **Lantern-Bearer** — Estranho e ambíguo, mas ainda fundamentado nas consequências. O mais peculiar.

Defina com `--gm-profile`: `trail tui --gm-profile lantern`

## Mochilo de Ledger (Opcional)

O Mochilo de Ledger rastreia seus 5 suprimentos principais (comida, água, medicamentos, munição, peças) como tokens na rede de testes XRPL. Cada ponto de controle da cidade registra um recibo de assentamento na blockchain. No final do seu jogo, seu ledger inclui IDs de transações que qualquer pessoa pode verificar.

Completamente opcional. O jogo funciona da mesma forma quando está desativado (o padrão). Ative-o do menu L na interface de texto ou via linha de comando:

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
```

Requer `pip install -e ".[xrpl]"` para a dependência `xrpl-py`.

## Comandos

| Comando | Descrição |
|---------|-------------|
| `trail tui` | Inicia a interface de texto em tela cheia |
| `trail new` | Inicia uma nova partida (modo CLI clássico) |
| `trail play` | Continua uma partida salva (modo CLI clássico) |
| `trail status` | Mostra o grupo, a carroça e os suprimentos |
| `trail journal` | Mostra as entradas recentes do diário |
| `trail self-check` | Verifica a saúde do ambiente do jogo |
| `trail version` | Mostra a versão |
| `trail ledger status` | Mostra o status do mochilo |
| `trail ledger enable` | Ativa o mochilo XRPL |
| `trail ledger disable` | Desativa o mochilo XRPL |
| `trail ledger settle` | Registra manualmente um ponto de controle |
| `trail ledger reconcile` | Tenta novamente registros falhos |
| `trail ledger wallet` | Mostra os detalhes da carteira |
| `trail parcel list` | Lista os pacotes recebidos |
| `trail parcel accept <id>` | Aceita um pacote pendente |

## Alertas

Por padrão, o jogo mostra avisos detalhados para ajudar os novos jogadores a identificar perigos precocemente. Jogadores experientes podem mudar para o modo mínimo, que mostra apenas avisos de "beira de precipício" (ameaças críticas de última hora):

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Requisitos

- Python 3.11 ou superior
- Ollama (opcional, para narração por IA)
- xrpl-py (opcional, para o "ledger backpack")

## Segurança

Não há coleta de dados de uso. Não há contas. Todos os recursos de rede (Ollama, XRPL) são opcionais e desativados por padrão. As operações do XRPL utilizam apenas a rede de testes (Testnet). Consulte o arquivo [SECURITY.md](SECURITY.md) para obter o modelo de ameaças completo.

## Licença

MIT

Desenvolvido por <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
