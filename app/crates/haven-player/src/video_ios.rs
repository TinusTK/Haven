//! Where mpv draws on an iPhone or iPad.
//!
//! Slint draws with Skia on Metal there (through wgpu), and mpv can only
//! draw with OpenGL. So mpv gets an OpenGL ES context of its own and draws
//! into an IOSurface; the same memory is opened as a Metal texture, wrapped
//! as a wgpu texture and shown as a Slint `Image`. Nothing is copied.
//!
//! Everything here runs on the UI thread, inside Slint's rendering
//! notifier. mpv's context is made current only while mpv draws and put
//! back afterwards ([`Gles::enter`]); Slint itself uses no OpenGL here.

use std::ffi::{CStr, c_char, c_void};

use glow::HasContext;
use objc2::encode::{Encoding, RefEncode};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool, MessageReceiver, ProtocolObject, Sel};
use objc2::msg_send;
use objc2_metal::{
    MTLDevice, MTLPixelFormat, MTLStorageMode, MTLTexture, MTLTextureDescriptor, MTLTextureType,
    MTLTextureUsage,
};
use slint::wgpu_30::wgpu;

type CFTypeRef = *const c_void;

/// An IOSurface, as Objective-C methods expect to be handed one.
#[repr(C)]
pub(super) struct IOSurface {
    _private: [u8; 0],
}

// SAFETY: IOSurfaceRef is `struct __IOSurface *` in Apple's headers.
unsafe impl RefEncode for IOSurface {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("__IOSurface", &[]));
}

/// Opaque: only their addresses are passed.
#[repr(C)]
struct CallBacks {
    _private: [u8; 0],
}

const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;
const K_CF_NUMBER_SINT32: isize = 3;
/// `kEAGLRenderingAPIOpenGLES3`.
const OPENGL_ES_3: usize = 3;

#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    static kCFAllocatorDefault: CFTypeRef;
    static kCFTypeDictionaryKeyCallBacks: CallBacks;
    static kCFTypeDictionaryValueCallBacks: CallBacks;
    static kIOSurfaceWidth: CFTypeRef;
    static kIOSurfaceHeight: CFTypeRef;
    static kIOSurfaceBytesPerElement: CFTypeRef;
    static kIOSurfaceBytesPerRow: CFTypeRef;
    static kIOSurfacePixelFormat: CFTypeRef;
    fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: isize,
        key_callbacks: *const CallBacks,
        value_callbacks: *const CallBacks,
    ) -> CFTypeRef;
    fn CFNumberCreate(allocator: CFTypeRef, kind: isize, value: *const c_void) -> CFTypeRef;
    fn CFRetain(object: CFTypeRef) -> CFTypeRef;
    fn CFRelease(object: CFTypeRef);
    fn IOSurfaceCreate(properties: CFTypeRef) -> *mut IOSurface;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// Looks up an OpenGL ES function (Haven links OpenGLES.framework).
pub(super) fn lookup(name: &CStr) -> *mut c_void {
    // SAFETY: dlsym only reads the name.
    unsafe { dlsym(RTLD_DEFAULT, name.as_ptr()) }
}

/// The lookup mpv is handed when it creates its render context.
pub(super) fn mpv_gl_lookup(_: &(), name: &str) -> *mut c_void {
    std::ffi::CString::new(name).map_or(std::ptr::null_mut(), |name| lookup(&name))
}

fn eagl_class() -> Result<&'static AnyClass, String> {
    AnyClass::get(c"EAGLContext").ok_or_else(|| "OpenGL ES is not available".to_owned())
}

/// mpv's OpenGL ES context, and the Metal device Slint draws with.
pub(super) struct Gles {
    context: Retained<AnyObject>,
    device: wgpu::Device,
    pub(super) gl: glow::Context,
}

/// While alive, mpv's context is the current one; the one before comes
/// back when it is dropped.
pub(super) struct Current {
    previous: Option<Retained<AnyObject>>,
}

