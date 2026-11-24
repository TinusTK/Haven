import SwiftUI

/// Stand-in first screen while the foundations are built (S0–S8). The tab
/// bar and pages replace it in S9.
struct RootView: View {
    let core: Result<Core, Error>

    var body: some View {
        ZStack {
            Color("Background").ignoresSafeArea()
            VStack(spacing: 14) {
                Image("Halo")
                    .resizable()
                    .scaledToFit()
                    .frame(width: 120, height: 120)
                Text("Haven")
                    .font(.custom("PixelifySans-Bold", size: 40))
                    .foregroundStyle(Color("Accent"))
                Text("Version \(Build.version) · \(Build.isStore ? "App Store build" : "GitHub build")")
                    .font(.custom("Figtree-Medium", size: 15))
                    .foregroundStyle(Color("TextDim"))
                Text(status)
                    .font(.custom("Figtree-Regular", size: 13))
                    .foregroundStyle(Color("TextDim"))
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 24)
            }
        }
    }

    private var status: String {
        switch core {
        case let .success(core):
            let watchlist = (try? core.store.watchlist().count) ?? 0
            let servers = (try? core.store.servers().count) ?? 0
            return "libmpv \(MPVInfo.clientAPIVersion) · \(watchlist) on the Watchlist · \(servers) media servers"
        case let .failure(error):
            return error.localizedDescription
        }
    }
}
