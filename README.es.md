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

Escape the Valley es un juego de supervivencia al estilo Oregon Trail que se ejecuta en tu terminal. Lidera a un grupo de colonos a través de una naturaleza salvaje generada por procedimientos. Administra la comida, el agua, el estado del carro y la moral mientras navegas por eventos, peligros y decisiones difíciles.

Un Game Master opcional con IA (impulsado por Ollama) narra tu viaje con tres voces distintas para contar historias. Una mochila opcional de Ledger XRPL Testnet rastrea los cambios en tus suministros como recibos en la cadena de bloques, lo que demuestra que sobreviviste o que al menos lo intentaste.

## Novedades en la versión 1.1.0

- **Narración en streaming:** el Game Master escribe token por token, componiendo cada momento en vivo en lugar de mostrar un bloque completo después de una pausa.
- **Finales clasificados:** las partidas terminan con un epílogo clasificado (triunfante, curtido, pírrico o perdido) que se lee a partir de quién sobrevivió, cuánto duró y qué te costó el viaje; no es solo una causa de muerte en una sola línea.
- **Riesgos reales:** los eventos ahora pueden herir o matar al grupo. Una mala decisión puede costar una vida, y la muerte se atribuye a su causa real.
- **Prueba de conciliación en el libro mayor:** un modo de auditoría que reproduce los recibos de asentamiento de una partida y los verifica con XRPL Testnet, para que el historial de suministros pueda verificarse de forma independiente.
- **Artefactos de la partida:** cada partida completada deja un recuerdo: una postal XRPL, tus estadísticas y una ruta para exportar o compartir.

## Inicio rápido

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

## Cómo jugar

En cada turno, eliges una acción desde el campamento:

| Acción | Qué hace |
|--------|-------------|
| **Travel** | Muévete hacia la salida del valle. Cuesta comida y agua. Riesgo de avería y eventos. |
| **Rest** | Cura al grupo, recupera la moral. Cuesta suministros pero no avanza. |
| **Hunt** | Gasta munición para tener una oportunidad de conseguir comida. Mejor en bosques y llanuras. |
| **Repair** | Gasta una pieza de repuesto para reparar el carro. Es fundamental para la supervivencia. |

Los **eventos** interrumpen el viaje con opciones (A/B/C). Las opciones cautelosas son más seguras, pero cuestan tiempo. Las opciones audaces son más rápidas, pero arriesgadas. No hay una respuesta siempre correcta.

**El carro es todo.** Si se avería y no tienes piezas de repuesto, la partida termina. Mantenlo en buen estado (por encima del 50 %) y realiza revisiones periódicas (descansa y luego repara) para obtener una resistencia temporal a las averías.

El **ritmo** controla la velocidad frente a la seguridad. El ritmo constante es el predeterminado. Un ritmo rápido cubre más terreno, pero consume más suministros y daña los carros más rápidamente.

Las **válvulas de escape** (racionamiento estricto, reparación desesperada, abandono de carga) existen para emergencias. Tienen efectos secundarios y períodos de recuperación; son el último recurso, no estrategias.

Para obtener consejos más detallados, consulta la [Guía de supervivencia](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/).

## Perfiles del Game Master

El narrador con IA da forma al tono, no a la mecánica. Los tres perfiles juegan el mismo juego.

- **Cronista:** Objetivo, práctico y conciso. Mínimo folclore. Informa sobre lo que sucedió.
- **Narrador junto al fuego:** Narrador serio junto al fuego. Sutiles momentos inquietantes. Es el perfil predeterminado.
- **Portador de la linterna:** Inquietante y liminal, pero aún así basado en las consecuencias. Es el más extraño.

Establece con `--gm-profile`: `trail tui --gm-profile lantern`

## Suministros

El juego rastrea 12 tipos de recursos en dos categorías:

**Consumibles:** comida, agua, leña, medicamentos, sal, munición, aceite para linternas, tela.

**Equipo:** piezas de repuesto, cuerda, herramientas, botas.

Los 5 suministros principales (comida, agua, medicamentos, munición y piezas de repuesto) son los más críticos. Los suministros adicionales como leña, sal, aceite para linternas y tela añaden profundidad: la leña alimenta los campamentos nocturnos, la sal previene el deterioro de los alimentos, el aceite para linternas permite viajar de noche con mayor seguridad y la tela remienda el equipo y la cubierta del carro.

