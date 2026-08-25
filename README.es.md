<p align="center">
  <a href="README.ja.md">日本語</a> | <a href="README.zh.md">中文</a> | <a href="README.md">English</a> | <a href="README.fr.md">Français</a> | <a href="README.hi.md">हिन्दी</a> | <a href="README.it.md">Italiano</a> | <a href="README.pt-BR.md">Português (BR)</a>
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

## ¿Qué es esto?

Escape the Valley es un juego de supervivencia al estilo de Oregon Trail que se ejecuta en tu terminal. Lidera a un grupo de colonos a través de una naturaleza salvaje generada por procedimientos. Administra los alimentos, el agua, el estado del carro y la moral mientras navegas por eventos, peligros y decisiones difíciles.

Un Game Master opcional con IA (impulsado por Ollama) narra tu viaje con tres voces distintas para contar historias. Una mochila opcional de Ledger XRPL Testnet rastrea los cambios en tus suministros como recibos en la cadena —prueba de que sobreviviste, o prueba de que lo intentaste.

## Novedades

**1.1.1** — `pip install "escape-the-valley[voice]"` se instala correctamente (la versión anterior tenía un paquete no publicado). El archivo binario de PyInstaller ya no incluye la voz adicional.

**1.1.0** — narración en streaming, finales graduados, eventos que pueden causar heridas, prueba de conciliación en el libro mayor, artefactos de ejecución.

Los archivos binarios adjuntos a las versiones 1.1.0 y 1.1.1 de GitHub Releases no se inician (una importación relativa en el punto de entrada congelado). `pip install escape-the-valley` es la instalación que funciona hasta la próxima versión, que incluye la biblioteca de eventos y la hoja de estilo, y prueba tanto `--help` como una cantidad cargada de eventos.

## Inicio rápido

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

## Cómo jugar

En cada turno, eliges una acción desde el campamento:

| Acción | Qué hace |
|--------|-------------|
| **Travel** | Muévete hacia la salida del valle. Cuesta alimentos y agua. Riesgo de avería y eventos. |
| **Rest** | Cura al grupo, recupera la moral. Cuesta suministros pero no hay progreso. |
| **Hunt** | Gasta munición para tener una oportunidad de conseguir comida. Mejor en bosques y llanuras. |
| **Repair** | Gasta una pieza de repuesto para reparar el carro. Es fundamental para la supervivencia. |

**Eventos** interrumpen el viaje con opciones (A/B/C). Las opciones cautelosas son más seguras, pero cuestan tiempo. Las opciones audaces son más rápidas, pero arriesgadas. No hay una respuesta siempre correcta.

**El carro es todo.** Si se avería y no quedan piezas de repuesto, el juego termina. Mantenlo en buen estado (por encima de la mitad) y realiza revisiones periódicas (descanso y reparación) para obtener resistencia temporal a las averías.

**Ritmo** controla la velocidad frente a la seguridad. El ritmo constante es el predeterminado. Un ritmo rápido cubre más terreno, pero consume más suministros y daña los carros más rápidamente.

**Válvulas de escape** (raciones escasas, reparaciones desesperadas, abandono de la carga) existen para emergencias. Tienen efectos secundarios y tiempos de espera: son el último recurso, no estrategias.

Para obtener consejos más detallados, consulta la [Guía de supervivencia](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/).

## Perfiles del Game Master

El narrador de IA da forma al tono, no a la mecánica. Los tres perfiles juegan el mismo juego.

- **Cronista** — Objetivo, práctico, sobrio. Mínimo folclore. Informa sobre lo que sucedió.
- **Narrador junto al fuego** — Narrador serio junto al fuego. Momentos sutilmente inquietantes. Es el predeterminado.
- **Portador de la linterna** — Inquietante y liminal, pero aún así basado en las consecuencias. Es el más extraño.

Se establece con `--gm-profile`: `trail tui --gm-profile lantern`

## Suministros

El juego rastrea 12 tipos de recursos en dos categorías:

**Consumibles:** alimentos, agua, leña, medicamentos, sal, munición, aceite para linternas, tela

**Equipo:** piezas de repuesto, cuerda, herramientas, botas

Los 5 suministros principales (alimentos, agua, medicamentos, munición y piezas de repuesto) son los más críticos. Los suministros adicionales como leña, sal, aceite para linternas y tela añaden profundidad: la leña alimenta los campamentos nocturnos, la sal previene el deterioro de los alimentos, el aceite para linternas permite viajar de noche con mayor seguridad y la tela remienda el equipo y la cubierta del carro.

