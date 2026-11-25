import SwiftUI

/// The page showing.
struct PageView: View {
    let model: AppModel

    var body: some View {
        switch model.page {
        case .home: HomePage(model: model)
        case .explore: ExplorePage(model: model)
        case .library: LibraryPage(model: model)
        case .liveTV: LiveTVPage(model: model, live: model.live)
        case .calendar: CalendarPage(model: model, calendar: model.calendar)
        case .settings: SettingsPage(model: model, settings: model.settingsPage)
        }
    }
}
