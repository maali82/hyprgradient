//! Wayland / wlr-layer-shell presentation.
//!
//! One background layer and one EGL window surface are created for every
//! Wayland output. Rendering itself is delegated to GlRenderer.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Result};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_dispatch2, delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::calloop::{timer::{TimeoutAction, Timer}, EventLoop},
    reexports::calloop_wayland_source::WaylandSource,
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_surface},
    Connection, Proxy, QueueHandle,
};
use wayland_egl::WlEglSurface;

use crate::config::GradientProfile;
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
    gradients: Vec<GradientProfile>,
    current_index: usize,
    texture_ready: bool,
    cycle_interval: Duration,
    exit: bool,
}

impl WaylandWallpaper {
    pub fn new(
        gradients: Vec<GradientProfile>,
        cycle_interval: Duration,
        texture_resolution: u32,
    ) -> Result<(Self, wayland_client::EventQueue<Self>)> {
        let conn = Connection::connect_to_env()?;
        let (globals, event_queue) = registry_queue_init::<Self>(&conn)?;
        let qh = event_queue.handle();

        let registry_state = RegistryState::new(&globals);
        let compositor = CompositorState::bind(&globals, &qh)
            .map_err(|e| anyhow!("wl_compositor: {e}"))?;
        let layer_shell = LayerShell::bind(&globals, &qh)
            .map_err(|e| anyhow!("compositor has no wlr-layer-shell: {e}"))?;
        let output_state = OutputState::new(&globals, &qh);

        let native_display = conn.backend().display_ptr() as *mut std::ffi::c_void;
        let renderer = GlRenderer::new(native_display, texture_resolution)?;

        Ok((Self {
            conn,
            registry_state,
            output_state,
            compositor,
            layer_shell,
            renderer,
            outputs: HashMap::new(),
            gradients,
            current_index: 0,
            texture_ready: false,
            cycle_interval,
            exit: false,
        }, event_queue))
    }

    pub fn run(mut self, event_queue: wayland_client::EventQueue<Self>) -> Result<()> {
        let mut event_loop: EventLoop<Self> = EventLoop::try_new()?;
        let loop_handle = event_loop.handle();

        WaylandSource::new(self.conn.clone(), event_queue)
            .insert(loop_handle.clone())
            .map_err(|e| anyhow!("failed to insert Wayland event source: {e}"))?;

        if self.gradients.len() > 1 {
            let interval = self.cycle_interval;
            loop_handle
                .insert_source(
                    Timer::from_duration(interval),
                    move |_deadline, _, app: &mut Self| {
                        if let Err(e) = app.next_gradient() {
                            log::error!("failed to render gradient: {e}");
                            app.exit = true;
                        }

                        TimeoutAction::ToDuration(interval)
                    },
                )
                .map_err(|e| anyhow!("failed to insert gradient timer: {e:?}"))?;
        }

        while !self.exit {
            event_loop.dispatch(None, &mut self)?;
        }
        Ok(())
    }

    fn next_gradient(&mut self) -> Result<()> {
        self.current_index = (self.current_index + 1) % self.gradients.len();
        let gradient = self.gradients[self.current_index].clone();
        log::debug!("cycling to gradient '{}'", gradient.name);
        self.render_all(&gradient)
    }

    fn render_all(&mut self, gradient: &GradientProfile) -> Result<()> {
        let first_surface = self.outputs.values().next().map(|o| o.egl_surface);
        let Some(surface) = first_surface else { return Ok(()); };

        self.renderer.make_current(surface)?;
        self.renderer.initialize_gl()?;
        self.renderer.render_gradient(gradient)?;
        self.texture_ready = true;

        for output in self.outputs.values() {
            if output.width != 0 && output.height != 0 {
                self.renderer.draw(output.egl_surface, output.width, output.height)?;
            }
        }
        Ok(())
    }

    fn create_output_surface(
        &mut self,
        qh: &QueueHandle<Self>,
        output: &wl_output::WlOutput,
    ) -> Result<()> {
        if self.outputs.contains_key(&output.id()) {
            return Ok(());
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

        let egl_window = WlEglSurface::new(layer.wl_surface().id(), 1, 1)
            .map_err(|e| anyhow!("failed to create wl_egl_window: {e:?}"))?;
        let egl_surface = self.renderer.create_window_surface(egl_window.ptr() as *mut std::ffi::c_void)?;

        self.outputs.insert(output.id(), OutputSurface {
            layer,
            egl_window,
            egl_surface,
            width: 0,
            height: 0,
        });
        Ok(())
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
    fn output_state(&mut self) -> &mut OutputState { &mut self.output_state }

    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        if let Err(e) = self.create_output_surface(qh, &output) {
            log::error!("failed to create wallpaper for output {:?}: {e:#}", output.id());
            self.exit = true;
        }
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        if let Some(surface) = self.outputs.remove(&output.id()) {
            self.renderer.destroy_surface(surface.egl_surface);
        }
        if self.outputs.is_empty() {
            self.exit = true;
        }
    }
}

impl LayerShellHandler for WaylandWallpaper {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if let Some(id) = self.outputs.iter()
            .find(|(_, output)| output.layer.wl_surface() == layer.wl_surface())
            .map(|(id, _)| id.clone())
        {
            if let Some(output) = self.outputs.remove(&id) {
                self.renderer.destroy_surface(output.egl_surface);
            }
        }
        if self.outputs.is_empty() {
            self.exit = true;
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
        let Some(output) = self.outputs.values_mut().find(|output| output.layer.wl_surface().id() == layer_id) else {
            return;
        };

        output.width = configure.new_size.0;
        output.height = configure.new_size.1;
        output.egl_window.resize(output.width as i32, output.height as i32, 0, 0);
        let width = output.width;
        let height = output.height;

        if width == 0 || height == 0 { return; }

        let egl_surface = output.egl_surface;

        // The first configure is when EGL rendering is safe. Subsequent
        // configures redraw the current texture at the new output size.
        let result = if !self.texture_ready {
            let gradient = self.gradients[self.current_index].clone();
            self.render_all(&gradient)
        } else {
            self.renderer.draw(egl_surface, width, height)
        };

        if let Err(e) = result {
            log::error!("failed to render wallpaper: {e:#}");
            self.exit = true;
        }
    }
}

delegate_registry!(WaylandWallpaper);
delegate_dispatch2!(WaylandWallpaper);

impl ProvidesRegistryState for WaylandWallpaper {
    fn registry(&mut self) -> &mut RegistryState { &mut self.registry_state }
    registry_handlers![OutputState];
}
