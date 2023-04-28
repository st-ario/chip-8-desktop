use ggez::audio::SoundSource;
use spin_sleep::SpinSleeper;
use std::sync::atomic::AtomicI16;

pub struct DelayTimer {
    value: AtomicI16, // can transiently be -1, in which case it's safe to treat it as == 0
    sleeper: SpinSleeper,
}

pub struct SoundTimer {
    value: AtomicI16, // can transiently be -1, in which case it's safe to treat it as == 0
    sleeper: SpinSleeper,
    sound: ggez::audio::Source,
}

pub trait Timer: details::Timer {
    fn start(&self) -> !;

    fn get(&self) -> u8 {
        use std::sync::atomic::Ordering::Relaxed;
        self.get_value().load(Relaxed).try_into().unwrap_or(0)
    }

    fn set(&self, val: u8) {
        use std::sync::atomic::Ordering::Relaxed;
        self.get_value().store(val as i16, Relaxed)
    }
}

impl DelayTimer {
    pub fn new() -> Self {
        Self {
            value: AtomicI16::new(0),
            sleeper: spin_sleep::SpinSleeper::default(),
        }
    }
}

impl SoundTimer {
    pub fn new(mut sound: ggez::audio::Source) -> Self {
        // sound is just a waveform that loops
        sound.set_repeat(true);

        Self {
            value: AtomicI16::new(0),
            sleeper: spin_sleep::SpinSleeper::default(),
            sound,
        }
    }
}

/* expose getters only in this module */
mod details {
    pub trait Timer {
        fn get_value(&self) -> &std::sync::atomic::AtomicI16;
        fn get_sleeper(&self) -> &spin_sleep::SpinSleeper;
    }
}

impl details::Timer for DelayTimer {
    fn get_value(&self) -> &AtomicI16 {
        &self.value
    }

    fn get_sleeper(&self) -> &spin_sleep::SpinSleeper {
        &self.sleeper
    }
}

impl details::Timer for SoundTimer {
    fn get_value(&self) -> &AtomicI16 {
        &self.value
    }

    fn get_sleeper(&self) -> &spin_sleep::SpinSleeper {
        &self.sleeper
    }
}

impl Timer for DelayTimer {
    fn start(&self) -> ! {
        use details::Timer;
        use std::sync::atomic::Ordering::Relaxed;

        loop {
            use std::time::Duration;

            const TARGET_CLOCK_SPEED: Duration = Duration::new(0, 16_666_667); // 60 Hz

            self.get_value().fetch_sub(1, Relaxed);
            self.get_value().fetch_max(0, Relaxed);

            self.get_sleeper()
                .sleep_ns(TARGET_CLOCK_SPEED.subsec_nanos() as u64);
        }
    }
}

impl Timer for SoundTimer {
    fn start(&self) -> ! {
        use details::Timer;
        use std::sync::atomic::Ordering::Relaxed;

        loop {
            use std::time::Duration;

            const TARGET_CLOCK_SPEED: Duration = Duration::new(0, 16_666_667); // 60 Hz

            self.get_value().fetch_sub(1, Relaxed);
            let last_val = self.get_value().fetch_max(0, Relaxed);

            if last_val > 1 {
                self.sound.resume()
            } else {
                self.sound.pause()
            };

            self.get_sleeper()
                .sleep_ns(TARGET_CLOCK_SPEED.subsec_nanos() as u64);
        }
    }
}
