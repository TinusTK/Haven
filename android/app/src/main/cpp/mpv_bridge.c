// Haven's bridge from Kotlin (net.haven.app.Mpv) to libmpv's client API.
// Only the parts of mpv's client.h Haven uses are declared here; they are
// stable across libmpv versions (client API 2.x).
#include <jni.h>

unsigned long mpv_client_api_version(void);

JNIEXPORT jlong JNICALL
Java_net_haven_app_Mpv_clientApiVersion(JNIEnv *env, jobject self) {
    (void) env;
    (void) self;
    return (jlong) mpv_client_api_version();
}
