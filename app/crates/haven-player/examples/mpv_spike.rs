//! Spike: mpv video inside a Slint window.
//!
//! mpv draws each frame into an OpenGL framebuffer (libmpv render API,
//! `vo=libmpv`). The framebuffer's texture is handed to Slint as an `Image`
//! (`BorrowedOpenGLTextureBuilder`), so normal Slint items can sit on top.
//! No native child window (`wid`) is used.
//!
//! Run from `app/`:
//!
//! ```text
//! cargo run -p haven-player --features mpv-spike --example mpv_spike -- <file or URL>
//! ```
//!
//! Self-check without looking at the screen:
//! `HAVEN_SPIKE_SNAPSHOT=<out.png>` saves the window (and `<out>.fbo.png`,
//! the raw mpv frame) after `HAVEN_SPIKE_SNAPSHOT_AT` seconds (default 4);
//! `HAVEN_SPIKE_QUIT_AFTER=<seconds>` closes the window by itself;
//! `HAVEN_SPIKE_SELFTEST=1` presses pause, seeks and resizes through the
//! same callbacks the controls use and prints PASS/FAIL lines.
//! (Taking the window snapshot blocks the UI thread for a few seconds in a
//! debug build, so frames are dropped around it: that is the hook, not mpv.)

use std::cell::RefCell;
use std::error::Error;
use std::ffi::{CStr, CString, c_void};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use glow::HasContext;
use libmpv2::Mpv;
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};
use slint::{BorrowedOpenGLTextureBuilder, ComponentHandle, GraphicsAPI, RenderingState};

slint::slint! {
    import { Button, Slider } from "std-widgets.slint";

    export component SpikeWindow inherits Window {
        title: "Haven video spike";
        preferred-width: 960px;
        preferred-height: 540px;
        background: black;

        in property <image> video-frame;
        in property <bool> paused;
        in property <float> duration: 1;
        in property <string> time-text: "0:00 / 0:00";
        in property <string> status;
        in-out property <float> position <=> slider.value;
        callback toggle-pause();
        callback seek(float);
        callback slider-moved();

        // The video. mpv letterboxes inside the frame, so "fill" keeps the shape.
        Image {
            width: 100%;
            height: 100%;
            source: root.video-frame;
            image-fit: fill;
        }

        Text {
            x: 12px;
            y: 10px;
            text: root.status;
            color: white;
            font-size: 12px;
        }

        // Controls drawn by Slint on top of the video.
        Rectangle {
            y: parent.height - self.height;
            height: 56px;
            background: #000000b0;

            HorizontalLayout {
                padding: 10px;
                spacing: 12px;

                Button {
                    text: root.paused ? "Play" : "Pause";
                    clicked => { root.toggle-pause(); }
                }

                slider := Slider {
                    minimum: 0;
                    maximum: root.duration;
                    changed => { root.slider-moved(); }
                    released(value) => { root.seek(value); }
                }

                Text {
                    text: root.time-text;
                    color: white;
                    vertical-alignment: center;
                }
            }
        }
    }
}

type ProcAddressFn<'a> = dyn Fn(&CStr) -> *const c_void + 'a;

