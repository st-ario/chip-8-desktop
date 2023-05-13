use crate::keyboard::*;
use crate::screen::*;
use crate::timers::*;
use crate::ProgramOptions;
use chip_8_core::{Chip8, IOCallbacks};
use ggez::audio::SoundSource;
use ggez::input::keyboard;
use std::pin::Pin;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

pub const DEFAULT_CLOCK_SPEED: u16 = 500;

pub struct Emulator {
    internals: Pin<Arc<EmulatorInternals>>,
    sleeper: spin_sleep::SpinSleeper,
    keyboard_status: [bool; 16],
    update_sync_pair: Arc<(Condvar, Mutex<State>)>,
    esp: EmulationSpeedParams,
}

/* state machine to handle waiting on a keypress */
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum State {
    #[default]
    Ready,
    UpdateRequested,
    WaitingForKey,
}

struct EmulationSpeedParams {
    instructions_per_tick: u64,
    time_budget_ns: u64,
    target_accuracy_ns: u64,
}

impl EmulationSpeedParams {
    fn new(clock_speed: u16) -> Self {
        let target_clock_ns: u64 = (1_000_000_000.0 / clock_speed as f64) as u64;

        /* multiple instructions per tick, to reduce jittering */
        // if the nubmer is too high, the input lag will become noticeable;
        // input lag should stay below 50 ms
        // as the emulated clock should be in the 400-1000 Hz, 10 instruction per emulator tick
        // should keep the input lag at 10-25 ms + system input lag (negligible)
        let instructions_per_tick: u64 = 10;
        let time_budget_ns: u64 = target_clock_ns * instructions_per_tick;

        /* time-skipping */
        // sleep only if we're ahead of more than 1/ACCURACY_FACTOR of
        // a target-clock-tick (on average, checked per emulator tick)
        // this means that we will skip the sleeping instruction only if the emulator tick took
        // almost as much time as the emulated system would have taken, or longer
        // (highly unlikely on a modern computer)
        let accuracy_factor: u64 = 10;
        let target_accuracy_ns: u64 = instructions_per_tick * target_clock_ns / accuracy_factor;

        Self {
            instructions_per_tick,
            time_budget_ns,
            target_accuracy_ns,
        }
    }
}

impl Emulator {
    pub fn new(ctx: &ggez::Context, options: &ProgramOptions) -> ggez::GameResult<Self> {
        let sync_pair = Arc::new((Condvar::new(), Mutex::new(State::default())));
        let sync_copy = Arc::clone(&sync_pair);

        Ok(Emulator {
            internals: EmulatorInternals::new(ctx, options, sync_copy)?,
            sleeper: spin_sleep::SpinSleeper::default(),
            keyboard_status: [false; 16],
            update_sync_pair: sync_pair,
            esp: EmulationSpeedParams::new(options.clock_speed),
        })
    }
}

impl ggez::event::EventHandler<ggez::GameError> for Emulator {
    fn update(&mut self, _ctx: &mut ggez::Context) -> ggez::GameResult {
        use once_cell::unsync::Lazy;
        use std::time::SystemTime;
        static mut TICK: Lazy<SystemTime> = Lazy::new(SystemTime::now);

        /* time skipping (see EmulationSpeedParams documentation) */
        {
            // safety: update() is called only from one thread, and TICK is scoped to this function
            let elapsed = unsafe { TICK.elapsed().unwrap().subsec_nanos() as u64 };

            // avoiding overflow in `if (TIME_BUDGET - elapsed > TARGET_ACCURACY)`
            if self.esp.time_budget_ns > self.esp.target_accuracy_ns + elapsed {
                self.sleeper.sleep_ns(self.esp.time_budget_ns - elapsed);
            }

            // safety: update() is called only from one thread, and TICK is scoped to this function
            unsafe { *TICK = SystemTime::now() };
        }

        /* game tick begins here */
        // this function is called on the main thread by the ggez runtime, so it can't block for too long;
        // `mtx` is shared only with `execute_next_instruction()`, which acquires it only when it
        // can no longer block
        let mut i: u64 = 0;
        while i < self.esp.instructions_per_tick {
            let (cond, mtx) = self.update_sync_pair.as_ref();

            // signal update request, unless we're still waiting from a previous iteration
            {
                let mut state = mtx.lock().unwrap();

                if *state == State::WaitingForKey {
                    break;
                }

                *state = State::UpdateRequested;
            }
            cond.notify_all();

            // wait for feedback message
            let state;
            {
                let mut feedback = mtx.lock().unwrap();

                while *feedback == State::UpdateRequested {
                    feedback = cond.wait(feedback).unwrap();
                }

                state = *feedback;
            }

            match state {
                State::WaitingForKey => break,
                State::Ready => {}
                State::UpdateRequested => unreachable!(),
            }

            i += 1;
        }

        Ok(())
    }

