use chip_8_core::*;
use ggez::graphics;
use std::mem::{self, size_of};
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
pub const SCREEN_SCALE_FACTOR: usize = 10;

pub struct Screen {
    verts: wgpu::Buffer,
    inds: wgpu::Buffer,
    pixel_buffer: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
}

impl Screen {
    pub fn new(ctx: &ggez::Context) -> ggez::GameResult<Screen> {
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

        const BLACK: FrameBuffer = [0; 256];
        let pixel_buffer =
            ctx.gfx
                .wgpu()
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: &BLACK, // no need to fix endianness for zeroes
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                });

        let push_scale =
            ctx.gfx
                .wgpu()
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: &u32::to_ne_bytes(SCREEN_SCALE_FACTOR as u32),
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

        Ok(Screen {
            verts,
            inds,
            pixel_buffer,
            pipeline,
            bind_group,
        })
    }

    pub fn draw(&self, ctx: &mut ggez::Context, fb: &FrameBuffer) -> ggez::GameResult {
        ctx.gfx
            .wgpu()
            .queue
            .write_buffer(&self.pixel_buffer, 0, &fix_u32_endianness(fb));

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

        Ok(())
    }
}

/* Utility function to correctly reinterpret the u8 FrameBuffer as a buffer of u32 */
fn fix_u32_endianness(bytes_slice: &FrameBuffer) -> FrameBuffer {
    let mut buffer = [0; size_of::<FrameBuffer>()];

    bytes_slice
        .chunks(4)
        .zip(buffer.chunks_mut(4))
        .for_each(|(v_in, v_out)| {
            v_out.copy_from_slice(&u32::to_ne_bytes(u32::from_be_bytes(
                v_in.try_into().unwrap(),
            )));
        });

    buffer
}
