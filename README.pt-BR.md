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

## O que há de novo?

**1.3.0** – Quatro finais que você realmente pode alcançar, incluindo o final “endurecido”. A carroça é uma ameaça, não o jogo inteiro. O início do inverno retarda um ano marcado. As travessias do deserto evitam a areia. A caçada mostra quem se machucou. `npx @mcptoolshop/escape-the-valley` lança o binário v1.3.0 (estava preso no artefato GitHub morto da v1.1.1).

**1.2.1** – O pacote PyPI é realmente carregado (Metadata-Version 2.3). Mesmo produto que a versão 1.2.0.

**1.2.0** – Os binários do GitHub Release são lançados (biblioteca de eventos + folha de estilo agrupada; as asserções `--help` e ≥200 eventos funcionam). `trail ledger proof` verifica o salvamento carregado. A TUI mostra semente/doutrina/reviravoltas/moral; `c` alterna o ritmo; o menu do livro-razão `R` comprova este salvamento. O HUD é legível em 80×24 e 120×30.

**1.1.1** – `pip install "escape-the-valley[voice]"` realmente instala (o extra estava fixado em um pacote não publicado). O binário PyInstaller não inclui mais o extra de voz.

**1.1.0** – narração em streaming, finais graduados, eventos que podem ferir, prova de reconciliação na cadeia, artefatos de execução.

Os binários anexados aos lançamentos do GitHub v1.1.0 e v1.1.1 não são executados (uma importação relativa no ponto de entrada congelado). Use `pip install escape-the-valley` ou o binário do GitHub v1.2.0+ / inicializador `npx`.

## Início rápido

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

## Como jogar

A cada turno, você escolhe uma ação no acampamento:

| Ação | O que ela faz |
|--------|-------------|
| **Travel** | Mova-se em direção à saída do vale. Custa comida e água. Risco de avaria e eventos. |
| **Rest** | Cure o grupo, recupere a moral. Custa suprimentos, mas não há progresso. |
| **Hunt** | Gaste munição para ter uma chance de obter comida. Melhor em florestas e planícies. |
| **Repair** | Gaste uma peça sobressalente para consertar a carroça. Essencial para a sobrevivência. |

**Eventos** interrompem a viagem com escolhas (A/B/C). Escolhas cautelosas são mais seguras, mas custam tempo. Escolhas ousadas são mais rápidas, mas arriscadas. Não há uma resposta sempre correta.

**A carroça é tudo.** Se ela quebrar e não houver peças sobressalentes, a jornada termina. Mantenha-a acima da metade de sua capacidade e faça manutenções (descanse e repare) para obter resistência temporária contra avarias.

**Ritmo** controla a velocidade em relação à segurança. O ritmo normal é o padrão. Um ritmo acelerado cobre mais terreno, mas consome mais suprimentos e danifica as carroças mais rapidamente.

**Válvulas de escape** (ração forçada, reparo desesperado, abandono da carga) existem para emergências. Elas têm efeitos colaterais e tempos de recarga — são o último recurso, não estratégias.

Para dicas mais detalhadas, consulte o [Guia de Sobrevivência](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/).

## Perfis do Mestre do Jogo

O narrador de IA molda o tom, não a mecânica. Todos os três perfis jogam o mesmo jogo.

- **Cronista** – Objetivo, prático, conciso. Folclore mínimo. Relata o que aconteceu.
- **Contador de Histórias** – Narrador sério ao redor da fogueira. Momentos sutis e estranhos. É o padrão.
- **Portador da Lanterna** – Estranho e liminar, mas ainda fundamentado em consequências. É o mais peculiar.

Defina com `--gm-profile`: `trail tui --gm-profile lantern`

## Suprimentos

O jogo rastreia 12 tipos de recursos em duas categorias:

**Consumíveis:** comida, água, lenha, medicamentos, sal, munição, óleo de lanterna, tecido

**Equipamentos:** peças, corda, ferramentas, botas

