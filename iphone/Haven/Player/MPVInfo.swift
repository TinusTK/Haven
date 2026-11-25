#if os(tvOS)
import Libmpv
#else
import Mpv
#endif

/// What Haven knows about the libmpv it was built with. The player itself
/// comes in section S13; this proves libmpv links and loads.
enum MPVInfo {
    /// libmpv's client API version as "major.minor".
    static var clientAPIVersion: String {
        let v = mpv_client_api_version()
        return "\(v >> 16).\(v & 0xffff)"
    }
}
