#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_arch = "x86")]
use std::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;
use std::mem::{self, size_of};

use chip_8_core::*;
use ggez::graphics;
use wgpu::util::DeviceExt;

// screen triangle
const INDEX_LIST: [u32; 3] = [0, 1, 2];
#[rustfmt::skip]
const VERTEX_LIST: [f32; 9] = [
    -1.0, -1.0, 0.0,
    -1.0,  3.0, 0.0,
     3.0, -1.0, 0.0,
];

// "pixel" size on output window
const SCREEN_SCALE_FACTOR: usize = 10;

#[rustfmt::skip]
static G_FB: FrameBuffer = unsafe {
    std::mem::transmute::<[u8; 256], FrameBuffer>([
        0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
        0xF0,0x20,0xF0,0xF0,0x90,0xF0,0xF0,0xF0,
        0x90,0x60,0x10,0x10,0x90,0x80,0x80,0x10,
        0x90,0x20,0xF0,0xF0,0xF0,0xF0,0xF0,0x20,
        0x90,0x20,0x80,0x10,0x10,0x10,0x90,0x40,
        0xF0,0x70,0xF0,0xF0,0x10,0xF0,0xF0,0x40,
        0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
        0xF0,0xF0,0xF0,0xE0,0xF0,0xE0,0xF0,0xF0,
        0x90,0x90,0x90,0x90,0x80,0x90,0x80,0x80,
        0xF0,0xF0,0xF0,0xE0,0x80,0x90,0xF0,0xF0,
        0x90,0x10,0x90,0x90,0x80,0x90,0x80,0x80,
        0xF0,0xF0,0x90,0xE0,0xF0,0xE0,0xF0,0x80,
        0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
        0xF0,0x20,0xF0,0xF0,0x90,0xF0,0xF0,0xF0,
        0x90,0x60,0x10,0x10,0x90,0x80,0x80,0x10,
        0x90,0x20,0xF0,0xF0,0xF0,0xF0,0xF0,0x20,
        0x90,0x20,0x80,0x10,0x10,0x10,0x90,0x40,
        0xF0,0x70,0xF0,0xF0,0x10,0xF0,0xF0,0x40,
        0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
        0xF0,0xF0,0xF0,0xE0,0xF0,0xE0,0xF0,0xF0,
        0x90,0x90,0x90,0x90,0x80,0x90,0x80,0x80,
        0xF0,0xF0,0xF0,0xE0,0x80,0x90,0xF0,0xF0,
        0x90,0x10,0x90,0x90,0x80,0x90,0x80,0x80,
        0xF0,0xF0,0x90,0xE0,0xF0,0xE0,0xF0,0x80,
        0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
        0x55,0x55,0x55,0x55,0x55,0x55,0x55,0x55,
        0xAA,0xAA,0xAA,0xAA,0xAA,0xAA,0xAA,0xAA,
        0x55,0x55,0x55,0x55,0x55,0x55,0x55,0x55,
        0xAA,0xAA,0xAA,0xAA,0xAA,0xAA,0xAA,0xAA,
        0x55,0x55,0x55,0x55,0x55,0x55,0x55,0x55,
        0xAA,0xAA,0xAA,0xAA,0xAA,0xAA,0xAA,0xAA,
        0x55,0x55,0x55,0x55,0x55,0x55,0x55,0x55,
    ])
};

