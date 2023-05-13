#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod emulator;
mod keyboard;
mod screen;
mod timers;

use emulator::*;
use screen::*;

pub struct ProgramOptions {
    schip_compatibility: bool,
    clip_sprites: bool,
    clock_speed: u16,
    program: Vec<u8>,
}

fn process_args(args: &Vec<String>) -> Option<ProgramOptions> {
    if args.is_empty() {
        return None;
    }

    let mut program = vec![];
    let mut schip_compatibility = false;
    let mut clip_sprites = false;
    let mut clock_speed = 0;

    // skip processing command line argument if it was the value of the previously processed flag
    let mut flag_argument = false;

    for (i, arg) in args.iter().enumerate().skip(1) {
        if arg.starts_with('-') && !flag_argument {
            let res = std::fs::read(arg);
            // only argument not requiring flag
            program = res.ok()?;
        } else {
            flag_argument = false;
            match &arg[..] {
                "--clip-sprites" | "-K" => clip_sprites = true,
                "--schip-opcodes" | "-S" => schip_compatibility = true,
                "--clock" | "-C" => {
                    if args.len() > i {
                        let val = &args[i + 1];
                        let speed = val.parse::<u16>().ok();
                        clock_speed = speed?;
                        flag_argument = true;
                    } else {
                        return None;
                    }
                }
                _ => {}
            }
        }
    }

    if program.is_empty() {
        return None;
    }

    if clock_speed == 0 {
        clock_speed = DEFAULT_CLOCK_SPEED;
    }

    Some(ProgramOptions {
        schip_compatibility,
        clip_sprites,
        clock_speed,
        program,
    })
}

fn main() -> ggez::GameResult {
    let args: Vec<String> = std::env::args().collect();

    let parsed = process_args(&args);

    if parsed.is_none() {
        println!("ERROR: Invalid arguments!");
        return Ok(());
    }

    let parsed = parsed.unwrap();

    let window_mode = ggez::conf::WindowMode {
        width: (chip_8_core::SCREEN_WIDTH * SCREEN_SCALE_FACTOR) as f32,
        height: (chip_8_core::SCREEN_HEIGHT * SCREEN_SCALE_FACTOR) as f32,
        maximized: false,
        fullscreen_type: ggez::conf::FullscreenType::Windowed,
        borderless: false,
        min_width: 1.0,
        max_width: 0.0,
        min_height: 1.0,
        max_height: 0.0,
        resizable: false,
        visible: true,
        transparent: false,
        resize_on_scale_factor_change: false,
        logical_size: None,
    };

    let window_setup = ggez::conf::WindowSetup {
        title: String::from("Chip-8 Emulator"),
        samples: ggez::conf::NumSamples::One,
        vsync: true,
        icon: String::new(), // TODO
        srgb: false,
    };

    let (ctx, event_loop) = ggez::ContextBuilder::new("chip-8-emulator", "Stefano Ariotta")
        .window_setup(window_setup)
        .window_mode(window_mode)
        .backend(ggez::conf::Backend::Vulkan)
        .build()?;

    let emulator = Emulator::new(&ctx, &parsed)?;

    ggez::event::run(ctx, event_loop, emulator)
}
