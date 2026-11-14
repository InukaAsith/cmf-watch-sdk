import SwiftUI

@main
struct WorkoutApp: App {
    @State private var store: AppStore
    @State private var sync: SyncController
    @Environment(\.scenePhase) private var phase
    @Namespace private var ns
    // DEBUG screenshots: `-tab 1` opens История
    @State private var tab = CommandLine.arguments.firstIndex(of: "-tab")
        .flatMap { Int(CommandLine.arguments[$0 + 1]) } ?? 0

    init() {
        let store = AppStore()
        #if DEBUG
        if CommandLine.arguments.contains("-demo") { store.loadDemo() }
        #endif
        _store = State(initialValue: store)
        let sync = SyncController(store: store)
        #if DEBUG
        sync.playerShown = CommandLine.arguments.contains("-player")
        #endif
        _sync = State(initialValue: sync)
    }

    var body: some Scene {
        WindowGroup {
            TabView(selection: $tab) {
                ProgramView()
                    .tabItem { Label("Программа", systemImage: "list.bullet.rectangle") }
                    .tag(0)
                    .badge(store.pending && !store.days.isEmpty ? "!" : nil)
                HistoryView()
                    .tabItem { Label("История", systemImage: "clock.arrow.circlepath") }
                    .tag(1)
                WatchView()
                    .tabItem { Label("Часы", systemImage: "applewatch") }
                    .tag(2)
                    .badge(Signing.expiry.map { $0.timeIntervalSinceNow < 86_400 } == true ? "!" : nil)
            }
            .tabViewBottomAccessory(isEnabled: store.open != nil) {
                MiniPlayer()
                    .contentShape(.rect)
                    .onTapGesture { sync.playerShown = true }
                    .matchedTransitionSource(id: "player", in: ns)
            }
            .fullScreenCover(isPresented: $sync.playerShown) {
                ActiveWorkout().navigationTransition(.zoom(sourceID: "player", in: ns))
            }
            .environment(store)
            .environment(sync)
            .onChange(of: phase) { sync.setForeground($1 == .active) }
        }
    }
}