    fn key_down_event(
        &mut self,
        _ctx: &mut ggez::Context,
        input: keyboard::KeyInput,
        _repeated: bool,
    ) -> Result<(), ggez::GameError> {
        // do not send more than one "pressed" signal if key is held
        #[rustfmt::skip]
        let keycode: u8 = match input.scancode {
            0x2D => { if !self.keyboard_status[0x0] {self.keyboard_status[0x0] = true; 0x0} else { return Ok(()) } },
            0x02 => { if !self.keyboard_status[0x1] {self.keyboard_status[0x1] = true; 0x1} else { return Ok(()) } },
            0x03 => { if !self.keyboard_status[0x2] {self.keyboard_status[0x2] = true; 0x2} else { return Ok(()) } },
            0x04 => { if !self.keyboard_status[0x3] {self.keyboard_status[0x3] = true; 0x3} else { return Ok(()) } },
            0x10 => { if !self.keyboard_status[0x4] {self.keyboard_status[0x4] = true; 0x4} else { return Ok(()) } },
            0x11 => { if !self.keyboard_status[0x5] {self.keyboard_status[0x5] = true; 0x5} else { return Ok(()) } },
            0x12 => { if !self.keyboard_status[0x6] {self.keyboard_status[0x6] = true; 0x6} else { return Ok(()) } },
            0x1E => { if !self.keyboard_status[0x7] {self.keyboard_status[0x7] = true; 0x7} else { return Ok(()) } },
            0x1F => { if !self.keyboard_status[0x8] {self.keyboard_status[0x8] = true; 0x8} else { return Ok(()) } },
            0x20 => { if !self.keyboard_status[0x9] {self.keyboard_status[0x9] = true; 0x9} else { return Ok(()) } },
            0x2C => { if !self.keyboard_status[0xA] {self.keyboard_status[0xA] = true; 0xA} else { return Ok(()) } },
            0x2E => { if !self.keyboard_status[0xB] {self.keyboard_status[0xB] = true; 0xB} else { return Ok(()) } },
            0x05 => { if !self.keyboard_status[0xC] {self.keyboard_status[0xC] = true; 0xC} else { return Ok(()) } },
            0x13 => { if !self.keyboard_status[0xD] {self.keyboard_status[0xD] = true; 0xD} else { return Ok(()) } },
            0x21 => { if !self.keyboard_status[0xE] {self.keyboard_status[0xE] = true; 0xE} else { return Ok(()) } },
            0x2F => { if !self.keyboard_status[0xF] {self.keyboard_status[0xF] = true; 0xF} else { return Ok(()) } },
            _ => return Ok(()),
        };

        self.internals.as_ref().key_down_event(keycode)
    }

