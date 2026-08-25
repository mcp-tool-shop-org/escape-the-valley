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

«Escape the Valley» é um jogo de sobrevivência no estilo «Oregon Trail», que pode ser jogado diretamente no seu terminal. Lidere um grupo de colonos por uma região selvagem gerada aleatoriamente. Gerencie os recursos alimentares e hídricos, o estado da carroça e o moral do grupo enquanto enfrenta eventos, perigos e decisões difíceis.

Um Mestre de Jogo com inteligência artificial opcional (com tecnologia da Ollama) narra a sua aventura com três vozes distintas para contar histórias. Um sistema opcional que acompanha as suas ações na rede de testes XRPL regista as alterações nos seus recursos, como comprovativos das transações efetuadas na cadeia de blocos — prova de que sobreviveu ou de que tentou fazê-lo.

## O que há de novo? / Quais são as novidades?

**1.1.1** – `pip install "escape-the-valley[voice]"` instala, na verdade, o pacote adicional (que estava fixado numa versão não publicada). O ficheiro binário do PyInstaller já não inclui o pacote de voz adicional.

**1.1.0** – narração em fluxo contínuo, finais com diferentes níveis de dificuldade, eventos que podem causar dano, prova de reconciliação no livro-razão, artefatos de execução.

Os arquivos binários anexados às versões v1.1.0 e v1.1.1 do GitHub não são executados (devido a uma importação relativa no ponto de entrada congelado). `pip install escape-the-valley` é a versão funcional até a próxima atualização, que inclui a biblioteca de eventos e a folha de estilo e testa tanto `--help` quanto a contagem de eventos carregada.

## Guia de Início Rápido

