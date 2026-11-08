#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use studio_bus_comp::{set_standalone_mode, StudioBusCompPlugin};

fn main() {
    set_standalone_mode(true);
    let mut args: Vec<String> = std::env::args().collect();

    if cfg!(windows) && !args.iter().any(|a| a == "-b" || a == "--backend") {
        args.push("-b".to_string());
        args.push("wasapi".to_string());
    }
    if !args.iter().any(|a| a == "-l" || a == "--audio-layout") {
        // Layout 2 (0 inputs, 2 outputs) evita que WASAPI falle cuando no hay micrófono/línea de entrada activa
        args.push("-l".to_string());
        args.push("2".to_string());
    }
    if cfg!(windows) && !args.iter().any(|a| a == "-p" || a == "--period-size") {
        args.push("-p".to_string());
        args.push("1056".to_string());
    }

    nih_plug::wrapper::standalone::nih_export_standalone_with_args::<StudioBusCompPlugin, _>(args);
}