    fn key_up_event(
        &mut self,
        _ctx: &mut ggez::Context,
        input: ggez::input::keyboard::KeyInput,
    ) -> Result<(), ggez::GameError> {
        #[rustfmt::skip]
        let keycode: u8 = match input.scancode {
            0x2D => { self.keyboard_status[0x0] = false; 0x0 },
            0x02 => { self.keyboard_status[0x1] = false; 0x1 },
            0x03 => { self.keyboard_status[0x2] = false; 0x2 },
            0x04 => { self.keyboard_status[0x3] = false; 0x3 },
            0x10 => { self.keyboard_status[0x4] = false; 0x4 },
            0x11 => { self.keyboard_status[0x5] = false; 0x5 },
            0x12 => { self.keyboard_status[0x6] = false; 0x6 },
            0x1E => { self.keyboard_status[0x7] = false; 0x7 },
            0x1F => { self.keyboard_status[0x8] = false; 0x8 },
            0x20 => { self.keyboard_status[0x9] = false; 0x9 },
            0x2C => { self.keyboard_status[0xA] = false; 0xA },
            0x2E => { self.keyboard_status[0xB] = false; 0xB },
            0x05 => { self.keyboard_status[0xC] = false; 0xC },
            0x13 => { self.keyboard_status[0xD] = false; 0xD },
            0x21 => { self.keyboard_status[0xE] = false; 0xE },
            0x2F => { self.keyboard_status[0xF] = false; 0xF },
            _ => return Ok(()),
        };

        self.internals.as_ref().key_up_event(keycode)
    }

    fn draw(&mut self, ctx: &mut ggez::Context) -> ggez::GameResult {
        self.internals.as_ref().draw(ctx)
    }
}

struct EmulatorInternals {
    _pin: std::marker::PhantomPinned,                 // self-referential
    keyboard_send_channel: Mutex<Sender<KeyMessage>>, // communicate press/release events
    screen: Screen,
    core: Mutex<Chip8<'static>>,
    update_sync_pair: Arc<(Condvar, Mutex<State>)>,
    // dyn Fn(...) is !Unpin
    time_setter: Pin<Box<dyn Fn(u8) + 'static + Send + Sync>>,
    time_getter: Pin<Box<dyn Fn() -> u8 + 'static + Send + Sync>>,
    sound_setter: Pin<Box<dyn Fn(u8) + 'static + Send + Sync>>,
    next_rand: Pin<Box<dyn Fn() -> u8 + 'static + Send + Sync>>,
    is_pressed: Pin<Box<dyn Fn(u8) -> bool + 'static + Send + Sync>>,
    wait_for_key: Pin<Box<dyn Fn() -> u8 + 'static + Send + Sync>>,
}

