# Changelog

Todos los cambios notables en **TEPUI // Gran Sabana Sonic Lab** se documentan automáticamente en este archivo siguiendo [Conventional Commits](https://www.conventionalcommits.org/) y [Semantic Versioning](https://semver.org/).

## [1.7.0](https://github.com/davidcastilloc/tepui-vst/compare/v1.6.0...v1.7.0) (2026-11-03)

### Nuevas Funcionalidades (Features)

* **ai:** desplegar Worker Edge en Cloudflare Workers AI y conectar cliente nativo en vivo ([43a8b8f](https://github.com/davidcastilloc/tepui-vst/commit/43a8b8f7a4746cb25f93e36e39d365b77239f10e))

# Changelog

Todos los cambios notables en **TEPUI // Gran Sabana Sonic Lab** se documentan automáticamente en este archivo siguiendo [Conventional Commits](https://www.conventionalcommits.org/) y [Semantic Versioning](https://semver.org/).

## [1.6.0](https://github.com/davidcastilloc/tepui-vst/compare/v1.5.0...v1.6.0) (2026-11-03)

### Nuevas Funcionalidades (Features)

* **ai:** integrar arquitecto Cloudflare de presets/samples y calibrador Piano-Roll YIN ([83fcbba](https://github.com/davidcastilloc/tepui-vst/commit/83fcbba5995b6766c2c0aedd81273158b3c5bac2))

# Changelog

Todos los cambios notables en **TEPUI // Gran Sabana Sonic Lab** se documentan automáticamente en este archivo siguiendo [Conventional Commits](https://www.conventionalcommits.org/) y [Semantic Versioning](https://semver.org/).

## [1.5.0](https://github.com/davidcastilloc/tepui-vst/compare/v1.4.0...v1.5.0) (2026-11-03)

### Nuevas Funcionalidades (Features)

* **dsp:** integrar velocity polifonica, ruedas pitch/mod, loop crossfade, delay ritmico e instalador Windows ([fc0d688](https://github.com/davidcastilloc/tepui-vst/commit/fc0d688991c7707df48e4b3082a0073180535045))

# Changelog

Todos los cambios notables en **TEPUI // Gran Sabana Sonic Lab** se documentan automáticamente en este archivo siguiendo [Conventional Commits](https://www.conventionalcommits.org/) y [Semantic Versioning](https://semver.org/).

## [1.4.0](https://github.com/davidcastilloc/tepui-vst/compare/v1.3.0...v1.4.0) (2026-11-02)

### Nuevas Funcionalidades (Features)

* **dsp:** implementar matriz 5x5 multirruta, cache NPU de samples, persistencia de presets y telemetria ODD ([cb093fe](https://github.com/davidcastilloc/tepui-vst/commit/cb093fee069c89ffd94a303bd335f12bf9c76e34))

# Changelog

Todos los cambios notables en **TEPUI // Gran Sabana Sonic Lab** se documentan automáticamente en este archivo siguiendo [Conventional Commits](https://www.conventionalcommits.org/) y [Semantic Versioning](https://semver.org/).

## [1.3.0](https://github.com/davidcastilloc/tepui-vst/compare/v1.2.0...v1.3.0) (2026-11-02)

### Nuevas Funcionalidades (Features)

* **ui:** aplicar mejoras de diseno hardware, iconografia SVG, matriz interactiva y telemetria ODD ([fb688e2](https://github.com/davidcastilloc/tepui-vst/commit/fb688e233a4a09b5406f0faf26f9369c6171a9c8))

# Changelog

Todos los cambios notables en **TEPUI // Gran Sabana Sonic Lab** se documentan automáticamente en este archivo siguiendo [Conventional Commits](https://www.conventionalcommits.org/) y [Semantic Versioning](https://semver.org/).

## [1.2.0](https://github.com/davidcastilloc/tepui-vst/compare/v1.1.0...v1.2.0) (2026-11-02)

### Nuevas Funcionalidades (Features)

* **ui:** habilitar redimensionado nativo y escalado responsivo en standalone y DAW ([3b48bfa](https://github.com/davidcastilloc/tepui-vst/commit/3b48bfa3835363a7329050c924f5609aa28f8aeb))

# Changelog

Todos los cambios notables en **TEPUI // Gran Sabana Sonic Lab** se documentan automáticamente en este archivo siguiendo [Conventional Commits](https://www.conventionalcommits.org/) y [Semantic Versioning](https://semver.org/).

## [1.1.0](https://github.com/davidcastilloc/tepui-vst/compare/v1.0.0...v1.1.0) (2026-11-02)

### Nuevas Funcionalidades (Features)

* **ui:** implementar consola Eurorack de 6 pantallas OLED, barra de Prompt IA y buscador de 1,250+ presets por packs ([5c4d4f3](https://github.com/davidcastilloc/tepui-vst/commit/5c4d4f32472f2b9a8e29b1145da8045f7f2477f7))

### Documentación

* **brand:** integrar banner oficial e icono del proyecto en repositorio, UI y ejecutable Windows ([573507f](https://github.com/davidcastilloc/tepui-vst/commit/573507f2243248f5799e1c534d0221e8a1203d4f))

# Changelog

Todos los cambios notables en **TEPUI // Gran Sabana Sonic Lab** se documentan automáticamente en este archivo siguiendo [Conventional Commits](https://www.conventionalcommits.org/) y [Semantic Versioning](https://semver.org/).

## 1.0.0 (2026-11-02)

### Nuevas Funcionalidades (Features)

* **dsp:** motor polifonico TEPUI VST3/CLAP con modelado fisico venezolano, reverb canon, arpegiador 6/8-5/8 y grabadora master ([ac787c5](https://github.com/davidcastilloc/tepui-vst/commit/ac787c5e4e0e0d54ea0709a35b874b6223f01022))

### Correcciones de Errores (Bug Fixes)

* **ci:** aislar sondeo WASAPI/WinMM en runners headless y compatibilidad Clippy 1.99 ([58fe399](https://github.com/davidcastilloc/tepui-vst/commit/58fe399c58c14417c644c4b8fb682a2f6f8b5f38))

# Changelog

Todos los cambios notables en este proyecto serán documentados en este archivo.

El formato está basado en [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/),
y este proyecto se adhiere a [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-11-02

### Added
- **Motor DSP Polifónico (`synth_core`)**:
  - Gestor polifónico de 16 voces con robo inteligente de voces (*voice stealing*) y fundido cruzado anti-click de `5 ms`.
  - Resampler cromático Hermite de 4 puntos (3.er orden) con centinelas de memoria y sobremuestreo 2x automático (`2x oversampling`) en transposiciones altas.
  - Filtro de Variable de Estado (SVF) Zero-Delay Feedback (TPT) de 12 dB/octava con modos `Lowpass`, `Highpass`, `Bandpass` y `Bypass`.
  - Oscilador de baja frecuencia (`LfoEngine`) con destinos `Cutoff`, `Pitch Vibrato` y `Stereo Tremolo` en cuadratura.
  - Arpegiador y generador de compases venezolanos en tiempo real (`TepuiArpeggiator`) libre de asignaciones dinámicas con modos `Off`, `Up`, `Down`, `Joropo 6/8` y `Merengue Venezolano 5/8`.
  - Rack de efectos espaciales (`StereoFxRack`) con Chorus en cuadratura, Ping-Pong Cross-Delay y Reverberación Algorítmica "Cañón del Tepui" (4 filtros Comb + 2 difusores All-Pass en serie por canal).
  - Soporte expresivo completo para `Pitch Bend`, `Sustain Pedal (CC#64)`, `ModWheel (CC#1)`, `Brightness (CC#74 / CC#11)` y `Aftertouch`.
- **Síntesis y Modelado Físico Venezolano (`ai_bridge`)**:
  - 14 instrumentos integrados incluyendo modelado físico de **Cuatro Venezolano**, **Arpa Llanera**, **Bandola Llanera**, **Maracas Capachos**, **Tambor Cumaco**, **Quitiplás de Barlovento** y **Furruco Zuliano Sub**, además de motor espectral para prompts abiertos.
- **Plugin VST3/CLAP, Host Standalone y Editor WebView2 (`plugin_vst`)**:
  - Interfaz en memoria bajo protocolo personalizado `tepui://localhost/` con osciloscopio OLED a 60 FPS, cabezal de reproducción en vivo, curva LFO y mapeador de 3 Keyzones (`LOW`, `MID`, `HIGH`).
  - Formato de patch autocontenido `.tepui` (JSON + WAV IEEE Float 32-bit Base64 para muestra principal y 3 Keyzones) y persistencia completa en sesión DAW.
  - Grabadora Master en vivo (`LiveMasterRecorder`: `LIVE REC -> WAV`) sin bloqueo ni asignaciones en el hilo de audio.
  - Herramienta de empaquetado e instalación automática (`cargo run --package xtask -- install`).
