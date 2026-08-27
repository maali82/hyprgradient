//! OpenGL/EGL rendering.
//!
//! Gradient generation happens on the GPU. The CPU only uploads the gradient
//! parameters (stops, type and direction). A square texture/FBO is generated
//! once per gradient and can then be displayed on any Wayland output.

use anyhow::{anyhow, Context, Result};
use khronos_egl as egl;
use std::ffi::{c_void, CString};

use crate::config::{GradientProfile, GradientType};

const PLANE_VS: &str = include_str!("shaders/plane_vs.glsl");
const GRADIENT_FS: &str = include_str!("shaders/gradient_fs.glsl");
const DISPLAY_FS: &str = include_str!("shaders/display_fs.glsl");
const MAX_STOPS: usize = 32;

pub struct GlRenderer {
    egl: egl::DynamicInstance<egl::EGL1_4>,
    display: egl::Display,
    config: egl::Config,
    context: egl::Context,

    gradient_program: gl::types::GLuint,
    display_program: gl::types::GLuint,

    gradient_fbo: gl::types::GLuint,
    gradient_texture: gl::types::GLuint,
    fullscreen_vbo: gl::types::GLuint,

    gradient_resolution: u32,

    gradient_type: gl::types::GLint,
    direction: gl::types::GLint,
    stop_count: gl::types::GLint,
    stop_positions: gl::types::GLint,
    stop_colors: gl::types::GLint,
    display_texture: gl::types::GLint,
}

impl GlRenderer {
    pub fn new(native_display: *mut c_void, gradient_resolution: u32) -> Result<Self> {
        if gradient_resolution == 0 {
            return Err(anyhow!("gradient texture resolution must be greater than zero"));
        }

        let lib = unsafe { libloading::Library::new("libEGL.so.1") }
            .context("unable to find libEGL.so.1 - is a Mesa/EGL driver installed?")?;
        let egl = unsafe { egl::DynamicInstance::<egl::EGL1_4>::load_required_from(lib) }
            .map_err(|e| anyhow!("failed to load EGL 1.4+ from libEGL.so.1: {e:?}"))?;

        let display = unsafe { egl.get_display(native_display) }
            .ok_or_else(|| anyhow!("eglGetDisplay failed"))?;
        egl.initialize(display).context("eglInitialize failed")?;

        let attributes = [
            egl::RED_SIZE, 8,
            egl::GREEN_SIZE, 8,
            egl::BLUE_SIZE, 8,
            egl::ALPHA_SIZE, 8,
            egl::SURFACE_TYPE, egl::WINDOW_BIT,
            egl::RENDERABLE_TYPE, egl::OPENGL_ES2_BIT,
            egl::NONE,
        ];
        let config = egl.choose_first_config(display, &attributes)
            .context("eglChooseConfig failed")?
            .ok_or_else(|| anyhow!("no suitable EGL config found"))?;

        egl.bind_api(egl::OPENGL_ES_API).context("eglBindAPI(ES) failed")?;
        let context_attributes = [egl::CONTEXT_CLIENT_VERSION, 2, egl::NONE];
        let context = egl.create_context(display, config, None, &context_attributes)
            .context("eglCreateContext failed")?;

        Ok(Self {
            egl,
            display,
            config,
            context,
            gradient_program: 0,
            display_program: 0,
            gradient_fbo: 0,
            gradient_texture: 0,
            fullscreen_vbo: 0,
            gradient_resolution,
            gradient_type: -1,
            direction: -1,
            stop_count: -1,
            stop_positions: -1,
            stop_colors: -1,
            display_texture: -1,
        })
    }

    pub fn create_window_surface(&self, native_window: *mut c_void) -> Result<egl::Surface> {
        unsafe {
            self.egl
                .create_window_surface(
                    self.display,
                    self.config,
                    native_window as egl::NativeWindowType,
                    None,
                )
                .context("eglCreateWindowSurface failed")
        }
    }

    pub fn destroy_surface(&self, surface: egl::Surface) {
        let _ = self.egl.destroy_surface(self.display, surface);
    }

    pub fn make_current(&self, surface: egl::Surface) -> Result<()> {
        self.egl
            .make_current(self.display, Some(surface), Some(surface), Some(self.context))
            .context("eglMakeCurrent failed")
    }

