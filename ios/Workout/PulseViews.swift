import Charts
import SwiftUI
import WireKit

/// The open workout (on the phone, the watch or both) as a row that opens it; without one, a start button.
struct ActiveSection: View {
    @Environment(AppStore.self) private var store
    @Environment(SyncController.self) private var sync
    @State private var open = false

    var body: some View {
        Section {
            if let o = store.open {
                Button { open = true } label: {
                    HStack(spacing: 14) {
                        Image(systemName: "figure.strengthtraining.traditional").font(.title2)
                            .foregroundStyle(Color(.secondarySystemGroupedBackground))
                            .frame(width: 44, height: 44)
                            .background(Color.accentColor, in: .rect(cornerRadius: 14, style: .continuous))
                            .symbolEffect(.pulse)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(dayName(o.day)).font(.body.weight(.semibold))
                            Text(startDate(o), style: .timer).font(.subheadline).foregroundStyle(.secondary).monospacedDigit()
                        }
                        Spacer()
                        if let bpm = sync.bpm(for: o) {
                            Label("\(bpm)", systemImage: "heart.fill").foregroundStyle(.red).monospacedDigit()
                        }
                        Image(systemName: "chevron.up").font(.footnote.bold()).foregroundStyle(.tertiary)
                    }
                    .padding(.vertical, 4)
                }
                .buttonStyle(.plain)
            } else if !store.days.isEmpty {
                Button { sync.startWorkout(day: store.nextDay); open = true } label: {
                    Label("Начать: \(dayName(store.nextDay))", systemImage: "play.fill").font(.body.weight(.semibold))
                }
                Menu("Другой день") {
                    ForEach(Array(store.days.enumerated()), id: \.element.id) { i, d in
                        Button(d.name) { sync.startWorkout(day: i); open = true }
                    }
                }
            }
        } header: {
            Text(store.open == nil ? "Тренировка" : "Активная тренировка")
        }
        .sheet(isPresented: $open) { ActiveWorkout().presentationDragIndicator(.visible) }
    }

    private func dayName(_ i: Int) -> String { i < store.days.count ? store.days[i].name : String(localized: "День \(i + 1)") }
}

func startDate(_ o: OpenWorkout) -> Date { Entry(start: o.start, durS: 0, day: 0, done: 0, planned: 0).date }

/// The workout as the watch shows it, bigger: time and heart rate, the current exercise with its set or the rest,
/// every exercise with its set dots, "Завершить".
struct ActiveWorkout: View {
    @Environment(AppStore.self) private var store
    @Environment(SyncController.self) private var sync
    @Environment(\.dismiss) private var dismiss
    @State private var picked: Int?
    @State private var skipped: Int? // sets count whose rest was skipped
    @State private var asking = false

    var body: some View {
        NavigationStack {
            ScrollView {
                if let o = store.open, o.day < store.days.count {
                    let day = store.days[o.day]
                    VStack(spacing: 12) {
                        header(o)
                        TimelineView(.periodic(from: .now, by: 1)) { ctx in
                            if let end = store.restEnd, end > ctx.date, skipped != o.sets.count {
                                rest(o, day, end: end)
                            } else if let ex = current(o, day) {
                                setCard(o, day.exercises[ex], ex)
                            }
                        }
                        exercises(o, day)
                        if let last = o.sets.last, last.ex < day.exercises.count {
                            Button("Отменить подход: \(day.exercises[last.ex].exercise.name)", systemImage: "arrow.uturn.backward") {
                                sync.undoSet()
                            }
                            .font(.subheadline)
                        }
                        Button(role: .destructive) { asking = true } label: {
                            Text("Завершить тренировку").font(.headline).frame(maxWidth: .infinity).padding(.vertical, 6)
                        }
                        .buttonStyle(.bordered).controlSize(.large)
                        .confirmationDialog("Завершить тренировку?", isPresented: $asking, titleVisibility: .visible) {
                            Button("Завершить", role: .destructive) { sync.finishWorkout(); dismiss() }
                        } message: {
                            Text("Подходов: \(o.sets.count)")
                        }
                    }
                    .padding()
                } else {
                    ContentUnavailableView("Тренировка закончилась", systemImage: "checkmark.circle").padding(.top, 120)
                }
            }
            .background(Color(.systemGroupedBackground))
            .navigationTitle(store.open.map { $0.day < store.days.count ? store.days[$0.day].name : "" } ?? "")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { Button("Готово") { dismiss() } }
        }
    }

    /// The picked exercise if it has sets left, else the newest set's, else the first open one from the top.
    private func current(_ o: OpenWorkout, _ day: DayItem) -> Int? { store.currentExercise(picked: picked) }

