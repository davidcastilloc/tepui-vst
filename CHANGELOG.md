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
