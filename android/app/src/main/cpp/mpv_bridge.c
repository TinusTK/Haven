// Haven's bridge from Kotlin (net.haven.app.player.MpvNative) to libmpv's
// client API. Only the parts of mpv's client.h Haven uses are declared here;
// they are stable across libmpv versions (client API 2.x). mpv draws straight
// through its render API (vo=libmpv) into an OpenGL ES context Haven makes
// itself on the Surface, on a thread of its own: the way the Rust build
// drew, which also works on the emulator (mpv's own Android context does not).
#include <jni.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <pthread.h>
#include <dlfcn.h>
#include <EGL/egl.h>
#include <android/native_window_jni.h>
#include <android/log.h>

typedef struct mpv_handle mpv_handle;

enum { MPV_FORMAT_STRING = 1, MPV_FORMAT_FLAG = 3, MPV_FORMAT_INT64 = 4, MPV_FORMAT_DOUBLE = 5 };
enum { MPV_EVENT_NONE = 0, MPV_EVENT_END_FILE = 7 };

typedef struct {
    int event_id;
    int error;
    uint64_t reply_userdata;
    void *data;
} mpv_event;

typedef struct {
    int reason;
    int error;
} mpv_event_end_file;

unsigned long mpv_client_api_version(void);
mpv_handle *mpv_create(void);
int mpv_initialize(mpv_handle *ctx);
void mpv_terminate_destroy(mpv_handle *ctx);
int mpv_set_option(mpv_handle *ctx, const char *name, int format, void *data);
int mpv_set_option_string(mpv_handle *ctx, const char *name, const char *data);
int mpv_set_property_string(mpv_handle *ctx, const char *name, const char *data);
int mpv_get_property(mpv_handle *ctx, const char *name, int format, void *data);
char *mpv_get_property_string(mpv_handle *ctx, const char *name);
int mpv_command(mpv_handle *ctx, const char **args);
mpv_event *mpv_wait_event(mpv_handle *ctx, double timeout);
void mpv_free(void *data);
int av_jni_set_java_vm(void *vm, void *log);

// mpv's render API (render.h, render_gl.h).
typedef struct mpv_render_context mpv_render_context;
typedef struct { int type; void *data; } mpv_render_param;
enum { MPV_RENDER_PARAM_INVALID = 0, MPV_RENDER_PARAM_API_TYPE = 1, MPV_RENDER_PARAM_OPENGL_INIT_PARAMS = 2,
       MPV_RENDER_PARAM_OPENGL_FBO = 3, MPV_RENDER_PARAM_FLIP_Y = 4 };
typedef struct { void *(*get_proc_address)(void *ctx, const char *name); void *get_proc_address_ctx; } mpv_opengl_init_params;
typedef struct { int fbo; int w; int h; int internal_format; } mpv_opengl_fbo;
int mpv_render_context_create(mpv_render_context **res, mpv_handle *mpv, mpv_render_param *params);
void mpv_render_context_set_update_callback(mpv_render_context *ctx, void (*callback)(void *), void *data);
uint64_t mpv_render_context_update(mpv_render_context *ctx);
int mpv_render_context_render(mpv_render_context *ctx, mpv_render_param *params);
void mpv_render_context_report_swap(mpv_render_context *ctx);
void mpv_render_context_free(mpv_render_context *ctx);
#define MPV_RENDER_UPDATE_FRAME 1

#define NOTE(...) __android_log_print(ANDROID_LOG_INFO, "haven", __VA_ARGS__)

// mpv's decoders (MediaCodec) and sound output (AudioTrack) need the Java machine.
JNIEXPORT jint JNI_OnLoad(JavaVM *vm, void *reserved) {
    (void) reserved;
    av_jni_set_java_vm(vm, NULL);
    return JNI_VERSION_1_6;
}

#define HANDLE(h) ((mpv_handle *) (intptr_t) (h))
#define FN(name) Java_net_haven_app_player_MpvNative_##name

JNIEXPORT jlong JNICALL FN(clientApiVersion)(JNIEnv *env, jobject self) {
    (void) env; (void) self;
    return (jlong) mpv_client_api_version();
}

