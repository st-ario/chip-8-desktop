#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod keyboard;
mod screen;
mod timers;

use chip_8_core::{Chip8, FrameBuffer, IOCallbacks};
use ggez::audio::SoundSource;
use keyboard::*;
use screen::*;
use std::pin::Pin;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use timers::*;

struct Emulator<'a> {
    _pin: std::marker::PhantomPinned, // self-referential
    sleeper: spin_sleep::SpinSleeper,
    keyboard_status: [bool; 16],
    keyboard_send_channel: Sender<KeyMessage>,
    screen: Screen,
    core: Chip8<'a>,
    // dyn Fn(...) is !Unpin
    time_setter: Pin<Box<dyn Fn(u8) + 'a>>,
    time_getter: Pin<Box<dyn Fn() -> u8 + 'a>>,
    sound_setter: Pin<Box<dyn Fn(u8) + 'a>>,
    next_rand: Pin<Box<dyn Fn() -> u8 + 'a>>,
    is_pressed: Pin<Box<dyn Fn(u8) -> bool + 'a>>,
    wait_for_key: Pin<Box<dyn Fn() -> u8 + 'a>>,
}

impl<'a> Emulator<'a> {
    fn pin_get_screen(self: Pin<&Self>) -> &Screen {
        &self.get_ref().screen
    }

    fn pin_get_framebuffer(self: Pin<&Self>) -> &FrameBuffer {
        self.get_ref().core.fb_ref()
    }

