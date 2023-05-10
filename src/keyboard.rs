use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::sync::{Condvar, Mutex};

pub type KeyValue = u8;
pub enum KeyAction {
    Pressed,
    Released,
}

/* keyboard events received from the emulator */
pub type KeyMessage = (KeyValue, KeyAction);

/* state of the keyboard thread
 * used to send messages to the emulator, and to manage the keyboard state machine */
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum KeyboardState {
    #[default]
    Normal,
    Waiting,
    PressedWhileWaiting(KeyValue),
}

pub struct KeyboardManager {
    pressed_keys: Mutex<[bool; 16]>,

    // initialized as None, after any assignment can only be set as None again by wait_for_key()
    last_key: Mutex<Option<KeyValue>>,
}

impl Default for KeyboardManager {
    fn default() -> Self {
        Self {
            pressed_keys: Mutex::new([false; 16]),
            last_key: Mutex::new(None),
        }
    }
}

impl KeyboardManager {
    pub fn new(rx_in: Receiver<KeyMessage>) -> (Arc<Self>, Arc<(Condvar, Mutex<KeyboardState>)>) {
        let km = KeyboardManager::default();
        let res = Arc::new(km);

        let sync_pair = Arc::new((Condvar::new(), Mutex::new(KeyboardState::default())));

        let r1 = Arc::clone(&res);
        let s1 = Arc::clone(&sync_pair);

        std::thread::spawn(move || r1.start(rx_in, s1));

        (res, sync_pair)
    }

    fn start(&self, rx_in: Receiver<KeyMessage>, sync_pair: Arc<(Condvar, Mutex<KeyboardState>)>) {
        /* keyboard thread loop */
        loop {
            let (key, action) = rx_in.recv().unwrap();
            let (cvar, mtx) = sync_pair.as_ref();

            match action {
                KeyAction::Pressed => {
                    {
                        **(self.last_key.lock().as_mut().unwrap()) = Some(key);
                    }
                    {
                        self.pressed_keys.lock().unwrap()[key as usize] = true;
                    }
                    {
                        let mut state = mtx.lock().unwrap();
                        match *state {
                            KeyboardState::Normal => continue,
                            KeyboardState::Waiting => {
                                *state = KeyboardState::PressedWhileWaiting(key)
                            }
                            KeyboardState::PressedWhileWaiting(_) => unreachable!(),
                        }
                    }
                    cvar.notify_all();
                }
                KeyAction::Released => {
                    self.pressed_keys.lock().unwrap()[key as usize] = false;
                }
            }
        }
    }

    pub fn is_pressed(&self, key_code: u8) -> bool {
        self.pressed_keys.lock().unwrap()[key_code as usize]
    }
}
