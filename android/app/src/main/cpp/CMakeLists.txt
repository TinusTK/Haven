# Haven's bridge to libmpv: libhavenmpv.so, linked against the libmpv.so
# fetched by android/fetch-libmpv.sh (packed into the APK beside it).
cmake_minimum_required(VERSION 3.22)
project(havenmpv C)

add_library(mpv SHARED IMPORTED)
set_target_properties(mpv PROPERTIES
    IMPORTED_LOCATION ${CMAKE_CURRENT_SOURCE_DIR}/../../../../libmpv/${ANDROID_ABI}/libmpv.so
    IMPORTED_NO_SONAME TRUE)

add_library(havenmpv SHARED mpv_bridge.c)
target_link_libraries(havenmpv mpv log android)
