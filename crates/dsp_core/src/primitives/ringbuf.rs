//! Cola circular SPSC (Single-Producer Single-Consumer) lock-free prealocada en pila/estructura.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Cola circular SPSC sin asignaciones dinámicas para transferencia de muestras o telemetría.
#[repr(C, align(64))]
pub struct SpscRingBuffer<const N: usize> {
    write_idx: AtomicUsize,
    read_idx: AtomicUsize,
    buffer: [std::sync::atomic::AtomicU32; N],
}

impl<const N: usize> SpscRingBuffer<N> {
    /// Crea un nuevo buffer circular con capacidad `N`.
    pub fn new() -> Self {
        Self {
            write_idx: AtomicUsize::new(0),
            read_idx: AtomicUsize::new(0),
            buffer: std::array::from_fn(|_| std::sync::atomic::AtomicU32::new(0)),
        }
    }

    /// Inserta una muestra si hay espacio disponible. Retorna `false` si está lleno.
    #[inline(always)]
    pub fn push(&self, sample: f32) -> bool {
        let w = self.write_idx.load(Ordering::Relaxed);
        let r = self.read_idx.load(Ordering::Acquire);
        let next_w = (w + 1) % N;
        if next_w == r {
            return false;
        }
        self.buffer[w].store(sample.to_bits(), Ordering::Relaxed);
        self.write_idx.store(next_w, Ordering::Release);
        true
    }

    /// Extrae la siguiente muestra disponible, o `None` si está vacío.
    #[inline(always)]
    pub fn pop(&self) -> Option<f32> {
        let r = self.read_idx.load(Ordering::Relaxed);
        let w = self.write_idx.load(Ordering::Acquire);
        if r == w {
            return None;
        }
        let val = f32::from_bits(self.buffer[r].load(Ordering::Relaxed));
        self.read_idx.store((r + 1) % N, Ordering::Release);
        Some(val)
    }
}

impl<const N: usize> Default for SpscRingBuffer<N> {
    fn default() -> Self {
        Self::new()
    }
}