/// Slint's OpenGL symbol lookup, lent to mpv while it creates its render
/// context. mpv resolves every GL function during that call and libmpv2
/// frees these parameters right after it, so the pointer never outlives the
/// rendering callback it came from.
struct ProcLoader(*const ProcAddressFn<'static>);

fn mpv_gl_lookup(loader: &ProcLoader, name: &str) -> *mut c_void {
    let Ok(name) = CString::new(name) else {
        return std::ptr::null_mut();
    };
    // SAFETY: see `ProcLoader`: only called inside `create_render_context`.
    unsafe { (*loader.0)(&name) as *mut c_void }
}

/// The framebuffer mpv draws into, and its colour texture that Slint shows.
struct Target {
    fbo: glow::NativeFramebuffer,
    texture: glow::NativeTexture,
    width: u32,
    height: u32,
}

impl Target {
    /// `width` and `height` are physical pixels.
    unsafe fn new(gl: &glow::Context, width: u32, height: u32) -> Result<Self, String> {
        unsafe {
            let texture = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            for (name, value) in [
                (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, name, value as i32);
            }
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );

            let fbo = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            if status != glow::FRAMEBUFFER_COMPLETE {
                gl.delete_framebuffer(fbo);
                gl.delete_texture(texture);
                return Err(format!("framebuffer incomplete: 0x{status:x}"));
            }
            Ok(Self { fbo, texture, width, height })
        }
    }

    unsafe fn delete(self, gl: &glow::Context) {
        unsafe {
            gl.delete_framebuffer(self.fbo);
            gl.delete_texture(self.texture);
        }
    }

    fn image(&self) -> slint::Image {
        // SAFETY: the texture was made in this window's GL context and is
        // only deleted after the UI stops showing it.
        unsafe {
            BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(
                self.texture.0,
                (self.width, self.height).into(),
            )
        }
        .build()
    }
}

const TEXTURE_UNITS: u32 = 4;
const CAPS: [u32; 5] =
    [glow::BLEND, glow::SCISSOR_TEST, glow::DEPTH_TEST, glow::STENCIL_TEST, glow::CULL_FACE];

/// The OpenGL state that our framebuffer work and mpv's renderer change.
/// Slint's renderer shares this context and expects to find it untouched.
struct GlState {
    draw_fbo: i32,
    read_fbo: i32,
    viewport: [i32; 4],
    scissor_box: [i32; 4],
    active_texture: i32,
    textures_2d: [i32; TEXTURE_UNITS as usize],
    program: i32,
    vertex_array: i32,
    array_buffer: i32,
    pixel_unpack_buffer: i32,
    pixel_pack_buffer: i32,
    unpack_alignment: i32,
    blend_func: [i32; 4],
    caps: [bool; CAPS.len()],
}

fn name(id: i32) -> Option<NonZeroU32> {
    NonZeroU32::new(id as u32)
}

impl GlState {
    unsafe fn save(gl: &glow::Context) -> Self {
        unsafe {
            let mut viewport = [0; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
            let mut scissor_box = [0; 4];
            gl.get_parameter_i32_slice(glow::SCISSOR_BOX, &mut scissor_box);
            let active_texture = gl.get_parameter_i32(glow::ACTIVE_TEXTURE);
            let mut textures_2d = [0; TEXTURE_UNITS as usize];
            for unit in 0..TEXTURE_UNITS {
                gl.active_texture(glow::TEXTURE0 + unit);
                textures_2d[unit as usize] = gl.get_parameter_i32(glow::TEXTURE_BINDING_2D);
            }
            gl.active_texture(active_texture as u32);
            Self {
                draw_fbo: gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING),
                read_fbo: gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING),
                viewport,
                scissor_box,
                active_texture,
                textures_2d,
                program: gl.get_parameter_i32(glow::CURRENT_PROGRAM),
                vertex_array: gl.get_parameter_i32(glow::VERTEX_ARRAY_BINDING),
                array_buffer: gl.get_parameter_i32(glow::ARRAY_BUFFER_BINDING),
                pixel_unpack_buffer: gl.get_parameter_i32(glow::PIXEL_UNPACK_BUFFER_BINDING),
                pixel_pack_buffer: gl.get_parameter_i32(glow::PIXEL_PACK_BUFFER_BINDING),
                unpack_alignment: gl.get_parameter_i32(glow::UNPACK_ALIGNMENT),
                blend_func: [
                    gl.get_parameter_i32(glow::BLEND_SRC_RGB),
                    gl.get_parameter_i32(glow::BLEND_DST_RGB),
                    gl.get_parameter_i32(glow::BLEND_SRC_ALPHA),
                    gl.get_parameter_i32(glow::BLEND_DST_ALPHA),
                ],
                caps: CAPS.map(|cap| gl.is_enabled(cap)),
            }
        }
    }

    unsafe fn restore(&self, gl: &glow::Context) {
        unsafe {
            gl.bind_framebuffer(
                glow::DRAW_FRAMEBUFFER,
                name(self.draw_fbo).map(glow::NativeFramebuffer),
            );
            gl.bind_framebuffer(
                glow::READ_FRAMEBUFFER,
                name(self.read_fbo).map(glow::NativeFramebuffer),
            );
            let [x, y, w, h] = self.viewport;
            gl.viewport(x, y, w, h);
            let [x, y, w, h] = self.scissor_box;
            gl.scissor(x, y, w, h);
            for unit in 0..TEXTURE_UNITS {
                gl.active_texture(glow::TEXTURE0 + unit);
                gl.bind_texture(
                    glow::TEXTURE_2D,
                    name(self.textures_2d[unit as usize]).map(glow::NativeTexture),
                );
            }
            gl.active_texture(self.active_texture as u32);
            gl.use_program(name(self.program).map(glow::NativeProgram));
            gl.bind_vertex_array(name(self.vertex_array).map(glow::NativeVertexArray));
            gl.bind_buffer(glow::ARRAY_BUFFER, name(self.array_buffer).map(glow::NativeBuffer));
            gl.bind_buffer(
                glow::PIXEL_UNPACK_BUFFER,
                name(self.pixel_unpack_buffer).map(glow::NativeBuffer),
            );
            gl.bind_buffer(
                glow::PIXEL_PACK_BUFFER,
                name(self.pixel_pack_buffer).map(glow::NativeBuffer),
            );
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, self.unpack_alignment);
            let [src_rgb, dst_rgb, src_alpha, dst_alpha] = self.blend_func.map(|v| v as u32);
            gl.blend_func_separate(src_rgb, dst_rgb, src_alpha, dst_alpha);
            for (cap, on) in CAPS.iter().zip(self.caps) {
                if on {
                    gl.enable(*cap);
                } else {
                    gl.disable(*cap);
                }
            }
        }
    }
}

