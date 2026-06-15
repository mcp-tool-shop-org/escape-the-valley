<p align="center">
  <a href="README.ja.md">日本語</a> | <a href="README.zh.md">中文</a> | <a href="README.es.md">Español</a> | <a href="README.fr.md">Français</a> | <a href="README.hi.md">हिन्दी</a> | <a href="README.it.md">Italiano</a> | <a href="README.md">English</a>
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

## O que é isto?

Escape the Valley é um jogo de sobrevivência no estilo Oregon Trail, que roda no seu terminal. Lidere um grupo de colonos por uma região selvagem gerada proceduralmente. Gerencie comida, água, condição da carroça e moral enquanto enfrenta eventos, perigos e decisões difíceis.

Um Mestre do Jogo de IA opcional (alimentado pelo Ollama) narra sua jornada com três vozes distintas para contar histórias. Um livro-razão XRPL Testnet opcional rastreia as mudanças em seus suprimentos como recibos na cadeia — prova de que você sobreviveu ou prova de que tentou.

## O que há de novo na versão 1.1.0?

- **Narração em streaming** — o Mestre do Jogo escreve token por token, compondo cada momento ao vivo, em vez de apresentar um bloco completo após uma pausa.
- **Finais graduados** — as partidas terminam com um epílogo graduado (triunfante, desgastado, pírrico ou perdido), lido a partir de quem sobreviveu, quanto tempo demorou e qual foi o custo da jornada — não apenas uma causa única da morte.
- **Riscos reais** — os eventos agora podem ferir ou matar o grupo. Uma má escolha pode custar uma vida, e a morte será atribuída à sua causa real.
- **Prova de reconciliação no livro-razão** — um modo de auditoria que reproduz os recibos de cada ponto de passagem e os verifica em relação ao XRPL Testnet, para que o histórico de suprimentos possa ser verificado independentemente.
- **Artefatos da jornada** — cada jornada concluída deixa uma lembrança: um cartão postal XRPL, suas estatísticas e um caminho para exportar/compartilhar.

## Guia rápido

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

## Como jogar

A cada turno, você escolhe uma ação no acampamento:

| Ação | O que ela faz |
|--------|-------------|
| **Travel** | Mova-se em direção à saída do vale. Custa comida e água. Risco de avaria e eventos. |
| **Rest** | Cure o grupo, recupere a moral. Custa suprimentos, mas não há progresso. |
| **Hunt** | Gaste munição para ter uma chance de conseguir comida. Melhor em florestas e planícies. |
| **Repair** | Gaste uma peça sobressalente para consertar a carroça. Essencial para a sobrevivência. |

**Eventos** interrompem a viagem com escolhas (A/B/C). Escolhas cautelosas são mais seguras, mas custam tempo. Escolhas ousadas são mais rápidas, mas arriscadas. Não há sempre uma resposta certa.

**A carroça é tudo.** Se ela quebrar e não houver peças sobressalentes, a jornada termina. Mantenha-a acima de metade da sua capacidade e faça manutenções (descanse e repare) para obter resistência temporária contra avarias.

**Ritmo** controla a velocidade em relação à segurança. O ritmo normal é o padrão. Um ritmo acelerado cobre mais terreno, mas consome mais suprimentos e danifica as carroças mais rapidamente.

**Válvulas de escape** (ração escassa, reparo desesperado, abandono da carga) existem para emergências. Elas têm efeitos colaterais e tempos de recarga — são o último recurso, não estratégias.

Para dicas mais detalhadas, consulte o [Guia de sobrevivência](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/).

## Perfis do Mestre do Jogo

O narrador de IA molda o tom, não a mecânica. Todos os três perfis jogam o mesmo jogo.

- **Cronista** — Objetivo, prático, conciso. Folclore mínimo. Relata o que aconteceu.
- **Contador de histórias** — Narrador sério ao redor da fogueira. Momentos sutis e estranhos. É o padrão.
- **Portador da lanterna** — Estranho e liminar, mas ainda fundamentado em consequências. É o mais peculiar.

Defina com `--gm-profile`: `trail tui --gm-profile lantern`

## Suprimentos

O jogo rastreia 12 tipos de recursos em duas categorias:

**Consumíveis:** comida, água, lenha, medicamentos, sal, munição, óleo para lanterna, tecido

**Equipamento:** peças sobressalentes, corda, ferramentas, botas