impl Drop for Current {
    fn drop(&mut self) {
        if let Ok(class) = eagl_class() {
            // SAFETY: setCurrentContext: takes a context or nil.
            let _: Bool = unsafe { msg_send![class, setCurrentContext: self.previous.as_deref()] };
        }
    }
}

fn enter(context: &AnyObject) -> Result<Current, String> {
    let class = eagl_class()?;
    // SAFETY: plain EAGLContext class methods.
    unsafe {
        let previous: Option<Retained<AnyObject>> = msg_send![class, currentContext];
        let done: Bool = msg_send![class, setCurrentContext: context];
        if !done.as_bool() {
            return Err("could not use the OpenGL ES context".into());
        }
        Ok(Current { previous })
    }
}

impl Gles {
    /// `api` must be Slint's wgpu on Metal.
    pub(super) fn new(api: &slint::GraphicsAPI) -> Result<Self, String> {
        let slint::GraphicsAPI::WGPU30 { device, .. } = api else {
            return Err(format!("the window is not drawn with wgpu ({api:?})"));
        };
        let class = eagl_class()?;
        // SAFETY: plain EAGLContext creation.
        let context: Option<Retained<AnyObject>> = unsafe {
            let allocated: Allocated<AnyObject> = msg_send![class, alloc];
            msg_send![allocated, initWithAPI: OPENGL_ES_3]
        };
        let context = context.ok_or("no OpenGL ES 3 context")?;
        let current = enter(&context)?;
        // SAFETY: mpv's context is current.
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| lookup(name)) };
        drop(current);
        Ok(Self { context, device: device.clone(), gl })
    }

    /// Makes mpv's context current until the returned guard is dropped.
    pub(super) fn enter(&self) -> Result<Current, String> {
        enter(&self.context)
    }

    fn metal_device(&self) -> Result<Retained<ProtocolObject<dyn MTLDevice>>, String> {
        // SAFETY: only the device's handle is taken, and retained.
        unsafe {
            let hal = self
                .device
                .as_hal::<wgpu::hal::api::Metal>()
                .ok_or("the window is not drawn with Metal")?;
            Ok(hal.raw_device().clone())
        }
    }
}

/// The picture mpv draws into: an IOSurface seen by OpenGL ES as a
/// framebuffer and by Slint as a wgpu texture.
pub(super) struct Target {
    surface: *mut IOSurface,
    gl_texture: glow::NativeTexture,
    pub(super) fbo: glow::NativeFramebuffer,
    pub(super) width: u32,
    pub(super) height: u32,
    texture: wgpu::Texture,
}

/// What the wgpu texture keeps alive until wgpu lets go of it.
struct Keep(CFTypeRef);
// SAFETY: CFRelease may be called from any thread.
unsafe impl Send for Keep {}
unsafe impl Sync for Keep {}

impl Target {
    /// `width` and `height` are physical pixels. mpv's context must be current.
    pub(super) unsafe fn new(gles: &Gles, width: u32, height: u32) -> Result<Self, String> {
        let surface = unsafe { new_surface(width, height)? };
        let made = unsafe { Self::around(gles, surface, width, height) };
        if made.is_err() {
            unsafe { CFRelease(surface.cast()) };
        }
        made
    }

    unsafe fn around(gles: &Gles, surface: *mut IOSurface, width: u32, height: u32) -> Result<Self, String> {
        let gl = &gles.gl;
        unsafe {
            // OpenGL ES side.
            let gl_texture = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(gl_texture));
            // `type:` is a Rust keyword, so this selector is named in full.
            let selector =
                Sel::register(c"texImageIOSurface:target:internalFormat:width:height:format:type:plane:");
            let attached: Bool = MessageReceiver::send_message(
                &*gles.context,
                selector,
                (
                    surface,
                    glow::TEXTURE_2D as usize,
                    glow::RGBA as usize,
                    width,
                    height,
                    glow::RGBA as usize,
                    glow::UNSIGNED_BYTE as usize,
                    0u32,
                ),
            );
            if !attached.as_bool() {
                gl.delete_texture(gl_texture);
                return Err("could not open the picture in OpenGL ES".into());
            }
            for (name, value) in [
                (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, name, value as i32);
            }
            gl.bind_texture(glow::TEXTURE_2D, None);
            let fbo = gl.create_framebuffer()?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(gl_texture),
                0,
            );
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            let fail = |text: String| {
                gl.delete_framebuffer(fbo);
                gl.delete_texture(gl_texture);
                Err(text)
            };
            if status != glow::FRAMEBUFFER_COMPLETE {
                return fail(format!("framebuffer incomplete: 0x{status:x}"));
            }

