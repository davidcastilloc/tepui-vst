//! Decodificador y codificador RIFF/WAVE nativo sin dependencias externas.
//!
//! Soporta lectura de archivos WAV PCM (8, 16, 24 y 32 bits) e IEEE Float (32 bits),
//! mono o multicanal, garantizando sanitización numérica y *sentinel padding* en [`SampleBuffer`].

use crate::buffer::SampleBuffer;

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// Decodifica un contenedor RIFF/WAVE en memoria hacia un [`SampleBuffer`] listo para interpolación Hermite.
///
/// # Errors
/// Retorna `Err(String)` si el encabezado no es un RIFF/WAVE válido, si el formato no está soportado
/// o si el archivo no contiene muestras audibles.
pub fn decode_wav(bytes: &[u8]) -> Result<SampleBuffer, String> {
    if bytes.len() < 44 {
        return Err("archivo WAV demasiado corto (< 44 bytes)".to_string());
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("encabezado RIFF/WAVE inválido".to_string());
    }

    let mut pos = 12usize;
    let mut format_tag: u16 = 0;
    let mut channels: u16 = 0;
    let mut sample_rate: u32 = 44100;
    let mut bits_per_sample: u16 = 0;
    let mut root_note: u8 = 60;
    let mut data_slice: Option<&[u8]> = None;

    while pos + 8 <= bytes.len() {
        let chunk_id = &bytes[pos..pos + 4];
        let chunk_size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let chunk_start = pos + 8;
        let chunk_end = chunk_start.saturating_add(chunk_size).min(bytes.len());

        match chunk_id {
            b"fmt " => {
                if chunk_end - chunk_start < 16 {
                    return Err("chunk fmt truncado".to_string());
                }
                let fmt = &bytes[chunk_start..chunk_end];
                let raw_tag = u16::from_le_bytes([fmt[0], fmt[1]]);
                channels = u16::from_le_bytes([fmt[2], fmt[3]]);
                sample_rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
                bits_per_sample = u16::from_le_bytes([fmt[14], fmt[15]]);

                format_tag = if raw_tag == WAVE_FORMAT_EXTENSIBLE && fmt.len() >= 26 {
                    u16::from_le_bytes([fmt[24], fmt[25]])
                } else {
                    raw_tag
                };
            }
            b"smpl" => {
                // El chunk 'smpl' almacena midi_unity_note en los bytes 12..16 del payload
                if chunk_end - chunk_start >= 16 {
                    let smpl = &bytes[chunk_start..chunk_end];
                    let unity_note = u32::from_le_bytes([smpl[12], smpl[13], smpl[14], smpl[15]]);
                    if unity_note <= 127 {
                        root_note = unity_note as u8;
                    }
                }
            }
            b"data" => {
                data_slice = Some(&bytes[chunk_start..chunk_end]);
            }
            _ => {}
        }

        // Los chunks RIFF están alineados a frontera par (word-aligned)
        let padded_size = chunk_size + (chunk_size & 1);
        pos = chunk_start.saturating_add(padded_size);
    }

    if channels == 0 || sample_rate == 0 {
        return Err("chunk fmt ausente o con canales/sample_rate en cero".to_string());
    }
    let data = data_slice.ok_or_else(|| "chunk data ausente en archivo WAV".to_string())?;
    let bytes_per_sample = (bits_per_sample / 8) as usize;
    if bytes_per_sample == 0 {
        return Err(format!("bits_per_sample no soportado: {bits_per_sample}"));
    }

    let frame_stride = bytes_per_sample * (channels as usize);
    if frame_stride == 0 || data.len() < frame_stride {
        return Err("el archivo WAV no contiene frames de audio completos".to_string());
    }

    let num_frames = data.len() / frame_stride;
    let mut left = Vec::with_capacity(num_frames + 2);
    let mut right = Vec::with_capacity(num_frames + 2);

    for frame_idx in 0..num_frames {
        let base = frame_idx * frame_stride;
        let l = decode_sample(
            &data[base..base + bytes_per_sample],
            format_tag,
            bits_per_sample,
        )?;
        let r = if channels >= 2 {
            let r_base = base + bytes_per_sample;
            decode_sample(
                &data[r_base..r_base + bytes_per_sample],
                format_tag,
                bits_per_sample,
            )?
        } else {
            l
        };
        left.push(sanitize_sample(l));
        right.push(sanitize_sample(r));
    }

    let mut buf = SampleBuffer::new_stereo(left, right, sample_rate as f32, root_note);
    buf.name = "Imported WAV".to_string();
    Ok(buf)
}

