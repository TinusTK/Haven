//! mpv video inside a Slint window.
//!
//! mpv draws each frame into an OpenGL framebuffer (libmpv render API,
//! `vo=libmpv`). The framebuffer's texture is handed to Slint as an `Image`
//! (`BorrowedOpenGLTextureBuilder`), so normal Slint items sit on top. No
//! native child window (`wid`) is used.
//!
//! How to use it, all on the UI thread:
//!
//! 1. Before the first window exists:
//!    `slint::BackendSelector::new().require_opengl().select()`.
//! 2. Make a [`Video`] and call [`Video::rendering`] from the window's
//!    rendering notifier, for every state.
//! 3. [`Video::play`] an address; a few times a second call
//!    [`Video::poll`] and [`Video::status`].
//! 4. After the event loop ends, drop the window (so the renderer is torn
//!    down with its OpenGL context current) and then call [`Video::finish`].
//!
//! Proven by `examples/mpv_spike.rs`, which this was lifted from.

use std::ffi::{CStr, CString, c_void};
use std::num::NonZeroU32;
use std::sync::{Arc, mpsc};

use glow::HasContext;
use libmpv2::Mpv;
use libmpv2::render::RenderContext;
use libmpv2::render::{OpenGLInitParams, RenderParam, RenderParamApiType};
use slint::BorrowedOpenGLTextureBuilder;
use slint::{GraphicsAPI, RenderingState};

use crate::tracks::{Track, parse_tracks};

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

/// Texture slots saved and put back around mpv's drawing. mpv uses more
/// than the first few (its dither and lookup textures), and a renderer
/// that remembers which texture sits in each slot (Skia) would otherwise
/// draw the wrong picture.
const TEXTURE_UNITS: u32 = 32;
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
    units: u32,
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
            let units = (gl.get_parameter_i32(glow::MAX_COMBINED_TEXTURE_IMAGE_UNITS).max(1) as u32)
                .min(TEXTURE_UNITS);
            let mut textures_2d = [0; TEXTURE_UNITS as usize];
            for unit in 0..units {
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
                units,
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
            for unit in 0..self.units {
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

/// mpv's reason for giving up on a file, in words.
fn reason(err: &libmpv2::Error) -> String {
    // The numbers are mpv's `mpv_error` codes.
    match err {
        libmpv2::Error::Raw(-13) => "the address could not be opened".to_owned(),
        libmpv2::Error::Raw(-17) => "the format was not recognised".to_owned(),
        libmpv2::Error::Raw(-15 | -16) => "there was no picture or sound in it".to_owned(),
        libmpv2::Error::Raw(code) => format!("mpv error {code}"),
        other => other.to_string(),
    }
}

/// What mpv reported since the last [`Video::poll`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The file is open and about to play.
    Loaded,
    /// Played to the end.
    Finished,
    /// Could not be opened, or broke off; the text is mpv's reason.
    Failed(String),
}

/// Where playback stands right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Status {
    /// Seconds from the start.
    pub position: f64,
    /// Running time in seconds; 0 while unknown and for live streams.
    pub duration: f64,
    pub paused: bool,
    /// Waiting for more of the stream to arrive.
    pub buffering: bool,
    /// How fast the stream is arriving, in bytes a second, if known.
    pub arriving: Option<f64>,
    /// How many seconds past the position have already arrived.
    pub ahead: Option<f64>,
}

/// How the picture fills the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    /// All of the picture, with bars where the window's shape differs.
    #[default]
    Whole,
    /// The window filled, the picture's edges cut off.
    Fill,
    /// The window filled, the picture pulled out of shape.
    Stretch,
}

/// What mpv knows about what is playing. Anything may be unknown.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Details {
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub frames_per_second: Option<f64>,
    pub video: Option<String>,
    pub audio: Option<String>,
    pub channels: Option<String>,
    pub container: Option<String>,
    /// The graphics card's decoder in use, if any.
    pub decoder: Option<String>,
}

struct Start {
    address: String,
    /// Seconds to start at, or from the beginning.
    at: Option<f64>,
}

/// The player. Lives on the UI thread.
pub struct Video {
    /// Asks the window to draw again; called from mpv's threads too.
    wake: Arc<dyn Fn() + Send + Sync>,
    /// mpv is created on a worker thread and arrives through this channel.
    incoming: Option<mpsc::Receiver<Result<Mpv, String>>>,
    /// Leaked so the render context can borrow it; freed in `teardown`.
    mpv: Option<&'static Mpv>,
    gl: Option<glow::Context>,
    render: Option<RenderContext<'static>>,
    target: Option<Target>,
    /// What to load as soon as mpv can draw.
    waiting: Option<Start>,
    /// Something is loaded (or about to be): frames are drawn.
    showing: bool,
    /// Setting up failed; nothing more is tried.
    broken: bool,
    /// The thread that ends mpv after teardown.
    ending: Option<std::thread::JoinHandle<()>>,
}

