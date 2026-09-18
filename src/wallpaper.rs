//! Wayland / wlr-layer-shell presentation.
//!
//! One background layer and one EGL window surface are created for every
//! Wayland output. Rendering itself is delegated to GlRenderer.

use std::collections::HashMap;

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_dispatch2, delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::calloop::EventLoop,
    reexports::calloop_wayland_source::WaylandSource,
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_surface},
    Connection, Proxy, QueueHandle,
};
use wayland_egl::WlEglSurface;

use crate::bail;
use crate::config::GradientProfile;
use crate::log;
use crate::render::GlRenderer;

struct OutputSurface {
    layer: LayerSurface,
    egl_window: WlEglSurface,
    egl_surface: khronos_egl::Surface,
    width: u32,
    height: u32,
}

pub struct WaylandWallpaper {
    conn: Connection,
    registry_state: RegistryState,
    output_state: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    renderer: GlRenderer,
    outputs: HashMap<wayland_client::backend::ObjectId, OutputSurface>,
    current_gradient: Option<GradientProfile>,
    texture_ready: bool,
}

impl WaylandWallpaper {
    pub fn new(texture_resolution: u32) -> (Self, wayland_client::EventQueue<Self>) {
        let conn = match Connection::connect_to_env() {
            Ok(conn) => conn,
            Err(error) => bail!("connecting to Wayland: {error}"),
        };
        let (globals, event_queue) = match registry_queue_init::<Self>(&conn) {
            Ok(result) => result,
            Err(error) => bail!("initializing Wayland registry: {error}"),
        };
        let qh = event_queue.handle();

        let registry_state = RegistryState::new(&globals);
        let compositor = match CompositorState::bind(&globals, &qh) {
            Ok(compositor) => compositor,
            Err(error) => bail!("wl_compositor: {error}"),
        };
        let layer_shell = match LayerShell::bind(&globals, &qh) {
            Ok(layer_shell) => layer_shell,
            Err(error) => bail!("compositor has no wlr-layer-shell: {error}"),
        };
        let output_state = OutputState::new(&globals, &qh);

        let native_display = conn.backend().display_ptr() as *mut std::ffi::c_void;
        let renderer = GlRenderer::new(native_display, texture_resolution);

        (
            Self {
                conn,
                registry_state,
                output_state,
                compositor,
                layer_shell,
                renderer,
                outputs: HashMap::new(),
                current_gradient: None,
                texture_ready: false,
            },
            event_queue,
        )
    }

    pub fn insert_wayland_source(
        &self,
        event_loop: &EventLoop<Self>,
        event_queue: wayland_client::EventQueue<Self>,
    ) {
        if let Err(error) = WaylandSource::new(self.conn.clone(), event_queue).insert(event_loop.handle()) {
            bail!("failed to insert Wayland event source: {error}");
        }
    }

    pub fn set_gradient(&mut self, gradient: GradientProfile) {
        log!("setting gradient '{}'", gradient.name);
        self.current_gradient = Some(gradient.clone());

        if self.outputs.is_empty() {
            return;
        }

        self.render_all(&gradient);
    }

    fn render_all(&mut self, gradient: &GradientProfile) {
        let first_surface = self.outputs.values().next().map(|o| o.egl_surface);
        let Some(surface) = first_surface else {
            return;
        };

        self.renderer.make_current(surface);
        self.renderer.initialize_gl();
        self.renderer.render_gradient(gradient);
        self.texture_ready = true;

        for output in self.outputs.values() {
            if output.width != 0 && output.height != 0 {
                self.renderer.draw(output.egl_surface, output.width, output.height);
            }
        }
    }

    fn create_output_surface(&mut self, qh: &QueueHandle<Self>, output: &wl_output::WlOutput) {
        if self.outputs.contains_key(&output.id()) {
            return;
        }

        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Background,
            Some("hyprgradient"),
            Some(output),
        );

        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(0, 0);
        layer.commit();

        let egl_window = match WlEglSurface::new(layer.wl_surface().id(), 1, 1) {
            Ok(egl_window) => egl_window,
            Err(error) => bail!("failed to create wl_egl_window: {error:?}"),
        };
        let egl_surface = self
            .renderer
            .create_window_surface(egl_window.ptr() as *mut std::ffi::c_void);

        self.outputs.insert(
            output.id(),
            OutputSurface {
                layer,
                egl_window,
                egl_surface,
                width: 0,
                height: 0,
            },
        );
    }
}

impl CompositorHandler for WaylandWallpaper {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for WaylandWallpaper {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.create_output_surface(qh, &output);
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        if let Some(surface) = self.outputs.remove(&output.id()) {
            self.renderer.destroy_surface(surface.egl_surface);
        }
    }
}

impl LayerShellHandler for WaylandWallpaper {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if let Some(id) = self
            .outputs
            .iter()
            .find(|(_, output)| output.layer.wl_surface() == layer.wl_surface())
            .map(|(id, _)| id.clone())
        {
            if let Some(output) = self.outputs.remove(&id) {
                self.renderer.destroy_surface(output.egl_surface);
            }
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let layer_id = layer.wl_surface().id();
        let Some(output) = self
            .outputs
            .values_mut()
            .find(|output| output.layer.wl_surface().id() == layer_id)
        else {
            return;
        };

        output.width = configure.new_size.0;
        output.height = configure.new_size.1;
        output.egl_window.resize(output.width as i32, output.height as i32, 0, 0);
        let width = output.width;
        let height = output.height;

        if width == 0 || height == 0 {
            return;
        }

        let egl_surface = output.egl_surface;

        // The first configure is when EGL rendering is safe. Subsequent
        // configures redraw the current texture at the new output size.
        if !self.texture_ready {
            let Some(gradient) = self.current_gradient.clone() else {
                return;
            };
            self.render_all(&gradient);
        } else {
            self.renderer.draw(egl_surface, width, height);
        }
    }
}

delegate_registry!(WaylandWallpaper);
delegate_dispatch2!(WaylandWallpaper);

impl ProvidesRegistryState for WaylandWallpaper {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}
