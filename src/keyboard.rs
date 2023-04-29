use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::sync::Mutex;

pub enum KeyAction {
    Pressed,
    Released,
}

pub type KeyMessage = (u8, KeyAction);
pub type KeyValue = u8;

pub struct KeyboardManager {
    pressed_keys: Mutex<[bool; 16]>,
    last_key: Mutex<Option<KeyValue>>, // initialized as None, after first assignment can only be
                                       // be made None again by wait_for_key()
}

impl KeyboardManager {
    pub fn init(rx_in: Receiver<KeyMessage>) -> Arc<Self> {
        let km = Self {
            last_key: Mutex::new(None),
            pressed_keys: Mutex::new([false; 16]),
        };

        let res = Arc::new(km);
        let r1 = Arc::clone(&res);

        std::thread::spawn(move || r1.start(rx_in));

        res
    }

    fn start(&self, rx_in: Receiver<KeyMessage>) {
        loop {
            let received_res = rx_in.recv();
            if received_res.is_err() {
                return;
            }

            let (key, action) = received_res.unwrap();

            match action {
                KeyAction::Pressed => {
                    // do not send more than one "pressed" signal if key is held
                    let already_pressed;
                    {
                        let arr = &self.pressed_keys.lock().unwrap();
                        already_pressed = arr[key as usize];
                    }
                    if !already_pressed {
                        // lock, update and release
                        {
                            **(self.last_key.lock().as_mut().unwrap()) = Some(key);
                        }
                        {
                            self.pressed_keys.lock().unwrap()[key as usize] = true;
                        }
                    }
                }
                KeyAction::Released => {
                    self.pressed_keys.lock().unwrap()[key as usize] = false;
                }
            }
        }
    }

    pub fn wait_for_key(&self) -> u8 {
        /* Currently Broken
        // empy `self.last_key`, since we want to wait for the _next_ value
        {
            **(self.last_key.lock().as_mut().unwrap()) = None;
        }

        loop {
            let key;

            // lock, copy and release
            {
                let lk = self.last_key.lock();
                key = Option::clone(lk.as_ref().unwrap());
            }

            match key {
                None => {
                    thread::yield_now();
                    continue;
                }
                Some(key) => return key,
            }
        }
        */
        5
    }

    pub fn is_pressed(&self, key_code: u8) -> bool {
        self.pressed_keys.lock().unwrap()[key_code as usize]
    }
}
