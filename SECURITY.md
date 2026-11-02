# Política de Seguridad (Security Policy)

## Versiones Soportadas

| Versión | Soporte de Seguridad |
| :--- | :--- |
| `0.1.x` | :white_check_mark: Activo |

---

## Modelo de Seguridad del Plugin y Standalone

1. **Aislamiento de Red y Protocolo en Memoria (`tepui://`)**:
   - El editor embebido WebView2 se comunica con el motor nativo de Rust exclusivamente a través del esquema en memoria `tepui://localhost/` y del canal IPC nativo `window.chrome.webview.postMessage`.
   - No se abren puertos TCP/UDP ni servidores HTTP locales en la interfaz de loopback (`127.0.0.1`), eliminando vectores de ataque de tipo *Cross-Site Request Forgery (CSRF)* local o colisión entre instancias del plugin.
2. **Validación Defensiva de Archivos `.wav` y `.tepui`**:
   - El decodificador RIFF/WAVE (`crates/synth_core/src/wav.rs`) y el cargador de patches `.tepui` validan cabeceras, tamaños de chunk y codificación Base64 sin emplear bloques `unsafe` de lectura descontrolada.
   - Todas las muestras importadas son sanitizadas contra valores `NaN`, `Inf` y subnormales antes de ingresar al motor de voces.
3. **Protección del Hilo de Audio en Tiempo Real**:
   - La etapa de salida Master incorpora un limitador/saturador suave (`±0.95` a `±0.999`) y un sanitizador numérico en punto flotante que impide picos fuera de escala capaces de dañar monitores de estudio o el oído del usuario.

---

## Reporte de Vulnerabilidades

Si descubres una vulnerabilidad de seguridad (por ejemplo, un archivo `.wav` o `.tepui` malformado que cause pánico o corrupción de memoria), por favor **no abras un issue público**.

Envía el reporte directamente a **contact@gransabana.audio** incluyendo:
- Descripción técnica del vector y componente afectado (`synth_core`, `ai_bridge` o `plugin_vst`).
- Pasos o payload mínimo para reproducir el problema.
- Impacto estimado.

Responderemos en un plazo máximo de **48 horas** con la confirmación y el plan de mitigación.
