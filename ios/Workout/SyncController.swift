import Foundation
import Observation
import Security
import UIKit
import WireKit

/// Keeps a K session to the watch and syncs whenever the workout app asks (its R): history in, program out unless
/// a workout runs. In the background a notification from the watch wakes the app (docs/abi-1.4.md, unsolicited
/// messages); in the foreground it also polls, which an older loader needs.
@MainActor @Observable
final class SyncController {
    enum Phase: Equatable { case bluetoothOff, searching, connecting, connected, failed(String) }

    private(set) var phase = Phase.searching
    private(set) var syncing = false
    private(set) var lastError: String?
    private(set) var installed: PackageHeader?
    private(set) var installProgress: Double?
    /// The workout running on the watch, from its P messages; nil when none came for a while.
    private(set) var live: Live?
    struct Live {
        var start: UInt32, day: Int, bpm: Int, ex: Int?, done: Int, exDone = 0, restEnd: Date?, at: Date
        /// heart rate since the phone saw this workout (phone time, bpm > 0)
        var samples: [(Date, Int)]
    }
    let bundled: Data? = Bundle.main.url(forResource: "workout", withExtension: "wp2app")
        .flatMap { try? Data(contentsOf: $0) }
    var bundledHeader: PackageHeader? { bundled.flatMap(parseHeader) }
    /// A loader package copied into Resources by hand for a one-off update from the phone; never committed.
    let bundledLoader: Data? = Bundle.main.url(forResource: "loader", withExtension: "wp2app")
        .flatMap { try? Data(contentsOf: $0) }
    private(set) var loaderInstalled = false
    /// Install needs the K session (BLE), not the TCP bridge.
    var canInstall: Bool { session != nil }

    private let store: AppStore
    private var ble: BLE?
    private var session: KSession?
    private var channel: Channel?
    private var lastRequest: Incoming?
    private var lastSync = Date.distantPast
    private var foreground = true

    private var gym: GymGuard?

    init(store: AppStore) {
        self.store = store
        gym = GymGuard { [weak self] in
            guard let self, let o = store.open, !GymGuard.tooYoung(o) else { return }
            let had = !o.sets.isEmpty
            finishWorkout()
            if had { GymGuard.notifyFinished() }
        }
        store.onSave = { [weak self] s in
            LiveWorkout.sync(s)
            self?.gym?.sync(s.open)
        }
        WorkoutActions.done = { [weak self] in
            guard let self, let ex = store.currentExercise() else { return }
            doneSet(ex)
        }
        WorkoutActions.finish = { [weak self] in self?.finishWorkout() }
        LiveWorkout.sync(store)
        gym?.sync(store.open)
        #if DEBUG
        // iOS Simulator: `wp2apps bridge --hci-port P` in front of the emulator, app launched with `-tcp 7700`
        if let i = CommandLine.arguments.firstIndex(of: "-tcp"), i + 1 < CommandLine.arguments.count,
           let port = UInt16(CommandLine.arguments[i + 1]) {
            channel = SimChannel(port: port)
            phase = .connected
            listen()
            return
        }
        #endif
        let ble = BLE()
        ble.onState = { [weak self] in self?.linkChanged($0) }
        self.ble = ble
    }

    func setForeground(_ on: Bool) {
        foreground = on
        Task { await session?.setPoll(on ? .milliseconds(250) : nil) }
    }

    private func linkChanged(_ s: BLE.State) {
        switch s {
        case .off: phase = .bluetoothOff
        case .searching: phase = .searching
        case .connecting: phase = .connecting
        case .disconnected:
            phase = .connecting
            session = nil
            channel = nil
        case .ready: Task { await openSession() }
        }
    }

    private func openSession() async {
        guard let ble, session == nil else { return }
        do {
            let (s, secret) = try await KSession.open(link: ble, secretHex: Keychain.secret,
                                                      poll: foreground ? .milliseconds(250) : nil)
            Keychain.secret = secret
            session = s
            channel = s
            phase = .connected
            installed = try? await WireKit.installed(s).first { $0.value.name == "workout" }?.value
            listen()
        } catch {
            Keychain.secret = nil // a factory reset changes the secret: read it again next time
            phase = .failed(String(localized: "Часы не ответили: \(String(describing: error))"))
        }
    }

