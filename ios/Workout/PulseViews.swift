import Charts
import SwiftUI
import WireKit

/// The workout running on the watch: heart rate now and since the phone saw it, the day and the open exercise.
struct LiveSection: View {
    @Environment(AppStore.self) private var store
    let live: SyncController.Live

    var body: some View {
        Section("Сейчас на часах") {
            HStack(alignment: .firstTextBaseline) {
                Image(systemName: "heart.fill").foregroundStyle(.red)
                Text(live.bpm > 0 ? "\(live.bpm)" : "–").font(.system(size: 44, weight: .bold, design: .rounded))
                    .monospacedDigit().contentTransition(.numericText())
                Text("уд/мин").foregroundStyle(.secondary)
            }
            .accessibilityElement(children: .combine)
            if live.samples.count > 1 {
                Chart(live.samples, id: \.0) { LineMark(x: .value("Время", $0.0), y: .value("Пульс", $0.1)) }
                    .foregroundStyle(.red).chartYScale(domain: .automatic(includesZero: false)).frame(height: 120)
                    .accessibilityLabel("Пульс за тренировку")
            }
            let day = live.day < store.days.count ? store.days[live.day] : nil
            if let day { LabeledContent("День", value: day.name) }
            if let ex = live.ex, let day, ex < day.exercises.count {
                LabeledContent("Упражнение", value: day.exercises[ex].exercise.name)
            }
            LabeledContent("Подходов сделано", value: "\(live.done)")
        }
    }
}

/// One workout: heart rate with its sets marked, the peak of each set and the same day last time.
struct HistoryDetail: View {
    @Environment(AppStore.self) private var store
    let entry: HistoryEntry

    var body: some View {
        List {
            Section {
                LabeledContent("Длительность", value: String(format: "%d:%02d", entry.durS / 3600, entry.durS % 3600 / 60))
                LabeledContent("Подходов", value: "\(entry.done) из \(entry.planned)")
                if let s = stats(entry) {
                    LabeledContent("Пульс средний / макс.", value: "\(s.avg) / \(s.max)")
                }
                if let prev, let a = stats(entry), let b = stats(prev) {
                    LabeledContent("Прошлый раз (\(prev.date.formatted(date: .abbreviated, time: .omitted)))",
                                   value: "\(b.avg) / \(b.max) (\(signed(a.avg - b.avg)))")
                }
            }
            if let t = entry.trace, t.hr.contains(where: { $0 > 0 }) {
                Section("Пульс") {
                    Chart {
                        ForEach(Array(t.hr.enumerated()).filter { $0.1 > 0 }, id: \.0) { i, bpm in
                            LineMark(x: .value("Минута", Double(i * Trace.step) / 60), y: .value("Пульс", Int(bpm)))
                                .foregroundStyle(.red)
                        }
                        ForEach(Array(t.sets.enumerated()), id: \.0) { _, s in
                            RuleMark(x: .value("Подход", Double(s.t) / 60)).foregroundStyle(.secondary.opacity(0.4))
                        }
                    }
                    .chartXAxisLabel("мин").chartYScale(domain: .automatic(includesZero: false)).frame(height: 200)
                    .accessibilityLabel("Пульс за тренировку, линии — подходы")
                }
            }
            if let t = entry.trace, !t.sets.isEmpty {
                Section("Подходы") {
                    ForEach(Array(t.sets.enumerated()), id: \.0) { k, s in
                        let peak = setPeak(t, k)
                        let before = prevPeak(of: s.ex, nth: t.sets[..<k].filter { $0.ex == s.ex }.count)
                        HStack {
                            VStack(alignment: .leading) {
                                Text(name(s.ex))
                                Text("\(t.sets[..<k].filter { $0.ex == s.ex }.count + 1)-й подход · \(clock(s.t))")
                                    .font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer()
                            VStack(alignment: .trailing) {
                                Text(peak > 0 ? "\(peak)" : "–").monospacedDigit()
                                if let before, peak > 0 {
                                    Text("было \(before)").font(.caption).foregroundStyle(.secondary)
                                }
                            }
                        }
                        .accessibilityElement(children: .combine)
                    }
                }
            } else if entry.trace == nil {
                Section { Text("Подробностей нет: часы хранят их только до следующей тренировки.").foregroundStyle(.secondary) }
            }
        }
        .navigationTitle(entry.dayName)
    }

    /// The same day's previous workout that has a trace.
    private var prev: HistoryEntry? {
        store.history.first { $0.start < entry.start && $0.dayName == entry.dayName && $0.trace != nil }
    }

    private func name(_ ex: Int) -> String {
        entry.exercises.flatMap { ex < $0.count ? $0[ex] : nil } ?? "Упражнение \(ex + 1)"
    }

    /// The peak of the `nth` set of the exercise with the same name in the previous workout.
    private func prevPeak(of ex: Int, nth: Int) -> Int? {
        guard let p = prev, let t = p.trace else { return nil }
        let pex = p.exercises?.firstIndex(of: name(ex)) ?? ex
        guard let k = t.sets.indices.filter({ t.sets[$0].ex == pex }).dropFirst(nth).first else { return nil }
        let v = setPeak(t, k)
        return v > 0 ? v : nil
    }
}

/// The highest bpm from the previous set's "Сделал" (or the start) to 20 s after this one's: the effort and its peak.
func setPeak(_ t: Trace, _ k: Int) -> Int {
    let from = k == 0 ? 0 : t.sets[k - 1].t, to = t.sets[k].t + 20
    let lo = from / Trace.step, hi = min(to / Trace.step, t.hr.count - 1)
    guard lo <= hi else { return 0 }
    return Int(t.hr[lo...hi].max() ?? 0)
}

func stats(_ e: HistoryEntry) -> (avg: Int, max: Int)? {
    let v = e.trace?.hr.filter { $0 > 0 }.map(Int.init) ?? []
    return v.isEmpty ? nil : (v.reduce(0, +) / v.count, v.max()!)
}

private func clock(_ s: Int) -> String { String(format: "%d:%02d", s / 60, s % 60) }
private func signed(_ v: Int) -> String { v > 0 ? "+\(v)" : "\(v)" }