JNIEXPORT jlong JNICALL FN(create)(JNIEnv *env, jobject self) {
    (void) env; (void) self;
    return (jlong) (intptr_t) mpv_create();
}

JNIEXPORT jint JNICALL FN(initialize)(JNIEnv *env, jobject self, jlong h) {
    (void) env; (void) self;
    return mpv_initialize(HANDLE(h));
}

JNIEXPORT void JNICALL FN(destroy)(JNIEnv *env, jobject self, jlong h) {
    (void) env; (void) self;
    mpv_terminate_destroy(HANDLE(h));
}

JNIEXPORT jint JNICALL FN(setOption)(JNIEnv *env, jobject self, jlong h, jstring name, jstring value) {
    (void) self;
    const char *n = (*env)->GetStringUTFChars(env, name, NULL);
    const char *v = (*env)->GetStringUTFChars(env, value, NULL);
    int result = mpv_set_option_string(HANDLE(h), n, v);
    (*env)->ReleaseStringUTFChars(env, name, n);
    (*env)->ReleaseStringUTFChars(env, value, v);
    return result;
}

JNIEXPORT jint JNICALL FN(setProperty)(JNIEnv *env, jobject self, jlong h, jstring name, jstring value) {
    (void) self;
    const char *n = (*env)->GetStringUTFChars(env, name, NULL);
    const char *v = (*env)->GetStringUTFChars(env, value, NULL);
    int result = mpv_set_property_string(HANDLE(h), n, v);
    (*env)->ReleaseStringUTFChars(env, name, n);
    (*env)->ReleaseStringUTFChars(env, value, v);
    return result;
}

JNIEXPORT jstring JNICALL FN(getString)(JNIEnv *env, jobject self, jlong h, jstring name) {
    (void) self;
    const char *n = (*env)->GetStringUTFChars(env, name, NULL);
    char *value = mpv_get_property_string(HANDLE(h), n);
    (*env)->ReleaseStringUTFChars(env, name, n);
    if (!value) return NULL;
    jstring out = (*env)->NewStringUTF(env, value);
    mpv_free(value);
    return out;
}

// A number property into out[0]; returns mpv's result (>= 0 when known).
JNIEXPORT jint JNICALL FN(getDouble)(JNIEnv *env, jobject self, jlong h, jstring name, jdoubleArray out) {
    (void) self;
    const char *n = (*env)->GetStringUTFChars(env, name, NULL);
    double value = 0;
    int result = mpv_get_property(HANDLE(h), n, MPV_FORMAT_DOUBLE, &value);
    (*env)->ReleaseStringUTFChars(env, name, n);
    if (result >= 0) (*env)->SetDoubleArrayRegion(env, out, 0, 1, &value);
    return result;
}

JNIEXPORT jint JNICALL FN(getLong)(JNIEnv *env, jobject self, jlong h, jstring name, jlongArray out) {
    (void) self;
    const char *n = (*env)->GetStringUTFChars(env, name, NULL);
    int64_t value = 0;
    int result = mpv_get_property(HANDLE(h), n, MPV_FORMAT_INT64, &value);
    (*env)->ReleaseStringUTFChars(env, name, n);
    if (result >= 0) {
        jlong v = (jlong) value;
        (*env)->SetLongArrayRegion(env, out, 0, 1, &v);
    }
    return result;
}

JNIEXPORT jint JNICALL FN(getFlag)(JNIEnv *env, jobject self, jlong h, jstring name) {
    (void) self;
    const char *n = (*env)->GetStringUTFChars(env, name, NULL);
    int value = 0;
    int result = mpv_get_property(HANDLE(h), n, MPV_FORMAT_FLAG, &value);
    (*env)->ReleaseStringUTFChars(env, name, n);
    return result < 0 ? -1 : (value ? 1 : 0);
}