```bash
pip install escape-the-valley

# Zero-prerequisite npm launcher (working from the next release; v1.1.0 and
# v1.1.1 GitHub binaries do not start — use pip until then):
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

Em cada turno, você escolhe uma ação a ser realizada no acampamento:

| Ação | Para que serve? / Qual a sua função? |
|--------|-------------|
| **Travel** | Dirija-se para a saída do vale. Consumo de alimentos e água. Risco de avaria e ocorrência de imprevistos. |
| **Rest** | Cure o grupo, restaure a moral. Custa recursos, mas não há progresso. |
| **Hunt** | Gaste munição para ter uma oportunidade de encontrar comida. É mais eficaz em florestas e planícies. |
| **Repair** | Use uma peça sobressalente para consertar a carroça. É essencial para a sobrevivência. |

**Eventos** inesperados interrompem a viagem, apresentando opções de escolha (A/B/C). As escolhas mais cautelosas são mais seguras, mas consomem mais tempo. As escolhas mais ousadas são mais rápidas, mas envolvem mais riscos. Não existe uma resposta que seja sempre a correta.

**O vagão é fundamental.** Se avariar e não houver peças de reposição disponíveis, a viagem termina. Mantenha-o em boas condições – pelo menos acima da metade – e programe períodos de manutenção (pausas para descanso seguidas de reparos) para aumentar a sua resistência a avarias temporárias.

**Ritmo** controla o equilíbrio entre velocidade e segurança. O ritmo normal é a configuração padrão. Um ritmo acelerado permite percorrer uma distância maior, mas consome mais recursos e danifica os vagões mais rapidamente.

Existem **válvulas de escape** (racionamento rigoroso, reparos desesperados, abandono da carga) para situações de emergência. Elas têm efeitos colaterais e períodos de recuperação – são soluções extremas, não estratégias.

Para obter dicas mais detalhadas, consulte o [Guia de Sobrevivência](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/).

## Perfis de Gerentes Gerais

A inteligência artificial define o tom da narrativa, não os aspetos técnicos. Os três perfis jogam o mesmo jogo.

- **Cronista** – Objetivo, prático, direto. Pouca ou nenhuma referência ao folclore. Relata o que aconteceu.
- **Contador de Histórias à Luz do Fogo** – Narrador sério em volta da fogueira. Momentos sutis e estranhos. A opção padrão.
- **Portador da Lanterna** – Estranho e misterioso, mas ainda assim com consequências reais. O mais peculiar.

Definir com `--gm-profile`: `trail tui --gm-profile lantern`

## Materiais/Fornecimentos

O jogo acompanha 12 tipos de recursos, divididos em duas categorias:

**Artigos de consumo:** alimentos, água, lenha, medicamentos, sal, munição, óleo para lamparinas, tecido.

**Equipamento:** peças, corda, ferramentas, botas

Os cinco itens essenciais (alimentos, água, medicamentos, munição e peças) são os mais importantes. Itens adicionais, como lenha, sal, óleo para lamparinas e tecido, complementam o conjunto: a lenha serve para acender fogueiras durante as paradas noturnas, o sal evita que os alimentos estraguem, o óleo para lamparinas permite viagens noturnas mais seguras e o tecido é usado para remendar equipamentos e cobrir a carroça.

## Mochila Ledger (opcional)

A mochila Ledger regista os seus cinco itens essenciais (alimentos, água, medicamentos, munições e peças) como tokens na rede de testes XRPL. Cada ponto de controlo da cidade regista um comprovativo de abastecimento na cadeia de blocos. No final da sua jornada, o registo do seu percurso inclui identificadores de transação que qualquer pessoa pode verificar.

Totalmente opcional. O jogo funciona exatamente da mesma forma com esta opção desativada (configuração padrão). Ative-a através do menu L na interface TUI ou pela linha de comandos:

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

Na interface do utilizador textual (TUI), a tecla **L** abre o menu de registo; com a mochila equipada, a tecla **R** confirma esta ação de guardar (mantém os mesmos resultados). Não é uma função de descanso.

Requer `pip install -e ".[xrpl]"` para a dependência `xrpl-py`.

## Comandos

| Comando / Ordenar / Controlar | Descrição |
|---------|-------------|
| `trail tui` | Ative a interface textual em tela cheia. |
| `trail new` | Inicie uma nova execução (modo clássico de linha de comandos). |
| `trail play` | Continue uma execução salva (modo clássico de linha de comando). |
| `trail status` | Mostre o grupo, a carruagem e os suprimentos. |
| `trail journal` | Mostrar as entradas mais recentes do diário. |
| `trail self-check` | Verifique o estado do ambiente de jogo. |
| `trail version` | Mostrar versão |
| `trail ledger status` | Mostrar o estado da mochila. |
| `trail ledger enable` | Ative a funcionalidade «XRPL backpack». |
| `trail ledger disable` | Desative a funcionalidade XRPL Backpack. |
| `trail ledger settle` | Definir manualmente um ponto de verificação. |
| `trail ledger reconcile` | Tentar novamente os pagamentos que falharam. |
| `trail ledger proof` | Verifique se o arquivo de salvamento foi carregado corretamente (APROVADO/REPROVADO/RESULTADO INCONCLUÍVE). |
| `trail ledger wallet` | Mostrar detalhes da carteira. |
| `trail stats` | Mostrar estatísticas de execução (suporta `--json`). |
| `trail parcel send <addr> <supply> <amount>` | Envie suprimentos para outro viajante. |
| `trail parcel list` | Lista de encomendas recebidas. |
| `trail parcel accept <id>` | Aceitar uma encomenda pendente. |
| `trail parcel sent` | Liste os pacotes que enviou. |
| `trail wallet share` | Imprima o endereço da sua carteira para realizar transações. |

## Alertas de segurança

Por padrão, o jogo exibe avisos detalhados para ajudar os jogadores iniciantes a identificar perigos com antecedência. Os jogadores experientes podem mudar para o modo mínimo, que mostra apenas avisos sobre áreas de risco (ameaças críticas e iminentes):

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Resolução de problemas

**Se algo parecer errado, execute `trail self-check` primeiro.** Ele indica se o Ollama está acessível, se os seus dados são carregados corretamente e qual modelo está instalado. Os três problemas mais comuns são:

| Sintoma | Causa | Consertar / Reparar / Arrumar |
|---------|-------|-----|
| **Generic / no narration** | O Ollama não está em execução (o GM é opcional e funciona como alternativa, nunca danificando o sistema). | Inicie o Ollama (`ollama serve`) ou utilize-o de forma determinística com `--gm-off`. Execute `trail self-check` para confirmar. |
| **Registo pendente / liquidação falhou** | A XRPL Testnet é uma rede de testes pública e, por vezes, apresenta instabilidade. | `trail ledger reconcile` tenta novamente as liquidações que falharam; execute-o novamente quando a rede se restabelecer. Os dados locais estão corretos em ambos os casos. |
| **Save won't resume** | `run.json` foi truncado ou corrompido durante a escrita. | O motor coloca-o em quarentena como `run.json.corrupt-<timestamp>` antes de o rejeitar, para que a sua próxima gravação não possa substituir as provas. Recupere a partir dessa cópia de segurança ou inicie uma nova execução a partir de um ficheiro inicial. |

A primeira jogada narrada carrega o modelo e pode demorar entre 10 e 30 segundos — isso é normal, não indica que o programa travou. Detalhes completos: [Manual de resolução de problemas](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/).

## Requisitos

- Python 3.11+
- Ollama (opcional, para narração com IA)
- xrpl-py (opcional, para o sistema de registo)

## Segurança

Sem telemetria. Sem contas. A narração GM está ativada por predefinição (HTTP para o Ollama local); passe `--gm-off` para desativar. A XRPL está desligada até `trail ledger enable` (apenas Testnet). O áudio é opcional (`--voice`). Consulte [SECURITY.md](SECURITY.md) para obter o modelo de ameaças completo.

## Licença

MIT

Criado por <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
