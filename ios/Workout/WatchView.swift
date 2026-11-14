import SwiftUI
import WireKit

struct WatchView: View {
    @Environment(AppStore.self) private var store
    @Environment(SyncController.self) private var sync

    var body: some View {
        NavigationStack {
            List {
                if let live = sync.live { LiveSection(live: live) }
                Section {
                    HStack(spacing: 16) {
                        Image(systemName: "applewatch")
                            .font(.system(size: 30, weight: .medium))
                            .foregroundStyle(connected ? Color(.secondarySystemGroupedBackground) : .secondary)
                            .frame(width: 64, height: 64)
                            .background(connected ? Color.accentColor : Color(.tertiarySystemGroupedBackground),
                                        in: .rect(cornerRadius: 20, style: .continuous))
                            .symbolEffect(.pulse, isActive: sync.syncing)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(linkText).font(.title3.bold())
                            if sync.syncing {
                                Text("Синхронизирую…").foregroundStyle(.secondary)
                            } else if let last = store.lastSync {
                                (Text("Синхронизация ") + Text(last, style: .relative) + Text(" назад"))
                                    .font(.subheadline).foregroundStyle(.secondary)
                            }
                        }
                    }
                    .padding(.vertical, 8)
                    LabeledContent("Программа на часах", value: programText)
                } footer: {
                    Text("Откройте «Тренировка» на часах: история и программа синхронизируются сами, в том числе когда телефон в кармане.")
                }

                Section("Приложение на часах") {
                    LabeledContent("Установлено", value: sync.installed.map { String(localized: "версия \($0.version)") } ?? String(localized: "нет"))
                    if let b = sync.bundledHeader {
                        LabeledContent("В телефоне", value: String(localized: "версия \(b.version)"))
                        if let p = sync.installProgress {
                            ProgressView(value: p)
                        } else if sync.installed?.version != b.version, sync.canInstall {
                            Button(sync.installed == nil ? "Установить" : "Обновить") {
                                Task { await sync.installWorkout() }
                            }
                        }
                    }
                }

                if let error = sync.lastError {
                    Section { Label(error, systemImage: "exclamationmark.triangle").foregroundStyle(.orange) }
                }

                if let expiry = Signing.expiry {
                    Section {
                        LabeledContent("Подпись до", value: expiry.formatted(date: .abbreviated, time: .shortened))
                    } footer: {
                        if expiry.timeIntervalSinceNow < 86_400 {
                            Text("Подпись скоро истечёт: подключите телефон к Маку и запустите scripts/ios-install.")
                                .foregroundStyle(.red)
                        }
                    }
                }
            }
            .listRowSpacing(10)
            .navigationTitle("Часы")
            .toolbar {
                // iOS keeps the app's language in its own Settings page (Settings > Тренировка > Язык)
                Button("Язык", systemImage: "globe") {
                    UIApplication.shared.open(URL(string: UIApplication.openSettingsURLString)!)
                }
            }
        }
    }

    private var connected: Bool { if case .connected = sync.phase { true } else { false } }

    private var linkText: String {
        switch sync.phase {
        case .bluetoothOff: String(localized: "Bluetooth выключен")
        case .searching: String(localized: "Ищу часы…")
        case .connecting: String(localized: "Подключаюсь…")
        case .connected: String(localized: "Подключено")
        case let .failed(why): why
        }
    }

    private var programText: String {
        guard case let .success(p) = store.encoded else { return "—" }
        if store.watchVersion == programVersion(p) { return String(localized: "актуальная") }
        return store.heldBack ? String(localized: "ждёт конца тренировки") : String(localized: "старая, обновится")
    }
}