impl Video {
    /// `wake` must ask the window to redraw and may be called from any
    /// thread. mpv itself is only started by the first [`Video::play`].
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            wake: Arc::new(wake),
            incoming: None,
            mpv: None,
            gl: None,
            render: None,
            target: None,
            waiting: None,
            showing: false,
            broken: false,
            ending: None,
        }
    }

    /// Starts mpv on its own thread: on macOS creating it on the main
    /// thread can deadlock against the main dispatch queue.
    fn start_mpv(&mut self) {
        if self.incoming.is_some() || self.mpv.is_some() {
            return;
        }
        let (sender, incoming) = mpsc::channel();
        self.incoming = Some(incoming);
        let wake = self.wake.clone();
        // mpv puts its stream cache here but does not make the folder.
        let cache = std::env::temp_dir().join("haven-stream");
        let _ = std::fs::create_dir_all(&cache);
        std::thread::spawn(move || {
            let made = Mpv::with_initializer(|init| {
                init.set_property("vo", "libmpv")?;
                // Decode on the GPU where possible, copy frames back to
                // memory: no GL interop needed, so it works everywhere.
                init.set_property("hwdec", "auto-copy")?;
                // Don't let mpv's renderer sleep on the UI thread.
                init.set_property("video-timing-offset", "0")?;
                // The volume may go up to 200% (mpv stops at 130% unless told).
                init.set_property("volume-max", "200")?;
                // Addresses are opened as they are: no helper programs.
                // A libmpv built without scripting (the one shipped on
                // macOS) has no such option, which comes to the same;
                // the rest are niceties an older libmpv may not know.
                let _ = init.set_property("ytdl", "no");
                let _ = init.set_property("sub-auto", "no");
                let _ = init.set_property("audio-display", "no");
                let _ = init.set_property("network-timeout", "30");
                // Fetch the whole film, not just the next minutes: the
                // read-ahead goes to a file on disk (removed when the
                // film closes) instead of a small memory buffer, and
                // what has been watched is kept for going back.
                let _ = init.set_property("cache", "yes");
                let _ = init.set_property("cache-on-disk", "yes");
                let _ = init.set_property("demuxer-cache-dir", cache.display().to_string());
                let _ = init.set_property("demuxer-max-bytes", "200GiB");
                let _ = init.set_property("demuxer-max-back-bytes", "200GiB");
                let _ = init.set_property("cache-secs", "1000000");
                let _ = init.set_property("demuxer-readahead-secs", "1000000");
                if cfg!(debug_assertions) {
                    let _ = init.set_property("terminal", "yes");
                    let _ = init.set_property("msg-level", "all=warn");
                }
                Ok(())
            })
            .map_err(|err| format!("mpv could not start: {err}"));
            let _ = sender.send(made);
            wake();
        });
    }

    /// Call from the window's rendering notifier, for every state. `size`
    /// is the window's size in physical pixels; `show` receives the picture
    /// to put on screen (an empty image when there is none).
    ///
    /// An error means the player cannot work on this machine (no OpenGL,
    /// or mpv would not start); it is returned once.
    pub fn rendering(
        &mut self,
        state: RenderingState,
        api: &GraphicsAPI,
        size: (u32, u32),
        show: &mut dyn FnMut(slint::Image),
    ) -> Result<(), String> {
        let step = match state {
            RenderingState::RenderingSetup => self.setup(api),
            RenderingState::BeforeRendering if !self.broken => {
                self.connect(api).and_then(|()| self.draw(size, show))
            }
            RenderingState::RenderingTeardown => {
                show(slint::Image::default());
                self.teardown(true);
                Ok(())
            }
            _ => Ok(()),
        };
        if step.is_err() {
            self.broken = true;
            self.showing = false;
            self.waiting = None;
        }
        step
    }

    fn setup(&mut self, api: &GraphicsAPI) -> Result<(), String> {
        let GraphicsAPI::NativeOpenGL { get_proc_address } = api else {
            return Err(format!("the window is not drawn with OpenGL ({api:?})"));
        };
        // SAFETY: Slint made its GL context current for this callback.
        self.gl =
            Some(unsafe { glow::Context::from_loader_function_cstr(|s| get_proc_address(s)) });
        Ok(())
    }

    /// Creates mpv's render context once both mpv and the GL context
    /// exist, then loads whatever was waiting for it.
    fn connect(&mut self, api: &GraphicsAPI) -> Result<(), String> {
        if self.render.is_none() && self.gl.is_some() {
            if self.mpv.is_none() {
                let Some(incoming) = &self.incoming else { return Ok(()) };
                match incoming.try_recv() {
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
                RenderParam::InitParams(OpenGLInitParams {
                    get_proc_address: mpv_gl_lookup,
                    ctx: loader,
                }),
            ]);
            unsafe { saved.restore(gl) };
            let mut render = created.map_err(|err| format!("mpv render context: {err}"))?;

            // mpv calls this from its own threads when a new frame is ready.
            let wake = self.wake.clone();
            render.set_update_callback(move || wake());
            self.render = Some(render);
        }
        // Only now: a file loaded before the render context exists has
        // nowhere to draw and mpv plays it without video.
        if self.render.is_some() {
            if let Some(start) = self.waiting.take() {
                self.load(&start)?;
            }
        }
        Ok(())
    }

    fn load(&self, start: &Start) -> Result<(), String> {
        let Some(mpv) = self.mpv else { return Ok(()) };
        let at = start.at.map_or_else(|| "none".to_owned(), |seconds| format!("{seconds:.1}"));
        mpv.set_property("start", at.as_str()).map_err(|err| format!("mpv start: {err}"))?;
        mpv.set_property("pause", false).map_err(|err| format!("mpv pause: {err}"))?;
        mpv.command("loadfile", &[&start.address, "replace"])
            .map_err(|err| format!("mpv loadfile: {err}"))
    }

    /// Lets mpv draw the current frame into the framebuffer.
    fn draw(&mut self, size: (u32, u32), show: &mut dyn FnMut(slint::Image)) -> Result<(), String> {
        if !self.showing {
            return Ok(());
        }
        let (Some(gl), Some(render)) = (self.gl.as_ref(), self.render.as_ref()) else {
            return Ok(());
        };
        // Physical pixels: on a 2x screen this is twice the logical size.
        let (width, height) = (size.0.max(1), size.1.max(1));

        // SAFETY: GL context is current; state is put back before Slint draws.
        unsafe {
            let saved = GlState::save(gl);
            let result = (|| {
                let made = self.target.as_ref().is_none_or(|t| (t.width, t.height) != (width, height));
                if made {
                    show(slint::Image::default());
                    if let Some(old) = self.target.take() {
                        old.delete(gl);
                    }
                    self.target = Some(Target::new(gl, width, height)?);
                }
                // Only when mpv has a new picture: the window also redraws
                // for the pages (scrolling under the mini player, hovering),
                // and drawing the video again each time made it lag.
                let new_frame = render
                    .update()
                    .map_or(true, |flags| flags & libmpv2::render::mpv_render_update::Frame != 0);
                if !made && !new_frame {
                    return Ok(());
                }
                let target = self.target.as_ref().expect("target was just made");
                // Not flipped: row 0 of the texture is the top of the picture,
                // which is what Slint's default (top-left) origin expects.
                render
                    .render::<ProcLoader>(
                        target.fbo.0.get() as i32,
                        width as i32,
                        height as i32,
                        false,
                    )
                    .map_err(|err| format!("mpv render: {err}"))?;
                show(target.image());
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
    fn teardown(&mut self, gl_current: bool) {
        self.showing = false;
        self.waiting = None;
        if let Some(render) = self.render.take() {
            if gl_current {
                drop(render);
            } else {
                std::mem::forget(render);
            }
        }
        if let (Some(gl), Some(target), true) = (self.gl.as_ref(), self.target.take(), gl_current) {
            // SAFETY: called from RenderingTeardown, GL context is current.
            unsafe { target.delete(gl) };
        }
        self.gl = None;
        if let Some(mpv) = self.mpv.take() {
            // SAFETY: made by Box::leak in `connect`; the render context that
            // borrowed it was dropped above and nothing else keeps the reference.
            let mpv = unsafe { Box::from_raw(std::ptr::from_ref(mpv).cast_mut()) };
            self.ending = Some(std::thread::spawn(move || drop(mpv)));
        }
    }

    /// Call after the window is gone. Waits for mpv to end; the UI thread
    /// is done by then, so this cannot deadlock it.
    pub fn finish(&mut self) {
        if self.render.is_some() || self.mpv.is_some() {
            // RenderingTeardown did not run: nothing can be freed safely.
            self.teardown(false);
        }
        if let Some(ending) = self.ending.take() {
            let _ = ending.join();
        }
    }

    /// Plays a web address, from `at` seconds in or from the beginning.
    /// Whatever was playing is replaced.
    pub fn play(&mut self, address: &str, at: Option<f64>) {
        if self.broken {
            return;
        }
        let start = Start { address: address.to_owned(), at };
        self.showing = true;
        if self.render.is_some() {
            if let Err(err) = self.load(&start) {
                eprintln!("haven: {err}");
            }
        } else {
            self.waiting = Some(start);
            self.start_mpv();
        }
        (self.wake)();
    }

    /// Stops playback and stops drawing. mpv stays ready for the next play.
    pub fn stop(&mut self) {
        self.waiting = None;
        self.showing = false;
        if let Some(mpv) = self.mpv {
            let _ = mpv.command("stop", &[]);
        }
    }

    /// Empties mpv's event queue and returns what matters from it.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let Some(mpv) = self.mpv else { return events };
        // libmpv2's own names for these are long; the numbers are mpv's
        // `mpv_end_file_reason`: 0 end of file, 4 error.
        const END_OF_FILE: u32 = 0;
        const ERROR: u32 = 4;
        loop {
            match mpv.wait_event(0.0) {
                Some(Ok(libmpv2::events::Event::FileLoaded)) => events.push(Event::Loaded),
                Some(Ok(libmpv2::events::Event::EndFile(reason))) => match reason as u32 {
                    END_OF_FILE => events.push(Event::Finished),
                    ERROR => events.push(Event::Failed("playback stopped".to_owned())),
                    _ => {}
                },
                Some(Ok(_)) => {}
                // An end-of-file event that carries an error arrives here.
                Some(Err(err)) => events.push(Event::Failed(reason(&err))),
                None => break,
            }
        }
        events
    }

    /// `None` until mpv is running.
    pub fn status(&self) -> Option<Status> {
        let mpv = self.mpv?;
        Some(Status {
            position: mpv.get_property::<f64>("time-pos").unwrap_or(0.0),
            duration: mpv.get_property::<f64>("duration").unwrap_or(0.0),
            paused: mpv.get_property::<bool>("pause").unwrap_or(false),
            buffering: mpv.get_property::<bool>("paused-for-cache").unwrap_or(false),
            arriving: mpv.get_property::<i64>("cache-speed").ok().map(|speed| speed.max(0) as f64),
            ahead: mpv.get_property::<f64>("demuxer-cache-duration").ok().map(|ahead| ahead.max(0.0)),
        })
    }

    pub fn set_paused(&self, paused: bool) {
        if let Some(mpv) = self.mpv {
            let _ = mpv.set_property("pause", paused);
        }
    }

    /// Jumps to `seconds` from the start.
    pub fn seek_to(&self, seconds: f64) {
        if let Some(mpv) = self.mpv {
            let _ = mpv.command("seek", &[&format!("{:.2}", seconds.max(0.0)), "absolute"]);
        }
    }

    /// Jumps forwards (or, with a negative number, back) by `seconds`.
    pub fn seek_by(&self, seconds: f64) {
        if let Some(mpv) = self.mpv {
            let _ = mpv.command("seek", &[&format!("{seconds:.2}"), "relative"]);
        }
    }

    /// mpv's track list as it gave it (JSON); empty when nothing is loaded.
    /// Cheap to compare, so the menus are only rebuilt when it changes.
    pub fn tracks_json(&self) -> String {
        self.mpv
            .and_then(|mpv| mpv.get_property::<String>("track-list").ok())
            .unwrap_or_default()
    }

    /// The audio and subtitle tracks of what is playing.
    pub fn tracks(&self) -> Vec<Track> {
        parse_tracks(&self.tracks_json())
    }

    pub fn select_audio(&self, id: i64) {
        if let Some(mpv) = self.mpv {
            let _ = mpv.set_property("aid", id);
        }
    }

    /// How far subtitles sit above the bottom of the picture, in mpv's
    /// units (the picture is 720 of them high). mpv's own default is 22.
    pub fn set_subtitle_margin(&self, margin: i64) {
        if let Some(mpv) = self.mpv {
            let _ = mpv.set_property("sub-margin-y", margin.clamp(0, 400));
        }
    }

    /// Loudness from 0 (silent) to 1 (as recorded), up to 2 (200%,
    /// amplified by mpv).
    pub fn set_volume(&self, volume: f64) {
        if let Some(mpv) = self.mpv {
            let _ = mpv.set_property("volume", (volume.clamp(0.0, 2.0) * 100.0).round());
        }
    }

    /// One line on what happens to the sound, for the log: the volume
    /// mpv holds, whether it is muted, the sound output and the format
    /// the sound reaches it in (an `spdif-…` format is passed through
    /// to a receiver untouched, so the volume cannot change it).
    pub fn sound_report(&self) -> Option<String> {
        let mpv = self.mpv?;
        let text = |name: &str| mpv.get_property::<String>(name).unwrap_or_else(|_| "-".to_owned());
        Some(format!(
            "volume={} mute={} volume-max={} ao={} ao-volume={} out={} {} codec={} spdif=\"{}\" af=\"{}\"",
            text("volume"),
            text("mute"),
            text("volume-max"),
            text("current-ao"),
            text("ao-volume"),
            text("audio-out-params/format"),
            text("audio-out-params/hr-channels"),
            text("audio-codec-name"),
            text("audio-spdif"),
            text("af"),
        ))
    }

    pub fn set_muted(&self, muted: bool) {
        if let Some(mpv) = self.mpv {
            let _ = mpv.set_property("mute", muted);
        }
    }

    /// Sends Dolby and DTS sound to the sound device as it is in the
    /// file, for an AV receiver or soundbar to decode. That is the only
    /// way Dolby Atmos and DTS:X reach one: decoded here, they play as
    /// their ordinary surround sound. Off, everything is decoded here.
    pub fn set_passthrough(&self, on: bool) {
        if let Some(mpv) = self.mpv {
            let formats = if on { "ac3,eac3,dts,dts-hd,truehd" } else { "" };
            let _ = mpv.set_property("audio-spdif", formats);
        }
    }

    /// 1 is normal speed. The sound's pitch is kept.
    pub fn set_speed(&self, speed: f64) {
        if let Some(mpv) = self.mpv {
            let _ = mpv.set_property("speed", speed.clamp(0.25, 4.0));
        }
    }

    /// How the picture fills the window.
    pub fn set_fit(&self, fit: Fit) {
        if let Some(mpv) = self.mpv {
            let (keep_shape, crop) = match fit {
                Fit::Whole => (true, 0.0),
                Fit::Fill => (true, 1.0),
                Fit::Stretch => (false, 0.0),
            };
            let _ = mpv.set_property("keepaspect", keep_shape);
            let _ = mpv.set_property("panscan", crop);
        }
    }

    /// Subtitle size, 1 being the usual size.
    pub fn set_subtitle_scale(&self, scale: f64) {
        if let Some(mpv) = self.mpv {
            let _ = mpv.set_property("sub-scale", scale.clamp(0.3, 3.0));
        }
    }

    /// Shows subtitles this many seconds later (earlier if negative).
    pub fn set_subtitle_delay(&self, seconds: f64) {
        if let Some(mpv) = self.mpv {
            let _ = mpv.set_property("sub-delay", seconds.clamp(-60.0, 60.0));
        }
    }

    /// Moves the subtitles in time so that the next line (`way` 1) or
    /// the line before (-1) shows now: the quick way to put subtitles
    /// back in step with the sound. Returns the delay that results.
    pub fn sync_subtitles(&self, way: i32) -> Option<f64> {
        let mpv = self.mpv?;
        mpv.command("sub-step", &[if way < 0 { "-1" } else { "1" }]).ok()?;
        mpv.get_property::<f64>("sub-delay").ok()
    }

    /// What is known about the picture and sound of what is playing.
    pub fn details(&self) -> Details {
        let Some(mpv) = self.mpv else { return Details::default() };
        let text = |name: &str| {
            mpv.get_property::<String>(name).ok().map(|text| text.trim().to_owned()).filter(|t| !t.is_empty())
        };
        let whole = |name: &str| mpv.get_property::<i64>(name).ok().filter(|n| *n > 0);
        Details {
            width: whole("width"),
            height: whole("height"),
            frames_per_second: mpv.get_property::<f64>("container-fps").ok().filter(|fps| *fps > 0.0),
            video: text("video-format").or_else(|| text("video-codec")),
            audio: text("audio-codec-name").or_else(|| text("audio-codec")),
            channels: text("audio-params/hr-channels").or_else(|| text("audio-params/channels")),
            container: text("file-format"),
            decoder: text("hwdec-current").filter(|decoder| decoder != "no"),
        }
    }

    /// `None` turns subtitles off.
    pub fn select_subtitle(&self, id: Option<i64>) {
        if let Some(mpv) = self.mpv {
            let _ = match id {
                Some(id) => mpv.set_property("sid", id),
                None => mpv.set_property("sid", "no"),
            };
        }
    }
}