/// Everything the window's rendering callback owns. Lives on the UI thread.
struct Player {
    /// File path or URL to play.
    source: String,
    /// mpv is created on a worker thread and arrives through this channel.
    incoming: mpsc::Receiver<Result<Mpv, String>>,
    /// Leaked so the render context can borrow it; freed in `teardown`.
    mpv: Option<&'static Mpv>,
    gl: Option<glow::Context>,
    render: Option<RenderContext<'static>>,
    target: Option<Target>,
    frames: u64,
    /// Longest single mpv render call since the last stats line.
    slowest_render: Duration,
    /// Set to save the next mpv frame straight from the framebuffer.
    dump_fbo_to: Option<String>,
    failed: bool,
    /// The thread that ends mpv after teardown.
    ending: Option<std::thread::JoinHandle<()>>,
}

impl Player {
    fn setup(&mut self, api: &GraphicsAPI) -> Result<(), String> {
        let GraphicsAPI::NativeOpenGL { get_proc_address } = api else {
            return Err(format!("Slint is not rendering with OpenGL ({api:?})"));
        };
        // SAFETY: Slint made its GL context current for this callback.
        let gl = unsafe { glow::Context::from_loader_function_cstr(|s| get_proc_address(s)) };
        unsafe {
            eprintln!(
                "spike: Slint renders with OpenGL: {} ({})",
                gl.get_parameter_string(glow::VERSION),
                gl.get_parameter_string(glow::RENDERER),
            );
        }
        self.gl = Some(gl);
        Ok(())
    }