Os 5 suprimentos principais (comida, água, medicamentos, munição e peças) são os mais críticos. Suprimentos estendidos, como lenha, sal, óleo de lanterna e tecido, adicionam profundidade: a lenha alimenta os acampamentos noturnos, o sal evita que os alimentos estraguem, o óleo de lanterna permite viagens noturnas mais seguras e o tecido remenda equipamentos e a cobertura da carroça.

## Livro-Razão (Opcional)

O livro-razão rastreia seus 5 suprimentos principais (comida, água, medicamentos, munição e peças) como tokens na XRPL Testnet. Cada ponto de verificação da cidade registra um recibo de assentamento na cadeia. No final da sua jornada, seu livro-razão incluirá IDs de transação que qualquer pessoa pode verificar.

Completamente opcional. O jogo funciona da mesma forma com ele desativado (o padrão). Ative-o a partir do menu L na TUI ou via CLI:

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

Na TUI, **L** abre o menu do livro-razão; com o livro-razão ativado, **R** comprova este salvamento (os mesmos resultados). Não descansa.

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
| `trail ledger status` | Mostre o status do livro-razão |
| `trail ledger enable` | Ative o livro-razão XRPL |
| `trail ledger disable` | Desative o livro-razão XRPL |
| `trail ledger settle` | Resolva manualmente um ponto de verificação |
| `trail ledger reconcile` | Tente novamente os assentamentos com falha |
| `trail ledger proof` | Comprove o salvamento carregado (APROVADO/REPROVADO/INCONCLUSIVO) |
| `trail ledger wallet` | Mostre os detalhes da carteira |
| `trail stats` | Mostre as estatísticas da jornada (suporta `--json`) |
| `trail parcel send <addr> <supply> <amount>` | Envie suprimentos para outro viajante |
| `trail parcel list` | Liste os pacotes recebidos |
| `trail parcel accept <id>` | Aceite um pacote pendente |
| `trail parcel sent` | Liste os pacotes que você enviou |
| `trail wallet share` | Imprima o endereço da sua carteira para negociação |

## Alertas

Por padrão, o jogo exibe avisos detalhados para ajudar os novos jogadores a identificar perigos antecipadamente. Jogadores experientes podem alternar para o modo mínimo, que mostra apenas avisos sobre áreas de risco (ameaças críticas e iminentes):

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Solução de problemas

**Se algo parecer errado, execute `trail self-check` primeiro.** Ele informa se o Ollama está acessível, se o seu jogo salvo é carregado e qual modelo está instalado. As três coisas que podem dar errado são:

| Sintoma | Causa | Solução |
|---------|-------|-----|
| **Generic / no narration** | O Ollama não está em execução (o GM é opcional e funciona como um recurso de segurança, nunca causa problemas) | Inicie o Ollama (`ollama serve`) ou jogue de forma determinística com `--gm-off`. Execute `trail self-check` para confirmar. |
| **Registro pendente / liquidação falhou** | A XRPL Testnet é uma rede de teste pública e, às vezes, apresenta instabilidade | `trail ledger reconcile` tenta novamente as liquidações com falha; execute-o novamente quando a rede se recuperar. Os recursos estão corretos localmente, de qualquer forma. |
| **Save won't resume** | `run.json` foi truncado ou corrompido durante a gravação | O motor o coloca em quarentena como `run.json.corrupt-<timestamp>` antes de rejeitá-lo, para que seu próximo jogo salvo não possa alterar as evidências. Recupere-se desse backup ou inicie um novo jogo a partir de uma semente. |

A primeira rodada narrada carrega o modelo e pode levar de 10 a 30 segundos — isso é normal, não é um travamento. Detalhes completos: [Manual de solução de problemas](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/).

## Requisitos

- Python 3.11+
- Ollama (opcional, para narração com IA)
- xrpl-py (opcional, para o sistema de registro)

## Segurança

Sem telemetria. Sem contas. A narração do GM está ativada por padrão (HTTP para o Ollama local); passe `--gm-off` para desativar. XRPL está desativado até `trail ledger enable` (apenas Testnet). O áudio é opcional (`--voice`). Consulte [SECURITY.md](SECURITY.md) para obter o modelo completo de ameaças.

## Licença

MIT

Criado por <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
