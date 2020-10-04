use ::anyhow::Result;
use ::log::*;
use ::x11rb::connection::Connection;
use ::x11rb::protocol::randr::ConnectionExt as RandRConnectionExt;
use ::x11rb::protocol::shape::{self, ConnectionExt as ShapeConnectionExt};
use ::x11rb::protocol::xproto::{self, ConnectionExt};

struct XcbHandle {
    window: u32,
    conn: *mut ::std::ffi::c_void,
}

unsafe impl raw_window_handle::HasRawWindowHandle for XcbHandle {
    fn raw_window_handle(&self) -> raw_window_handle::RawWindowHandle {
        raw_window_handle::RawWindowHandle::Xcb(raw_window_handle::unix::XcbHandle {
            window: self.window,
            connection: self.conn,
            ..raw_window_handle::unix::XcbHandle::empty()
        })
    }
}

#[allow(dead_code)]
fn read_spv(p: impl AsRef<::std::path::Path>) -> Result<Vec<u32>> {
    use ::byteorder::{LittleEndian, ReadBytesExt};
    use ::std::io::Read;
    let mut buf = Vec::new();
    ::std::fs::File::open(p)?.read_to_end(&mut buf)?;
    let mut buf32 = Vec::new();
    buf32.resize_with(buf.len() / 4, Default::default);
    buf.as_slice().read_u32_into::<LittleEndian>(&mut buf32)?;
    Ok(buf32)
}

fn slice_to_u32(mut slice: &[u8]) -> Vec<u32> {
    use ::byteorder::{LittleEndian, ReadBytesExt};
    let mut buf32 = Vec::new();
    buf32.resize_with(slice.len() / 4, Default::default);
    slice.read_u32_into::<LittleEndian>(&mut buf32).unwrap();
    buf32
}