            // Metal side, then wgpu, which Slint takes.
            let device = match gles.metal_device() {
                Ok(device) => device,
                Err(err) => return fail(err),
            };
            let descriptor = MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                MTLPixelFormat::RGBA8Unorm,
                width as usize,
                height as usize,
                false,
            );
            descriptor.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
            descriptor.setStorageMode(MTLStorageMode::Shared);
            let metal: Option<Retained<ProtocolObject<dyn MTLTexture>>> = msg_send![
                &*device,
                newTextureWithDescriptor: &*descriptor,
                iosurface: surface,
                plane: 0usize
            ];
            let Some(metal) = metal else {
                return fail("could not open the picture in Metal".into());
            };
            let keep = Keep(CFRetain(surface.cast()));
            let hal = wgpu::hal::metal::Device::texture_from_raw(
                metal,
                wgpu::TextureFormat::Rgba8Unorm,
                MTLTextureType::Type2D,
                1,
                1,
                wgpu::hal::CopyExtent { width, height, depth: 1 },
                Some(Box::new(move || {
                    let keep = keep;
                    CFRelease(keep.0)
                })),
            );
            let texture = gles.device.create_texture_from_hal::<wgpu::hal::api::Metal>(
                hal,
                &wgpu::TextureDescriptor {
                    label: Some("mpv picture"),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    // Slint asks for both, though it only reads the picture.
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                },
                wgpu::TextureUses::RESOURCE,
            );
            Ok(Self { surface, gl_texture, fbo, width, height, texture })
        }
    }

    pub(super) fn image(&self) -> slint::Image {
        slint::Image::try_from(self.texture.clone()).unwrap_or_default()
    }

    /// mpv's context must be current.
    pub(super) unsafe fn delete(self, gles: &Gles) {
        unsafe {
            gles.gl.delete_framebuffer(self.fbo);
            gles.gl.delete_texture(self.gl_texture);
            CFRelease(self.surface.cast());
        }
    }
}

/// A new IOSurface of `width` × `height` RGBA pixels.
unsafe fn new_surface(width: u32, height: u32) -> Result<*mut IOSurface, String> {
    let row = (width as i32 * 4 + 63) & !63;
    let format = i32::from_be_bytes(*b"RGBA");
    let numbers = [width as i32, height as i32, 4, row, format];
    unsafe {
        let values: Vec<CFTypeRef> = numbers
            .iter()
            .map(|n| CFNumberCreate(kCFAllocatorDefault, K_CF_NUMBER_SINT32, std::ptr::from_ref(n).cast()))
            .collect();
        let keys = [
            kIOSurfaceWidth,
            kIOSurfaceHeight,
            kIOSurfaceBytesPerElement,
            kIOSurfaceBytesPerRow,
            kIOSurfacePixelFormat,
        ];
        let properties = CFDictionaryCreate(
            kCFAllocatorDefault,
            keys.as_ptr(),
            values.as_ptr(),
            keys.len() as isize,
            &raw const kCFTypeDictionaryKeyCallBacks,
            &raw const kCFTypeDictionaryValueCallBacks,
        );
        for value in values {
            CFRelease(value);
        }
        let surface = IOSurfaceCreate(properties);
        CFRelease(properties);
        if surface.is_null() { Err("could not make the picture".into()) } else { Ok(surface) }
    }
}