    /// Waits for the workout app's R and syncs. A new R starts a sync only when something can have changed: the
    /// first one, another version, the workout started or ended, or a minute since the last sync (the app repeats
    /// R every 3 s on its start screen).
    private func listen() {
        Task {
            while let ch = channel {
                do {
                    let raw = try await ch.recv(timeout: .seconds(15))
                    let r = parseIncoming(raw)
                    if raw.count >= 19, raw.dropFirst(2).first == UInt8(ascii: "P") { hrDiag = Array(raw.dropFirst(14)) }
                    if case let .pulse(start, day, bpm, ex, done, exDone, restLeft) = r {
                        var l = live?.start == start ? live! : Live(start: start, day: day, bpm: 0, ex: nil, done: 0,
                                                                     restEnd: nil, at: .now, samples: [])
                        (l.bpm, l.ex, l.done, l.exDone, l.at) = (bpm, ex, done, exDone, .now)
                        l.restEnd = restLeft > 0 ? .now.addingTimeInterval(TimeInterval(restLeft)) : nil
                        if bpm > 0 { l.samples.append((.now, bpm)) }
                        live = l
                        let w = WatchWorkout(start: start, day: day, done: done)
                        if w != watch {
                            watch = w
                            await reconcile(ch)
                        }
                        continue
                    }
                    guard case let .request(_, running) = r else { continue }
                    if !running { live = nil }
                    if r != lastRequest || Date.now.timeIntervalSince(lastSync) > 60 {
                        await run(ch, r)
                        await sendZones()
                    }
                    if !running {
                        // the watch ended the shared workout (its entry came with the sync, or it had no sets)
                        if let o = store.open, let w = watch, w.start == o.start {
                            store.setOpen(nil)
                        }
                        watch = nil
                        await reconcile(ch)
                    }
                } catch WireError.timeout {
                    if let l = live, Date.now.timeIntervalSince(l.at) > 30 { live = nil } // the app closed or ended
                    continue
                } catch {
                    if ch === channel { channel = nil }
                }
            }
        }
    }

    /// The watch's open workout as its last P said; nil when it said none (R) or before it said anything.
    private var watch: WatchWorkout?

    /// Brings the phone's and the watch's open workouts together (WireKit `meet`). Errors leave it for the next P/R.
    private func reconcile(_ ch: Channel) async {
        let version = store.watchVersion ?? 0
        do {
            if watch == nil {
                for e in store.unsent {
                    try await ch.send(entryMessage(e))
                    store.markSent(e.start)
                }
            } else if let w = watch, let e = store.unsent.first(where: { $0.start == w.start }) {
                try await ch.send(finishMessage(start: w.start, end: e.start &+ UInt32(e.durS))) // finished here already
                store.markSent(e.start)
                return
            }
            switch meet(phone: store.open, watch: watch, watchVersion: version) {
            case .nothing, .independent:
                break
            case .pushOwn:
                try await pushOwn(ch)
            case .exchange:
                guard var o = store.open, let w = watch else { return }
                let theirs = w.done == o.sets.count ? o.sets : try await watchSets(ch, start: o.start)
                o.union(theirs, planned: store.planned)
                store.setOpen(o)
                for s in o.sets where !theirs.contains(s) { try await ch.send(setMessage(o, s)) }
                for s in o.removed ?? [] where theirs.contains(s) { try await ch.send(unsetMessage(o, s)) }
            case .adoptWatch:
                guard let w = watch else { return }
                store.setOpen(OpenWorkout(start: w.start, day: w.day, version: version))
                var o = store.open!
                o.union(try await watchSets(ch, start: w.start), planned: store.planned)
                store.setOpen(o)
            case .replaceWatch:
                guard let w = watch else { return }
                try await ch.send(finishMessage(start: w.start, end: watchSeconds()))
                try await pushOwn(ch)
            case .yieldToWatch:
                if let e = store.finishWorkout(told: true) { try await ch.send(entryMessage(e)) }
                await reconcile(ch)
            }
        } catch {
            lastError = String(localized: "Синхронизация тренировки не удалась: \(String(describing: error))")
        }
    }

    private func pushOwn(_ ch: Channel) async throws {
        guard let o = store.open else { return }
        try await ch.send(setMessage(o, nil))
        for s in o.sets { try await ch.send(setMessage(o, s)) }
    }

    /// The watch's sets of the workout that started at `start` (its trace, T → X); empty if it has another.
    private func watchSets(_ ch: Channel, start: UInt32) async throws -> [SetMark] {
        try await ch.send(traceQuery())
        var parts: [Int: Data] = [:]
        var got: UInt32 = 0
        while true {
            guard case let .trace(i, n, s, d) = parseIncoming(try await ch.recv(timeout: .seconds(10))) else { continue }
            (parts[i], got) = (d, s)
            if parts.count >= n { break }
        }
        guard got == start, let t = parseTrace(parts.keys.sorted().reduce(Data()) { $0 + parts[$1]! }) else { return [] }
        return t.sets
    }

    /// The zones' bounds to the watch (it keeps them; a repeat changes nothing).
    func sendZones() async {
        guard let ch = channel, let z = store.zones else { return }
        try? await ch.send(zonesMessage(z))
    }

    /// The watch's heart-rate diagnostics from its last P: heart_rate() result, HEART events, the stock cells
    /// worn / owner / bpm (docs/workout.md, Protocol).
    private(set) var hrDiag: [UInt8]?

    /// The full-screen player is up.
    var playerShown = false
    /// The rest after this many sets was skipped (the player and the mini player show the next set).
    var skippedRest: Int?