## Mochila del libro mayor (opcional)

La mochila del libro mayor rastrea tus 5 suministros principales (alimentos, agua, medicamentos, munición y piezas de repuesto) como tokens en la XRPL Testnet. Cada punto de control de la ciudad registra un recibo de asentamiento en la cadena. Al final de tu juego, tu registro de viaje incluye los ID de transacción que cualquiera puede verificar.

Completamente opcional. El juego se juega de la misma manera con él desactivado (que es el valor predeterminado). Actívalo desde el menú L en la TUI o a través de la CLI:

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

En la TUI, **L** abre el menú del libro mayor; con la mochila activada, **R** verifica esta partida guardada (los mismos resultados). No descansa.

Requiere `pip install -e ".[xrpl]"` para la dependencia `xrpl-py`.

## Comandos

| Comando | Descripción |
|---------|-------------|
| `trail tui` | Inicia la interfaz de usuario textual de pantalla completa |
| `trail new` | Comienza una nueva partida (modo CLI clásico) |
| `trail play` | Continúa una partida guardada (modo CLI clásico) |
| `trail status` | Muestra el grupo, el carro y los suministros |
| `trail journal` | Muestra las entradas recientes del diario |
| `trail self-check` | Verifica la salud del entorno del juego |
| `trail version` | Muestra la versión |
| `trail ledger status` | Muestra el estado de la mochila |
| `trail ledger enable` | Activa la mochila XRPL |
| `trail ledger disable` | Desactiva la mochila XRPL |
| `trail ledger settle` | Realiza un asentamiento manual en un punto de control |
| `trail ledger reconcile` | Reintenta los asentamientos fallidos |
| `trail ledger proof` | Verifica la partida guardada cargada (PASADO/FALLIDO/INCONCLUSIVO) |
| `trail ledger wallet` | Muestra los detalles de la billetera |
| `trail stats` | Muestra las estadísticas de la partida (admite `--json`) |
| `trail parcel send <addr> <supply> <amount>` | Envía suministros a otro viajero |
| `trail parcel list` | Enumera los paquetes recibidos |
| `trail parcel accept <id>` | Acepta un paquete pendiente |
| `trail parcel sent` | Enumera los paquetes que has enviado |
| `trail wallet share` | Imprime tu dirección de billetera para intercambiar |

## Alertas

De forma predeterminada, el juego muestra alertas detalladas para ayudar a los nuevos jugadores a detectar el peligro desde el principio. Los jugadores experimentados pueden cambiar al modo mínimo, que solo muestra las alertas de último momento (amenazas críticas):

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Solución de problemas

**Si algo parece incorrecto, ejecuta `trail self-check` primero.** Informa si Ollama está en funcionamiento, si tu partida guardada se carga y qué modelo está instalado. Las tres cosas que pueden salir mal:

| Síntoma | Causa | Solución |
|---------|-------|-----|
| **Generic / no narration** | Ollama no se está ejecutando (el Game Master es opcional y retrocede, nunca daña el juego) | Inicie Ollama (`ollama serve`) o juegue de forma determinista con `--gm-off`. Ejecute `trail self-check` para confirmar. |
| **Registro pendiente / liquidación fallida** | XRPL Testnet es una red de prueba pública y, a veces, puede ser inestable. | `trail ledger reconcile` reintenta las liquidaciones fallidas; ejecútelo nuevamente cuando la red se recupere. Los datos son correctos localmente en cualquier caso. |
| **Save won't resume** | `run.json` fue truncado o dañado a mitad de la escritura. | El motor lo pone en cuarentena como `run.json.corrupt-<timestamp>` antes de rechazarlo, por lo que su próximo guardado no puede alterar las pruebas. Recupérese de esa copia de seguridad o comience una nueva ejecución desde una semilla. |

La primera ronda narrada carga el modelo y puede tardar entre 10 y 30 segundos; esto es normal, no indica un problema. Detalles completos: [Manual de solución de problemas](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/).

## Requisitos

- Python 3.11+
- Ollama (opcional, para la narración con IA)
- xrpl-py (opcional, para el registro de datos)

## Seguridad

No hay telemetría. No hay cuentas. La narración GM está activada por defecto (HTTP a Ollama local); pase `--gm-off` para desactivarla. XRPL está desactivado hasta `trail ledger enable` (solo Testnet). El audio es opcional (`--voice`). Consulte [SECURITY.md](SECURITY.md) para conocer el modelo de amenazas completo.

## Licencia

MIT

Creado por <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
