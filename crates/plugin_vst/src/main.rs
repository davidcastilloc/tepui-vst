use cpal::traits::{DeviceTrait, HostTrait};
use plugin_vst::audio_devices;
use plugin_vst::AiSamplerPlugin;

fn main() {
    // 0. Indicar que estamos corriendo en modo Standalone
    audio_devices::set_standalone_mode(true);

    let mut args: Vec<String> = std::env::args().collect();
    let saved_config = audio_devices::load_config();

    // 1. Diagnóstico y selección de dispositivo de salida de audio
    let host = cpal::default_host();
    let default_name = host.default_output_device().and_then(|d| d.name().ok());
    println!("[TEPUI AUDIO] Dispositivo de salida por defecto del sistema: {:?}", default_name);

    let mut available_output_names = Vec::new();
    if let Ok(devices) = host.output_devices() {
        println!("[TEPUI AUDIO] Dispositivos de audio disponibles en el sistema:");
        for dev in devices {
            if let Ok(name) = dev.name() {
                println!("  - {}", name);
                available_output_names.push(name);
            }
        }
    }

    let has_output_arg = args.iter().any(|a| a == "--output-device");
    if !has_output_arg {
        if let Some(ref saved_dev) = saved_config.output_device {
            if available_output_names.contains(saved_dev) {
                println!(
                    "[TEPUI AUDIO] Usando dispositivo de salida configurado por el usuario: {}",
                    saved_dev
                );
                args.push("--output-device".to_string());
                args.push(saved_dev.clone());
            } else {
                println!(
                    "[TEPUI AUDIO] Dispositivo guardado {:?} no está disponible actualmente. Evaluando por defecto.",
                    saved_dev
                );
            }
        }
        if !args.iter().any(|a| a == "--output-device") {
            if let Some(ref def_name) = default_name {
                if available_output_names.contains(def_name) {
                    println!(
                        "[TEPUI AUDIO] Usando dispositivo de salida por defecto del sistema (Windows Default): {}",
                        def_name
                    );
                    args.push("--output-device".to_string());
                    args.push(def_name.clone());
                }
            }
        }
    }

    // 2. Detección automática del layout y características del dispositivo
    let output_dev_name = args
        .iter()
        .position(|a| a == "--output-device")
        .and_then(|idx| args.get(idx + 1))
        .cloned()
        .or_else(|| default_name.clone());

    let mut detected_channels = 2u16;
    let mut native_sample_rate = 48000u32;
    let mut supported_rates = Vec::new();

    if let Some(ref dev_name) = output_dev_name {
        if let Ok(devices) = host.output_devices() {
            for dev in devices {
                if let Ok(name) = dev.name() {
                    if &name == dev_name {
                        if let Ok(cfg) = dev.default_output_config() {
                            detected_channels = cfg.channels();
                            native_sample_rate = cfg.sample_rate().0;
                        }
                        if let Ok(configs) = dev.supported_output_configs() {
                            for c in configs {
                                let min_r = c.min_sample_rate().0;
                                let max_r = c.max_sample_rate().0;
                                for &r in &[44100, 48000, 88200, 96000, 192000] {
                                    if r >= min_r && r <= max_r && !supported_rates.contains(&r) {
                                        supported_rates.push(r);
                                    }
                                }
                            }
                        }
                        break;
                    }
                }
            }
        }
    }
    if supported_rates.is_empty() {
        supported_rates.push(native_sample_rate);
    }

    let has_layout_arg = args.iter().any(|a| a == "-l" || a == "--audio-layout");
    if !has_layout_arg {
        let layout_idx = match detected_channels {
            8 => "2", // 0 inputs, 8 outputs (7.1 THX Spatial Audio)
            6 => "3", // 0 inputs, 6 outputs (5.1 Surround)
            1 => "4", // Mono (1 output)
            _ => "1", // Stereo (2 outputs)
        };
        println!(
            "[TEPUI AUDIO] Configurando layout de audio automático: {} ({} canales para {:?})",
            layout_idx, detected_channels, output_dev_name
        );
        args.push("-l".to_string());
        args.push(layout_idx.to_string());
    }

    // 3. Frecuencia de muestreo (Sample Rate) con validación de hardware
    let has_rate_arg = args.iter().any(|a| a == "-r" || a == "--sample-rate");
    if !has_rate_arg {
        let target_rate = if supported_rates.contains(&saved_config.sample_rate) {
            saved_config.sample_rate
        } else {
            println!(
                "[TEPUI AUDIO] Frecuencia guardada ({} Hz) no soportada por el dispositivo. Usando nativa: {} Hz",
                saved_config.sample_rate, native_sample_rate
            );
            native_sample_rate
        };
        println!(
            "[TEPUI AUDIO] Frecuencia de muestreo configurada: {} Hz",
            target_rate
        );
        args.push("-r".to_string());
        args.push(target_rate.to_string());
    }

    // 4. Tamaño de buffer (Period Size) con protección WASAPI
    let has_period_arg = args.iter().any(|a| a == "-p" || a == "--period-size");
    if !has_period_arg {
        // En Windows WASAPI Shared Mode, el driver entrega paquetes en múltiplos fijos (ej. 1056 muestras para Realtek / THX Spatial).
        // nih_plug exige de forma estricta que actual_sample_count <= buffer_size, de lo contrario entra en pánico en cpal.rs:832.
        // Aseguramos un mínimo de 1056 muestras para evitar fallos de audio thread.
        let safe_period = if cfg!(windows) && saved_config.period_size < 1056 {
            println!(
                "[TEPUI AUDIO] Ajustando buffer mínimo seguro a 1056 muestras (solicitado: {}) para evitar pánico de buffer WASAPI",
                saved_config.period_size
            );
            1056
        } else {
            saved_config.period_size
        };
        println!(
            "[TEPUI AUDIO] Tamaño de buffer configurado: {} muestras",
            safe_period
        );
        args.push("-p".to_string());
        args.push(safe_period.to_string());
    }

    // 5. En Windows, si no se especifica backend, preferir WASAPI nativo
    let has_backend_arg = args.iter().any(|a| a == "-b" || a == "--backend");
    if !has_backend_arg {
        args.push("-b".to_string());
        args.push("wasapi".to_string());
    }

    // 7. Dispositivo de entrada de audio
    let has_input_arg = args.iter().any(|a| a == "--input-device");
    if !has_input_arg {
        if let Some(ref saved_in) = saved_config.input_device {
            println!(
                "[TEPUI AUDIO] Usando dispositivo de entrada configurado: {}",
                saved_in
            );
            args.push("--input-device".to_string());
            args.push(saved_in.clone());
        }
    }

    // 8. Dispositivo de entrada MIDI
    let has_midi_arg = args.iter().any(|a| a == "--midi-input");
    if !has_midi_arg {
        if let Some(ref saved_midi) = saved_config.midi_input {
            println!(
                "[TEPUI AUDIO] Usando dispositivo MIDI configurado: {}",
                saved_midi
            );
            args.push("--midi-input".to_string());
            args.push(saved_midi.clone());
        }
    }

    nih_plug::wrapper::standalone::nih_export_standalone_with_args::<AiSamplerPlugin, _>(args);
}
