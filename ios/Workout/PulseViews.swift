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

/// The watch's accent (RGB565 0xFA23): the player is dark, the app's own accent is not.
let brand = Color(red: 1, green: 0.27, blue: 0.09)

/// The open workout full screen, as a music player with its queue: "now playing" is the current exercise (or the
/// rest), the queue is the day's exercises with their set dots, the transport is undo / Сделал / next.
struct ActiveWorkout: View {
    @Environment(AppStore.self) private var store
    @Environment(SyncController.self) private var sync
    @Environment(\.dismiss) private var dismiss
    @State private var picked: Int?
    @State private var asking = false

    var body: some View {
        VStack(spacing: 0) {
            Capsule().fill(.white.opacity(0.35)).frame(width: 40, height: 5)
                .padding(.vertical, 10).contentShape(.rect).onTapGesture { dismiss() }
            if let n = Now(store, sync, picked: picked) {
                TimelineView(.periodic(from: .now, by: 1)) { ctx in
                    VStack(spacing: 0) {
                        header(n)
                        ScrollView {
                            VStack(spacing: 14) {
                                nowCard(n, now: ctx.date)
                                queue(n)
                            }
                            .padding(.vertical, 12)
                        }
                        .scrollIndicators(.hidden)
                        transport(n, resting: n.resting(ctx.date))
                    }
                }
            } else {
                Spacer()
                ContentUnavailableView("Тренировка закончилась", systemImage: "checkmark.circle")
                Button("Закрыть") { dismiss() }.buttonStyle(.bordered)
                Spacer()
            }
        }
        .padding(.horizontal, 20)
        .tint(brand)
        .foregroundStyle(.white)
        .environment(\.colorScheme, .dark)
        .background {
            ZStack {
                Color.black
                LinearGradient(colors: [brand.opacity(0.35), .clear], startPoint: .top, endPoint: .center)
            }
            .ignoresSafeArea()
        }
        .sensoryFeedback(.success, trigger: store.open?.sets.count)
    }

    private func header(_ n: Now) -> some View {
        HStack(alignment: .center, spacing: 12) {
            VStack(alignment: .leading, spacing: 0) {
                Text(n.day.name).font(.headline).lineLimit(1)
                Text(startDate(n.o), style: .timer).font(.subheadline).monospacedDigit().foregroundStyle(.secondary)
            }
            Spacer()
            if let bpm = sync.bpm(for: n.o) {
                let z = store.zones?.zone(bpm) ?? 0
                Label("\(bpm)", systemImage: "heart.fill")
                    .font(.headline).monospacedDigit()
                    .foregroundStyle(z > 0 ? zoneColors[z - 1] : .red)
            }
            Button("Завершить") { asking = true }
                .font(.subheadline.bold()).foregroundStyle(.white)
                .padding(.horizontal, 12).frame(height: 32)
                .background(.white.opacity(0.15), in: .capsule)
                .confirmationDialog("Завершить тренировку?", isPresented: $asking, titleVisibility: .visible) {
                    Button("Завершить", role: .destructive) { sync.finishWorkout(); dismiss() }
                } message: {
                    Text("Подходов: \(n.o.sets.count) из \(n.planned)")
                }
        }
    }