    /// The watch's heart rate for this workout, while it sends one.
    func bpm(for o: OpenWorkout) -> Int? { live.flatMap { $0.start == o.start && $0.bpm > 0 ? $0.bpm : nil } }

    // MARK: the phone's own actions on the open workout

    func startWorkout(day: Int) {
        store.startWorkout(day: day)
        Rest.ask()
        if let ch = channel { Task { await reconcile(ch) } }
    }

    func doneSet(_ ex: Int) {
        guard let m = store.doneSet(ex), let o = store.open else { return }
        Rest.schedule(store.restEnd, name: o.day < store.days.count && ex < store.days[o.day].exercises.count
                      ? store.days[o.day].exercises[ex].exercise.name : "")
        if let ch = channel, watch?.start == o.start || watch == nil {
            Task { try? await ch.send(setMessage(o, m)) }
            if watch == nil { Task { await reconcile(ch) } } // the watch app idle: it adopts the workout
        }
    }

    func undoSet() {
        guard let m = store.undoSet(), let o = store.open else { return }
        Rest.cancel()
        if let ch = channel, watch?.start == o.start { Task { try? await ch.send(unsetMessage(o, m)) } }
    }

    func finishWorkout() {
        let shared = store.open != nil && watch?.start == store.open?.start && channel != nil
        let end = watchSeconds()
        guard let start = store.open?.start else { return }
        store.finishWorkout(end: end) // stays unsent until the watch took F or H: a lost F must not revive it
        Rest.cancel()
        if shared, let ch = channel {
            Task { if (try? await ch.send(finishMessage(start: start, end: end))) != nil { store.markSent(start) } }
        }
    }

    private func run(_ ch: Channel, _ r: Incoming?) async {
        syncing = true
        let task = UIApplication.shared.beginBackgroundTask()
        defer {
            syncing = false
            UIApplication.shared.endBackgroundTask(task)
        }
        let program = try? store.encoded.get()
        if let program { store.willPush(program) }
        do {
            let result = try await sync(channel: ch, program: program, knownStarts: store.knownStarts, tracedStarts: store.tracedStarts,
                                        firstRequest: r)
            store.apply(result)
            lastRequest = result.pushed.map { .request(version: $0, running: false) } ?? r
            lastSync = .now
            lastError = nil
        } catch {
            lastError = String(localized: "Синхронизация не удалась: \(String(describing: error))")
        }
    }

    func installLoader() async {
        guard let session, let bundledLoader else { return }
        installProgress = 0
        defer { installProgress = nil }
        do {
            _ = try await install(session, package: bundledLoader) { p in Task { @MainActor in self.installProgress = p } }
            loaderInstalled = true
            lastError = nil
        } catch InstallError.busy {
            lastError = String(localized: "Закройте Custom Apps на часах и повторите.")
        } catch {
            lastError = String(localized: "Установка не удалась: \(String(describing: error))")
        }
    }

    func installWorkout() async {
        guard let session, let bundled else { return }
        installProgress = 0
        defer { installProgress = nil }
        do {
            _ = try await install(session, package: bundled) { p in Task { @MainActor in self.installProgress = p } }
            installed = bundledHeader
            lastError = nil
        } catch InstallError.busy {
            lastError = String(localized: "Закройте приложение «Тренировка» на часах и повторите.")
        } catch {
            lastError = String(localized: "Установка не удалась: \(String(describing: error))")
        }
    }
}

/// The pairing secret (K), cached so a reconnect needs no shell round trip. Personal data: Keychain only.
enum Keychain {
    private static var query: [String: Any] { [kSecClass as String: kSecClassGenericPassword,
                                               kSecAttrService as String: "wp2.workout.secret"] }

    static var secret: String? {
        get {
            var out: AnyObject?
            var q = query
            q[kSecReturnData as String] = true
            guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let d = out as? Data else { return nil }
            return String(data: d, encoding: .utf8)
        }
        set {
            SecItemDelete(query as CFDictionary)
            guard let newValue else { return }
            var q = query
            q[kSecValueData as String] = Data(newValue.utf8)
            q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock // background sync after a reboot
            SecItemAdd(q as CFDictionary, nil)
        }
    }
}

/// Free Personal Team profiles expire after 7 days: the app stops launching and background sync stops.
enum Signing {
    nonisolated(unsafe) static let expiry: Date? = {
        guard let url = Bundle.main.url(forResource: "embedded", withExtension: "mobileprovision"),
              let raw = try? Data(contentsOf: url),
              let start = raw.range(of: Data("<?xml".utf8)), let end = raw.range(of: Data("</plist>".utf8)),
              let plist = try? PropertyListSerialization.propertyList(
                  from: raw[start.lowerBound..<end.upperBound], format: nil) as? [String: Any] else { return nil }
        return plist["ExpirationDate"] as? Date
    }()
}