// Each word is passed as it is (never read by a shell or as an option).
JNIEXPORT jint JNICALL FN(command)(JNIEnv *env, jobject self, jlong h, jobjectArray words) {
    (void) self;
    jsize count = (*env)->GetArrayLength(env, words);
    const char **args = calloc((size_t) count + 1, sizeof(char *));
    jstring *held = calloc((size_t) count, sizeof(jstring));
    if (!args || !held) { free(args); free(held); return -2; }
    for (jsize i = 0; i < count; i++) {
        held[i] = (jstring) (*env)->GetObjectArrayElement(env, words, i);
        args[i] = (*env)->GetStringUTFChars(env, held[i], NULL);
    }
    int result = mpv_command(HANDLE(h), args);
    for (jsize i = 0; i < count; i++) {
        (*env)->ReleaseStringUTFChars(env, held[i], args[i]);
        (*env)->DeleteLocalRef(env, held[i]);
    }
    free(args);
    free(held);
    return result;
}

// The next event: [id, end-file reason, end-file error], id 0 when none.
JNIEXPORT jintArray JNICALL FN(nextEvent)(JNIEnv *env, jobject self, jlong h) {
    (void) self;
    mpv_event *event = mpv_wait_event(HANDLE(h), 0);
    jint out[3] = {MPV_EVENT_NONE, 0, 0};
    if (event) {
        out[0] = event->event_id;
        if (event->event_id == MPV_EVENT_END_FILE && event->data) {
            mpv_event_end_file *end = (mpv_event_end_file *) event->data;
            out[1] = end->reason;
            out[2] = end->error;
        }
    }
    jintArray array = (*env)->NewIntArray(env, 3);
    (*env)->SetIntArrayRegion(env, array, 0, 3, out);
    return array;
}

// Drawing: one thread per Surface, with its own EGL context.

typedef struct {
    mpv_handle *mpv;
    ANativeWindow *window;
    pthread_t thread;
    pthread_mutex_t lock;
    pthread_cond_t wake;
    int frame;  // mpv has a new frame
    int stop;   // the Surface is going
    int ready;  // the render context is made (or failed)
    int ok;
} drawer;

static drawer *current = NULL;
static void *gles = NULL;

static void *proc(void *ctx, const char *name) {
    (void) ctx;
    void *found = (void *) eglGetProcAddress(name);
    if (!found) {
        if (!gles) gles = dlopen("libGLESv2.so", RTLD_NOW | RTLD_LOCAL);
        if (gles) found = dlsym(gles, name);
    }
    return found;
}

static void updated(void *data) {
    drawer *d = data;
    pthread_mutex_lock(&d->lock);
    d->frame = 1;
    pthread_cond_signal(&d->wake);
    pthread_mutex_unlock(&d->lock);
}

static void say_ready(drawer *d, int ok) {
    pthread_mutex_lock(&d->lock);
    d->ready = 1;
    d->ok = ok;
    pthread_cond_broadcast(&d->wake);
    pthread_mutex_unlock(&d->lock);
}

