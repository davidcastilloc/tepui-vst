//! Motor de Arpegiador Polifónico y Compases Venezolanos en Tiempo Real (`TepuiArpeggiator`).
//!
//! 100% libre de asignaciones dinámicas en el hilo de audio (`0` heap allocations),
//! operando exclusivamente sobre arreglos estáticos en pila (`[bool; 128]`, `[u8; 16]`).
//!
//! Incluye patrones clásicos (`Up`, `Down`) y cadencias rítmicas tradicionales venezolanas:
//! - **Joropo Llanero (`3/4 - 6/8`)**: Bordón grave acentuado + floreo de tiple y trancao.
//! - **Merengue Venezolano (`5/8`)**: Compás asimétrico caraqueño de 5 corcheas con acento sincopado en el 1, 3 y 5.

/// Modos de operación del Arpegiador TEPUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArpMode {
    /// Arpegiador desactivado (ejecución polifónica directa)
    Off = 0,
    /// Ascendente clásico (semicorcheas)
    Up = 1,
    /// Descendente clásico (semicorcheas)
    Down = 2,
    /// Joropo Llanero Venezolano (6/8 Bordón + Tiple + Trancao)
    Joropo = 3,
    /// Merengue Venezolano / Caraqueño (5/8 Síncopa Venezolana)
    Merengue = 4,
}

impl ArpMode {
    #[inline]
    pub fn from_i32(val: i32) -> Self {
        match val {
            1 => Self::Up,
            2 => Self::Down,
            3 => Self::Joropo,
            4 => Self::Merengue,
            _ => Self::Off,
        }
    }

    #[inline]
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

/// Evento de paso disparado por el arpegiador durante el bloque de audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArpStepEvent {
    pub note_on: u8,
    pub velocity: f32,
    pub prev_note_off: Option<u8>,
}

/// Arpegiador y generador de compases venezolanos en tiempo real (zero-allocation).
#[derive(Debug, Clone)]
pub struct TepuiArpeggiator {
    mode: ArpMode,
    sample_rate: f32,
    bpm: f32,
    held_notes: [bool; 128],
    held_count: usize,
    step_index: usize,
    samples_until_step: usize,
    last_sounding_note: Option<u8>,
    steps_triggered_count: u32,
}

impl Default for TepuiArpeggiator {
    fn default() -> Self {
        Self::new(44100.0)
    }
}