    /// Must be called while an EGL surface belonging to this context is current.
    pub fn initialize_gl(&mut self) -> Result<()> {
        if self.gradient_program != 0 {
            return Ok(());
        }

        gl::load_with(|name| {
            self.egl
                .get_proc_address(name)
                .map(|f| f as *const c_void)
                .unwrap_or(std::ptr::null())
        });

        unsafe {
            self.gradient_program = compile_program(PLANE_VS, GRADIENT_FS)?;
            self.display_program = compile_program(PLANE_VS, DISPLAY_FS)?;

            self.gradient_type = uniform(self.gradient_program, "u_gradient_type");
            self.direction = uniform(self.gradient_program, "u_direction");
            self.stop_count = uniform(self.gradient_program, "u_stop_count");
            self.stop_positions = uniform(self.gradient_program, "u_stop_positions");
            self.stop_colors = uniform(self.gradient_program, "u_stop_colors");
            self.display_texture = uniform(self.display_program, "u_texture");

            self.gradient_texture = create_texture(self.gradient_resolution)?;
            self.gradient_fbo = create_fbo(self.gradient_texture)?;
            self.fullscreen_vbo = create_fullscreen_vbo();
        }

        Ok(())
    }

    /// Render the configured gradient into the GPU-resident square texture.
    /// No gradient pixels are generated on the CPU.
    pub fn render_gradient(&mut self, gradient: &GradientProfile) -> Result<()> {
        if self.gradient_program == 0 {
            return Err(anyhow!("OpenGL renderer has not been initialized"));
        }
        if gradient.stops.len() > MAX_STOPS {
            return Err(anyhow!(
                "gradient `{}` has {} stops; maximum supported is {}",
                gradient.name, gradient.stops.len(), MAX_STOPS
            ));
        }

        let mut positions = [0.0f32; MAX_STOPS];
        let mut colors = [0.0f32; MAX_STOPS * 3];
        for (i, stop) in gradient.stops.iter().enumerate() {
            positions[i] = stop.position as f32 / 100.0;
            colors[i * 3] = stop.color[0] as f32 / 255.0;
            colors[i * 3 + 1] = stop.color[1] as f32 / 255.0;
            colors[i * 3 + 2] = stop.color[2] as f32 / 255.0;
        }

        let radians = gradient.direction.to_radians();
        let direction = [radians.cos(), radians.sin()];
        let gradient_type = match gradient.gradient_type {
            GradientType::Linear => 0,
        };

        unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, self.gradient_fbo);
            gl::Viewport(
                0,
                0,
                self.gradient_resolution as i32,
                self.gradient_resolution as i32,
            );
            gl::UseProgram(self.gradient_program);

            gl::Uniform1i(self.gradient_type, gradient_type);
            gl::Uniform2fv(self.direction, 1, direction.as_ptr());
            gl::Uniform1i(self.stop_count, gradient.stops.len() as i32);
            gl::Uniform1fv(self.stop_positions, MAX_STOPS as i32, positions.as_ptr());
            gl::Uniform3fv(self.stop_colors, MAX_STOPS as i32, colors.as_ptr());

            gl::BindBuffer(gl::ARRAY_BUFFER, self.fullscreen_vbo);
            gl::EnableVertexAttribArray(0);
            gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 0, std::ptr::null());
            gl::DrawArrays(gl::TRIANGLES, 0, 3);
            gl::DisableVertexAttribArray(0);
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            gl::UseProgram(0);
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        }

        Ok(())
    }

    /// Display the generated gradient texture on a Wayland/EGL surface.
    pub fn draw(&self, surface: egl::Surface, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }

        self.make_current(surface)?;

        unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            gl::Viewport(0, 0, width as i32, height as i32);
            gl::UseProgram(self.display_program);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, self.gradient_texture);
            gl::Uniform1i(self.display_texture, 0);

            gl::BindBuffer(gl::ARRAY_BUFFER, self.fullscreen_vbo);
            gl::EnableVertexAttribArray(0);
            gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 0, std::ptr::null());
            gl::DrawArrays(gl::TRIANGLES, 0, 3);
            gl::DisableVertexAttribArray(0);
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            gl::BindTexture(gl::TEXTURE_2D, 0);
            gl::UseProgram(0);
        }

        self.egl.swap_buffers(self.display, surface).context("eglSwapBuffers failed")
    }
}

impl Drop for GlRenderer {
    fn drop(&mut self) {
        //let _ = self.egl.destroy_context(self.display, self.context);
        //let _ = self.egl.terminate(self.display);
    }
}

