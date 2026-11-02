# Guía de Contribución (Contributing)

¡Gracias por tu interés en contribuir a **TEPUI // Gran Sabana Sonic Lab**! Este documento establece las normas de ingeniería, las reglas de seguridad de tiempo real y el flujo de trabajo con **Conventional Commits** requerido para nuestro pipeline de **Semantic Release**.

---

## 1. Reglas Inquebrantables de Ingeniería de Audio (Real-Time DSP)

Cualquier cambio que toque `crates/synth_core` o `AiSamplerPlugin::process()` en `crates/plugin_vst/src/lib.rs` debe cumplir estrictamente:

1. **Cero Asignaciones en el Hilo de Audio**:
   - Está prohibido usar `Vec::push`, `Vec::resize`, `String`, `Box::new`, `Arc::new` o cualquier operación que invoque al asignador del sistema dentro del callback de audio.
   - El proyecto compila con la feature `assert_process_allocs` de `nih_plug` activada: cualquier asignación en `process()` provocará un fallo inmediato en los tests.
2. **Cero Bloqueos en el Hilo de Audio**:
   - Está prohibido invocar `Mutex::lock()`, `RwLock::read()`, `RwLock::write()`, llamadas de sistema de archivos o red dentro de `process()`.
   - Utiliza exclusivamente variables atómicas (`Ordering::Relaxed`), canales acotados (`try_recv()` / `try_send()`) o `try_lock()` con degradación segura.
3. **Sanitización Numérica IEEE 754**:
   - Todo bucle de retroalimentación (filtros IIR, líneas de retardo, reverberación) debe purgar números subnormales (`!x.is_subnormal()`) y proteger la salida contra `NaN` o `Inf`.

---

## 2. Convención de Mensajes de Commit (Conventional Commits & Semantic Release)

Este repositorio utiliza **Semantic Release** para calcular automáticamente el número de versión (`MAJOR.MINOR.PATCH`) y generar [`CHANGELOG.md`](CHANGELOG.md). Todos los mensajes de commit deben seguir el formato [Conventional Commits](https://www.conventionalcommits.org/es/v1.0.0/):

```text
<tipo>(<alcance opcional>): <descripción concisa en español o inglés>

[cuerpo opcional explicando el qué y el porqué técnico]

[pie opcional: BREAKING CHANGE: descripción]
```

### Tipos soportados y su impacto en la versión (`SemVer`):

| Tipo | Descripción | Impacto en Versión |
| :--- | :--- | :--- |
| `feat` | Nueva funcionalidad en DSP, síntesis, formato de plugin o interfaz (`feat(dsp): ...`, `feat(ui): ...`) | **Minor** (`0.x.0`) |
| `fix` | Corrección de un bug de audio, concurrencia, serialización o interfaz (`fix(vst3): ...`) | **Patch** (`0.0.x`) |
| `perf` | Optimización de rendimiento en el bucle de audio o reducción de uso de CPU (`perf(filter): ...`) | **Patch** (`0.0.x`) |
| `refactor` | Reestructuración interna de código sin cambios funcionales | Sin release (salvo `BREAKING CHANGE`) |
| `docs` | Cambios exclusivos en documentación (`README.md`, `ARCHITECTURE.md`, docstrings) | Sin release |
| `test` | Adición o mejora de pruebas unitarias y de integración | Sin release |
| `ci` / `chore` | Configuración de pipelines de GitHub Actions, `xtask` o dependencias | Sin release |
| `BREAKING CHANGE:` | Cambio incompatible en contratos de sesión o parámetros (en el pie o con `!`, ej. `feat(params)!: ...`) | **Major** (`x.0.0`) |

---

## 3. Flujo de Trabajo y Verificación Pre-Push

1. Crea tu rama a partir de `develop`:
   ```powershell
   git checkout develop
   git pull origin develop
   git checkout -b feat/nombre-descriptivo
   ```
2. Antes de hacer commit o abrir un Pull Request, ejecuta obligatoriamente la validación completa en local:
   ```powershell
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --no-deps -- -D warnings
   cargo test --workspace
   ```
3. Abre un Pull Request hacia `develop` (para integración continua) o desde `develop` hacia `main` (para disparar una nueva versión mediante Semantic Release).