    // safety: safe to use anywhere, since arrays are Copy
    unsafe fn pin_get_keyboard_status(self: Pin<&mut Self>) -> &mut [bool; 16] {
        &mut self.get_unchecked_mut().keyboard_status
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

        let st = Arc::clone(&sound_timer);
        let dt1 = Arc::clone(&delay_timer);
        let dt2 = Arc::clone(&delay_timer);

        /* generate and immediately discard random u8 to initialize the thread-local PRNG state,
         * so that the first call to `random()` during emulation is not slower than subsequent ones
         * (rand::ThreadRng is lazily initialized); if `black_box()` is ignored by the compiler
         * the very first call to `random()` that is actually used will be slower
         */
        let _ = std::hint::black_box(rand::random::<u8>());

        let callbacks = IOCallbacks {
            sound_setter: &|_x| {},
            time_setter: &|_x| {},
            time_getter: &|| 0,
            is_pressed: &|_x| false,
            wait_for_key: &|| 0,
            rng: &|| 0,
        };

        let (tx, rx): (Sender<KeyMessage>, Receiver<KeyMessage>) = mpsc::channel();

        let keyboard = KeyboardManager::init(rx);
        let kb1 = Arc::clone(&keyboard);
        let kb2 = Arc::clone(&keyboard);

        let mut res = Box::new(Self {
            _pin: std::marker::PhantomPinned::default(),
            sleeper: spin_sleep::SpinSleeper::default(),
            //keyboard: kb,
            keyboard_status: [false; 16],
            keyboard_send_channel: tx,
            screen,
            sound_setter: Box::pin(move |x| st.set(x)),
            time_setter: Box::pin(move |x| dt1.set(x)),
            time_getter: Box::pin(move || dt2.get()),
            next_rand: Box::pin(rand::random::<u8>),
            is_pressed: Box::pin(move |x| kb1.is_pressed(x)),
            wait_for_key: Box::pin(move || kb2.wait_for_key()),
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
        let is_pressed =
            unsafe { &*(res.is_pressed.as_ref().get_ref() as *const dyn Fn(u8) -> bool) };
        let wait_for_key =
            unsafe { &*(res.wait_for_key.as_ref().get_ref() as *const dyn Fn() -> u8) };

        let callbacks = IOCallbacks {
            sound_setter,
            time_setter,
            time_getter,
            is_pressed,
            wait_for_key,
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
        const INSTRUCTIONS_PER_TICK: u32 = 4;
        const TIME_BUDGET: u64 = TARGET_EMULATION_CLOCK_SPEED
            .saturating_mul(INSTRUCTIONS_PER_TICK)
            .subsec_nanos() as u64;

        let now = SystemTime::now();

        let mut i: u32 = 0;
        while i < INSTRUCTIONS_PER_TICK {
            self.as_mut().execute_next_instruction();
            i += 1;
        }

        let elapsed = now.elapsed().unwrap().subsec_nanos() as u64;
        let diff = TIME_BUDGET - elapsed;

        if diff > TARGET_ACCURACY {
            self.sleeper.sleep_ns(diff);
        }

        Ok(())
    }

    /* hopefully events are not managed in the main thread */

    fn key_down_event(
        &mut self,
        _ctx: &mut ggez::Context,
        input: ggez::input::keyboard::KeyInput,
        _repeated: bool,
    ) -> Result<(), ggez::GameError> {
        // safety: pin_get_keyboard_status() is actually safe
        #[rustfmt::skip]
        let keycode: u8 = match input.scancode {
            0x2D => unsafe { if !self.keyboard_status[0x0] {self.as_mut().pin_get_keyboard_status()[0x0] = true; 0x0} else { return Ok(()) } },
            0x02 => unsafe { if !self.keyboard_status[0x1] {self.as_mut().pin_get_keyboard_status()[0x1] = true; 0x1} else { return Ok(()) } },
            0x03 => unsafe { if !self.keyboard_status[0x2] {self.as_mut().pin_get_keyboard_status()[0x2] = true; 0x2} else { return Ok(()) } },
            0x04 => unsafe { if !self.keyboard_status[0x3] {self.as_mut().pin_get_keyboard_status()[0x3] = true; 0x3} else { return Ok(()) } },
            0x10 => unsafe { if !self.keyboard_status[0x4] {self.as_mut().pin_get_keyboard_status()[0x4] = true; 0x4} else { return Ok(()) } },
            0x11 => unsafe { if !self.keyboard_status[0x5] {self.as_mut().pin_get_keyboard_status()[0x5] = true; 0x5} else { return Ok(()) } },
            0x12 => unsafe { if !self.keyboard_status[0x6] {self.as_mut().pin_get_keyboard_status()[0x6] = true; 0x6} else { return Ok(()) } },
            0x1E => unsafe { if !self.keyboard_status[0x7] {self.as_mut().pin_get_keyboard_status()[0x7] = true; 0x7} else { return Ok(()) } },
            0x1F => unsafe { if !self.keyboard_status[0x8] {self.as_mut().pin_get_keyboard_status()[0x8] = true; 0x8} else { return Ok(()) } },
            0x20 => unsafe { if !self.keyboard_status[0x9] {self.as_mut().pin_get_keyboard_status()[0x9] = true; 0x9} else { return Ok(()) } },
            0x2C => unsafe { if !self.keyboard_status[0xA] {self.as_mut().pin_get_keyboard_status()[0xA] = true; 0xA} else { return Ok(()) } },
            0x2E => unsafe { if !self.keyboard_status[0xB] {self.as_mut().pin_get_keyboard_status()[0xB] = true; 0xB} else { return Ok(()) } },
            0x05 => unsafe { if !self.keyboard_status[0xC] {self.as_mut().pin_get_keyboard_status()[0xC] = true; 0xC} else { return Ok(()) } },
            0x13 => unsafe { if !self.keyboard_status[0xD] {self.as_mut().pin_get_keyboard_status()[0xD] = true; 0xD} else { return Ok(()) } },
            0x21 => unsafe { if !self.keyboard_status[0xE] {self.as_mut().pin_get_keyboard_status()[0xE] = true; 0xE} else { return Ok(()) } },
            0x2F => unsafe { if !self.keyboard_status[0xF] {self.as_mut().pin_get_keyboard_status()[0xF] = true; 0xF} else { return Ok(()) } },
            _ => return Ok(()),
        };

        let err = self
            .keyboard_send_channel
            .send((keycode, KeyAction::Pressed));

        if err.is_err() {
            return Err(ggez::GameError::CustomError(String::from(
                "Error while trying to communicate with keyboard thread",
            )));
        }

        Ok(())
    }

    fn key_up_event(
        &mut self,
        _ctx: &mut ggez::Context,
        input: ggez::input::keyboard::KeyInput,
    ) -> Result<(), ggez::GameError> {
        // safety: pin_get_keyboard_status() is actually safe
        #[rustfmt::skip]
        let keycode: u8 = match input.scancode {
            0x2D => unsafe { self.as_mut().pin_get_keyboard_status()[0x0] = false; 0x0 },
            0x02 => unsafe { self.as_mut().pin_get_keyboard_status()[0x1] = false; 0x1 },
            0x03 => unsafe { self.as_mut().pin_get_keyboard_status()[0x2] = false; 0x2 },
            0x04 => unsafe { self.as_mut().pin_get_keyboard_status()[0x3] = false; 0x3 },
            0x10 => unsafe { self.as_mut().pin_get_keyboard_status()[0x4] = false; 0x4 },
            0x11 => unsafe { self.as_mut().pin_get_keyboard_status()[0x5] = false; 0x5 },
            0x12 => unsafe { self.as_mut().pin_get_keyboard_status()[0x6] = false; 0x6 },
            0x1E => unsafe { self.as_mut().pin_get_keyboard_status()[0x7] = false; 0x7 },
            0x1F => unsafe { self.as_mut().pin_get_keyboard_status()[0x8] = false; 0x8 },
            0x20 => unsafe { self.as_mut().pin_get_keyboard_status()[0x9] = false; 0x9 },
            0x2C => unsafe { self.as_mut().pin_get_keyboard_status()[0xA] = false; 0xA },
            0x2E => unsafe { self.as_mut().pin_get_keyboard_status()[0xB] = false; 0xB },
            0x05 => unsafe { self.as_mut().pin_get_keyboard_status()[0xC] = false; 0xC },
            0x13 => unsafe { self.as_mut().pin_get_keyboard_status()[0xD] = false; 0xD },
            0x21 => unsafe { self.as_mut().pin_get_keyboard_status()[0xE] = false; 0xE },
            0x2F => unsafe { self.as_mut().pin_get_keyboard_status()[0xF] = false; 0xF },
            _ => return Ok(()),
        };

        let err = self
            .keyboard_send_channel
            .send((keycode, KeyAction::Released));

        if err.is_err() {
            return Err(ggez::GameError::CustomError(String::from(
                "Error while trying to communicate with keyboard thread",
            )));
        }

        Ok(())
        /* Needed on Linux?
        0x82 => Some(0x1),
        0x83 => Some(0x2),
        0x84 => Some(0x3),
        0x85 => Some(0xC),
        0x90 => Some(0x4),
        0x91 => Some(0x5),
        0x92 => Some(0x6),
        0x93 => Some(0xD),
        0x9E => Some(0x7),
        0x9F => Some(0x8),
        0xA0 => Some(0x9),
        0xA1 => Some(0xE),
        0xAC => Some(0xA),
        0xAD => Some(0x0),
        0xAE => Some(0xB),
        0xAF => Some(0xF),
        _ => None, */
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
