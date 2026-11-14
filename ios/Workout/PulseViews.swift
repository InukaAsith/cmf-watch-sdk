import Charts
import SwiftUI
import WireKit

/// Without an open workout: start the next day (or another). An open one lives in the mini player.
struct ActiveSection: View {
    @Environment(AppStore.self) private var store
    @Environment(SyncController.self) private var sync

    var body: some View {
        if store.open == nil, !store.days.isEmpty {
            Section("Тренировка") {
                Button { sync.startWorkout(day: store.nextDay); sync.playerShown = true } label: {
                    Label("Начать: \(dayName(store.nextDay))", systemImage: "play.fill").font(.body.weight(.semibold))
                }
                Menu("Другой день") {
                    ForEach(Array(store.days.enumerated()), id: \.element.id) { i, d in
                        Button(d.name) { sync.startWorkout(day: i); sync.playerShown = true }
                    }
                }
            }
        }
    }

    private func dayName(_ i: Int) -> String { i < store.days.count ? store.days[i].name : String(localized: "День \(i + 1)") }
}

func startDate(_ o: OpenWorkout) -> Date { Entry(start: o.start, durS: 0, day: 0, done: 0, planned: 0).date }

/// What the player shows now: the exercise, its set, the rest.
@MainActor struct Now {
    let o: OpenWorkout, day: DayItem, ex: Int?, restEnd: Date?

    init?(_ store: AppStore, _ sync: SyncController, picked: Int? = nil) {
        guard let o = store.open, o.day < store.days.count else { return nil }
        self.o = o
        day = store.days[o.day]
        ex = store.currentExercise(picked: picked)
        restEnd = sync.skippedRest == o.sets.count ? nil : store.restEnd
    }

    func resting(_ now: Date) -> Bool { restEnd.map { $0 > now } ?? false }
    var exercise: Exercise? { ex.map { day.exercises[$0].exercise } }
    var setText: String {
        guard let ex, let e = exercise else { return String(localized: "Все подходы сделаны") }
        return String(localized: "Подход \(min(o.done(ex) + 1, e.sets)) из \(e.sets)")
    }
    var planned: Int { day.exercises.map(\.exercise.sets).reduce(0, +) }
}

/// The open workout above the tab bar, as Podcasts' mini player: the exercise or the rest, "Сделал".
struct MiniPlayer: View {
    @Environment(AppStore.self) private var store
    @Environment(SyncController.self) private var sync
    @Environment(\.tabViewBottomAccessoryPlacement) private var placement

    var body: some View {
        if let n = Now(store, sync) {
            TimelineView(.periodic(from: .now, by: 1)) { ctx in
                HStack(spacing: 10) {
                    Image(systemName: "figure.strengthtraining.traditional").font(.system(size: 16, weight: .semibold))
                        .frame(width: 32, height: 32).foregroundStyle(.white)
                        .background(Color.accentColor, in: .rect(cornerRadius: 8, style: .continuous))
                    VStack(alignment: .leading, spacing: 0) {
                        Text(n.resting(ctx.date) ? String(localized: "Отдых") : n.exercise?.name ?? n.day.name)
                            .font(.subheadline.weight(.semibold)).lineLimit(1)
                        Text(n.resting(ctx.date) ? "\(n.day.name)" : n.setText).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer(minLength: 0)
                    if let end = n.restEnd, n.resting(ctx.date) {
                        Text(timerInterval: ctx.date...end, countsDown: true).font(.subheadline.weight(.semibold))
                            .monospacedDigit().foregroundStyle(Color.accentColor)
                    } else if let ex = n.ex {
                        Button { sync.doneSet(ex) } label: { Image(systemName: "checkmark").font(.system(size: 18, weight: .bold)) }
                            .frame(width: 32, height: 32)
                    }
                    if placement != .inline, let bpm = sync.bpm(for: n.o) {
                        Label("\(bpm)", systemImage: "heart.fill").font(.caption.weight(.semibold)).foregroundStyle(.red)
                            .monospacedDigit()
                    }
                }
                .padding(.horizontal, 12)
            }
        }
    }
}

/// The open workout full screen, as Podcasts' player: the ring of the set or the rest where the cover would be,
/// the workout's progress as the scrubber, undo / Сделал / next as -15 / play / +30.
struct ActiveWorkout: View {
    @Environment(AppStore.self) private var store
    @Environment(SyncController.self) private var sync
    @Environment(\.dismiss) private var dismiss
    @State private var picked: Int?
    @State private var asking = false

    var body: some View {
        VStack(spacing: 0) {
            Capsule().fill(.white.opacity(0.45)).frame(width: 40, height: 5)
                .padding(.vertical, 10).contentShape(.rect).onTapGesture { dismiss() }
            if let n = Now(store, sync, picked: picked) {
                TimelineView(.periodic(from: .now, by: 1)) { ctx in
                    player(n, now: ctx.date)
                }
            } else {
                Spacer()
                ContentUnavailableView("Тренировка закончилась", systemImage: "checkmark.circle")
                Button("Закрыть") { dismiss() }.buttonStyle(.bordered)
                Spacer()
            }
        }
        .padding(.horizontal, 28)
        .padding(.bottom, 8)
        .tint(.white)
        .foregroundStyle(.white)
        .environment(\.colorScheme, .dark)
        .background {
            ZStack {
                Color.black
                Circle().fill(Color.accentColor).frame(width: 420).blur(radius: 120).offset(y: -160).opacity(0.7)
                Color.black.opacity(0.25)
            }
            .ignoresSafeArea()
        }
        .sensoryFeedback(.success, trigger: store.open?.sets.count)
    }