#[async_std::main]
async fn main() -> Result<()> {
    env_logger::init();
    let (conn, screen) = x11rb::rust_connection::RustConnection::connect(None)?;
    let screen = &conn.setup().roots[screen];
    let mut pointer = conn.query_pointer(screen.root)?.reply()?;

    let monitors = conn.randr_get_screen_resources(screen.root)?.reply()?;
    let crtc_info_cookies: Vec<_> = monitors
        .crtcs
        .iter()
        .map(|crtc| conn.randr_get_crtc_info(*crtc, monitors.config_timestamp))
        .collect::<Result<_, _>>()?;

    let mut monitor_dim = None;
    for cookie in crtc_info_cookies {
        let crtc_info = cookie.reply()?;
        if pointer.root_x >= crtc_info.x
            && pointer.root_x < crtc_info.x + crtc_info.width as i16
            && pointer.root_y >= crtc_info.y
            && pointer.root_y < crtc_info.y + crtc_info.height as i16
        {
            monitor_dim = Some((crtc_info.width, crtc_info.height));
        }
    }

    let monitor_dim = monitor_dim.expect("Pointer wasn't in any of the moniters");
    let (w, h) = (monitor_dim.0 / 10, monitor_dim.0 / 10);
    debug!("Pointer monitor dimension: {:?}", monitor_dim);

    // Find a 32-bit viusal
    let depth32_info = screen
        .allowed_depths
        .iter()
        .find(|x| x.depth == 32)
        .expect("32 bit depth not supported");
    let visual32_info = depth32_info
        .visuals
        .iter()
        .find(|v| v.bits_per_rgb_value == 8 && v.class == xproto::VisualClass::TrueColor)
        .expect("No usable 32 bit visual found");

    let colormap_id = conn.generate_id()?;
    conn.create_colormap(
        xproto::ColormapAlloc::None,
        colormap_id,
        screen.root,
        visual32_info.visual_id,
    )?
    .check()?;

    let wid = conn.generate_id()?;
    conn.create_window(
        32,
        wid,
        screen.root,
        pointer.root_x - (w / 2) as i16,
        pointer.root_y - (h / 2) as i16,
        w,
        h,
        0,
        xproto::WindowClass::InputOutput,
        visual32_info.visual_id,
        &xproto::CreateWindowAux::new()
            .backing_pixel(screen.white_pixel)
            .colormap(colormap_id)
            .border_pixel(0)
            .override_redirect(1),
    )?
    .check()?;

    conn.shape_rectangles(
        shape::SO::Intersect,
        shape::SK::Input,
        xproto::ClipOrdering::YSorted,
        wid,
        0,
        0,
        &[],
    )?.check()?;
    conn.map_window(wid)?.check()?;

    let mut xcb_screen = 0;
    let xcb_conn =
        unsafe { xcb::ffi::base::xcb_connect(::std::ptr::null(), &mut xcb_screen as *mut _) };
    let window_handle = XcbHandle {
        window: wid,
        conn: xcb_conn as *mut _,
    };

    let instance = wgpu::Instance::new(wgpu::BackendBit::PRIMARY);
    let surface = unsafe { instance.create_surface(&window_handle) };
    let adapter_opt = wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
    };

    let adapter = instance
        .request_adapter(&adapter_opt)
        .await
        .expect("No usable adapter found");
    assert!(adapter.features().contains(wgpu::Features::PUSH_CONSTANTS));
    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                features: adapter.features(),
                limits: adapter.limits(),
                ..Default::default()
            },
            None,
        )
        .await?;

    //let vert = read_spv(::std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vert.spv"))?;
    //let frag = read_spv(::std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("frag.spv"))?;
    let vert = slice_to_u32(include_bytes!(concat!(env!("OUT_DIR"), "/vert.spv")));
    let frag = slice_to_u32(include_bytes!(concat!(env!("OUT_DIR"), "/frag.spv")));

    let vert = device.create_shader_module(wgpu::ShaderModuleSource::SpirV(vert.as_slice().into()));
    let frag = device.create_shader_module(wgpu::ShaderModuleSource::SpirV(frag.as_slice().into()));

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[],
        push_constant_ranges: &[wgpu::PushConstantRange {
            stages: wgpu::ShaderStage::FRAGMENT,
            range: 0..4,
        }],
    });
    let pipeline_d = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        vertex_stage: wgpu::ProgrammableStageDescriptor {
            entry_point: "main",
            module: &vert,
        },
        fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
            entry_point: "main",
            module: &frag,
        }),
        rasterization_state: None,
        primitive_topology: wgpu::PrimitiveTopology::TriangleList,
        depth_stencil_state: None,
        vertex_state: wgpu::VertexStateDescriptor {
            index_format: wgpu::IndexFormat::Uint16,
            vertex_buffers: &[wgpu::VertexBufferDescriptor {
                stride: 8,
                step_mode: wgpu::InputStepMode::Vertex,
                attributes: &[wgpu::VertexAttributeDescriptor {
                    offset: 0,
                    format: wgpu::VertexFormat::Float2,
                    shader_location: 0,
                }],
            }],
        },
        color_states: &[wgpu::ColorStateDescriptor {
            format: wgpu::TextureFormat::Bgra8Unorm,
            alpha_blend: wgpu::BlendDescriptor::REPLACE,
            color_blend: wgpu::BlendDescriptor::REPLACE,
            write_mask: wgpu::ColorWrite::ALL,
        }],
        sample_count: 1,
        sample_mask: !0,
        alpha_to_coverage_enabled: false,
    });

    let sc_desc = wgpu::SwapChainDescriptor {
        usage: wgpu::TextureUsage::OUTPUT_ATTACHMENT,
        format: wgpu::TextureFormat::Bgra8Unorm,
        width: w as u32,
        height: h as u32,
        present_mode: wgpu::PresentMode::Fifo,
    };

    let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        mapped_at_creation: true,
        size: 8 * 6,
        usage: wgpu::BufferUsage::MAP_WRITE | wgpu::BufferUsage::VERTEX,
    });

    {
        use ::std::ops::DerefMut;
        let mut view = vertex_buffer.slice(..).get_mapped_range_mut();
        let view_slice = view.deref_mut();
        let view: &mut [f32] = unsafe {
            ::std::slice::from_raw_parts_mut(
                view_slice.as_mut_ptr() as *mut _,
                view_slice.len() / 4,
            )
        };
        view.copy_from_slice(&[
            -1.0, -1.0, 1.0, -1.0, 1.0, 1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0,
        ]);
    }
    vertex_buffer.unmap();

    let mut sc = device.create_swap_chain(&surface, &sc_desc);

    let start = ::std::time::Instant::now();
    loop {
        if ::std::time::Instant::now() - start > ::std::time::Duration::from_secs(5) {
            break;
        }
        let now = ::std::time::Instant::now();
        let frame = sc.get_current_frame()?.output;
        let elapsed_ms = (now - start).as_secs_f32();
        let push_constant = std::slice::from_ref(unsafe { std::mem::transmute(&elapsed_ms) });
        let new_pointer = conn.query_pointer(screen.root)?.reply()?;
        if (new_pointer.root_x, new_pointer.root_y) != (pointer.root_x, pointer.root_y) {
            conn.configure_window(
                wid,
                &xproto::ConfigureWindowAux::new()
                    .x(Some((new_pointer.root_x - (w / 2) as i16) as i32))
                    .y(Some((new_pointer.root_y - (h / 2) as i16) as i32)),
            )?
            .check()?;
            pointer = new_pointer;
        }
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                    attachment: &frame.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: true,
                    },
                }],
                depth_stencil_attachment: None,
            });
            pass.set_pipeline(&pipeline_d);
            pass.set_push_constants(wgpu::ShaderStage::FRAGMENT, 0, push_constant);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.draw(0..6, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }

    Ok(())
}
