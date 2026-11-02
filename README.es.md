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

Escape the Valley es un juego de supervivencia al estilo de Oregon Trail que se ejecuta en tu terminal. Dirige a un grupo de colonos a través de un territorio salvaje generado de forma procedural. Administra los alimentos, el agua, el estado de la carreta y la moral mientras afrontas eventos, peligros y decisiones difíciles.

Un Maestro de Juego de IA opcional (impulsado por Ollama) narra tu viaje con tres voces narrativas distintas. Un libro de registro XRPL Testnet opcional realiza un seguimiento de los cambios en tus suministros como recibos en la cadena de bloques: prueba de que sobreviviste o prueba de que lo intentaste.

## Novedades

**1.3.2** — Enfriamiento por `event_id` (`N=4`) evita que el mismo evento se repita en turnos consecutivos o casi consecutivos (#21; 1.3.1 → 1.3.2 mueve la selección de eventos después de que se hubiera producido una repetición, por lo que una partida guardada en 1.3.1 continúa de manera diferente después de actualizar a 1.3.2 porque la selección de eventos se mueve. Dentro de una misma versión, la misma semilla y las mismas elecciones dan como resultado la misma partida). El panel de resultados de la CLI sin GM muestra el resumen predeterminado una vez (#20), las sugerencias de elección eliminan la puntuación doble final y `1 mile remaining.` utiliza una gramática singular. Los archivos `.trail/secrets.json` corruptos se ponen en cuarentena antes de reescribirse (#31.7). La salida de `THIRD_PARTY_NOTICES*.txt` y `trail licenses` se normaliza a LF en todas las plataformas, y CI verifica la integridad de la licencia incluida en comparación con el archivo python-build-standalone descargado (#31.1, #31.2, #31.3, #31.5).

**1.3.1** — Las ejecuciones de la CLI (`trail new` / `trail continue`) ahora emiten tarjetas de memoria y la causa canónica de la muerte, como la TUI (`trail stats`, los días de muerte del registro del viaje y los resúmenes de memoria del GM funcionan en ambos modos). TUI Abandon Cargo informa sobre la carga real que se ha abandonado. Las indicaciones de la ruta alternativa ya no consumen un turno de mantenimiento de la carreta o de enfriamiento de la válvula de escape. Las partidas repetidas con la misma semilla obtienen `run_id` distintos para XRPL memorandos. Los binarios GitHub Release / `npx` ahora incluyen `xrpl-py`. La salida canalizada de Windows (`cp1252`) ya no se bloquea con los caracteres Rich de dibujo de cuadros (#19).

**1.3.0** — Cuatro finales a los que realmente puedes llegar, incluido el de "superviviente curtido". La carreta es una amenaza, no todo el juego. El inicio del invierno ralentiza un año marcado. Los vados del desierto se mantienen alejados de la arena. La caza indica quién resultó herido. `npx @mcptoolshop/escape-the-valley` lanza el binario v1.3.0 (había estado atascado en el artefacto muerto v1.1.1 GitHub).

**1.2.1** — La rueda PyPI realmente se carga (versión de metadatos 2.3). El mismo producto que 1.2.0.

**1.2.0** — Los binarios GitHub Release se lanzan (biblioteca de eventos + hoja de estilo incluidos; las pruebas de humo verifican `--help` y ≥200 eventos). `trail ledger proof` audita la partida guardada cargada. La TUI muestra la semilla/doctrina/giros/moral; `c` cambia el ritmo; el menú del registro `R` prueba esta partida guardada. La interfaz de usuario es legible en 80×24 y 120×30.

**1.1.1** — `pip install "escape-the-valley[voice]"` realmente se instala (el paquete adicional había fijado un paquete no publicado). El binario PyInstaller ya no incluye el paquete de voz adicional.

**1.1.0** — narración en streaming, finales graduados, eventos que pueden causar heridas, prueba de conciliación en el registro, artefactos de la partida.

Los binarios adjuntos a los v1.1.0 y v1.1.1 GitHub Release no se lanzan (una importación relativa en el punto de entrada congelado). Utiliza `pip install escape-the-valley` o el binario v1.2.0+ GitHub / el lanzador `npx`.

## Inicio rápido

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

## Cómo jugar

En cada turno, eliges una acción desde el campamento:

| Acción | Qué hace |
|--------|-------------|
| **Travel** | Muévete hacia la salida del valle. Cuesta alimentos y agua. Riesgo de avería y eventos. |
| **Rest** | Cura al grupo, recupera la moral. Cuesta suministros, pero no hay progreso. |
| **Hunt** | Gasta munición para tener la oportunidad de conseguir alimentos. Mejor en bosques y llanuras. |
| **Repair** | Gasta una pieza de repuesto para arreglar la carreta. Es fundamental para la supervivencia. |

Los **eventos** interrumpen el viaje con opciones (A/B/C). Las opciones cautelosas son más seguras, pero cuestan tiempo. Las opciones audaces son más rápidas, pero arriesgadas. No hay una respuesta siempre correcta.

**La carreta es todo.** Si se avería y no hay piezas de repuesto, la partida termina. Mantenla en un estado superior al 50 % y realiza revisiones (descansa y luego repara) para obtener una resistencia temporal a las averías.

El **ritmo** controla la velocidad frente a la seguridad. El ritmo constante es el predeterminado. El ritmo rápido cubre más terreno, pero consume más suministros y daña las carretas más rápidamente.

Las **válvulas de escape** (raciones escasas, reparación desesperada, abandono de la carga) existen para emergencias. Tienen efectos secundarios y tiempos de enfriamiento: son el último recurso, no estrategias.

Para obtener consejos más detallados, consulta la [Guía de supervivencia](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/survival-guide/).

## Perfiles del GM

El narrador de IA da forma al tono, no a la mecánica. Los tres perfiles juegan al mismo juego.

- **Chronicler** — Práctico, realista y sobrio. Mínimo folclore. Informa sobre lo que sucedió.
- **Fireside** — Narrador de historias alrededor de la fogata. Momentos sutilmente inquietantes. Es el predeterminado.
- **Lantern-Bearer** — Inquietante y liminal, pero aún así con consecuencias. Es el más extraño.

Establecer con `--gm-profile`: `trail tui --gm-profile lantern`

## Suministros

El juego realiza un seguimiento de 12 tipos de recursos en dos categorías:

**Consumibles:** alimentos, agua, leña, medicamentos, sal, munición, aceite para lámparas, tela

**Equipamiento:** piezas, cuerda, herramientas, botas

Los 5 suministros básicos (alimentos, agua, medicamentos, munición y piezas) son los más críticos. Los suministros adicionales, como la leña, la sal, el aceite para lámparas y la tela, añaden profundidad: la leña alimenta los campamentos nocturnos, la sal evita que los alimentos se echen a perder, el aceite para lámparas permite viajar de noche con mayor seguridad y la tela remienda el equipo y la cubierta de la carreta.

## Ledger Backpack (Opcional)

El Ledger Backpack realiza un seguimiento de tus 5 suministros básicos (alimentos, agua, medicamentos, munición y piezas) como tokens en el XRPL Testnet. En cada punto de control de la ciudad, se registra un recibo del asentamiento en la cadena de bloques. Al final de tu partida, tu registro trail incluye los ID de transacción que cualquiera puede verificar.

Completamente opcional. El juego se puede jugar de la misma manera con él desactivado (que es la configuración predeterminada). Actívalo desde el menú L en la TUI o a través de la CLI:

```bash
trail ledger enable
trail ledger status
trail ledger reconcile  # retry failed settlements
trail ledger proof      # PASS / FAIL / INCONCLUSIVE on this save
```

En la TUI, **L** abre el menú del registro; con la mochila activada, **R** prueba esta partida guardada (los mismos resultados). No descansa.

Incluido en el archivo binario de la versión `npx` / GitHub; para las instalaciones `pip`, ejecute `pip install "escape-the-valley[xrpl]"` (o `pip install -e ".[xrpl]"`) para la dependencia `xrpl-py`.

## Comandos

| Comando | Descripción |
|---------|-------------|
| `trail tui` | Iniciar la interfaz de usuario Textual en pantalla completa |
| `trail new` | Iniciar una nueva partida (modo CLI clásico) |
| `trail play` | Continuar una partida guardada (modo CLI clásico) |
| `trail status` | Mostrar el grupo, el vagón y los suministros |
| `trail journal` | Mostrar las entradas recientes del diario |
| `trail self-check` | Verificar el estado del entorno del juego |
| `trail version` | Mostrar la versión |
| `trail licenses` | Mostrar la licencia y los avisos de terceros (o `trail --licenses`) |
| `trail ledger status` | Mostrar el estado de la mochila |
| `trail ledger enable` | Habilitar la mochila XRPL |
| `trail ledger disable` | Deshabilitar la mochila XRPL |
| `trail ledger settle` | Establecer manualmente un punto de control |
| `trail ledger reconcile` | Reintentar los asentamientos fallidos |
| `trail ledger proof` | Verificar la partida guardada cargada (PASS/FAIL/INCONCLUSIVE) |
| `trail ledger wallet` | Mostrar los detalles de la billetera |
| `trail stats` | Mostrar las estadísticas de la partida y las tarjetas de memoria (admite `--json`) |
| `trail parcel send <addr> <supply> <amount>` | Enviar suministros a otro viajero |
| `trail parcel list` | Listar los paquetes recibidos |
| `trail parcel accept <id>` | Aceptar un paquete pendiente |
| `trail parcel sent` | Listar los paquetes que ha enviado |
| `trail wallet share` | Imprimir la dirección de su billetera para el intercambio |

## Alertas de advertencia

De forma predeterminada, el juego muestra advertencias detalladas para ayudar a los nuevos jugadores a detectar el peligro desde el principio. Los jugadores experimentados pueden cambiar al modo mínimo, que solo muestra las advertencias de peligro inminente (amenazas críticas de último momento):

```bash
trail tui --callouts minimal
trail new --callouts minimal
```

## Solución de problemas

**Si algo parece incorrecto, ejecute `trail self-check` primero.** Informa si Ollama es accesible, si su partida guardada se carga y qué modelo está instalado. Las tres cosas que pueden salir mal son:

| Síntoma | Causa | Solución |
|---------|-------|-----|
| **Generic / no narration** | Ollama no se está ejecutando (el GM es opcional y retrocede, nunca causa problemas) | Inicie Ollama (`ollama serve`) o juegue de forma determinista con `--gm-off`. Ejecute `trail self-check` para confirmar. |
| **Registro pendiente / asentamiento fallido** | XRPL Testnet es una red de prueba pública y, a veces, es inestable | `trail ledger reconcile` reintenta los asentamientos fallidos; ejecútelo de nuevo cuando la red se recupere. Los suministros son correctos localmente en cualquier caso. |
| **Save won't resume** | `run.json` se truncó o corrompió a la mitad de la escritura | El motor lo pone en cuarentena como `run.json.corrupt-<timestamp>` antes de rechazarlo, por lo que su próxima partida guardada no puede destruir la evidencia. Recupérese de esa copia de seguridad o comience una nueva partida desde una semilla. |

La primera partida narrada carga el modelo y puede tardar entre 10 y 30 segundos; esto es normal, no es un problema. Detalles completos: [Manual de solución de problemas](https://mcp-tool-shop-org.github.io/escape-the-valley/handbook/troubleshooting/).

## Requisitos

- Python 3.11+
- Ollama (opcional, para la narración de IA)
- xrpl-py (opcional, para la mochila del registro en las instalaciones `pip`; incluido en el archivo binario de la versión `npx` / GitHub)

## Seguridad

No hay telemetría. No hay cuentas. La narración del GM está activada de forma predeterminada (HTTP a Ollama local); pase `--gm-off` para desactivarla. XRPL está desactivado hasta `trail ledger enable` (solo Testnet). La voz es opcional (`--voice`). Consulte [SECURITY.md](SECURITY.md) para obtener el modelo de amenazas completo.

## Licencia

MIT: ejecute `trail licenses` (o `trail --licenses`) para ver la licencia del juego y los avisos de dependencias de terceros (también adjuntos a cada GitHub Release, un archivo por plataforma: `THIRD_PARTY_NOTICES-<os>-<arch>.txt`).

Creado por <a href="https://mcp-tool-shop.github.io/">MCP Tool Shop</a>