## Mochila del libro mayor (opcional)

La mochila del libro mayor rastrea tus 5 suministros principales (comida, agua, medicamentos, munición y piezas de repuesto) como tokens en XRPL Testnet. Cada punto de control registra un recibo de asentamiento en la cadena de bloques. Al final de tu partida, tu libro mayor incluirá los ID de transacción que cualquiera puede verificar.

Completamente opcional. El juego se juega igual con él desactivado (que es el valor predeterminado). Actívalo desde el menú L en la TUI o a través de la CLI:

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
```

Requiere `pip install -e ".[xrpl]"` para la dependencia `xrpl-py`.

## Comandos

| Comando | Descripción |
|---------|-------------|
| `trail tui` | Inicia la interfaz de usuario textual en pantalla completa. |
| `trail new` | Comienza una nueva partida (modo CLI clásico). |
| `trail play` | Continúa una partida guardada (modo CLI clásico). |
| `trail status` | Muestra el grupo, el carro y los suministros. |
| `trail journal` | Muestra las entradas recientes del diario. |
| `trail self-check` | Verifica el estado del entorno del juego. |
| `trail version` | Muestra la versión. |
| `trail ledger status` | Muestra el estado de la mochila. |
| `trail ledger enable` | Activa la mochila XRPL. |
| `trail ledger disable` | Desactiva la mochila XRPL. |
| `trail ledger settle` | Realiza un asentamiento manual en un punto de control. |
| `trail ledger reconcile` | Reintenta los asentamientos fallidos. |
| `trail ledger wallet` | Muestra los detalles de la billetera. |
| `trail stats` | Muestra las estadísticas de la partida (admite `--json`). |
| `trail parcel send <addr> <supply> <amount>` | Envía suministros a otro viajero. |
| `trail parcel list` | Enumera los paquetes recibidos. |
| `trail parcel accept <id>` | Acepta un paquete pendiente. |
| `trail parcel sent` | Enumera los paquetes que has enviado. |
| `trail wallet share` | Imprime tu dirección de billetera para intercambiar. |

## Alertas

De forma predeterminada, el juego muestra alertas detalladas para ayudar a los jugadores nuevos a detectar el peligro desde el principio. Los jugadores experimentados pueden cambiar al modo mínimo, que solo muestra las alertas de último momento (amenazas críticas):

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Solución de problemas

**Si algo parece incorrecto, ejecuta `trail self-check` primero.** Informa si Ollama es accesible, si tu partida se carga y qué modelo está instalado. Las tres cosas que pueden salir mal:

| Síntoma | Causa | Solución |
|---------|-------|-----|
| **Generic / no narration** | Ollama no se está ejecutando (el GM es opcional y funciona como respaldo, nunca causa problemas irreversibles). | Inicie Ollama (`ollama serve`) o juegue de forma determinista con `--gm-off`. Ejecute `trail self-check` para confirmar. |
| **Registro pendiente / liquidación fallida** | XRPL Testnet es una red de prueba pública y, a veces, puede ser inestable. | `trail ledger reconcile` reintenta las liquidaciones fallidas; ejecútelo nuevamente cuando la red se recupere. Los datos locales son correctos en cualquier caso. |
| **Save won't resume** | `run.json` se truncó o corrompió durante la escritura. | El motor lo pone en cuarentena como `run.json.corrupt-<marca de tiempo>` antes de rechazarlo, para que su próximo guardado no pueda alterar las pruebas. Restaure desde esa copia de seguridad o inicie una nueva ejecución a partir de una semilla. |

La primera ronda narrada carga el modelo y puede tardar entre 10 y 30 segundos; esto es normal, no indica un problema. Consulte los detalles completos en: [Manual de solución de problemas](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/).

## Requisitos

- Python 3.11+
- Ollama (opcional, para la narración con IA)
- xrpl-py (opcional, para el registro de datos)

## Seguridad

No se recopilan datos de telemetría. No hay cuentas. Todas las funciones de red (Ollama, XRPL) son opcionales y están desactivadas por defecto. Las operaciones de XRPL solo utilizan Testnet. Consulte [SECURITY.md](SECURITY.md) para obtener el modelo completo de amenazas.

## Licencia

MIT

Creado por <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