    /// Creates mpv's render context once both mpv and the GL context exist.
    fn connect_mpv(&mut self, api: &GraphicsAPI, ui: &slint::Weak<SpikeWindow>) -> Result<(), String> {
        if self.render.is_some() || self.gl.is_none() {
            return Ok(());
        }
        if self.mpv.is_none() {
            match self.incoming.try_recv() {
                Ok(Ok(mpv)) => self.mpv = Some(Box::leak(Box::new(mpv))),
                Ok(Err(err)) => return Err(err),
                Err(_) => return Ok(()), // not ready yet
            }
        }
        let (Some(mpv), Some(gl)) = (self.mpv, self.gl.as_ref()) else {
            return Ok(());
        };
        let GraphicsAPI::NativeOpenGL { get_proc_address } = api else {
            return Err("OpenGL went away".into());
        };
        // SAFETY: only the lifetime changes; see `ProcLoader`.
        let loader = ProcLoader(unsafe {
            std::mem::transmute::<*const ProcAddressFn<'_>, *const ProcAddressFn<'static>>(
                *get_proc_address,
            )
        });

        // SAFETY: GL context is current; state is put back before Slint draws.
        let saved = unsafe { GlState::save(gl) };
        let created = mpv.create_render_context(vec![
            RenderParam::ApiType(RenderParamApiType::OpenGl),
            RenderParam::InitParams(OpenGLInitParams { get_proc_address: mpv_gl_lookup, ctx: loader }),
        ]);
        unsafe { saved.restore(gl) };
        let mut render = created.map_err(|err| format!("mpv render context: {err}"))?;

        // mpv calls this from its own threads when a new frame is ready.
        let ui = ui.clone();
        render.set_update_callback(move || {
            let _ = ui.upgrade_in_event_loop(|ui| ui.window().request_redraw());
        });
        self.render = Some(render);
        eprintln!("spike: mpv render context created (OpenGL, no wid)");
        // Only now: a file loaded before the render context exists has
        // nowhere to draw and mpv plays it without video.
        mpv.command("loadfile", &[&self.source, "replace"])
            .map_err(|err| format!("mpv loadfile: {err}"))
    }

    /// Lets mpv draw the current frame into the framebuffer.
    fn draw(&mut self, ui: &SpikeWindow) -> Result<(), String> {
        let (Some(gl), Some(render)) = (self.gl.as_ref(), self.render.as_ref()) else {
            return Ok(());
        };
        // Physical pixels: on a 2x screen this is twice the logical size.
        let size = ui.window().size();
        let (width, height) = (size.width.max(1), size.height.max(1));

        // SAFETY: GL context is current; state is put back before Slint draws.
        unsafe {
            let saved = GlState::save(gl);
            let result = (|| {
                if self.target.as_ref().is_none_or(|t| (t.width, t.height) != (width, height)) {
                    ui.set_video_frame(slint::Image::default());
                    if let Some(old) = self.target.take() {
                        old.delete(gl);
                    }
                    self.target = Some(Target::new(gl, width, height)?);
                    eprintln!(
                        "spike: framebuffer {width}x{height} physical px (scale factor {})",
                        ui.window().scale_factor()
                    );
                }
                let target = self.target.as_ref().expect("target was just made");
                // Not flipped: row 0 of the texture is the top of the picture,
                // which is what Slint's default (top-left) origin expects.
                let started = Instant::now();
                render
                    .render::<ProcLoader>(
                        target.fbo.0.get() as i32,
                        width as i32,
                        height as i32,
                        false,
                    )
                    .map_err(|err| format!("mpv render: {err}"))?;
                self.slowest_render = self.slowest_render.max(started.elapsed());
                self.frames += 1;
                if let Some(path) = self.dump_fbo_to.take() {
                    let mut pixels = vec![0u8; (width * height * 4) as usize];
                    gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.fbo));
                    gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
                    gl.read_pixels(
                        0,
                        0,
                        width as i32,
                        height as i32,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::Slice(Some(&mut pixels)),
                    );
                    match save_png(&path, width, height, &pixels) {
                        Ok(()) => eprintln!("spike: saved mpv framebuffer to {path}"),
                        Err(err) => eprintln!("spike: could not save {path}: {err}"),
                    }
                }
                ui.set_video_frame(target.image());
                Ok(())
            })();
            saved.restore(gl);
            result
        }
    }

    /// Frees mpv's render context and our GL objects while the GL context
    /// is still current, then ends mpv off the UI thread.
    ///
    /// `gl_current` is true inside RenderingTeardown. Without a current GL
    /// context the GL objects are left to die with the context instead.
    fn teardown(
        &mut self,
        ui: Option<&SpikeWindow>,
        gl_current: bool,
    ) -> Option<std::thread::JoinHandle<()>> {
        if let Some(ui) = ui {
            ui.set_video_frame(slint::Image::default());
        }
        if let Some(render) = self.render.take() {
            if gl_current {
                drop(render);
                eprintln!("spike: mpv render context freed ({} frames drawn)", self.frames);
            } else {
                std::mem::forget(render);
            }
        }
        if let (Some(gl), Some(target), true) = (self.gl.as_ref(), self.target.take(), gl_current) {
            // SAFETY: called from RenderingTeardown, GL context is current.
            unsafe { target.delete(gl) };
        }
        self.gl = None;
        let mpv = self.mpv.take()?;
        // SAFETY: made by Box::leak in `connect_mpv`; the render context that
        // borrowed it was dropped above and nothing else keeps the reference.
        let mpv = unsafe { Box::from_raw(std::ptr::from_ref(mpv).cast_mut()) };
        Some(std::thread::spawn(move || drop(mpv)))
    }
}

