//! Window, OpenGL context and the ImGui frame loop (winit + glutin + glow).

use glow::HasContext;
use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext};
use glutin::display::{GetGlDisplay, GlDisplay};
use glutin::surface::{GlSurface, Surface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use imgui_winit_support::{HiDpiMode, WinitPlatform};
use raw_window_handle::HasWindowHandle;
use std::num::NonZeroU32;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

/// Called once per frame to build the UI.
pub trait Frame {
    fn frame(&mut self, ui: &imgui::Ui);
    /// Called once after the ImGui context exists, to load fonts and style.
    fn setup(&mut self, ctx: &mut imgui::Context, hidpi: f32);
}

struct Gfx {
    window: Window,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    platform: WinitPlatform,
    imgui: imgui::Context,
    renderer: imgui_glow_renderer::AutoRenderer,
    last: Instant,
}

struct App<F: Frame> {
    title: String,
    size: (u32, u32),
    clear: [f32; 4],
    gfx: Option<Gfx>,
    frame: F,
}

pub fn run<F: Frame + 'static>(title: &str, size: (u32, u32), clear: [f32; 4], frame: F) {
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App { title: title.into(), size, clear, gfx: None, frame };
    event_loop.run_app(&mut app).expect("event loop error");
}

impl<F: Frame> App<F> {
    fn create(&mut self, el: &ActiveEventLoop) -> Gfx {
        let attrs = WindowAttributes::default()
            .with_title(&self.title)
            .with_inner_size(LogicalSize::new(self.size.0, self.size.1));
        let (window, cfg) = glutin_winit::DisplayBuilder::new()
            .with_window_attributes(Some(attrs))
            .build(el, ConfigTemplateBuilder::new(), |mut c| c.next().expect("no GL config"))
            .expect("failed to create OpenGL window");
        let window = window.expect("no window");
        let raw = window.window_handle().expect("window handle").as_raw();
        let context = unsafe { cfg.display().create_context(&cfg, &ContextAttributesBuilder::new().build(Some(raw))) }
            .expect("failed to create GL context");
        let px = window.inner_size();
        let sattrs = SurfaceAttributesBuilder::<WindowSurface>::new()
            .with_srgb(Some(false)) // theme hex values are sRGB already; don't let GL convert again
            .build(raw, NonZeroU32::new(px.width.max(1)).unwrap(), NonZeroU32::new(px.height.max(1)).unwrap());
        let surface = unsafe { cfg.display().create_window_surface(&cfg, &sattrs) }.expect("GL surface");
        let context = context.make_current(&surface).expect("make GL context current");
        let _ = surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::new(1).unwrap()));
        let gl = unsafe { glow::Context::from_loader_function_cstr(|s| context.display().get_proc_address(s).cast()) };

        let mut imgui = imgui::Context::create();
        imgui.set_ini_filename(None);
        let mut platform = WinitPlatform::new(&mut imgui);
        platform.attach_window(imgui.io_mut(), &window, HiDpiMode::Default);
        let hidpi = platform.hidpi_factor() as f32;
        self.frame.setup(&mut imgui, hidpi);
        let renderer = imgui_glow_renderer::AutoRenderer::new(gl, &mut imgui).expect("imgui renderer");
        Gfx { window, surface, context, platform, imgui, renderer, last: Instant::now() }
    }
}

impl<F: Frame> ApplicationHandler for App<F> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_none() {
            self.gfx = Some(self.create(el));
        }
    }

    fn new_events(&mut self, _: &ActiveEventLoop, _: winit::event::StartCause) {
        if let Some(g) = &mut self.gfx {
            let now = Instant::now();
            g.imgui.io_mut().update_delta_time(now - g.last);
            g.last = now;
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(g) = &mut self.gfx {
            g.platform.prepare_frame(g.imgui.io_mut(), &g.window).expect("prepare frame");
            g.window.request_redraw();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(g) = &mut self.gfx else { return };
        match &event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) if s.width > 0 && s.height > 0 => {
                g.surface.resize(&g.context, NonZeroU32::new(s.width).unwrap(), NonZeroU32::new(s.height).unwrap());
            }
            WindowEvent::RedrawRequested => {
                let [r, gr, b, a] = self.clear;
                unsafe {
                    g.renderer.gl_context().clear_color(r, gr, b, a);
                    g.renderer.gl_context().clear(glow::COLOR_BUFFER_BIT);
                }
                let ui = g.imgui.frame();
                self.frame.frame(ui);
                g.platform.prepare_render(ui, &g.window);
                let draw = g.imgui.render();
                g.renderer.render(draw).expect("imgui render");
                g.surface.swap_buffers(&g.context).expect("swap buffers");
                return;
            }
            _ => {}
        }
        g.platform.handle_event::<()>(g.imgui.io_mut(), &g.window, &Event::WindowEvent { window_id: id, event });
    }
}