#[inline]
fn sanitize_sample(val: f32) -> f32 {
    if val.is_finite() {
        val.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

#[inline]
fn decode_sample(raw: &[u8], format_tag: u16, bits_per_sample: u16) -> Result<f32, String> {
    match (format_tag, bits_per_sample) {
        (WAVE_FORMAT_PCM, 8) => {
            // 8-bit WAV es unsigned [0..255], centro en 128
            let u = raw[0] as f32;
            Ok((u - 128.0) / 128.0)
        }
        (WAVE_FORMAT_PCM, 16) => {
            let v = i16::from_le_bytes([raw[0], raw[1]]);
            Ok(v as f32 / 32768.0)
        }
        (WAVE_FORMAT_PCM, 24) => {
            let b0 = raw[0] as i32;
            let b1 = (raw[1] as i32) << 8;
            let b2 = (raw[2] as i8 as i32) << 16;
            let v = b0 | b1 | b2;
            Ok(v as f32 / 8_388_608.0)
        }
        (WAVE_FORMAT_PCM, 32) => {
            let v = i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
            Ok((v as f64 / 2_147_483_648.0) as f32)
        }
        (WAVE_FORMAT_IEEE_FLOAT, 32) => {
            let v = f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
            Ok(v)
        }
        _ => Err(format!(
            "formato WAV no soportado (tag={format_tag}, bits={bits_per_sample})"
        )),
    }
}

/// Codifica un [`SampleBuffer`] hacia un contenedor RIFF/WAVE estéreo de 32-bit IEEE Float.
pub fn encode_wav_f32(buffer: &SampleBuffer) -> Vec<u8> {
    let num_frames = buffer.len();
    let channels: u16 = 2;
    let sample_rate = buffer.sample_rate.round().max(1.0) as u32;
    let bits_per_sample: u16 = 32;
    let block_align: u16 = channels * (bits_per_sample / 8);
    let byte_rate: u32 = sample_rate * (block_align as u32);
    let data_size: u32 = (num_frames as u32) * (block_align as u32);
    let riff_size: u32 = 36 + data_size;

    let mut out = Vec::with_capacity(44 + data_size as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");

    // Chunk 'fmt '
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&WAVE_FORMAT_IEEE_FLOAT.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());

    // Chunk 'data'
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());

    for i in 0..num_frames {
        let l = sanitize_sample(buffer.get_sample(0, i));
        let r = sanitize_sample(buffer.get_sample(1, i));
        out.extend_from_slice(&l.to_le_bytes());
        out.extend_from_slice(&r.to_le_bytes());
    }

    out
}

/// Codifica un [`SampleBuffer`] hacia un contenedor RIFF/WAVE estéreo de 16-bit PCM estándar.
pub fn encode_wav_pcm16(buffer: &SampleBuffer) -> Vec<u8> {
    let num_frames = buffer.len();
    let channels: u16 = 2;
    let sample_rate = buffer.sample_rate.round().max(1.0) as u32;
    let bits_per_sample: u16 = 16;
    let block_align: u16 = channels * (bits_per_sample / 8);
    let byte_rate: u32 = sample_rate * (block_align as u32);
    let data_size: u32 = (num_frames as u32) * (block_align as u32);
    let riff_size: u32 = 36 + data_size;

    let mut out = Vec::with_capacity(44 + data_size as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");

    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&WAVE_FORMAT_PCM.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());

    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());

    for i in 0..num_frames {
        let l = (sanitize_sample(buffer.get_sample(0, i)) * 32767.0).round() as i16;
        let r = (sanitize_sample(buffer.get_sample(1, i)) * 32767.0).round() as i16;
        out.extend_from_slice(&l.to_le_bytes());
        out.extend_from_slice(&r.to_le_bytes());
    }

    out
}

const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Codifica bytes arbitrarios (ej. archivo WAV) en Base64 estándar (RFC 4648) sin dependencias externas.
pub fn base64_encode(input: &[u8]) -> String {
    if input.is_empty() {
        return String::new();
    }
    let mut out = Vec::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(B64_CHARS[((triple >> 18) & 0x3F) as usize]);
        out.push(B64_CHARS[((triple >> 12) & 0x3F) as usize]);
        if chunk.len() > 1 {
            out.push(B64_CHARS[((triple >> 6) & 0x3F) as usize]);
        } else {
            out.push(b'=');
        }
        if chunk.len() > 2 {
            out.push(B64_CHARS[(triple & 0x3F) as usize]);
        } else {
            out.push(b'=');
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Decodifica una cadena Base64 estándar (RFC 4648) hacia bytes binarios.
pub fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    let clean: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if clean.is_empty() {
        return Ok(Vec::new());
    }
    if !clean.len().is_multiple_of(4) {
        return Err("Invalid Base64 length".to_string());
    }

    let decode_val = |c: u8| -> Result<u32, String> {
        match c {
            b'A'..=b'Z' => Ok((c - b'A') as u32),
            b'a'..=b'z' => Ok((c - b'a' + 26) as u32),
            b'0'..=b'9' => Ok((c - b'0' + 52) as u32),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(format!("Invalid Base64 byte: {}", c)),
        }
    };

    let mut out = Vec::with_capacity((clean.len() / 4) * 3);
    for chunk in clean.chunks_exact(4) {
        let v0 = decode_val(chunk[0])?;
        let v1 = decode_val(chunk[1])?;
        let v2 = if chunk[2] == b'=' {
            0
        } else {
            decode_val(chunk[2])?
        };
        let v3 = if chunk[3] == b'=' {
            0
        } else {
            decode_val(chunk[3])?
        };

        let triple = (v0 << 18) | (v1 << 12) | (v2 << 6) | v3;
        out.push(((triple >> 16) & 0xFF) as u8);
        if chunk[2] != b'=' {
            out.push(((triple >> 8) & 0xFF) as u8);
        }
        if chunk[3] != b'=' {
            out.push((triple & 0xFF) as u8);
        }
    }
    Ok(out)
}
