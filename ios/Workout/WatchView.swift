import SwiftUI
import WireKit

struct WatchView: View {
    @Environment(AppStore.self) private var store
    @Environment(SyncController.self) private var sync

    var body: some View {
        NavigationStack {
            List {
                Section {
                    LabeledContent("Связь", value: linkText)
                    if let last = store.lastSync {
                        LabeledContent("Синхронизация") { Text(last, style: .relative) + Text(" назад") }
                    }
                    LabeledContent("Программа на часах", value: programText)
                    if sync.syncing { ProgressView("Синхронизирую…") }
                } footer: {
                    Text("Откройте «Тренировка» на часах: история и программа синхронизируются сами, "
                         + "в том числе когда телефон в кармане.")
                }

                Section("Приложение на часах") {
                    LabeledContent("Установлено", value: sync.installed.map { "версия \($0.version)" } ?? "нет")
                    if let b = sync.bundledHeader {
                        LabeledContent("В телефоне", value: "версия \(b.version)")
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
            .navigationTitle("Часы")
        }
    }

    private var linkText: String {
        switch sync.phase {
        case .bluetoothOff: "Bluetooth выключен"
        case .searching: "Ищу часы…"
        case .connecting: "Подключаюсь…"
        case .connected: "Подключено"
        case let .failed(why): why
        }
    }

    private var programText: String {
        guard case let .success(p) = store.encoded else { return "—" }
        if store.watchVersion == programVersion(p) { return "актуальная" }
        return store.heldBack ? "ждёт конца тренировки" : "старая, обновится"
    }
}