unsafe fn create_texture(resolution: u32) -> Result<gl::types::GLuint> {
    let mut texture = 0;
    gl::GenTextures(1, &mut texture);
    if texture == 0 { return Err(anyhow!("glGenTextures returned zero")); }
    gl::BindTexture(gl::TEXTURE_2D, texture);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
    gl::TexImage2D(
        gl::TEXTURE_2D, 0, gl::RGBA as i32,
        resolution as i32, resolution as i32, 0,
        gl::RGBA, gl::UNSIGNED_BYTE, std::ptr::null(),
    );
    gl::BindTexture(gl::TEXTURE_2D, 0);
    Ok(texture)
}

unsafe fn create_fbo(texture: gl::types::GLuint) -> Result<gl::types::GLuint> {
    let mut fbo = 0;
    gl::GenFramebuffers(1, &mut fbo);
    gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
    gl::FramebufferTexture2D(
        gl::FRAMEBUFFER, gl::COLOR_ATTACHMENT0,
        gl::TEXTURE_2D, texture, 0,
    );
    let status = gl::CheckFramebufferStatus(gl::FRAMEBUFFER);
    gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
    if status != gl::FRAMEBUFFER_COMPLETE {
        return Err(anyhow!("gradient framebuffer is incomplete: 0x{status:04x}"));
    }
    Ok(fbo)
}

unsafe fn create_fullscreen_vbo() -> gl::types::GLuint {
    let vertices: [f32; 6] = [-1.0, -1.0, 3.0, -1.0, -1.0, 3.0];
    let mut vbo = 0;
    gl::GenBuffers(1, &mut vbo);
    gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
    gl::BufferData(
        gl::ARRAY_BUFFER,
        (vertices.len() * std::mem::size_of::<f32>()) as isize,
        vertices.as_ptr() as *const c_void,
        gl::STATIC_DRAW,
    );
    gl::BindBuffer(gl::ARRAY_BUFFER, 0);
    vbo
}

unsafe fn uniform(program: gl::types::GLuint, name: &str) -> gl::types::GLint {
    let name = CString::new(name).unwrap();
    gl::GetUniformLocation(program, name.as_ptr())
}

unsafe fn compile_program(vertex_src: &str, fragment_src: &str) -> Result<gl::types::GLuint> {
    let vs = compile_shader(gl::VERTEX_SHADER, vertex_src)?;
    let fs = match compile_shader(gl::FRAGMENT_SHADER, fragment_src) {
        Ok(s) => s,
        Err(e) => { gl::DeleteShader(vs); return Err(e); }
    };
    let program = gl::CreateProgram();
    gl::AttachShader(program, vs);
    gl::AttachShader(program, fs);
    let attr = CString::new("a_pos").unwrap();
    gl::BindAttribLocation(program, 0, attr.as_ptr());
    gl::LinkProgram(program);
    let mut status = 0;
    gl::GetProgramiv(program, gl::LINK_STATUS, &mut status);
    gl::DeleteShader(vs);
    gl::DeleteShader(fs);
    if status != gl::TRUE as i32 {
        let log = program_log(program);
        gl::DeleteProgram(program);
        return Err(anyhow!("shader program link failed: {log}"));
    }
    Ok(program)
}

unsafe fn compile_shader(kind: gl::types::GLenum, source: &str) -> Result<gl::types::GLuint> {
    let shader = gl::CreateShader(kind);
    let source = CString::new(source).context("shader source contains NUL")?;
    gl::ShaderSource(shader, 1, &source.as_ptr(), std::ptr::null());
    gl::CompileShader(shader);
    let mut status = 0;
    gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut status);
    if status != gl::TRUE as i32 {
        let log = shader_log(shader);
        gl::DeleteShader(shader);
        return Err(anyhow!("shader compile failed: {log}"));
    }
    Ok(shader)
}

unsafe fn shader_log(shader: gl::types::GLuint) -> String {
    let mut len = 0;
    gl::GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut len);
    if len <= 0 { return String::new(); }
    let mut buf = vec![0u8; len as usize];
    gl::GetShaderInfoLog(shader, len, std::ptr::null_mut(), buf.as_mut_ptr() as *mut i8);
    String::from_utf8_lossy(&buf).trim_end_matches('\0').to_string()
}

unsafe fn program_log(program: gl::types::GLuint) -> String {
    let mut len = 0;
    gl::GetProgramiv(program, gl::INFO_LOG_LENGTH, &mut len);
    if len <= 0 { return String::new(); }
    let mut buf = vec![0u8; len as usize];
    gl::GetProgramInfoLog(program, len, std::ptr::null_mut(), buf.as_mut_ptr() as *mut i8);
    String::from_utf8_lossy(&buf).trim_end_matches('\0').to_string()
}