impl TepuiArpeggiator {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            mode: ArpMode::Off,
            sample_rate: sample_rate.max(500.0),
            bpm: 120.0,
            held_notes: [false; 128],
            held_count: 0,
            step_index: 0,
            samples_until_step: 0,
            last_sounding_note: None,
            steps_triggered_count: 0,
        }
    }

    #[inline]
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        if sample_rate > 0.0 {
            self.sample_rate = sample_rate.max(500.0);
        }
    }

    #[inline]
    pub fn set_bpm(&mut self, bpm: f32) {
        if bpm.is_finite() && bpm > 20.0 {
            self.bpm = bpm.clamp(40.0, 260.0);
        }
    }

    #[inline]
    pub fn bpm(&self) -> f32 {
        self.bpm
    }

    #[inline]
    pub fn set_mode(&mut self, mode: ArpMode) {
        if self.mode != mode {
            self.mode = mode;
            self.step_index = 0;
            self.samples_until_step = 0;
        }
    }

    #[inline]
    pub fn mode(&self) -> ArpMode {
        self.mode
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        self.mode != ArpMode::Off
    }

    /// Contador ODD de pasos rítmicos disparados por el arpegiador.
    #[inline]
    pub fn steps_triggered_count(&self) -> u32 {
        self.steps_triggered_count
    }

    /// Registra una nota pulsada en el pool del arpegiador.
    #[inline]
    pub fn press_note(&mut self, note: u8) {
        let idx = (note as usize).min(127);
        if !self.held_notes[idx] {
            let was_empty = self.held_count == 0;
            self.held_notes[idx] = true;
            self.held_count += 1;
            if was_empty {
                self.step_index = 0;
                self.samples_until_step = 0;
            }
        }
    }

    /// Libera una nota del pool del arpegiador. Retorna `Some(last_note)` si ya no quedan notas pulsadas.
    #[inline]
    pub fn release_note(&mut self, note: u8) -> Option<u8> {
        let idx = (note as usize).min(127);
        if self.held_notes[idx] {
            self.held_notes[idx] = false;
            self.held_count = self.held_count.saturating_sub(1);
            if self.held_count == 0 {
                self.step_index = 0;
                self.samples_until_step = 0;
                return self.last_sounding_note.take();
            }
        }
        None
    }

    /// Limpia todas las notas retenidas en el arpegiador.
    #[inline]
    pub fn clear(&mut self) -> Option<u8> {
        self.held_notes = [false; 128];
        self.held_count = 0;
        self.step_index = 0;
        self.samples_until_step = 0;
        self.last_sounding_note.take()
    }

    #[inline]
    fn collect_sorted_notes(&self, out: &mut [u8; 16]) -> usize {
        let mut count = 0;
        for (note, &held) in self.held_notes.iter().enumerate() {
            if held {
                out[count] = note as u8;
                count += 1;
                if count == 16 {
                    break;
                }
            }
        }
        count
    }

    #[inline]
    fn step_duration_samples(&self) -> usize {
        let beat_secs = 60.0 / self.bpm.clamp(40.0, 260.0);
        let step_secs = match self.mode {
            ArpMode::Off => beat_secs * 0.25,
            ArpMode::Up | ArpMode::Down => beat_secs * 0.25, // Semicorchea 1/16
            ArpMode::Joropo => beat_secs / 3.0,              // Seisillo / Corchea de Joropo 6/8
            ArpMode::Merengue => beat_secs * 0.25,           // Compás de 5/8 Caraqueño
        };
        ((step_secs * self.sample_rate).round() as usize).max(64)
    }

    #[inline]
    fn compute_step_note(&mut self, sorted: &[u8], count: usize) -> (u8, f32) {
        let step = self.step_index;
        self.step_index = self.step_index.wrapping_add(1);

        match self.mode {
            ArpMode::Off | ArpMode::Up => {
                let n = sorted[step % count];
                (n, 0.90)
            }
            ArpMode::Down => {
                let idx = (count - 1) - (step % count);
                (sorted[idx], 0.90)
            }
            ArpMode::Joropo => {
                // Compás de 6 pulsos de Joropo Llanero (Bordón -> Tiple -> Medio -> Trancao -> Tiple -> Bordón)
                let root = sorted[0];
                let high = sorted[count - 1];
                let mid = sorted[count / 2];
                let fifth = if count > 1 {
                    sorted[1]
                } else {
                    (root + 7).min(108)
                };
                match step % 6 {
                    0 => (if root >= 24 { root - 12 } else { root }, 0.98), // Bordón grave acentuado
                    1 => (high, 0.78),                                      // Floreo agudo
                    2 => (mid, 0.84),                                       // Cuerda media
                    3 => (fifth, 0.95),                                     // Acento de Trancao (pulso 4)
                    4 => ((high + 12).min(108), 0.80),                      // Tiple octavado
                    _ => (root, 0.88),                                      // Cierre de bordón
                }
            }
            ArpMode::Merengue => {
                // Compás de 5/8 Merengue Venezolano (1-2-3-4-5 con síncopa caraqueña en 1, 3 y 5)
                let root = sorted[0];
                let mid = sorted[count / 2];
                let high = sorted[count - 1];
                let second = if count > 1 {
                    sorted[1]
                } else {
                    (root + 4).min(108)
                };
                match step % 5 {
                    0 => (root, 0.98),                        // Pulso 1 (Fuerte)
                    1 => (second, 0.74),                      // Pulso 2
                    2 => (mid, 0.92),                         // Pulso 3 (Acento interno)
                    3 => (high, 0.76),                        // Pulso 4
                    _ => ((high + 12).min(108), 0.96),        // Pulso 5 (Quinto asincopado venezolano)
                }
            }
        }
    }

    /// Avanza el reloj del arpegiador por `frames` muestras y retorna un `ArpStepEvent` si corresponde disparar un paso.
    #[inline]
    pub fn advance_chunk(&mut self, frames: usize) -> Option<ArpStepEvent> {
        if self.mode == ArpMode::Off || self.held_count == 0 || frames == 0 {
            return None;
        }

        if self.samples_until_step == 0 {
            let mut sorted = [0u8; 16];
            let count = self.collect_sorted_notes(&mut sorted);
            if count == 0 {
                return None;
            }
            let (note_on, velocity) = self.compute_step_note(&sorted, count);
            let prev_note_off = self.last_sounding_note.replace(note_on);
            self.samples_until_step = self.step_duration_samples();
            self.steps_triggered_count = self.steps_triggered_count.wrapping_add(1);
            return Some(ArpStepEvent {
                note_on,
                velocity,
                prev_note_off,
            });
        }

        self.samples_until_step = self.samples_until_step.saturating_sub(frames);
        None
    }
}