fn save_png(path: &str, width: u32, height: u32, rgba: &[u8]) -> Result<(), Box<dyn Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

fn env_seconds(name: &str) -> Option<Duration> {
    std::env::var(name).ok()?.parse::<f64>().ok().map(Duration::from_secs_f64)
}

fn main() -> Result<(), Box<dyn Error>> {
    let Some(source) = std::env::args().nth(1) else {
        eprintln!("usage: mpv_spike <video file or URL>");
        std::process::exit(2);
    };

    // Must run before the first window exists, or Slint may pick a renderer
    // that is not OpenGL and mpv would have nothing to draw with.
    slint::BackendSelector::new().require_opengl().select()?;

    let ui = SpikeWindow::new()?;
    ui.set_status("Starting mpv…".into());

    // mpv is created on its own thread: on macOS creating it on the main
    // thread can deadlock against the main dispatch queue.
    let (sender, incoming) = mpsc::channel();
    {
        let ui = ui.as_weak();
        std::thread::spawn(move || {
            let made = Mpv::with_initializer(|init| {
                init.set_property("vo", "libmpv")?;
                // Decode on the GPU where possible, copy frames back to
                // memory: no GL interop needed, so it works everywhere.
                init.set_property("hwdec", "auto-copy")?;
                // Don't let mpv's renderer sleep on the UI thread.
                init.set_property("video-timing-offset", "0")?;
                init.set_property("terminal", "yes")?;
                init.set_property("msg-level", "all=warn")?;
                Ok(())
            })
            .map_err(|err| format!("mpv: {err}"));
            let _ = sender.send(made);
            let _ = ui.upgrade_in_event_loop(|ui| ui.window().request_redraw());
        });
    }

    let player = Rc::new(RefCell::new(Player {
        source,
        incoming,
        mpv: None,
        gl: None,
        render: None,
        target: None,
        frames: 0,
        slowest_render: Duration::ZERO,
        dump_fbo_to: None,
        failed: false,
        ending: None,
    }));

    {
        let player = player.clone();
        let ui_weak = ui.as_weak();
        ui.window().set_rendering_notifier(move |state, api| {
            let mut player = player.borrow_mut();
            let ui = ui_weak.upgrade();
            let step = match state {
                RenderingState::RenderingSetup => player.setup(api),
                RenderingState::BeforeRendering if !player.failed => {
                    player.connect_mpv(api, &ui_weak).and_then(|()| match &ui {
                        Some(ui) => player.draw(ui),
                        None => Ok(()),
                    })
                }
                RenderingState::RenderingTeardown => {
                    player.ending = player.teardown(ui.as_ref(), true);
                    Ok(())
                }
                _ => Ok(()),
            };
            if let Err(err) = step {
                eprintln!("spike: {err}");
                player.failed = true;
                if let Some(ui) = &ui {
                    ui.set_status(err.into());
                }
            }
        })?;
    }

    {
        let player = player.clone();
        let ui_weak = ui.as_weak();
        ui.on_toggle_pause(move || {
            let (Some(mpv), Some(ui)) = (player.borrow().mpv, ui_weak.upgrade()) else {
                return;
            };
            let paused = !mpv.get_property::<bool>("pause").unwrap_or(false);
            if mpv.set_property("pause", paused).is_ok() {
                ui.set_paused(paused);
            }
        });
    }

    // While the slider is being dragged the position timer must not fight it.
    let slider_touched = Rc::new(RefCell::new(None::<Instant>));
    {
        let slider_touched = slider_touched.clone();
        ui.on_slider_moved(move || *slider_touched.borrow_mut() = Some(Instant::now()));
    }
    {
        let player = player.clone();
        let slider_touched = slider_touched.clone();
        ui.on_seek(move |seconds| {
            *slider_touched.borrow_mut() = Some(Instant::now());
            if let Some(mpv) = player.borrow().mpv {
                let _ = mpv.command("seek", &[&format!("{seconds:.2}"), "absolute"]);
            }
        });
    }

    // Position, duration and a line of stats, four times a second.
    let poll = slint::Timer::default();
    {
        let player = player.clone();
        let ui_weak = ui.as_weak();
        let mut last_log = Instant::now();
        let mut last_frames = 0;
        poll.start(slint::TimerMode::Repeated, Duration::from_millis(250), move || {
            let (Some(mpv), Some(ui)) = (player.borrow().mpv, ui_weak.upgrade()) else {
                return;
            };
            let poll_started = Instant::now();
            // Empty mpv's event queue so it never backs up.
            loop {
                match mpv.wait_event(0.0) {
                    Some(Ok(libmpv2::events::Event::EndFile(reason))) => {
                        eprintln!("spike: playback ended ({reason:?})");
                    }
                    Some(Ok(_)) => {}
                    Some(Err(err)) => eprintln!("spike: mpv event error: {err}"),
                    None => break,
                }
            }
            let position = mpv.get_property::<f64>("time-pos").unwrap_or(0.0);
            let duration = mpv.get_property::<f64>("duration").unwrap_or(0.0);
            ui.set_duration(duration.max(1.0) as f32);
            ui.set_paused(mpv.get_property::<bool>("pause").unwrap_or(false));
            ui.set_time_text(format!("{} / {}", clock(position), clock(duration)).into());
            let dragging =
                slider_touched.borrow().is_some_and(|at| at.elapsed() < Duration::from_millis(700));
            if !dragging {
                ui.set_position(position as f32);
            }

            let hwdec = mpv.get_property::<String>("hwdec-current").unwrap_or_default();
            let width = mpv.get_property::<i64>("video-params/w").unwrap_or(0);
            let height = mpv.get_property::<i64>("video-params/h").unwrap_or(0);
            if !player.borrow().failed {
                ui.set_status(format!("{width}x{height}  hwdec: {hwdec}").into());
            }
            if last_log.elapsed() >= Duration::from_secs(2) {
                let frames = player.borrow().frames;
                let slowest_render = std::mem::take(&mut player.borrow_mut().slowest_render);
                eprintln!(
                    "spike: t={position:.2}s  drawn {:.1} fps  avsync={:+.3}s  dropped={}  \
                     video {width}x{height}  hwdec={hwdec}  audio={}  \
                     slowest render {:.1} ms  poll {:.1} ms",
                    (frames - last_frames) as f64 / last_log.elapsed().as_secs_f64(),
                    mpv.get_property::<f64>("avsync").unwrap_or(f64::NAN),
                    mpv.get_property::<i64>("frame-drop-count").unwrap_or(-1),
                    mpv.get_property::<String>("audio-codec-name").unwrap_or_else(|_| "none".into()),
                    slowest_render.as_secs_f64() * 1000.0,
                    poll_started.elapsed().as_secs_f64() * 1000.0,
                );
                last_frames = frames;
                last_log = Instant::now();
            }
        });
    }

    // Optional self-check hooks (see the top of this file).
    let snapshot = slint::Timer::default();
    if let Ok(path) = std::env::var("HAVEN_SPIKE_SNAPSHOT") {
        let player = player.clone();
        let ui_weak = ui.as_weak();
        let at = env_seconds("HAVEN_SPIKE_SNAPSHOT_AT").unwrap_or(Duration::from_secs(4));
        snapshot.start(slint::TimerMode::SingleShot, at, move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            player.borrow_mut().dump_fbo_to = Some(format!("{path}.fbo.png"));
            ui.window().request_redraw();
            match ui.window().take_snapshot() {
                Ok(shot) => match save_png(&path, shot.width(), shot.height(), shot.as_bytes()) {
                    Ok(()) => eprintln!("spike: saved window snapshot to {path}"),
                    Err(err) => eprintln!("spike: could not save {path}: {err}"),
                },
                Err(err) => eprintln!("spike: window snapshot failed: {err}"),
            }
        });
    }
    let selftest = slint::Timer::default();
    if std::env::var_os("HAVEN_SPIKE_SELFTEST").is_some() {
        let player = player.clone();
        let ui_weak = ui.as_weak();
        let mut step = 0;
        let mut held_at = 0.0;
        selftest.start(slint::TimerMode::Repeated, Duration::from_secs(1), move || {
            let (Some(mpv), Some(ui)) = (player.borrow().mpv, ui_weak.upgrade()) else {
                return;
            };
            let position = mpv.get_property::<f64>("time-pos").unwrap_or(-1.0);
            let paused = mpv.get_property::<bool>("pause").unwrap_or(false);
            let verdict = |ok: bool| if ok { "PASS" } else { "FAIL" };
            step += 1;
            match step {
                2 => ui.invoke_toggle_pause(),
                3 => {
                    held_at = position;
                    eprintln!("selftest: {} pause button pauses (t={position:.2})", verdict(paused));
                }
                4 => {
                    let held = (position - held_at).abs() < 0.05;
                    eprintln!("selftest: {} picture holds while paused", verdict(held));
                    ui.invoke_toggle_pause();
                }
                5 => {
                    eprintln!(
                        "selftest: {} play resumes (t={position:.2})",
                        verdict(!paused && position > held_at + 0.3)
                    );
                    ui.invoke_seek(12.0);
                }
                6 => {
                    eprintln!(
                        "selftest: {} seek to 12 s lands (t={position:.2})",
                        verdict((12.0..14.5).contains(&position))
                    );
                    ui.window().set_size(slint::PhysicalSize::new(1400, 1200));
                }
                7 => {
                    let size = ui.window().size();
                    let target = player.borrow().target.as_ref().map(|t| (t.width, t.height));
                    eprintln!(
                        "selftest: {} framebuffer follows window resize (window {}x{}, framebuffer {target:?})",
                        verdict(target == Some((size.width, size.height))),
                        size.width,
                        size.height,
                    );
                }
                _ => {}
            }
        });
    }
    let quit = slint::Timer::default();
    if let Some(after) = env_seconds("HAVEN_SPIKE_QUIT_AFTER") {
        quit.start(slint::TimerMode::SingleShot, after, || {
            let _ = slint::quit_event_loop();
        });
    }

    ui.run()?;

    // Dropping the window destroys its renderer, which runs
    // RenderingTeardown with the GL context current.
    drop((poll, snapshot, selftest, quit));
    drop(ui);
    let mut player = player.borrow_mut();
    if player.render.is_some() {
        eprintln!("spike: warning: RenderingTeardown did not run; render context leaked");
        player.ending = player.teardown(None, false);
    }
    // The UI thread is done, so waiting for mpv to end cannot deadlock it.
    if let Some(ending) = player.ending.take() {
        let _ = ending.join();
        eprintln!("spike: mpv ended");
    }
    Ok(())
}