#[cfg(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    target_feature = "avx2"
))]
fn fix_u32_endianness(bytes_slice: &FlatFrameBuffer) -> FlatFrameBuffer {
    const BUFFER_SIZE: usize = size_of::<FlatFrameBuffer>() / size_of::<__m256i>();

    // copy the FrameBuffer
    let mut buffer: [__m256i; BUFFER_SIZE] =
        unsafe { std::mem::transmute_copy::<FlatFrameBuffer, [__m256i; BUFFER_SIZE]>(bytes_slice) };

    // cast the argument to be of the same type as `buffer`
    let slice_cast =
        unsafe { *(bytes_slice as *const FlatFrameBuffer as *const [__m256i; BUFFER_SIZE]) };

    /* Although defining slice_cast first and then defining `mut buffer = slice_cast.clone()` would
     * arguably be more elegant, cloning the buffer the way we do adds a static check to ensure
     * that the two arrays have indeed the same size
     */

    #[rustfmt::skip]
    const SHUFFLE_CONTROL_MASK: [u8; 32] = [
        // twice the permutation ABCD EFGH IJKL MNOP -> DCBA HGFE LKJI PONM
        // `_mm256_shuffle_epi8()` reads only the bottom 4 bits of each byte in the control mask
        0x03, 0x02, 0x01, 0x00,
        0x07, 0x06, 0x05, 0x04,
        0x0B, 0x0A, 0x09, 0x08,
        0x0F, 0x0E, 0x0D, 0x0C,
        0x03, 0x02, 0x01, 0x00,
        0x07, 0x06, 0x05, 0x04,
        0x0B, 0x0A, 0x09, 0x08,
        0x0F, 0x0E, 0x0D, 0x0C,
    ];

    #[rustfmt::skip]
    buffer.iter_mut().enumerate().for_each(|(i, x)| {
        *x = unsafe {
            _mm256_shuffle_epi8(
                slice_cast[i],
                mem::transmute_copy(&SHUFFLE_CONTROL_MASK)
            )
        };
    });

    unsafe { *(&buffer as *const [__m256i; BUFFER_SIZE] as *const FlatFrameBuffer) }
}

#[cfg(all(
    any(target_arch = "x86", target_arch = "x86_64"),
    not(target_feature = "avx2"),
    target_feature = "ssse3"
))]
fn fix_u32_endianness(bytes_slice: &FlatFrameBuffer) -> FlatFrameBuffer {
    const BUFFER_SIZE: usize = size_of::<FlatFrameBuffer>() / size_of::<__m128i>();

    // copy the FrameBuffer
    let mut buffer: [__m128i; BUFFER_SIZE] =
        unsafe { std::mem::transmute_copy::<FlatFrameBuffer, [__m128i; BUFFER_SIZE]>(bytes_slice) };

    // cast the argument to be of the same type as `buffer`
    let slice_cast =
        unsafe { *(bytes_slice as *const FlatFrameBuffer as *const [__m128i; BUFFER_SIZE]) };

    /* Although defining slice_cast first and then defining `mut buffer = slice_cast.clone()` would
     * arguably be more elegant, cloning the buffer the way we do adds a static check to ensure
     * that the two arrays have indeed the same size
     */

    #[rustfmt::skip]
    const SHUFFLE_CONTROL_MASK: [u8; 16] = [
        // permutation ABCD EFGH IJKL MNOP -> DCBA HGFE LKJI PONM
        // `_mm_shuffle_epi8()` reads only the bottom 4 bits of each byte in the control mask
        0x03, 0x02, 0x01, 0x00,
        0x07, 0x06, 0x05, 0x04,
        0x0B, 0x0A, 0x09, 0x08,
        0x0F, 0x0E, 0x0D, 0x0C,
    ];

    #[rustfmt::skip]
    buffer.iter_mut().enumerate().for_each(|(i, x)| {
        *x = unsafe {
            _mm_shuffle_epi8(
                slice_cast[i],
                mem::transmute_copy(&SHUFFLE_CONTROL_MASK)
            )
        };
    });

    unsafe { *(&buffer as *const [__m128i; BUFFER_SIZE] as *const FlatFrameBuffer) }
}

#[cfg(all(
    not(target_endian = "big"),
    not(any(target_feature = "avx2", target_feature = "ssse3"))
))]
fn fix_u32_endianness(bytes_slice: &FlatFrameBuffer) -> FlatFrameBuffer {
    let mut buffer = bytes_slice.clone();

    bytes_slice
        .chunks(4)
        .zip(buffer.chunks_mut(4))
        .for_each(|(v_in, v_out)| {
            v_out[0] = v_in[3];
            v_out[1] = v_in[2];
            v_out[2] = v_in[1];
            v_out[3] = v_in[0];
        });

    buffer
}

#[cfg(target_endian = "big")]
fn fix_u32_endianness(bytes_slice: &FlatFrameBuffer) -> FlatFrameBuffer {
    /* in theory we could avoid the copy, but realistically
     * (1) the impact is negligible,
     * (2) this code will never run on a big-endian architecture
     */
    bytes_slice.clone()
}

struct Emulator {
    fb: chip_8_core::FrameBuffer,
    verts: wgpu::Buffer,
    inds: wgpu::Buffer,
    pixel_buffer: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
}

