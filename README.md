# Chip8-Desktop

A multi-platform (Linux, Windows) Chip8 emulator written in Rust

## Features

Chip8-Desktop uses [Chip8-Desktop](https://github.com/st-ario/chip-8-desktop) as a simulation basis
and [ggez](https://github.com/ggez/ggez) to set up the game loop.

The framebuffer is rendered through a custom vertex shader written in wgsl. Input handling and
rendering are each processed in their dedicated threads, and thread synchronization is achieved
through Rust's `std::sync` primitives.

Users can pick their desired emulation speed through the `-C [clock-speed]` flag to the emulator,
expressing the speed in Hertz.

Frame-skipping is adopted in order to reduce jittering, frame-rate is dynamic.