    private func header(_ o: OpenWorkout) -> some View {
        HStack(alignment: .firstTextBaseline) {
            Text(startDate(o), style: .timer)
                .font(.system(size: 48, weight: .bold, design: .rounded)).monospacedDigit()
            Spacer()
            HStack(spacing: 4) {
                Image(systemName: "heart.fill").foregroundStyle(.red).symbolEffect(.pulse, isActive: sync.bpm(for: o) != nil)
                Text(sync.bpm(for: o).map(String.init) ?? "–").font(.system(.title, design: .rounded, weight: .semibold))
                    .monospacedDigit().contentTransition(.numericText())
            }
            .accessibilityElement(children: .combine)
        }
        .card()
    }

    private func setCard(_ o: OpenWorkout, _ item: ExerciseItem, _ ex: Int) -> some View {
        let e = item.exercise
        return VStack(alignment: .leading, spacing: 10) {
            Text(e.name).font(.title2.bold())
            Text("Подход \(min(o.done(ex) + 1, e.sets)) из \(e.sets)").font(.subheadline).foregroundStyle(.secondary)
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(e.repsMin == e.repsMax ? "\(e.repsMin)" : "\(e.repsMin)–\(e.repsMax)")
                    .font(.system(size: 56, weight: .bold, design: .rounded)).monospacedDigit()
                Text("повт.").foregroundStyle(.secondary)
                Spacer()
                if !e.weight.isEmpty { Text(e.weight).font(.title3.weight(.semibold)).foregroundStyle(Color.accentColor) }
            }
            Dots(done: o.done(ex), total: e.sets, size: 12)
            Button { sync.doneSet(ex) } label: {
                Text("Сделал").font(.title3.bold()).frame(maxWidth: .infinity).padding(.vertical, 10)
            }
            .buttonStyle(.borderedProminent).buttonBorderShape(.capsule).controlSize(.large)
            .sensoryFeedback(.success, trigger: o.sets.count)
        }
        .card()
    }

    private func rest(_ o: OpenWorkout, _ day: DayItem, end: Date) -> some View {
        let last = o.sets.last!
        let total = Double(last.ex < day.exercises.count ? day.exercises[last.ex].exercise.restS : 60)
        let left = end.timeIntervalSinceNow
        return VStack(spacing: 14) {
            ZStack {
                Circle().stroke(Color.accentColor.opacity(0.15), lineWidth: 14)
                Circle().trim(from: 0, to: max(0, min(1, left / total)))
                    .stroke(Color.accentColor, style: StrokeStyle(lineWidth: 14, lineCap: .round))
                    .rotationEffect(.degrees(-90))
                    .animation(.linear(duration: 1), value: left)
                VStack(spacing: 2) {
                    Text("Отдых").font(.subheadline).foregroundStyle(.secondary)
                    Text(timerInterval: Date.now...end, countsDown: true)
                        .font(.system(size: 48, weight: .bold, design: .rounded)).monospacedDigit()
                }
            }
            .frame(width: 200, height: 200)
            if let ex = current(o, day) {
                Text("Дальше: \(day.exercises[ex].exercise.name)").font(.subheadline).foregroundStyle(.secondary)
            }
            Button("Пропустить") { skipped = o.sets.count }.buttonStyle(.bordered).buttonBorderShape(.capsule)
        }
        .frame(maxWidth: .infinity)
        .card()
        .sensoryFeedback(.warning, trigger: left <= 0.5)
    }

    private func exercises(_ o: OpenWorkout, _ day: DayItem) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(day.exercises.enumerated()), id: \.element.id) { i, item in
                let e = item.exercise
                let complete = o.done(i) >= e.sets
                Button { picked = i; skipped = o.sets.count } label: {
                    HStack(spacing: 12) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(e.name).font(.body.weight(i == current(o, day) ? .semibold : .regular))
                                .foregroundStyle(complete ? .secondary : .primary)
                            Text(item.summary).font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Dots(done: o.done(i), total: e.sets, size: 9)
                    }
                    .padding(.vertical, 10)
                    .contentShape(.rect)
                }
                .buttonStyle(.plain)
                .disabled(complete)
                if i < day.exercises.count - 1 { Divider() }
            }
        }
        .card()
    }
}

/// The sets of an exercise as on the watch: done ones filled in the accent.
struct Dots: View {
    let done: Int, total: Int, size: CGFloat

    var body: some View {
        HStack(spacing: size * 0.6) {
            ForEach(0..<min(total, 8), id: \.self) { k in
                Circle().fill(k < done ? Color.accentColor : Color(.tertiarySystemFill)).frame(width: size, height: size)
            }
        }
        .accessibilityElement()
        .accessibilityLabel("Подходов \(done) из \(total)")
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