Os 5 suprimentos principais (comida, água, medicamentos, munição e peças) são os mais críticos. Suprimentos adicionais, como lenha, sal, óleo para lanterna e tecido, adicionam profundidade: a lenha alimenta os acampamentos noturnos, o sal evita que os alimentos estraguem, o óleo para lanterna permite viagens noturnas mais seguras e o tecido remenda equipamentos e a cobertura da carroça.

## Livro-razão (Opcional)

O livro-razão rastreia seus 5 suprimentos principais (comida, água, medicamentos, munição e peças) como tokens na XRPL Testnet. Cada ponto de passagem registra um recibo no livro-razão. No final da sua jornada, seu livro-razão incluirá IDs de transação que qualquer pessoa pode verificar.

Completamente opcional. O jogo funciona da mesma forma com ele desativado (o padrão). Ative-o a partir do menu L na interface TUI ou via CLI:

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
```

Requer `pip install -e ".[xrpl]"` para a dependência `xrpl-py`.

## Comandos

| Comando | Descrição |
|---------|-------------|
| `trail tui` | Inicie a interface textual de tela cheia |
| `trail new` | Comece uma nova jornada (modo CLI clássico) |
| `trail play` | Continue uma jornada salva (modo CLI clássico) |
| `trail status` | Mostre o grupo, a carroça e os suprimentos |
| `trail journal` | Mostre as entradas recentes do diário |
| `trail self-check` | Verifique a saúde do ambiente do jogo |
| `trail version` | Mostre a versão |
| `trail ledger status` | Mostre o status da mochila |
| `trail ledger enable` | Ative a mochila XRPL |
| `trail ledger disable` | Desative a mochila XRPL |
| `trail ledger settle` | Resolva manualmente um ponto de passagem |
| `trail ledger reconcile` | Tente novamente as resoluções com falha |
| `trail ledger wallet` | Mostre os detalhes da carteira |
| `trail stats` | Mostre as estatísticas da jornada (suporta `--json`) |
| `trail parcel send <addr> <supply> <amount>` | Envie suprimentos para outro viajante |
| `trail parcel list` | Liste os pacotes recebidos |
| `trail parcel accept <id>` | Aceite um pacote pendente |
| `trail parcel sent` | Liste os pacotes que você enviou |
| `trail wallet share` | Imprima o endereço da sua carteira para negociação |

## Alertas

Por padrão, o jogo exibe alertas detalhados para ajudar os novos jogadores a identificar perigos precocemente. Jogadores experientes podem alternar para o modo mínimo, que mostra apenas alertas de último momento (ameaças críticas):

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Solução de problemas

**Se algo parecer errado, execute `trail self-check` primeiro.** Ele informa se o Ollama está acessível, se seu jogo salvo é carregado e qual modelo está instalado. As três coisas que podem dar errado:

| Sintoma | Causa | Correção |
|---------|-------|-----|
| **Generic / no narration** | Requisitos | - Python 3.11+
- Ollama (opcional, para narração com IA)
- xrpl-py (opcional, para o "ledger backpack") |
| Segurança | Sem telemetria. Sem contas. Todos os recursos de rede (Ollama, XRPL) são opcionais e desativados por padrão. As operações XRPL usam apenas a Testnet. Consulte [SECURITY.md](SECURITY.md) para obter o modelo completo de ameaças. | Licença |
| **Save won't resume** | MIT | Criado por <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a> |

**Registro pendente / liquidação falhou**

## A XRPL Testnet é uma rede de teste pública e, às vezes, apresenta instabilidade

`trail ledger reconcile` tenta novamente as liquidações com falha; execute-o novamente quando a rede se recuperar. Os saldos estão corretos localmente, de qualquer forma.

## `run.json` foi truncado ou corrompido durante a gravação

O motor o coloca em quarentena como `run.json.corrupt-<timestamp>` antes de recusá-lo, para que sua próxima salvaguarda não possa apagar as evidências. Recupere do backup ou inicie uma nova execução a partir de uma semente.

## A primeira rodada narrada carrega o modelo e pode levar de 10 a 30 segundos — isso é normal, não é um travamento. Detalhes completos: [Manual de solução de problemas](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/)

Inicie o Ollama (`ollama serve`) ou jogue deterministicamente com `--gm-off`. Execute `trail self-check` para confirmar.

Ollama não está em execução (o GM é opcional e, se falhar, nunca corrompe os dados).