    /// "Now playing": the current exercise with its set, reps, weight and dots; the rest with its countdown ring.
    @ViewBuilder private func nowCard(_ n: Now, now: Date) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            if n.resting(now), let end = n.restEnd, let last = n.o.sets.last, last.ex < n.day.exercises.count {
                let total = Double(max(n.day.exercises[last.ex].exercise.restS, 1))
                HStack(spacing: 20) {
                    ZStack {
                        Circle().stroke(.white.opacity(0.12), lineWidth: 10)
                        Circle().trim(from: 0, to: max(0, min(1, end.timeIntervalSince(now) / total)))
                            .stroke(brand, style: StrokeStyle(lineWidth: 10, lineCap: .round))
                            .rotationEffect(.degrees(-90)).animation(.linear(duration: 1), value: now)
                        Text(timerInterval: now...end, countsDown: true)
                            .font(.system(size: 30, weight: .bold, design: .rounded)).monospacedDigit()
                    }
                    .frame(width: 120, height: 120)
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Отдых").font(.title2.bold())
                        if let e = n.exercise { Text("Дальше: \(e.name)").foregroundStyle(.secondary) }
                        Text(n.setText).foregroundStyle(.secondary)
                    }
                }
            } else if let ex = n.ex, let e = n.exercise {
                Text("Текущее упражнение").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                Text(e.name).font(.title.bold()).lineLimit(2)
                // reps on a line of their own: a long weight beside them wrapped "8–10"
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Text(e.repsMin == e.repsMax ? "\(e.repsMin)" : "\(e.repsMin)–\(e.repsMax)")
                        .font(.system(size: 64, weight: .bold, design: .rounded)).monospacedDigit()
                        .lineLimit(1).minimumScaleFactor(0.5)
                    Text("повт.").font(.title3).foregroundStyle(.secondary)
                }
                if !e.weight.isEmpty {
                    Label(e.weight, systemImage: "scalemass.fill")
                        .font(.title3.bold()).foregroundStyle(brand).lineLimit(1)
                }
                HStack {
                    Text(n.setText).foregroundStyle(.secondary)
                    Spacer()
                    Dots(done: n.o.done(ex), total: e.sets, size: 12, color: brand)
                }
            } else {
                Label("Все подходы сделаны", systemImage: "flag.checkered").font(.title2.bold())
                Text("Завершите тренировку").foregroundStyle(.secondary)
            }
        }
        .padding(18)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.white.opacity(0.08), in: .rect(cornerRadius: 24, style: .continuous))
    }

    /// The queue: every exercise with its dots, the current one marked; a tap makes it current.
    private func queue(_ n: Now) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Упражнения").font(.headline).padding(.leading, 4).padding(.bottom, 4)
            ForEach(Array(n.day.exercises.enumerated()), id: \.element.id) { i, item in
                let e = item.exercise, done = n.o.done(i), complete = done >= e.sets
                Button { picked = i; sync.skippedRest = n.o.sets.count } label: {
                    HStack(spacing: 12) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(e.name).font(.body.weight(i == n.ex ? .semibold : .regular))
                                .foregroundStyle(complete ? .secondary : .primary).lineLimit(1)
                            Text(item.summary).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                        Spacer()
                        Dots(done: done, total: e.sets, size: 9, color: brand)
                    }
                    .padding(.vertical, 12).padding(.horizontal, 14)
                    .background(i == n.ex ? brand.opacity(0.18) : .clear, in: .rect(cornerRadius: 14, style: .continuous))
                    .contentShape(.rect)
                }
                .buttonStyle(.plain)
                .disabled(complete)
            }
        }
    }

    /// The workout's progress, then undo / Сделал / next.
    private func transport(_ n: Now, resting: Bool) -> some View {
        VStack(spacing: 14) {
            VStack(spacing: 6) {
                ProgressView(value: Double(n.o.sets.count), total: Double(max(n.planned, 1))).tint(brand)
                HStack {
                    Text("\(n.o.sets.count) из \(n.planned) подходов")
                    Spacer()
                    if let z = store.zones, let bpm = sync.bpm(for: n.o), z.zone(bpm) > 0 {
                        Text("Зона \(z.zone(bpm)) · \(Text(LocalizedStringKey(zoneNames[z.zone(bpm) - 1])))")
                            .foregroundStyle(zoneColors[z.zone(bpm) - 1])
                    }
                }
                .font(.caption.weight(.medium)).monospacedDigit().foregroundStyle(.secondary)
            }
            if resting {
                Button { sync.skippedRest = n.o.sets.count } label: {
                    Text("Пропустить отдых").font(.title3.bold()).frame(maxWidth: .infinity).padding(.vertical, 8)
                }
                .buttonStyle(.bordered).buttonBorderShape(.capsule).controlSize(.large)
            } else {
                Button { if let ex = n.ex { sync.doneSet(ex); picked = nil } } label: {
                    Text("Сделал").font(.title3.bold()).frame(maxWidth: .infinity).padding(.vertical, 8)
                }
                .buttonStyle(.borderedProminent).buttonBorderShape(.capsule).controlSize(.large)
                .disabled(n.ex == nil)
            }
            if let last = n.o.sets.last, last.ex < n.day.exercises.count {
                Button("Отменить подход: \(n.day.exercises[last.ex].exercise.name)", systemImage: "arrow.uturn.backward") {
                    sync.undoSet()
                }
                .font(.subheadline).foregroundStyle(.secondary)
            }
        }
        .padding(.top, 10)
        .padding(.bottom, 6)
    }
}

/// The sets of an exercise as on the watch: done ones filled in the accent.
struct Dots: View {
    let done: Int, total: Int, size: CGFloat
    var color: Color = .accentColor

    var body: some View {
        HStack(spacing: size * 0.6) {
            ForEach(0..<min(total, 8), id: \.self) { k in
                Circle().fill(k < done ? color : Color(.tertiarySystemFill)).frame(width: size, height: size)
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
                        if let z = store.zones {
                            ForEach(0..<5, id: \.self) { k in
                                RectangleMark(yStart: .value("от", z.lows[k]), yEnd: .value("до", k < 4 ? z.lows[k + 1] : z.max + 10))
                                    .foregroundStyle(zoneColors[k].opacity(0.12))
                            }
                        }
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
                    if let z = store.zones {
                        let mins = zoneMinutes(t, z)
                        ForEach(0..<5, id: \.self) { k in
                            HStack {
                                Circle().fill(zoneColors[k]).frame(width: 10, height: 10)
                                Text(LocalizedStringKey(zoneNames[k]))
                                Spacer()
                                Text("\(Int(mins[k].rounded())) мин").monospacedDigit().foregroundStyle(.secondary)
                            }
                        }
                    }
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
