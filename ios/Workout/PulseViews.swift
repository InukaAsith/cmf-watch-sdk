import Charts
import SwiftUI
import WireKit

/// The workout running on the watch, as a row that opens it.
struct ActiveSection: View {
    @Environment(AppStore.self) private var store
    @State private var open = false
    let live: SyncController.Live

    var body: some View {
        Section("Активная тренировка") {
            Button { open = true } label: {
                HStack(spacing: 14) {
                    Image(systemName: "figure.strengthtraining.traditional").font(.title2)
                        .foregroundStyle(Color(.secondarySystemGroupedBackground))
                        .frame(width: 44, height: 44)
                        .background(Color.accentColor, in: .rect(cornerRadius: 14, style: .continuous))
                        .symbolEffect(.pulse)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(live.dayName(store)).font(.body.weight(.semibold))
                        Text(live.started, style: .timer).font(.subheadline).foregroundStyle(.secondary).monospacedDigit()
                    }
                    Spacer()
                    if live.bpm > 0 {
                        Label("\(live.bpm)", systemImage: "heart.fill").labelStyle(.titleAndIcon)
                            .foregroundStyle(.red).monospacedDigit().contentTransition(.numericText())
                    }
                    Image(systemName: "chevron.up").font(.footnote.bold()).foregroundStyle(.tertiary)
                }
                .padding(.vertical, 4)
            }
            .buttonStyle(.plain)
        }
        .sheet(isPresented: $open) { ActiveWorkout().presentationDragIndicator(.visible) }
    }
}

/// What the watch shows, bigger: time, heart rate, the open exercise and its set or rest.
struct ActiveWorkout: View {
    @Environment(AppStore.self) private var store
    @Environment(SyncController.self) private var sync
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                if let live = sync.live {
                    let day = live.day < store.days.count ? store.days[live.day] : nil
                    VStack(spacing: 12) {
                        Text(live.started, style: .timer)
                            .font(.system(size: 56, weight: .bold, design: .rounded)).monospacedDigit()
                            .frame(maxWidth: .infinity)
                        HStack(alignment: .firstTextBaseline, spacing: 6) {
                            Image(systemName: "heart.fill").foregroundStyle(.red)
                                .symbolEffect(.pulse, isActive: live.bpm > 0)
                            Text(live.bpm > 0 ? "\(live.bpm)" : "–").font(.system(size: 34, weight: .semibold, design: .rounded))
                                .monospacedDigit().contentTransition(.numericText())
                            Text("уд/мин").foregroundStyle(.secondary)
                        }
                        .accessibilityElement(children: .combine)
                        if live.samples.count > 1 {
                            Chart(live.samples, id: \.0) {
                                LineMark(x: .value("Время", $0.0), y: .value("Пульс", $0.1)).interpolationMethod(.catmullRom)
                            }
                            .foregroundStyle(.red).chartYScale(domain: .automatic(includesZero: false))
                            .chartXAxis(.hidden).frame(height: 100)
                            .accessibilityLabel("Пульс за тренировку")
                        }
                        if let ex = live.ex, let day, ex < day.exercises.count {
                            let e = day.exercises[ex]
                            VStack(alignment: .leading, spacing: 8) {
                                Text(e.exercise.name).font(.title2.bold())
                                Text(e.summary).font(.subheadline).foregroundStyle(.secondary)
                                if let end = live.restEnd, end > .now {
                                    HStack(alignment: .firstTextBaseline) {
                                        Text("Отдых").font(.headline)
                                        Spacer()
                                        Text(timerInterval: Date.now...end, countsDown: true)
                                            .font(.system(.largeTitle, design: .rounded, weight: .bold)).monospacedDigit()
                                    }
                                    .padding(.top, 4)
                                } else {
                                    Text("Подход \(min(live.exDone + 1, e.exercise.sets)) из \(e.exercise.sets)")
                                        .font(.system(.title, design: .rounded, weight: .bold)).padding(.top, 4)
                                }
                                ProgressView(value: Double(live.exDone), total: Double(max(e.exercise.sets, 1)))
                            }
                            .card()
                        }
                        if let day {
                            VStack(alignment: .leading, spacing: 10) {
                                ForEach(Array(day.exercises.enumerated()), id: \.element.id) { i, e in
                                    HStack {
                                        Image(systemName: i == live.ex ? "play.circle.fill" : "circle")
                                            .foregroundStyle(i == live.ex ? Color.accentColor : .secondary)
                                        Text(e.exercise.name).fontWeight(i == live.ex ? .semibold : .regular)
                                        Spacer()
                                        Text("\(e.exercise.sets)×").foregroundStyle(.secondary).monospacedDigit()
                                    }
                                }
                                Divider()
                                LabeledContent("Подходов сделано", value: "\(live.done) из \(day.exercises.map(\.exercise.sets).reduce(0, +))")
                            }
                            .card()
                        }
                    }
                    .padding()
                } else {
                    ContentUnavailableView("Тренировка закончилась", systemImage: "checkmark.circle")
                        .padding(.top, 120)
                }
            }
            .background(Color(.systemGroupedBackground))
            .navigationTitle(sync.live.map { $0.dayName(store) } ?? "")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { Button("Готово") { dismiss() } }
        }
    }
}

extension SyncController.Live {
    var started: Date { Entry(start: start, durS: 0, day: 0, done: 0, planned: 0).date }
    @MainActor func dayName(_ store: AppStore) -> String {
        day < store.days.count ? store.days[day].name : String(localized: "День \(day + 1)")
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
