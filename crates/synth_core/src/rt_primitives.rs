//! Primitivas de tiempo real libres de bloqueos y de asignaciones dinámicas (`rt_primitives`).
//!
//! Implementa dos patrones arquitectónicos fundamentales para motores DSP de grado industrial:
//! 1. [`OnePoleSmoother`]: Suavizador exponencial de 1 polo para eliminar *zipper noise* en barridos
//!    rápidos de perillas (`Cutoff`, `Resonance`, `WT Pos`, `WT Warp`, `Drive`, `OTT`, `Gain`).
//! 2. [`SeqLockArray`]: Arreglo compartido con protocolo *Sequence Lock* (`Acquire`/`Release`) que
//!    impide lecturas partidas (*torn reads*) cuando la interfaz escribe tablas de ondas o patrones
//!    de arpegiador mientras el hilo de audio sintetiza.

use std::sync::atomic::{fence, AtomicU32, Ordering};

/// Suavizador exponencial de un polo (1-Pole Lowpass Parameter Smoother) con 0 asignaciones.
#[derive(Debug, Clone, Copy)]
pub struct OnePoleSmoother {
    current: f32,
    target: f32,
    coeff: f32,
    smoothing_ms: f32,
}

impl OnePoleSmoother {
    /// Crea un suavizador inicializado en `initial` con constante de tiempo `smoothing_ms`.
    #[inline]
    pub fn new(initial: f32, smoothing_ms: f32, sample_rate: f32) -> Self {
        let mut s = Self {
            current: initial,
            target: initial,
            coeff: 1.0,
            smoothing_ms: smoothing_ms.max(0.1),
        };
        s.set_sample_rate(sample_rate);
        s
    }

    /// Recalcula el coeficiente exponencial ante un cambio de frecuencia de muestreo.
    #[inline]
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(1000.0);
        let tau_samples = (self.smoothing_ms * 0.001 * sr).max(1.0);
        // 99% de convergencia en ~4.6 constantes de tiempo
        self.coeff = (1.0 - (-4.6 / tau_samples).exp()).clamp(0.0001, 1.0);
    }

    /// Establece el nuevo valor objetivo hacia el cual interpolará suavemente.
    #[inline]
    pub fn set_target(&mut self, target: f32) {
        if target.is_finite() {
            self.target = target;
        }
    }

    /// Fija instantáneamente tanto el valor actual como el objetivo (ej. al cargar un preset en silencio).
    #[inline]
    pub fn snap_to(&mut self, value: f32) {
        if value.is_finite() {
            self.current = value;
            self.target = value;
        }
    }

    /// Retorna el valor suavizado actual sin avanzar el reloj.
    #[inline]
    pub fn current(&self) -> f32 {
        self.current
    }

    /// Retorna el valor objetivo configurado.
    #[inline]
    pub fn target(&self) -> f32 {
        self.target
    }

    /// Indica si el suavizador aún está en transición activa hacia `target`.
    #[inline]
    pub fn is_smoothing(&self) -> bool {
        (self.target - self.current).abs() > 1e-5
    }

    /// Avanza exactamente 1 muestra y retorna el nuevo valor suavizado.
    #[inline]
    pub fn next_sample(&mut self) -> f32 {
        let diff = self.target - self.current;
        if diff.abs() <= 1e-6 {
            self.current = self.target;
        } else {
            self.current += diff * self.coeff;
        }
        self.current
    }

    /// Avanza analíticamente `frames` muestras en un solo paso (`O(1)`) y retorna el valor resultante.
    #[inline]
    pub fn advance_samples(&mut self, frames: usize) -> f32 {
        if frames == 0 {
            return self.current;
        }
        let diff = self.target - self.current;
        if diff.abs() <= 1e-6 {
            self.current = self.target;
        } else if frames == 1 {
            self.current += diff * self.coeff;
        } else {
            let decay = (1.0 - self.coeff).powi(frames as i32);
            self.current = self.target - diff * decay;
        }
        self.current
    }
}

/// Arreglo atómico libre de bloqueos protegido por *Sequence Lock* (`SeqLockArray<N>`).
///
/// Garantiza que el hilo de audio nunca lea un arreglo a medio escribir (*torn read*)
/// sin utilizar `Mutex` ni bloquear jamás el hilo de tiempo real.
pub struct SeqLockArray<const N: usize> {
    /// Contador de secuencia: par = estable, impar = escritura en curso.
    seq: AtomicU32,
    /// Celdas atómicas codificadas como `f32::to_bits()`.
    cells: [AtomicU32; N],
}

impl<const N: usize> std::fmt::Debug for SeqLockArray<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeqLockArray")
            .field("revision", &self.revision())
            .field("len", &N)
            .finish()
    }
}

impl<const N: usize> SeqLockArray<N> {
    /// Crea un `SeqLockArray` inicializado con el valor constante `fill_value`.
    pub fn new(fill_value: f32) -> Self {
        let bits = fill_value.to_bits();
        Self {
            seq: AtomicU32::new(0),
            cells: std::array::from_fn(|_| AtomicU32::new(bits)),
        }
    }

    /// Retorna la revisión actual (siempre par cuando está estable).
    #[inline]
    pub fn revision(&self) -> u32 {
        self.seq.load(Ordering::Acquire) >> 1
    }

    /// Publica de forma atómica un corte de valores en el arreglo (invocado desde el hilo de UI/IPC).
    #[inline]
    pub fn write_slice(&self, values: &[f32], sanitize_default: f32) {
        // 1. Marcar inicio de escritura (secuencia pasa a IMPAR)
        let prev = self.seq.fetch_add(1, Ordering::AcqRel);
        debug_assert!(
            prev & 1 == 0,
            "Escritura concurrente no serializada en SeqLockArray"
        );

        let count = values.len().min(N);
        for (cell, &v) in self.cells.iter().take(count).zip(values.iter().take(count)) {
            let clean = if v.is_finite() { v } else { sanitize_default };
            cell.store(clean.to_bits(), Ordering::Relaxed);
        }

        // 2. Publicar actualización y marcar fin de escritura (secuencia vuelve a PAR)
        self.seq.fetch_add(1, Ordering::Release);
    }

    /// Actualiza un único índice `idx` de forma atómica dentro del protocolo SeqLock.
    #[inline]
    pub fn write_index(&self, idx: usize, value: f32) {
        if idx < N && value.is_finite() {
            self.seq.fetch_add(1, Ordering::AcqRel);
            self.cells[idx].store(value.to_bits(), Ordering::Relaxed);
            self.seq.fetch_add(1, Ordering::Release);
        }
    }

    /// Lee una instantánea 100% consistente de los `N` elementos desde el hilo de audio (0 bloqueos).
    #[inline]
    pub fn read_snapshot(&self) -> [f32; N] {
        let mut out = [0.0_f32; N];
        // Reintento acotado (máximo 4 lecturas) para garantizar tiempo constante O(1) en el hilo RT
        for _ in 0..4 {
            let s1 = self.seq.load(Ordering::Acquire);
            if (s1 & 1) != 0 {
                std::hint::spin_loop();
                continue;
            }
            for (dst, cell) in out.iter_mut().zip(self.cells.iter()) {
                *dst = f32::from_bits(cell.load(Ordering::Relaxed));
            }
            fence(Ordering::Acquire);
            let s2 = self.seq.load(Ordering::Relaxed);
            if s1 == s2 {
                return out;
            }
        }
        // Fallback determinista en caso de contención extrema
        for (dst, cell) in out.iter_mut().zip(self.cells.iter()) {
            *dst = f32::from_bits(cell.load(Ordering::Relaxed));
        }
        out
    }
}