/// The iPhone and iPad halves of [`super::Video`]'s rendering steps.
impl super::Video {
    pub(super) fn setup(&mut self, api: &slint::GraphicsAPI) -> Result<(), String> {
        if self.gles.is_none() {
            self.gles = Some(Gles::new(api)?);
        }
        Ok(())
    }

    /// Creates mpv's render context once both mpv and its OpenGL ES
    /// context exist, then loads whatever was waiting for it.
    pub(super) fn connect(&mut self, _api: &slint::GraphicsAPI) -> Result<(), String> {
        if self.render.is_none() && self.gles.is_some() {
            if self.mpv.is_none() {
                let Some(incoming) = &self.incoming else { return Ok(()) };
                match incoming.try_recv() {
                    Ok(Ok(mpv)) => self.mpv = Some(Box::leak(Box::new(mpv))),
                    Ok(Err(err)) => return Err(err),
                    Err(_) => return Ok(()), // not ready yet
                }
            }
            let (Some(mpv), Some(gles)) = (self.mpv, self.gles.as_ref()) else {
                return Ok(());
            };
            let _current = gles.enter()?;
            let mut render = mpv
                .create_render_context(vec![
                    libmpv2::render::RenderParam::ApiType(libmpv2::render::RenderParamApiType::OpenGl),
                    libmpv2::render::RenderParam::InitParams(libmpv2::render::OpenGLInitParams {
                        get_proc_address: mpv_gl_lookup,
                        ctx: (),
                    }),
                ])
                .map_err(|err| format!("mpv render context: {err}"))?;
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

    /// Lets mpv draw the current frame into the shared picture.
    pub(super) fn draw(
        &mut self,
        size: (u32, u32),
        show: &mut dyn FnMut(slint::Image),
    ) -> Result<(), String> {
        if !self.showing {
            return Ok(());
        }
        let (Some(gles), Some(render)) = (self.gles.as_ref(), self.render.as_ref()) else {
            return Ok(());
        };
        let (width, height) = (size.0.max(1), size.1.max(1));
        let _current = gles.enter()?;
        if self.target.as_ref().is_none_or(|t| (t.width, t.height) != (width, height)) {
            show(slint::Image::default());
            if let Some(old) = self.target.take() {
                // SAFETY: mpv's context is current.
                unsafe { old.delete(gles) };
            }
            // SAFETY: mpv's context is current.
            self.target = Some(unsafe { Target::new(gles, width, height)? });
        }
        let target = self.target.as_ref().expect("target was just made");
        // Not flipped: row 0 is the top of the picture, as Metal reads it.
        render
            .render::<()>(target.fbo.0.get() as i32, width as i32, height as i32, false)
            .map_err(|err| format!("mpv render: {err}"))?;
        // Hands the drawing to the GPU before Metal reads the picture.
        // SAFETY: mpv's context is current.
        unsafe { gles.gl.flush() };
        show(target.image());
        Ok(())
    }

    /// Frees mpv's render context and the picture (mpv's context is its
    /// own, so it can always be made current), then ends mpv off the UI
    /// thread.
    pub(super) fn teardown(&mut self, _slint_context_current: bool) {
        self.showing = false;
        self.waiting = None;
        let current = self.gles.as_ref().and_then(|gles| gles.enter().ok());
        if let Some(render) = self.render.take() {
            if current.is_some() {
                drop(render);
            } else {
                std::mem::forget(render);
            }
        }
        if let (Some(gles), Some(target), true) = (self.gles.as_ref(), self.target.take(), current.is_some()) {
            // SAFETY: mpv's context is current.
            unsafe { target.delete(gles) };
        }
        drop(current);
        self.gles = None;
        if let Some(mpv) = self.mpv.take() {
            // SAFETY: made by Box::leak in `connect`; the render context that
            // borrowed it was dropped above and nothing else keeps the reference.
            let mpv = unsafe { Box::from_raw(std::ptr::from_ref(mpv).cast_mut()) };
            self.ending = Some(std::thread::spawn(move || drop(mpv)));
        }
    }
}
