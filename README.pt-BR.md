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

Escape the Valley é um jogo de sobrevivência no estilo Oregon Trail que roda no seu terminal. Lidere um grupo de colonos por uma região selvagem gerada proceduralmente. Gerencie comida, água, condição da carroça e moral enquanto enfrenta eventos, perigos e decisões difíceis.

Um Mestre de Jogo de IA opcional (alimentado por Ollama) narra sua jornada com três vozes distintas de narração. Um livro de registro XRPL Testnet opcional rastreia suas mudanças de suprimentos como recibos na blockchain — prova de que você sobreviveu ou prova de que tentou.

## O que há de novo

**1.3.2** — Tempo de espera por `event_id` (`N=4`) impede que o mesmo evento se repita em turnos consecutivos ou quase consecutivos (#21; 1.3.1 → 1.3.2 move a seleção de eventos após uma repetição, então uma partida salva em 1.3.1 continua de forma diferente após a atualização para 1.3.2 porque a seleção de eventos muda. Dentro de uma mesma versão, a mesma semente e as mesmas escolhas resultam na mesma partida). O painel de resultados da CLI com o GM desativado renderiza o resumo padrão uma vez (#20), as dicas de escolha removem a pontuação dupla no final e `1 mile remaining.` usa gramática singular. Arquivos `.trail/secrets.json` corrompidos são colocados em quarentena antes de serem reescritos (#31.7). A saída de `THIRD_PARTY_NOTICES*.txt` e `trail licenses` é normalizada para LF em todas as plataformas, e o CI verifica a integridade da licença fornecida em relação ao arquivo python-build-standalone baixado (#31.1, #31.2, #31.3, #31.5).

**1.3.1** — As execuções da CLI (`trail new` / `trail continue`) agora emitem cartões de memória e a causa canônica da morte, como na TUI (`trail stats`, dias de morte no registro da trilha e resumos de memória do GM funcionam em ambos os modos). A TUI Abandon Cargo relata a carga real descartada. Os prompts da rota alternativa não consomem mais um turno de manutenção da carroça ou tempo de espera da válvula de escape. As partidas repetidas com a mesma semente obtêm `run_id`s distintos para XRPL memorandos. Os binários GitHub Release / `npx` agora incluem `xrpl-py`. A saída do Windows em pipeline (`cp1252`) não causa mais falhas em caracteres Rich de desenho de caixa (#19).

**1.3.0** — Quatro finais que você realmente pode alcançar, incluindo o final de sobrevivência. A carroça é uma ameaça, não o jogo inteiro. O início do inverno retarda um ano marcado. As travessias do deserto evitam a areia. A caça indica quem se machucou. `npx @mcptoolshop/escape-the-valley` inicia o binário v1.3.0 (estava preso no artefato morto v1.1.1 GitHub).

**1.2.1** — PyPI realmente faz o upload (Metadata-Version 2.3). O mesmo produto da versão 1.2.0.

**1.2.0** — Os binários GitHub Release são lançados (biblioteca de eventos + folha de estilo incluída; as asserções de fumaça verificam `--help` e ≥200 eventos). `trail ledger proof` audita o salvamento carregado. A TUI mostra semente/doutrina/reviravoltas/moral; `c` alterna o ritmo; o menu do registro `R` comprova este salvamento. O HUD é legível em 80×24 e 120×30.

**1.1.1** — `pip install "escape-the-valley[voice]"` realmente instala (o extra estava fixado em um pacote não publicado). O binário PyInstaller não inclui mais o extra de voz.

**1.1.0** — narração em streaming, finais graduados, eventos que podem ferir, prova de reconciliação no registro, artefatos da partida.

Os binários anexados aos v1.1.0 e v1.1.1 GitHub Releases não são iniciados (uma importação relativa no ponto de entrada congelado). Use `pip install escape-the-valley` ou o binário v1.2.0+ GitHub / lançador `npx`.

## Início rápido

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

## Como jogar

A cada turno, você escolhe uma ação no acampamento:

| Ação | O que ela faz |
|--------|-------------|
| **Travel** | Mova-se em direção à saída do vale. Custa comida e água. Risco de avaria e eventos. |
| **Rest** | Cure o grupo, recupere a moral. Custa suprimentos, mas não há progresso. |
| **Hunt** | Gaste munição para ter uma chance de obter comida. Melhor em florestas e planícies. |
| **Repair** | Gaste uma peça sobressalente para consertar a carroça. Essencial para a sobrevivência. |

**Eventos** interrompem a viagem com escolhas (A/B/C). Escolhas cautelosas são mais seguras, mas custam tempo. Escolhas ousadas são mais rápidas, mas arriscadas. Não há uma resposta sempre correta.

**A carroça é tudo.** Se ela quebrar e não houver peças, a partida termina. Mantenha-a acima de metade da condição e faça janelas de manutenção (descanse e, em seguida, repare) para obter resistência temporária à avaria.

**Ritmo** controla a velocidade em relação à segurança. O ritmo constante é o padrão. O ritmo acelerado cobre mais terreno, mas consome mais suprimentos e quebra as carroças mais rapidamente.

**Válvulas de escape** (ração forçada, reparo desesperado, abandono de carga) existem para emergências. Elas têm efeitos colaterais e tempos de espera — são o último recurso, não estratégias.

Para dicas mais detalhadas, consulte o [Guia de sobrevivência](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/).

## Perfis do GM

O narrador de IA molda o tom, não a mecânica. Todos os três perfis jogam o mesmo jogo.

- **Chronicler** — Prático, realista, simples. Mínimo de folclore. Relata o que aconteceu.
- **Fireside** — Narrador de fogueira sério. Momentos sutis e estranhos. O padrão.
- **Lantern-Bearer** — Estranho e liminar, mas ainda fundamentado em consequências. O mais peculiar.

Defina com `--gm-profile`: `trail tui --gm-profile lantern`

## Suprimentos

O jogo rastreia 12 tipos de recursos em duas categorias:

**Consumíveis:** comida, água, lenha, medicamentos, sal, munição, óleo de lanterna, tecido

**Equipamento:** peças, corda, ferramentas, botas

Os 5 suprimentos principais (comida, água, medicamentos, munição, peças) são os mais críticos. Suprimentos estendidos, como lenha, sal, óleo de lanterna e tecido, adicionam profundidade: a lenha alimenta os acampamentos noturnos, o sal evita a deterioração dos alimentos, o óleo de lanterna permite viagens noturnas mais seguras e o tecido remenda equipamentos e a cobertura da carroça.

## Ledger Backpack (Opcional)

O Ledger Backpack rastreia seus 5 suprimentos principais (comida, água, medicamentos, munição, peças) como tokens no XRPL Testnet. Cada ponto de verificação da cidade registra um recibo de assentamento na blockchain. No final da sua partida, seu registro trail inclui IDs de transação que qualquer pessoa pode verificar.

Completamente opcional. O jogo é jogado da mesma forma com ele desativado (o padrão). Ative-o a partir do menu L na TUI ou via CLI:

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

Na TUI, **L** abre o menu do registro; com a mochila ativada, **R** comprova este salvamento (os mesmos resultados). Não descansa.

Incluído no binário da versão `npx` / GitHub; para instalações `pip`, execute `pip install "escape-the-valley[xrpl]"` (ou `pip install -e ".[xrpl]"`) para a dependência `xrpl-py`.

## Comandos

| Comando | Descrição |
|---------|-------------|
| `trail tui` | Iniciar a interface Textual em tela cheia |
| `trail new` | Iniciar uma nova sessão (modo CLI clássico) |
| `trail play` | Continuar uma sessão salva (modo CLI clássico) |
| `trail status` | Mostrar o grupo, a carroça e os suprimentos |
| `trail journal` | Mostrar as entradas recentes do diário |
| `trail self-check` | Verificar o estado do ambiente do jogo |
| `trail version` | Mostrar a versão |
| `trail licenses` | Mostrar a licença e os avisos de terceiros (ou `trail --licenses`) |
| `trail ledger status` | Mostrar o estado da mochila |
| `trail ledger enable` | Ativar a mochila XRPL |
| `trail ledger disable` | Desativar a mochila XRPL |
| `trail ledger settle` | Confirmar manualmente um ponto de controle |
| `trail ledger reconcile` | Tentar novamente as confirmações com falha |
| `trail ledger proof` | Verificar o arquivo de salvamento carregado (PASS/FAIL/INCONCLUSIVE) |
| `trail ledger wallet` | Mostrar os detalhes da carteira |
| `trail stats` | Mostrar as estatísticas da sessão e os cartões de memória (suporta `--json`) |
| `trail parcel send <addr> <supply> <amount>` | Enviar suprimentos para outro viajante |
| `trail parcel list` | Listar os pacotes recebidos |
| `trail parcel accept <id>` | Aceitar um pacote pendente |
| `trail parcel sent` | Listar os pacotes que você enviou |
| `trail wallet share` | Imprimir o endereço da sua carteira para negociação |

## Alertas

Por padrão, o jogo exibe alertas detalhados para ajudar os novos jogadores a identificar os perigos mais cedo. Os jogadores experientes podem alternar para o modo mínimo, que exibe apenas alertas de perigo iminente (ameaças críticas de última hora):

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Solução de problemas

**Se algo parecer errado, execute `trail self-check` primeiro.** Ele informa se Ollama está acessível, se o seu arquivo de salvamento é carregado e qual modelo está instalado. As três coisas que podem dar errado são:

| Sintoma | Causa | Solução |
|---------|-------|-----|
| **Generic / no narration** | Ollama não está em execução (o GM é opcional e, se não estiver, não causa problemas) | Inicie Ollama (`ollama serve`) ou jogue de forma determinística com `--gm-off`. Execute `trail self-check` para confirmar. |
| **Registro pendente / confirmação com falha** | XRPL Testnet é uma rede de teste pública e, às vezes, é instável | `trail ledger reconcile` tenta novamente as confirmações com falha; execute-o novamente quando a rede se recuperar. Os suprimentos estarão corretos localmente, de qualquer forma. |
| **Save won't resume** | `run.json` foi truncado ou corrompido durante a gravação | O mecanismo o coloca em quarentena como `run.json.corrupt-<timestamp>` antes de recusá-lo, para que o seu próximo arquivo de salvamento não possa corromper as evidências. Recupere-se desse backup ou inicie uma nova sessão a partir de uma semente. |

A primeira sessão narrada carrega o modelo e pode levar de 10 a 30 segundos — isso é normal, não é uma falha. Detalhes completos: [Manual de solução de problemas](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/).

## Requisitos

- Python 3.11+
- Ollama (opcional, para narração de IA)
- xrpl-py (opcional, para a mochila de registro em instalações `pip`; incluído no binário da versão `npx` / GitHub)

## Segurança

Sem telemetria. Sem contas. A narração do GM está ativada por padrão (HTTP para Ollama local); passe `--gm-off` para desativar. XRPL está desativado até `trail ledger enable` (somente Testnet). A voz é opcional (`--voice`). Consulte [SECURITY.md](SECURITY.md) para obter o modelo de ameaças completo.

## Licença

MIT — execute `trail licenses` (ou `trail --licenses`) para visualizar a licença do jogo e os avisos de dependências de terceiros (também anexados a cada GitHub Release, um arquivo por plataforma: `THIRD_PARTY_NOTICES-<os>-<arch>.txt`).

Criado por <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
