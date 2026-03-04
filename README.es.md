<p align="center">
  <a href="README.ja.md">日本語</a> | <a href="README.zh.md">中文</a> | <a href="README.md">English</a> | <a href="README.fr.md">Français</a> | <a href="README.hi.md">हिन्दी</a> | <a href="README.it.md">Italiano</a> | <a href="README.pt-BR.md">Português (BR)</a>
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

## ¿Qué es esto?

Escape the Valley es un juego de supervivencia al estilo de Oregon Trail que se ejecuta en tu terminal. Dirige un grupo de colonos a través de un terreno generado aleatoriamente. Administra alimentos, agua, el estado del carro y la moral, mientras navegas por eventos, peligros y decisiones difíciles.

Un narrador de inteligencia artificial (IA) opcional (impulsado por Ollama) narra tu viaje con tres voces narrativas distintas. Una "mochila" opcional de registro de la red de pruebas XRPL rastrea tus cambios de suministros como recibos en la cadena de bloques, una prueba de que sobreviviste, o una prueba de que lo intentaste.

## Guía de inicio rápido

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

## Cómo jugar

En cada turno, eliges una acción desde el campamento:

| Acción | Lo que hace |
|--------|-------------|
| **Travel** | Avanza hacia la salida del valle. Consume alimentos y agua. Riesgo de avería y eventos. |
| **Rest** | Restaura la moral del grupo. Consume suministros, pero no avanza. |
| **Hunt** | Usa munición para tener una oportunidad de obtener alimentos. Es más efectivo en bosques y llanuras. |
| **Repair** | Usa una pieza de repuesto para reparar el carro. Es crucial para la supervivencia. |

**Eventos:** Interrumpen el viaje con opciones (A/B/C). Las opciones cautelosas son más seguras, pero consumen tiempo. Las opciones audaces son más rápidas, pero arriesgadas. No siempre hay una respuesta correcta.

**El carro es lo más importante.** Si se avería y no tienes piezas de repuesto, el juego termina. Mantén su estado por encima de la mitad y realiza mantenimientos (descansa y luego repara) para una resistencia temporal a las averías.

**Ritmo:** Controla la velocidad frente a la seguridad. El ritmo constante es el predeterminado. Un ritmo rápido cubre más terreno, pero consume más suministros y avería los carros más rápido.

**Válvulas de escape** (racionamiento extremo, reparación desesperada, abandonar carga) existen para emergencias. Tienen efectos secundarios y tiempos de recuperación; son un último recurso, no una estrategia.

Para obtener más consejos, consulta la [Guía de Supervivencia](docs/survival-guide.md).

## Perfiles del Narrador (IA)

El narrador de IA define el tono, no la mecánica del juego. Los tres perfiles juegan el mismo juego.

- **Chronicler (Cronista):** Práctico, directo y conciso. Mínimo folclore. Informa sobre lo que sucedió.
- **Fireside (Junto al Fuego):** Narrador serio, como si estuvieras junto a una fogata. Tiene momentos sutiles e inquietantes. Es el predeterminado.
- **Lantern-Bearer (Portador de la Linterna):** Inquietante y enigmático, pero aún arraigado en las consecuencias. Es el más extraño.

Configura con `--gm-profile`: `trail tui --gm-profile lantern`

## Mochila de Registro (Opcional)

La mochila de registro rastrea tus 5 suministros principales (alimentos, agua, medicamentos, munición, piezas) como tokens en la red de pruebas XRPL. Cada punto de control de la ciudad registra un recibo de asentamiento en la cadena de bloques. Al final de tu partida, tu registro incluye los ID de transacción que cualquier persona puede verificar.

Completamente opcional. El juego se juega de la misma manera con ella desactivada (que es la opción predeterminada). Actívala desde el menú L en la interfaz de usuario de texto o a través de la línea de comandos:

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
```

Requiere `pip install -e ".[xrpl]"` para la dependencia `xrpl-py`.

## Comandos

| Comando | Descripción |
|---------|-------------|
| `trail tui` | Inicia la interfaz de usuario de texto en pantalla completa |
| `trail new` | Comienza una nueva partida (modo clásico de línea de comandos) |
| `trail play` | Continúa una partida guardada (modo clásico de línea de comandos) |
| `trail status` | Muestra el estado del grupo, el carro y los suministros |
| `trail journal` | Muestra las entradas recientes del diario |
| `trail self-check` | Verifica el estado del entorno del juego |
| `trail version` | Muestra la versión |
| `trail ledger status` | Muestra el estado de la mochila |
| `trail ledger enable` | Activa la mochila XRPL |
| `trail ledger disable` | Desactiva la mochila XRPL |
| `trail ledger settle` | Realiza manualmente un asentamiento en un punto de control |
| `trail ledger reconcile` | Reintenta asentamientos fallidos |
| `trail ledger wallet` | Muestra los detalles de la billetera |
| `trail parcel list` | Lista los paquetes recibidos |
| `trail parcel accept <id>` | Acepta un paquete pendiente |

## Advertencias

Por defecto, el juego muestra advertencias detalladas para ayudar a los nuevos jugadores a identificar el peligro desde el principio. Los jugadores experimentados pueden cambiar al modo mínimo, que solo muestra las advertencias de "borde del precipicio" (amenazas críticas de último momento):

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Requisitos

- Python 3.11 o superior
- Ollama (opcional, para la narración con inteligencia artificial)
- xrpl-py (opcional, para la función de "ledger backpack")

## Seguridad

No se recopilan datos de telemetría. No se utilizan cuentas. Todas las funciones de red (Ollama, XRPL) son opcionales y están desactivadas por defecto. Las operaciones de XRPL solo utilizan la red de pruebas (Testnet). Consulte el archivo [SECURITY.md](SECURITY.md) para obtener información completa sobre el modelo de amenazas.

## Licencia

MIT

Desarrollado por <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
