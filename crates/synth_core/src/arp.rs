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

/// Subdivisión rítmica sincronizada al transporte del DAW para el Arpegiador / Secuenciador de 16 pasos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArpRateDivision {
    /// Negra (1/4)
    Div1_4 = 0,
    /// Corchea (1/8)
    Div1_8 = 1,
    /// Semicorchea estándar (1/16)
    Div1_16 = 2,
    /// Tresillo de corchea (1/8T)
    Div1_8T = 3,
    /// Semicorchea con puntillo (1/16D)
    Div1_16D = 4,
}

impl ArpRateDivision {
    #[inline]
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => Self::Div1_4,
            1 => Self::Div1_8,
            2 => Self::Div1_16,
            3 => Self::Div1_8T,
            4 => Self::Div1_16D,
            _ => Self::Div1_16,
        }
    }

    #[inline]
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    #[inline]
    pub fn beat_fraction(self) -> f32 {
        match self {
            Self::Div1_4 => 1.0,
            Self::Div1_8 => 0.5,
            Self::Div1_16 => 0.25,
            Self::Div1_8T => 1.0 / 3.0,
            Self::Div1_16D => 0.375,
        }
    }
}

/// Evento de paso disparado por el arpegiador durante el bloque de audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArpStepEvent {
    pub note_on: u8,
    pub velocity: f32,
    pub prev_note_off: Option<u8>,
}

/// Patrones de velocidad de 12 pasos sincronizados 1:1 con las 12 barras del visor `#oled-arp`.
pub const DEFAULT_ARP_ACCENTS_UP: [f32; 12] = [
    0.25, 0.40, 0.55, 0.70, 0.85, 1.00, 0.25, 0.40, 0.55, 0.70, 0.85, 1.00,
];
pub const DEFAULT_ARP_ACCENTS_DOWN: [f32; 12] = [
    1.00, 0.85, 0.70, 0.55, 0.40, 0.25, 1.00, 0.85, 0.70, 0.55, 0.40, 0.25,
];
pub const DEFAULT_ARP_ACCENTS_JOROPO: [f32; 12] = [
    0.95, 0.48, 0.78, 0.92, 0.52, 0.86, 0.95, 0.48, 0.78, 0.92, 0.52, 0.86,
];
pub const DEFAULT_ARP_ACCENTS_MERENGUE: [f32; 12] = [
    0.96, 0.55, 0.82, 0.48, 0.90, 0.96, 0.55, 0.82, 0.48, 0.90, 0.75, 0.60,
];

