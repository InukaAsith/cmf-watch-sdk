import Charts
import SwiftUI
import WireKit

/// Zone colours 1...5 as on the watch: grey, blue, green, orange, red.
let zoneColors: [Color] = [.gray, .blue, .green, .orange, .red]
let zoneNames = ["Восстановление", "Лёгкая", "Аэробная", "Пороговая", "Максимальная"]

/// Age (and, if known, the resting rate) in, the five zones out; the watch gets their bounds at the next sync.
struct ZonesSection: View {
    @Environment(AppStore.self) private var store
    @Environment(SyncController.self) private var sync

    var body: some View {
        Section {
            if let z = store.zones {
                Stepper("Возраст: \(z.age)", value: Binding(get: { z.age }, set: { v in change { $0.age = v } }), in: 10...90)
                Toggle("Учитывать пульс покоя", isOn: Binding(get: { z.resting != nil },
                                                             set: { on in change { $0.resting = on ? 60 : nil } }))
                if let r = z.resting {
                    Stepper("Пульс покоя: \(r)", value: Binding(get: { r }, set: { v in change { $0.resting = v } }), in: 35...100)
                }
                LabeledContent("Максимальный", value: "\(z.max) уд/мин")
                ForEach(0..<5, id: \.self) { k in
                    HStack {
                        Circle().fill(zoneColors[k]).frame(width: 10, height: 10)
                        Text("Зона \(k + 1)")
                        Text(LocalizedStringKey(zoneNames[k])).foregroundStyle(.secondary)
                        Spacer()
                        Text(k < 4 ? "\(z.lows[k])–\(z.lows[k + 1] - 1)" : "\(z.lows[k])+").monospacedDigit()
                    }
                }
            } else {
                Button("Задать возраст", systemImage: "heart.text.square") { store.zones = HeartZones(age: 30); push() }
            }
        } footer: {
            Text("Максимум по формуле Танаки (208 − 0,7 × возраст), зоны по 10 % от 50 %; с пульсом покоя — от резерва (Карвонен). Часы показывают зону цветом.")
        }
    }

    private func change(_ f: (inout HeartZones) -> Void) {
        guard var z = store.zones else { return }
        f(&z)
        store.zones = z
        push()
    }

    private func push() { Task { await sync.sendZones() } }
}

/// Five segments, the current zone lit and named (the reference's "ZONE 2" pill).
struct ZoneBar: View {
    let zone: Int

    var body: some View {
        HStack(spacing: 4) {
            ForEach(0..<5, id: \.self) { k in
                Capsule().fill(zoneColors[k].opacity(k + 1 == zone ? 1 : 0.25))
                    .frame(height: 8)
                    .overlay {
                        if k + 1 == zone {
                            Text("Зона \(zone)").font(.caption2.bold()).foregroundStyle(.white).fixedSize()
                                .padding(.horizontal, 6).padding(.vertical, 2)
                                .background(zoneColors[k], in: .capsule)
                        }
                    }
            }
        }
        .animation(.spring(duration: 0.4), value: zone)
        .accessibilityElement()
        .accessibilityLabel(zone > 0 ? Text("Зона \(zone): ") + Text(LocalizedStringKey(zoneNames[zone - 1])) : Text("Вне зон"))
    }
}

/// Minutes in each zone of a trace.
func zoneMinutes(_ t: Trace, _ z: HeartZones) -> [Double] {
    var m = [Double](repeating: 0, count: 5)
    for bpm in t.hr { let k = z.zone(Int(bpm)); if k > 0 { m[k - 1] += Double(Trace.step) / 60 } }
    return m
}
