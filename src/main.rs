#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod emulator;
mod keyboard;
mod screen;
mod timers;

use emulator::*;
use screen::*;
use std::pin::Pin;

impl<'a> ggez::event::EventHandler<ggez::GameError> for Pin<Box<Emulator<'a>>> {
    fn update(&mut self, _ctx: &mut ggez::Context) -> ggez::GameResult {
        self.as_mut().update()
    }

    fn key_down_event(
        &mut self,
        _ctx: &mut ggez::Context,
        input: ggez::input::keyboard::KeyInput,
        _repeated: bool,
    ) -> Result<(), ggez::GameError> {
        self.as_mut().key_down_event(input)
    }

    fn key_up_event(
        &mut self,
        _ctx: &mut ggez::Context,
        input: ggez::input::keyboard::KeyInput,
    ) -> Result<(), ggez::GameError> {
        self.as_mut().key_up_event(input)
    }

    fn draw(&mut self, ctx: &mut ggez::Context) -> ggez::GameResult {
        self.as_ref().draw(ctx)
    }
}

fn main() -> ggez::GameResult {
    let args: Vec<String> = std::env::args().collect();

    let filepath = &args[1];

    let program = std::fs::read(filepath).expect("Unable to read file");

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
        vsync: false,
        icon: String::new(), // TODO
        srgb: false,
    };

    let (ctx, event_loop) = ggez::ContextBuilder::new("chip-8-emulator", "Stefano Ariotta")
        .window_setup(window_setup)
        .window_mode(window_mode)
        .backend(ggez::conf::Backend::Vulkan)
        .build()?;

    let emulator = Emulator::new(&ctx, &program)?;

    ggez::event::run(ctx, event_loop, emulator)
}