    @ViewBuilder private func player(_ n: Now, now: Date) -> some View {
        let resting = n.resting(now)
        // the "cover": the rest ring, or the set's reps in a ring of its sets
        ZStack {
            Circle().stroke(.white.opacity(0.12), lineWidth: 16)
            if resting, let end = n.restEnd, let last = n.o.sets.last, last.ex < n.day.exercises.count {
                let total = Double(n.day.exercises[last.ex].exercise.restS)
                Circle().trim(from: 0, to: max(0, min(1, end.timeIntervalSince(now) / max(total, 1))))
                    .stroke(Color.accentColor, style: StrokeStyle(lineWidth: 16, lineCap: .round))
                    .rotationEffect(.degrees(-90)).animation(.linear(duration: 1), value: now)
                VStack(spacing: 4) {
                    Text("отдых").font(.headline).foregroundStyle(.secondary)
                    Text(timerInterval: now...end, countsDown: true)
                        .font(.system(size: 64, weight: .bold, design: .rounded)).monospacedDigit()
                }
            } else if let ex = n.ex, let e = n.exercise {
                Circle().trim(from: 0, to: Double(n.o.done(ex)) / Double(max(e.sets, 1)))
                    .stroke(Color.accentColor, style: StrokeStyle(lineWidth: 16, lineCap: .round))
                    .rotationEffect(.degrees(-90)).animation(.spring(duration: 0.5), value: n.o.done(ex))
                VStack(spacing: 4) {
                    Text(e.repsMin == e.repsMax ? "\(e.repsMin)" : "\(e.repsMin)–\(e.repsMax)")
                        .font(.system(size: 72, weight: .bold, design: .rounded)).monospacedDigit()
                    Text("повторений").font(.headline).foregroundStyle(.secondary)
                    if !e.weight.isEmpty { Text(e.weight).font(.title3.bold()).foregroundStyle(Color.accentColor) }
                }
            } else {
                VStack(spacing: 8) {
                    Image(systemName: "flag.checkered").font(.system(size: 56))
                    Text("Все подходы сделаны").font(.headline)
                }
            }
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)

        // title block, the exercises as the chapters menu
        HStack(alignment: .bottom) {
            VStack(alignment: .leading, spacing: 2) {
                Text(n.day.name).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                Text(n.exercise?.name ?? String(localized: "Готово")).font(.title2.bold()).lineLimit(2)
                Text(n.setText).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer()
            Menu {
                ForEach(Array(n.day.exercises.enumerated()), id: \.element.id) { i, item in
                    let done = n.o.done(i), sets = item.exercise.sets
                    Button { picked = i; sync.skippedRest = n.o.sets.count } label: {
                        Label("\(item.exercise.name) · \(done)/\(sets)", systemImage: i == n.ex ? "figure.strengthtraining.traditional"
                              : done >= sets ? "checkmark.circle.fill" : "circle")
                    }
                    .disabled(done >= sets)
                }
            } label: {
                Image(systemName: "list.bullet").font(.headline).frame(width: 36, height: 36)
                    .background(.white.opacity(0.15), in: .circle)
            }
        }

        // the "scrubber": sets done of the workout, elapsed on the left
        VStack(spacing: 6) {
            ProgressView(value: Double(n.o.sets.count), total: Double(max(n.planned, 1))).tint(.white)
            HStack {
                Text(startDate(n.o), style: .timer)
                Spacer()
                if let bpm = sync.bpm(for: n.o) { Label("\(bpm)", systemImage: "heart.fill").foregroundStyle(.red) }
                Spacer()
                Text("\(n.o.sets.count) из \(n.planned)")
            }
            .font(.caption.weight(.medium)).monospacedDigit().foregroundStyle(.secondary)
        }
        .padding(.top, 22)

        // undo, Сделал, next
        HStack {
            Spacer()
            Button { sync.undoSet() } label: { Image(systemName: "arrow.uturn.backward").font(.system(size: 30, weight: .medium)) }
                .disabled(n.o.sets.isEmpty)
            Spacer()
            Button { if let ex = n.ex { sync.doneSet(ex); picked = nil } } label: {
                Image(systemName: "checkmark.circle.fill").font(.system(size: 84))
                    .symbolRenderingMode(.hierarchical)
            }
            .disabled(n.ex == nil || resting)
            Spacer()
            Button { next(n, resting: resting) } label: { Image(systemName: "forward.end.fill").font(.system(size: 30, weight: .medium)) }
                .disabled(n.ex == nil && !resting)
            Spacer()
        }
        .padding(.vertical, 22)

        HStack {
            if let last = n.o.sets.last, last.ex < n.day.exercises.count {
                Text("Последний: \(n.day.exercises[last.ex].exercise.name)").font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer()
            Button("Завершить") { asking = true }
                .font(.subheadline.bold()).padding(.horizontal, 14).frame(height: 32)
                .background(.white.opacity(0.15), in: .capsule)
                .confirmationDialog("Завершить тренировку?", isPresented: $asking, titleVisibility: .visible) {
                    Button("Завершить", role: .destructive) { sync.finishWorkout(); dismiss() }
                } message: {
                    Text("Подходов: \(n.o.sets.count) из \(n.planned)")
                }
        }
    }

    /// Ends the rest, or moves on to the next exercise with sets left.
    private func next(_ n: Now, resting: Bool) {
        if resting { sync.skippedRest = n.o.sets.count; return }
        let open = n.day.exercises.indices.filter { n.o.done($0) < n.day.exercises[$0].exercise.sets }
        guard let ex = n.ex, let i = open.firstIndex(of: ex) else { return }
        picked = open[(i + 1) % open.count]
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