impl EmulatorInternals {
    fn new(
        ctx: &ggez::Context,
        options: &ProgramOptions,
        sync_pair: Arc<(Condvar, Mutex<State>)>,
    ) -> ggez::GameResult<Pin<Arc<Self>>> {
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
        let (keyboard, kb_pair) = KeyboardManager::new(rx);
        let kb1 = Arc::clone(&keyboard);
        let pair = Arc::clone(&sync_pair);

        // IMPORTANT: the wait_for_key callback must update the State mutex in the calling thread
        // (i.e. it shouldn't spawn a new thread and modify the State mutex from it)
        let wait_for_key = move || {
            // signal the emulator thread
            let (cond, mtx) = pair.as_ref();
            {
                let mut state = mtx.lock().unwrap();
                *state = State::WaitingForKey;
            }
            cond.notify_all();

            // signal the keyboard thread
            let (kb_cond, kb_mtx) = kb_pair.as_ref();
            {
                let mut kb_state = kb_mtx.lock().unwrap();
                *kb_state = KeyboardState::Waiting;
            }
            kb_cond.notify_all();

            let mut kb_state = kb_mtx.lock().unwrap();
            let res;
            loop {
                kb_state = kb_cond.wait(kb_state).unwrap();
                match *kb_state {
                    KeyboardState::Normal => continue,
                    KeyboardState::Waiting => continue,
                    KeyboardState::PressedWhileWaiting(val) => {
                        *kb_state = KeyboardState::Normal;
                        res = val;
                        break;
                    }
                }
            }
            kb_cond.notify_all();

            res
        };

        let res = Arc::pin(Self {
            _pin: std::marker::PhantomPinned::default(),
            keyboard_send_channel: Mutex::new(tx),
            screen,
            update_sync_pair: sync_pair,
            sound_setter: Box::pin(move |x| st.set(x)),
            time_setter: Box::pin(move |x| dt1.set(x)),
            time_getter: Box::pin(move || dt2.get()),
            next_rand: Box::pin(rand::random::<u8>),
            is_pressed: Box::pin(move |x| kb1.is_pressed(x)),
            wait_for_key: Box::pin(wait_for_key),
            core: Mutex::new(Chip8::new(
                &[],
                callbacks,
                options.clip_sprites,
                options.schip_compatibility,
            )),
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
        let rng = unsafe {
            &*(res.next_rand.as_ref().get_ref() as *const (dyn Fn() -> u8 + Send + Sync))
        };
        let sound_setter =
            unsafe { &*(res.sound_setter.as_ref().get_ref() as *const (dyn Fn(u8) + Send + Sync)) };
        let time_setter =
            unsafe { &*(res.time_setter.as_ref().get_ref() as *const (dyn Fn(u8) + Send + Sync)) };
        let time_getter = unsafe {
            &*(res.time_getter.as_ref().get_ref() as *const (dyn Fn() -> u8 + Send + Sync))
        };
        let is_pressed = unsafe {
            &*(res.is_pressed.as_ref().get_ref() as *const (dyn Fn(u8) -> bool + Send + Sync))
        };
        let wait_for_key = unsafe {
            &*(res.wait_for_key.as_ref().get_ref() as *const (dyn Fn() -> u8 + Send + Sync))
        };

        let callbacks = IOCallbacks {
            sound_setter,
            time_setter,
            time_getter,
            is_pressed,
            wait_for_key,
            rng,
        };

        {
            let mut x = res.core.lock().unwrap();
            let y = &mut *x;
            *y = Chip8::new(
                &options.program[..],
                callbacks,
                options.clip_sprites,
                options.schip_compatibility,
            );
        }

        let temp = res.clone();
        std::thread::spawn(move || {
            let x = temp.as_ref();
            x.start();
        });

        Ok(res)
    }

    fn start(self: Pin<&Self>) {
        let (cond, mtx) = self.update_sync_pair.as_ref();

        /* emulator thread loop */
        loop {
            // wait for next "update" signal
            {
                let mut state = mtx.lock().unwrap();

                // still waiting from a previous iteration?
                if *state == State::WaitingForKey {
                    drop(state);
                    std::thread::yield_now();
                    continue;
                }

                while *state != State::UpdateRequested {
                    state = cond.wait(state).unwrap();
                }
            }

            // will block on `wait_for_key`
            self.execute_next_instruction();
        }
    }

    fn draw(self: Pin<&Self>, ctx: &mut ggez::Context) -> ggez::GameResult {
        let lock = self.core.try_lock();

        // the emulator thread might hold onto `core` if it's waiting on a keypress, in which case
        // we don't have to update the window content
        if lock.is_err() {
            return Ok(());
        }

        let core_mtx = lock.unwrap();
        self.as_ref().pin_get_screen().draw(ctx, core_mtx.fb_ref())
    }

    fn key_down_event(self: Pin<&Self>, keycode: u8) -> Result<(), ggez::GameError> {
        self.keyboard_send_channel
            .lock()
            .unwrap()
            .send((keycode, KeyAction::Pressed))
            .unwrap();

        Ok(())
    }

    fn key_up_event(self: Pin<&Self>, keycode: u8) -> Result<(), ggez::GameError> {
        self.keyboard_send_channel
            .lock()
            .unwrap()
            .send((keycode, KeyAction::Released))
            .unwrap();

        Ok(())
    }

    fn pin_get_screen(self: Pin<&Self>) -> &Screen {
        &self.get_ref().screen
    }

    fn execute_next_instruction(self: Pin<&Self>) {
        // will block on `wait_for_key`
        {
            self.core.lock().unwrap().execute_next_instruction();
        }

        // `mtx` is shared with the main thread, so it's important to lock it only once we're sure
        // we can no longer block
        let (cond, mtx) = self.update_sync_pair.as_ref();
        {
            let mut state = mtx.lock().unwrap();
            *state = State::Ready;
        }
        cond.notify_all();
    }
}
