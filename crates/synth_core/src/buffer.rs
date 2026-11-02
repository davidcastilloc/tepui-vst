//! Representación optimizada de buffers de audio para samplers cromáticos.

use std::sync::Arc;

/// Buffer de audio en memoria para reproducción en el sampler.
/// Almacena canales planares (Left / Right) para máxima velocidad de lectura en la CPU.
#[derive(Debug, Clone)]
pub struct SampleBuffer {
    /// Nombre descriptivo del sample o timbre activo
    pub name: String,
    /// Datos de audio del canal izquierdo (o mono)
    pub left: Vec<f32>,
    /// Datos de audio del canal derecho (si es mono, apunta al mismo o vacío)
    pub right: Vec<f32>,
    /// Tasa de muestreo original del archivo de audio (ej. 44100.0 o 48000.0)
    pub sample_rate: f32,
    /// Nota MIDI de afinación original (60 = C4)
    pub root_note: u8,
    /// Desplazamiento fino en semitonos o cents (afinación precisa)
    pub tune_cents: f32,
    /// Punto inicial de reproducción (en muestras)
    pub start_sample: usize,
    /// Punto final de reproducción (en muestras, 0 = final del archivo)
    pub end_sample: usize,
    /// Habilitar bucle de sostenimiento
    pub loop_enabled: bool,
    /// Inicio del bucle (en muestras)
    pub loop_start: usize,
    /// Fin del bucle (en muestras)
    pub loop_end: usize,
    /// Longitud del crossfade en el bucle para eliminar clicks
    pub loop_crossfade: usize,
}

impl Default for SampleBuffer {
    fn default() -> Self {
        Self {
            name: "Grand Piano C4".to_string(),
            left: Vec::new(),
            right: Vec::new(),
            sample_rate: 44100.0,
            root_note: 60, // C4
            tune_cents: 0.0,
            start_sample: 0,
            end_sample: 0,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            loop_crossfade: 256,
        }
    }
}

impl SampleBuffer {
    /// Crea un nuevo buffer de audio estéreo a partir de canales planares.
    /// Incluye 2 muestras de guarda (sentinel padding) al final de cada canal
    /// para permitir interpolación Hermite de 4 puntos sin bifurcaciones condicionales.
    pub fn new_stereo(
        mut left: Vec<f32>,
        mut right: Vec<f32>,
        sample_rate: f32,
        root_note: u8,
    ) -> Self {
        let len = left.len().min(right.len());
        left.truncate(len);
        right.truncate(len);
        left.extend_from_slice(&[0.0, 0.0]);
        right.extend_from_slice(&[0.0, 0.0]);
        Self {
            name: "Custom Sample".to_string(),
            left,
            right,
            sample_rate,
            root_note,
            tune_cents: 0.0,
            start_sample: 0,
            end_sample: len,
            loop_enabled: false,
            loop_start: 0,
            loop_end: len,
            loop_crossfade: 256,
        }
    }

    /// Crea un buffer mono duplicando el canal a estéreo e incorporando 2 muestras de guarda (sentinel padding).
    pub fn new_mono(mut mono: Vec<f32>, sample_rate: f32, root_note: u8) -> Self {
        let len = mono.len();
        mono.extend_from_slice(&[0.0, 0.0]);
        Self {
            name: "Custom Sample".to_string(),
            left: mono.clone(),
            right: mono,
            sample_rate,
            root_note,
            tune_cents: 0.0,
            start_sample: 0,
            end_sample: len,
            loop_enabled: false,
            loop_start: 0,
            loop_end: len,
            loop_crossfade: 256,
        }
    }

    /// Alias de construcción mono con sentinel padding integrado.
    pub fn from_mono(mono: Vec<f32>, sample_rate: f32, root_note: u8) -> Self {
        Self::new_mono(mono, sample_rate, root_note)
    }

    /// Asigna un nombre descriptivo al buffer de muestra.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Retorna la cantidad total de muestras grabadas en el buffer (excluyendo el sentinel padding),
    /// independientemente de `end_sample`.
    #[inline(always)]
    pub fn raw_len(&self) -> usize {
        self.left.len().saturating_sub(2)
    }

    /// Retorna la cantidad de muestras audibles del sample (excluyendo el sentinel padding).
    #[inline(always)]
    pub fn len(&self) -> usize {
        let padded_len = self.raw_len();
        if self.end_sample > 0 {
            self.end_sample.min(padded_len)
        } else {
            padded_len
        }
    }

    /// Indica si el buffer no contiene muestras audibles.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Retorna la duración en segundos del sample audible.
    pub fn duration_seconds(&self) -> f32 {
        if self.sample_rate > 0.0 {
            self.len() as f32 / self.sample_rate
        } else {
            0.0
        }
    }

    /// Obtiene una muestra leída de forma segura sin peligro de pánico.
    #[inline(always)]
    pub fn get_sample(&self, channel: usize, index: usize) -> f32 {
        let chan = if channel == 0 {
            &self.left
        } else {
            &self.right
        };
        if index < chan.len() {
            chan[index]
        } else {
            0.0
        }
    }
}

/// Referencia compartida atómicamente al sample activo.
pub type SharedSampleBuffer = Arc<SampleBuffer>;
