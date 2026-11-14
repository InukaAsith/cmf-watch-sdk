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

    init(store: AppStore) {
        self.store = store
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
                    let r = parseIncoming(try await ch.recv(timeout: .seconds(15)))
                    if case let .pulse(start, day, bpm, ex, done, exDone, restLeft) = r {
                        var l = live?.start == start ? live! : Live(start: start, day: day, bpm: 0, ex: nil, done: 0,
                                                                     restEnd: nil, at: .now, samples: [])
                        (l.bpm, l.ex, l.done, l.exDone, l.at) = (bpm, ex, done, exDone, .now)
                        l.restEnd = restLeft > 0 ? .now.addingTimeInterval(TimeInterval(restLeft)) : nil
                        if bpm > 0 { l.samples.append((.now, bpm)) }
                        live = l
                        continue
                    }
                    guard case let .request(_, running) = r else { continue }
                    if !running { live = nil }
                    if r != lastRequest || Date.now.timeIntervalSince(lastSync) > 60 { await run(ch, r) }
                } catch WireError.timeout {
                    if let l = live, Date.now.timeIntervalSince(l.at) > 30 { live = nil } // the app closed or ended
                    continue
                } catch {
                    if ch === channel { channel = nil }
                }
            }
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