/// Arpegiador y generador de compases venezolanos en tiempo real (zero-allocation).
#[derive(Debug, Clone)]
pub struct TepuiArpeggiator {
    mode: ArpMode,
    rate_division: ArpRateDivision,
    step_count: usize,
    sample_rate: f32,
    bpm: f32,
    held_notes: [bool; 128],
    held_velocities: [f32; 128],
    held_count: usize,
    step_index: usize,
    samples_until_step: usize,
    gate_samples_remaining: usize,
    last_sounding_note: Option<u8>,
    steps_triggered_count: u32,
    ratchet_hits_count: u32,
    custom_step_accents: [f32; 16],
    step_octaves: [i8; 16],
    step_gates: [f32; 16],
    step_ratchets: [u8; 16],
    ratchet_sub_remaining: u8,
    ratchet_sub_period: usize,
    ratchet_samples_until_next: usize,
    ratchet_note: u8,
    ratchet_vel: f32,
    has_custom_accents: bool,
    last_step_velocity: f32,
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
            rate_division: ArpRateDivision::Div1_16,
            step_count: 16,
            sample_rate: sample_rate.max(500.0),
            bpm: 120.0,
            held_notes: [false; 128],
            held_velocities: [0.90; 128],
            held_count: 0,
            step_index: 0,
            samples_until_step: 0,
            gate_samples_remaining: 0,
            last_sounding_note: None,
            steps_triggered_count: 0,
            ratchet_hits_count: 0,
            custom_step_accents: [1.0; 16],
            step_octaves: [0; 16],
            step_gates: [0.76; 16],
            step_ratchets: [1; 16],
            ratchet_sub_remaining: 0,
            ratchet_sub_period: 0,
            ratchet_samples_until_next: 0,
            ratchet_note: 60,
            ratchet_vel: 0.90,
            has_custom_accents: false,
            last_step_velocity: 0.90,
        }
    }

    #[inline]
    pub fn set_rate_division(&mut self, div: ArpRateDivision) {
        self.rate_division = div;
    }

    #[inline]
    pub fn rate_division(&self) -> ArpRateDivision {
        self.rate_division
    }

    #[inline]
    pub fn set_step_count(&mut self, count: usize) {
        self.step_count = count.clamp(4, 16);
    }

    #[inline]
    pub fn step_count(&self) -> usize {
        self.step_count
    }

    #[inline]
    pub fn set_step_octave(&mut self, step_idx: usize, octave_offset: i8) {
        if step_idx < 16 {
            self.step_octaves[step_idx] = octave_offset.clamp(-2, 2);
        }
    }

    #[inline]
    pub fn step_octave(&self, step_idx: usize) -> i8 {
        self.step_octaves.get(step_idx).copied().unwrap_or(0)
    }

    #[inline]
    pub fn set_step_gate(&mut self, step_idx: usize, gate_ratio: f32) {
        if step_idx < 16 && gate_ratio.is_finite() {
            self.step_gates[step_idx] = gate_ratio.clamp(0.10, 0.98);
        }
    }

    #[inline]
    pub fn step_gate(&self, step_idx: usize) -> f32 {
        self.step_gates.get(step_idx).copied().unwrap_or(0.76)
    }

    #[inline]
    pub fn set_step_ratchet(&mut self, step_idx: usize, repeats: u8) {
        if step_idx < 16 {
            self.step_ratchets[step_idx] = repeats.clamp(1, 4);
        }
    }

    #[inline]
    pub fn step_ratchet(&self, step_idx: usize) -> u8 {
        self.step_ratchets.get(step_idx).copied().unwrap_or(1)
    }

    #[inline]
    pub fn ratchet_hits_count(&self) -> u32 {
        self.ratchet_hits_count
    }

    /// Configura el acento/velocidad personalizado (`0.05..=1.0`) de un paso individual (`0..16`) desde el visor OLED.
    #[inline]
    pub fn set_step_accent(&mut self, step_idx: usize, accent: f32) {
        if step_idx < 16 && accent.is_finite() {
            if !self.has_custom_accents {
                // Inicializar los 16 pasos con el patrón por defecto del modo actual antes de aplicar la edición
                let defaults = Self::default_accents_for_mode(self.mode);
                for i in 0..16 {
                    self.custom_step_accents[i] = defaults[i % 12];
                }
            }
            self.custom_step_accents[step_idx] = accent.clamp(0.05, 1.0);
            self.has_custom_accents = true;
        }
    }

    /// Sincroniza la tabla completa de 16 acentos de pasos desde el almacén lock-free.
    #[inline]
    pub fn set_step_accents(&mut self, accents: &[f32; 16], active: bool) {
        self.has_custom_accents = active;
        if active {
            for (dst, &src) in self.custom_step_accents.iter_mut().zip(accents.iter()) {
                *dst = if src.is_finite() {
                    src.clamp(0.05, 1.0)
                } else {
                    1.0
                };
            }
        }
    }

    /// Actualiza en vivo la velocidad base de todas las notas actualmente retenidas en el arpegiador.
    #[inline]
    pub fn set_live_velocity(&mut self, velocity: f32) {
        if velocity.is_finite() {
            let v = velocity.clamp(0.05, 1.0);
            for (note_idx, &held) in self.held_notes.iter().enumerate() {
                if held {
                    self.held_velocities[note_idx] = v;
                }
            }
        }
    }

    /// Retorna la velocidad efectiva del último paso disparado por el arpegiador (`0.05..=1.0`).
    #[inline]
    pub fn last_step_velocity(&self) -> f32 {
        self.last_step_velocity
    }

    #[inline]
    pub fn default_accents_for_mode(mode: ArpMode) -> &'static [f32; 12] {
        match mode {
            ArpMode::Off | ArpMode::Up => &DEFAULT_ARP_ACCENTS_UP,
            ArpMode::Down => &DEFAULT_ARP_ACCENTS_DOWN,
            ArpMode::Joropo => &DEFAULT_ARP_ACCENTS_JOROPO,
            ArpMode::Merengue => &DEFAULT_ARP_ACCENTS_MERENGUE,
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
            self.ratchet_sub_remaining = 0;
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

    /// Registra una nota pulsada en el pool del arpegiador con velocidad explícita (`0.0..=1.0`).
    #[inline]
    pub fn press_note_with_velocity(&mut self, note: u8, velocity: f32) {
        let idx = (note as usize).min(127);
        self.held_velocities[idx] = velocity.clamp(0.05, 1.0);
        if !self.held_notes[idx] {
            let was_empty = self.held_count == 0;
            self.held_notes[idx] = true;
            self.held_count += 1;
            if was_empty {
                self.step_index = 0;
                self.samples_until_step = 0;
                self.ratchet_sub_remaining = 0;
            }
        }
    }

    /// Registra una nota pulsada en el pool del arpegiador con velocidad por defecto (`0.90`).
    #[inline]
    pub fn press_note(&mut self, note: u8) {
        self.press_note_with_velocity(note, 0.90);
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
                self.ratchet_sub_remaining = 0;
                return self.last_sounding_note.take();
            }
        }
        None
    }

    /// Limpia todas las notas retenidas en el arpegiador.
    #[inline]
    pub fn clear(&mut self) -> Option<u8> {
        self.held_notes = [false; 128];
        self.held_velocities = [0.90; 128];
        self.held_count = 0;
        self.step_index = 0;
        self.samples_until_step = 0;
        self.ratchet_sub_remaining = 0;
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
        let step_secs = if self.rate_division != ArpRateDivision::Div1_16 {
            beat_secs * self.rate_division.beat_fraction()
        } else {
            match self.mode {
                ArpMode::Off => beat_secs * 0.25,
                ArpMode::Up | ArpMode::Down => beat_secs * 0.25, // Semicorchea 1/16
                ArpMode::Joropo => beat_secs / 3.0,              // Seisillo / Corchea de Joropo 6/8
                ArpMode::Merengue => beat_secs * 0.25,           // Compás de 5/8 Caraqueño
            }
        };
        ((step_secs * self.sample_rate).round() as usize).max(64)
    }

    #[inline]
    fn compute_step_note(&mut self, sorted: &[u8], count: usize) -> (u8, f32, usize) {
        let active_len = self.step_count.clamp(4, 16);
        let seq_idx = self.step_index % active_len;
        let step = self.step_index;
        self.step_index = self.step_index.wrapping_add(1);

        let mut avg_vel = 0.0_f32;
        for &n in &sorted[..count] {
            avg_vel += self.held_velocities[(n as usize).min(127)];
        }
        let chord_vel = if count > 0 {
            (avg_vel / count as f32).clamp(0.05, 1.0)
        } else {
            0.90
        };
        let vel_scale = (chord_vel / 0.90).clamp(0.06, 1.12);

        let (base_note_out, note_vel_scale) = match self.mode {
            ArpMode::Off | ArpMode::Up => {
                let n = sorted[step % count];
                let nv = self.held_velocities[(n as usize).min(127)].clamp(0.05, 1.0);
                (n, (nv / 0.95).clamp(0.06, 1.08))
            }
            ArpMode::Down => {
                let idx = (count - 1) - (step % count);
                let n = sorted[idx];
                let nv = self.held_velocities[(n as usize).min(127)].clamp(0.05, 1.0);
                (n, (nv / 0.95).clamp(0.06, 1.08))
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
                let note = match step % 6 {
                    0 => {
                        if root >= 24 {
                            root - 12
                        } else {
                            root
                        }
                    } // Bordón grave acentuado
                    1 => high,                 // Floreo agudo
                    2 => mid,                  // Cuerda media
                    3 => fifth,                // Acento de Trancao (pulso 4)
                    4 => (high + 12).min(108), // Tiple octavado
                    _ => root,                 // Cierre de bordón
                };
                (note, vel_scale)
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
                let note = match step % 5 {
                    0 => root,                 // Pulso 1 (Fuerte)
                    1 => second,               // Pulso 2
                    2 => mid,                  // Pulso 3 (Acento interno)
                    3 => high,                 // Pulso 4
                    _ => (high + 12).min(108), // Pulso 5 (Quinto asincopado venezolano)
                };
                (note, vel_scale)
            }
        };

        let oct_shift = (self.step_octaves[seq_idx] as i32) * 12;
        let note_out = (base_note_out as i32 + oct_shift).clamp(12, 120) as u8;

        let bar_accent = if self.has_custom_accents {
            self.custom_step_accents[seq_idx].clamp(0.05, 1.0)
        } else {
            Self::default_accents_for_mode(self.mode)[seq_idx % 12]
        };

        let final_vel = (bar_accent * note_vel_scale).clamp(0.05, 1.0);
        self.last_step_velocity = final_vel;
        (note_out, final_vel, seq_idx)
    }

    /// Avanza el temporizador de Gate del paso actual y retorna `Some(note)` cuando debe liberarse la nota antes del siguiente paso.
    #[inline]
    pub fn advance_gate_off(&mut self, frames: usize) -> Option<u8> {
        if self.mode == ArpMode::Off || self.gate_samples_remaining == 0 || frames == 0 {
            return None;
        }
        if frames >= self.gate_samples_remaining {
            self.gate_samples_remaining = 0;
            return self.last_sounding_note.take();
        }
        self.gate_samples_remaining -= frames;
        None
    }

    /// Avanza el reloj del arpegiador por `frames` muestras y retorna un `ArpStepEvent` si corresponde disparar un paso o sub-repetición Ratchet.
    #[inline]
    pub fn advance_chunk(&mut self, frames: usize) -> Option<ArpStepEvent> {
        if self.mode == ArpMode::Off || self.held_count == 0 || frames == 0 {
            return None;
        }

        // 1. Verificar si hay sub-repeticiones Ratchet pendientes dentro del paso actual
        if self.ratchet_sub_remaining > 0 {
            if self.ratchet_samples_until_next == 0 {
                self.ratchet_sub_remaining -= 1;
                self.ratchet_samples_until_next = self.ratchet_sub_period;
                let prev_note_off = self.last_sounding_note.replace(self.ratchet_note);
                self.gate_samples_remaining = (self.ratchet_sub_period * 3 / 4).max(24);
                self.steps_triggered_count = self.steps_triggered_count.wrapping_add(1);
                self.ratchet_hits_count = self.ratchet_hits_count.wrapping_add(1);
                return Some(ArpStepEvent {
                    note_on: self.ratchet_note,
                    velocity: (self.ratchet_vel * 0.92).clamp(0.05, 1.0),
                    prev_note_off,
                });
            }
            self.ratchet_samples_until_next =
                self.ratchet_samples_until_next.saturating_sub(frames);
        }

        if self.samples_until_step == 0 {
            let mut sorted = [0u8; 16];
            let count = self.collect_sorted_notes(&mut sorted);
            if count == 0 {
                return None;
            }
            let (note_on, velocity, seq_idx) = self.compute_step_note(&sorted, count);
            let prev_note_off = self.last_sounding_note.replace(note_on);
            let step_dur = self.step_duration_samples();
            let consumed = if frames > 1 { frames } else { 0 };
            self.samples_until_step = step_dur.saturating_sub(consumed);

            let ratchets = self.step_ratchets[seq_idx].clamp(1, 4);
            let gate_ratio = self.step_gates[seq_idx].clamp(0.10, 0.98);
            if ratchets > 1 {
                let sub_period = (step_dur / ratchets as usize).max(48);
                self.ratchet_sub_remaining = ratchets - 1;
                self.ratchet_sub_period = sub_period;
                self.ratchet_samples_until_next = sub_period.saturating_sub(consumed);
                self.ratchet_note = note_on;
                self.ratchet_vel = velocity;
                self.gate_samples_remaining = ((sub_period as f32 * gate_ratio).round() as usize)
                    .saturating_sub(consumed)
                    .max(24);
            } else {
                self.ratchet_sub_remaining = 0;
                self.gate_samples_remaining = ((step_dur as f32 * gate_ratio).round() as usize)
                    .saturating_sub(consumed)
                    .max(32);
            }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_16_step_sequencer_ratchet_octave_and_rate_divisions() {
        let mut arp = TepuiArpeggiator::new(48000.0);
        arp.set_bpm(120.0);
        arp.set_mode(ArpMode::Up);
        arp.set_step_count(16);
        arp.set_rate_division(ArpRateDivision::Div1_8T);
        arp.set_step_octave(0, 1); // +1 octava en el paso 0
        arp.set_step_ratchet(0, 2); // 2x ratchet en el paso 0
        arp.press_note_with_velocity(60, 0.9);

        let evt0 = arp.advance_chunk(64).expect("Step 0 should trigger");
        assert_eq!(
            evt0.note_on, 72,
            "Step 0 octave shift +1 should transpose C4 (60) to C5 (72)"
        );

        // Avanzar hasta que dispare el sub-golpe ratchet dentro del mismo paso 0
        let mut ratchet_triggered = false;
        for _ in 0..200 {
            if let Some(evt_sub) = arp.advance_chunk(64) {
                if arp.ratchet_hits_count() > 0 {
                    assert_eq!(evt_sub.note_on, 72);
                    ratchet_triggered = true;
                    break;
                }
            }
        }
        assert!(
            ratchet_triggered,
            "Ratchet subdivision should trigger extra hit and bump ratchet_hits_count"
        );
    }
}
