import Testing
@testable import Haven

struct BuildTests {
    @Test func storeBuildHasNoAddons() {
        // Exactly one of the two kinds of build.
        #expect(Build.isStore != Build.hasAddons)
        if Build.isStore {
            #expect(Build.ownNetworkOnly)
        }
    }

    @Test func libmpvLoads() {
        #expect(MPVInfo.clientAPIVersion.hasPrefix("2."))
    }

    @Test func buildKeysAreSafeLiterals() {
        // No built-in key may hold anything but letters, digits and - _ .
        let allowed = Set("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-")
        for key in [BuildKeys.traktClientID, BuildKeys.traktClientSecret,
                    BuildKeys.simklClientID, BuildKeys.omdbAPIKey] {
            #expect(key.allSatisfy { allowed.contains($0) })
        }
    }
}
