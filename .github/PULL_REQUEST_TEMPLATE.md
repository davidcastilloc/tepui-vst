## Resumen Técnico

Describe de forma concisa el propósito técnico de este Pull Request, los componentes afectados (`synth_core`, `ai_bridge`, `plugin_vst`, `xtask`) y el problema o requerimiento resuelto.

## Cambios Principales

- [ ] Motor DSP / Audio Thread (`crates/synth_core`)
- [ ] Síntesis Espectral / Modelado Físico (`crates/ai_bridge`)
- [ ] Plugin VST3/CLAP, Host Standalone o Interfaz WebView2 (`crates/plugin_vst`)
- [ ] Empaquetado, CI/CD o Documentación (`crates/xtask`, `.github/`)

## Checklist de Calidad y Seguridad Real-Time (DSP)

- [ ] **Cero Asignaciones en Tiempo Real**: No se introdujeron asignaciones de memoria (`Vec`, `String`, `Box`, `Arc`) dentro de `AiSamplerPlugin::process()` ni en `synth_core` durante el procesamiento por bloque.
- [ ] **Cero Bloqueos en el Hilo de Audio**: No se utilizan `Mutex::lock()` ni operaciones de I/O bloqueantes en el callback de audio.
- [ ] **Estabilidad Numérica IEEE 754**: Los filtros, líneas de retardo y moduladores purgan números subnormales (`!is_subnormal()`) y previenen `NaN` / `Inf`.
- [ ] **Linter Limpio**: `cargo clippy --workspace --all-targets --no-deps -- -D warnings` finaliza con `0` advertencias.
- [ ] **Pruebas Automatizadas**: `cargo test --workspace` pasa en su totalidad e incluye pruebas para la nueva funcionalidad.
- [ ] **Conventional Commits**: Los commits siguen el estándar semántico (`feat:`, `fix:`, `perf:`, `refactor:`, `docs:`, `test:`, `chore:`).