static void *draw_loop(void *data) {
    drawer *d = data;
    EGLDisplay display = eglGetDisplay(EGL_DEFAULT_DISPLAY);
    EGLSurface surface = EGL_NO_SURFACE;
    EGLContext context = EGL_NO_CONTEXT;
    mpv_render_context *render = NULL;
    if (!eglInitialize(display, NULL, NULL)) { NOTE("player: eglInitialize failed"); say_ready(d, 0); return NULL; }
    const EGLint wanted[] = {EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT, EGL_SURFACE_TYPE, EGL_WINDOW_BIT,
                             EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_NONE};
    EGLConfig config;
    EGLint found = 0;
    if (!eglChooseConfig(display, wanted, &config, 1, &found) || found < 1) { NOTE("player: no EGL config"); say_ready(d, 0); goto done; }
    surface = eglCreateWindowSurface(display, config, d->window, NULL);
    // OpenGL ES 3 where there is one, else 2.
    const EGLint three[] = {0x3098 /* EGL_CONTEXT_CLIENT_VERSION */, 3, EGL_NONE};
    const EGLint two[] = {0x3098, 2, EGL_NONE};
    context = eglCreateContext(display, config, EGL_NO_CONTEXT, three);
    if (context == EGL_NO_CONTEXT) context = eglCreateContext(display, config, EGL_NO_CONTEXT, two);
    if (surface == EGL_NO_SURFACE || context == EGL_NO_CONTEXT || !eglMakeCurrent(display, surface, surface, context)) {
        NOTE("player: could not make an OpenGL ES context (0x%x)", eglGetError());
        say_ready(d, 0);
        goto done;
    }
    mpv_opengl_init_params init = {proc, NULL};
    mpv_render_param params[] = {
        {MPV_RENDER_PARAM_API_TYPE, (void *) "opengl"},
        {MPV_RENDER_PARAM_OPENGL_INIT_PARAMS, &init},
        {MPV_RENDER_PARAM_INVALID, NULL},
    };
    int made = mpv_render_context_create(&render, d->mpv, params);
    if (made < 0) { NOTE("player: mpv could not draw (%d)", made); render = NULL; say_ready(d, 0); goto done; }
    mpv_render_context_set_update_callback(render, updated, d);
    say_ready(d, 1);
    for (;;) {
        pthread_mutex_lock(&d->lock);
        while (!d->frame && !d->stop) pthread_cond_wait(&d->wake, &d->lock);
        int stop = d->stop;
        d->frame = 0;
        pthread_mutex_unlock(&d->lock);
        if (stop) break;
        if (!(mpv_render_context_update(render) & MPV_RENDER_UPDATE_FRAME)) continue;
        EGLint w = 0, h = 0;
        eglQuerySurface(display, surface, EGL_WIDTH, &w);
        eglQuerySurface(display, surface, EGL_HEIGHT, &h);
        mpv_opengl_fbo fbo = {0, w, h, 0};
        int flip = 1;
        mpv_render_param frame[] = {
            {MPV_RENDER_PARAM_OPENGL_FBO, &fbo},
            {MPV_RENDER_PARAM_FLIP_Y, &flip},
            {MPV_RENDER_PARAM_INVALID, NULL},
        };
        mpv_render_context_render(render, frame);
        eglSwapBuffers(display, surface);
        mpv_render_context_report_swap(render);
    }
done:
    if (render) mpv_render_context_free(render);
    eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
    if (context != EGL_NO_CONTEXT) eglDestroyContext(display, context);
    if (surface != EGL_NO_SURFACE) eglDestroySurface(display, surface);
    return NULL;
}

static void stop_drawing(void) {
    drawer *d = current;
    if (!d) return;
    current = NULL;
    pthread_mutex_lock(&d->lock);
    d->stop = 1;
    pthread_cond_signal(&d->wake);
    pthread_mutex_unlock(&d->lock);
    pthread_join(d->thread, NULL);
    ANativeWindow_release(d->window);
    pthread_mutex_destroy(&d->lock);
    pthread_cond_destroy(&d->wake);
    free(d);
}

// mpv draws on this Surface from now on; true once it can.
JNIEXPORT jboolean JNICALL FN(attachSurface)(JNIEnv *env, jobject self, jlong h, jobject view) {
    (void) self;
    stop_drawing();
    drawer *d = calloc(1, sizeof(drawer));
    if (!d) return JNI_FALSE;
    d->mpv = HANDLE(h);
    d->window = ANativeWindow_fromSurface(env, view);
    if (!d->window) { free(d); return JNI_FALSE; }
    pthread_mutex_init(&d->lock, NULL);
    pthread_cond_init(&d->wake, NULL);
    if (pthread_create(&d->thread, NULL, draw_loop, d) != 0) {
        ANativeWindow_release(d->window);
        free(d);
        return JNI_FALSE;
    }
    current = d;
    pthread_mutex_lock(&d->lock);
    while (!d->ready) pthread_cond_wait(&d->wake, &d->lock);
    int ok = d->ok;
    pthread_mutex_unlock(&d->lock);
    if (!ok) stop_drawing();
    return ok ? JNI_TRUE : JNI_FALSE;
}

// The Surface is going: drawing stops and mpv's render context is freed.
JNIEXPORT void JNICALL FN(detachSurface)(JNIEnv *env, jobject self, jlong h) {
    (void) env; (void) self; (void) h;
    stop_drawing();
}

// Asks for a frame to be drawn again (the Surface changed size).
JNIEXPORT void JNICALL FN(redraw)(JNIEnv *env, jobject self) {
    (void) env; (void) self;
    if (current) updated(current);
}
