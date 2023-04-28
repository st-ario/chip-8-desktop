#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod screen;
mod timers;

use chip_8_core::{Chip8, FrameBuffer, IOCallbacks};
use ggez::audio::SoundSource;
use screen::*;
use std::pin::Pin;
use std::sync::Arc;
use timers::*;

struct Emulator<'a> {
    _pin: std::marker::PhantomPinned, // self-referential
    sleeper: spin_sleep::SpinSleeper,
    screen: Screen,
    core: Chip8<'a>,
    // dyn Fn(...) is !Unpin
    time_setter: Pin<Box<dyn Fn(u8) + 'a>>,
    time_getter: Pin<Box<dyn Fn() -> u8 + 'a>>,
    sound_setter: Pin<Box<dyn Fn(u8) + 'a>>,
    next_rand: Pin<Box<dyn Fn() -> u8 + 'a>>,
}

impl<'a> Emulator<'a> {
    fn pin_get_screen(self: Pin<&Self>) -> &Screen {
        &self.get_ref().screen
    }

    fn pin_get_framebuffer(self: Pin<&Self>) -> &FrameBuffer {
        self.get_ref().core.fb_ref()
    }

    fn execute_next_instruction(self: Pin<&mut Self>) {
        /* Safety: execute_next_instruction() doesn't move data out of core */
        unsafe { self.get_unchecked_mut().core.execute_next_instruction() }
    }

    fn new(ctx: &ggez::Context, program: &[u8]) -> ggez::GameResult<Pin<Box<Self>>> {
        let screen = Screen::new(ctx)?;

        /* create system sound */
        let waveform = std::include_bytes!("../resources/sound.ogg");
        let sound_data = ggez::audio::SoundData::from_bytes(waveform);
        let mut sound = ggez::audio::Source::from_data(ctx, sound_data)?;
        sound.set_repeat(true);
        sound.play_later()?; // seems there's no way to initialize the playback in a paused state
        sound.pause();

        /* timers generation and initialization */
        let sound_timer = Arc::new(SoundTimer::new(sound));
        let delay_timer = Arc::new(DelayTimer::new());
        let st = Arc::clone(&sound_timer);
        let dt = Arc::clone(&delay_timer);
        std::thread::spawn(move || st.start());
        std::thread::spawn(move || dt.start());

        let callbacks = IOCallbacks {
            sound_setter: &|_x| {},
            time_setter: &|_x| {},
            time_getter: &|| 0,
            is_pressed: &|_x| false,
            wait_for_key: &|| 0,
            rng: &|| 0,
        };

        let st = Arc::clone(&sound_timer);
        let dt1 = Arc::clone(&delay_timer);
        let dt2 = Arc::clone(&delay_timer);

        /* generate and immediately discard random u8 to initialize the thread-local PRNG state,
         * so that the first call to `random()` during emulation is not slower than subsequent ones
         * (rand::ThreadRng is lazily initialized); if `black_box()` is ignored by the compiler
         * the very first call to `random()` that is actually used will be slower
         */
        let _ = std::hint::black_box(rand::random::<u8>());

        let mut res = Box::new(Self {
            _pin: std::marker::PhantomPinned::default(),
            sleeper: spin_sleep::SpinSleeper::default(),
            screen,
            sound_setter: Box::pin(move |x| st.set(x)),
            time_setter: Box::pin(move |x| dt1.set(x)),
            time_getter: Box::pin(move || dt2.get()),
            next_rand: Box::pin(rand::random::<u8>),
            core: chip_8_core::Chip8::new(&[], callbacks),
        });

        /* Safety:
         * Lifetime: we are pointing to members of ref to construct `core`, another member of ref;
         * neither the closures nor `core` can be invalidated after construction (we return a pinned
         * emulator without mutable projections to either).
         *
         * Address stability: currently (rustc 1.68.2) the T in Pin<Box<T>> doesn't get marked as
         * `noalias` in the LLVM representation (if T is !Unpin); therefore, relying on address
         * stability for Pin<Box<T>> is correct as long as this keeps being the case (and it's
         * currently done in many crates with self-referential structs), but until the aliasing
         * rules aren't standardised, this is technically UB.
         *
         * https://github.com/rust-lang/unsafe-code-guidelines/issues/326
         * https://github.com/rust-lang/unsafe-code-guidelines/issues/148
         */
        let rng = unsafe { &*(res.next_rand.as_ref().get_ref() as *const dyn Fn() -> u8) };
        let sound_setter = unsafe { &*(res.sound_setter.as_ref().get_ref() as *const dyn Fn(u8)) };
        let time_setter = unsafe { &*(res.time_setter.as_ref().get_ref() as *const dyn Fn(u8)) };
        let time_getter =
            unsafe { &*(res.time_getter.as_ref().get_ref() as *const dyn Fn() -> u8) };

        let callbacks = IOCallbacks {
            sound_setter,
            time_setter,
            time_getter,
            is_pressed: &|_x| false,
            wait_for_key: &|| 0,
            rng,
        };

        res.core = chip_8_core::Chip8::new(program, callbacks);

        Ok(Box::into_pin(res))
    }
}

impl<'a> ggez::event::EventHandler<ggez::GameError> for Pin<Box<Emulator<'a>>> {
    fn update(&mut self, _ctx: &mut ggez::Context) -> ggez::GameResult {
        use std::time::{Duration, SystemTime};

        const TARGET_EMULATION_CLOCK_SPEED: Duration = Duration::from_millis(2);
        const TARGET_ACCURACY: u64 = TARGET_EMULATION_CLOCK_SPEED.subsec_nanos() as u64 / 4;
        const INSTRUCTIONS_PER_TICK: u32 = 5;
        const TIME_BUDGET: Duration =
            TARGET_EMULATION_CLOCK_SPEED.saturating_mul(INSTRUCTIONS_PER_TICK);

        let now = SystemTime::now();

        let mut i: u32 = 0;
        while i < INSTRUCTIONS_PER_TICK {
            self.as_mut().execute_next_instruction();
            i += 1;
        }

        let delta = match now.elapsed() {
            Ok(t) => t,
            Err(e) => return Err(ggez::GameError::CustomError(e.to_string())),
        };

        let diff = TIME_BUDGET.saturating_sub(delta);
        if diff > Duration::from_nanos(TARGET_ACCURACY) {
            self.sleeper.sleep_ns(diff.subsec_nanos() as u64);
        }

        Ok(())
    }

    fn draw(&mut self, ctx: &mut ggez::Context) -> ggez::GameResult {
        self.as_ref()
            .pin_get_screen()
            .draw(ctx, self.as_ref().pin_get_framebuffer())
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