impl Emulator {
    fn new(ctx: &mut ggez::Context) -> ggez::GameResult<Emulator> {
        //let fb = FrameBuffer::default();
        let fb = G_FB.clone();

        let shader = ctx
            .gfx
            .wgpu()
            .device
            .create_shader_module(wgpu::include_wgsl!("scale_pixels.wgsl"));

        let verts = ctx
            .gfx
            .wgpu()
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: unsafe { &mem::transmute::<[f32; 9], [u8; 36]>(VERTEX_LIST) },
                usage: wgpu::BufferUsages::VERTEX,
            });

        let inds = ctx
            .gfx
            .wgpu()
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: unsafe { &mem::transmute::<[u32; 3], [u8; 12]>(INDEX_LIST) },
                usage: wgpu::BufferUsages::INDEX,
            });

        let pipeline =
            ctx.gfx
                .wgpu()
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: None,
                    layout: None,
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: "vs_main",
                        buffers: &[wgpu::VertexBufferLayout {
                            array_stride: size_of::<[f32; 3]>() as _,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &[
                                // pos
                                wgpu::VertexAttribute {
                                    format: wgpu::VertexFormat::Float32x3,
                                    offset: 0,
                                    shader_location: 0,
                                },
                            ],
                        }],
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        strip_index_format: None,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode: None,
                        unclipped_depth: false,
                        polygon_mode: wgpu::PolygonMode::Fill,
                        conservative: false,
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: "fs_main",
                        targets: &[Some(wgpu::ColorTargetState {
                            format: ctx.gfx.surface_format(),
                            blend: Some(wgpu::BlendState::REPLACE),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview: None,
                });

        let pixel_buffer =
            ctx.gfx
                .wgpu()
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: &fix_u32_endianness(fb.as_ref()),
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                });

        let push_scale =
            ctx.gfx
                .wgpu()
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: unsafe {
                        std::mem::transmute::<&u32, &[u8; 4]>(&(SCREEN_SCALE_FACTOR as u32))
                    },
                    usage: wgpu::BufferUsages::UNIFORM,
                });

        let bind_group = ctx
            .gfx
            .wgpu()
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &push_scale,
                            offset: 0,
                            size: None,
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &pixel_buffer,
                            offset: 0,
                            size: None,
                        }),
                    },
                ],
            });

        Ok(Emulator {
            fb,
            verts,
            inds,
            pixel_buffer,
            pipeline,
            bind_group,
        })
    }
}

impl ggez::event::EventHandler<ggez::GameError> for Emulator {
    fn update(&mut self, _ctx: &mut ggez::Context) -> ggez::GameResult {
        Ok(())
    }

    fn draw(&mut self, ctx: &mut ggez::Context) -> ggez::GameResult {
        ctx.gfx.wgpu().queue.write_buffer(
            &self.pixel_buffer,
            0,
            &fix_u32_endianness(&self.fb.as_ref()),
        );

        {
            let frame = ctx.gfx.frame().clone();
            let cmd = ctx.gfx.commands().unwrap();

            let mut pass = cmd.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: frame.wgpu().1,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(
                            graphics::LinearColor::from(graphics::Color::new(0.5, 0.4, 0.2, 1.0))
                                .into(),
                        ),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });

            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.verts.slice(..));
            pass.set_index_buffer(self.inds.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..3, 0, 0..1);
        }

        Ok(())
    }
}

fn main() -> ggez::GameResult {
    let window_mode = ggez::conf::WindowMode {
        width: (SCREEN_WIDTH * SCREEN_SCALE_FACTOR) as f32,
        height: (SCREEN_HEIGHT * SCREEN_SCALE_FACTOR) as f32,
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

    let mut window_setup = ggez::conf::WindowSetup::default();
    window_setup.title = String::from("Chip-8 Emulator");
    window_setup.vsync = false;
    window_setup.srgb = false;
    //window_setup.icon= TODO,

    let (mut ctx, event_loop) = ggez::ContextBuilder::new("chip-8-emulator", "Stefano Ariotta")
        .window_setup(window_setup)
        .window_mode(window_mode)
        .backend(ggez::conf::Backend::Vulkan)
        .build()
        .unwrap();

    let emulator = Emulator::new(&mut ctx)?;

    ggez::event::run(ctx, event_loop, emulator);
}
