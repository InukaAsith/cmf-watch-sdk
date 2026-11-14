import Foundation
import Observation
import WireKit

/// A day and its exercises as the editor holds them: WireKit's types plus stable ids for lists.
struct DayItem: Codable, Identifiable, Hashable {
    var id = UUID()
    var name: String
    var exercises: [ExerciseItem]

    var day: Day { Day(name: name, exercises: exercises.map(\.exercise)) }
}

struct ExerciseItem: Codable, Identifiable, Hashable {
    var id = UUID()
    var exercise: Exercise

    var summary: String {
        let e = exercise
        let reps = e.repsMin == e.repsMax ? "\(e.repsMin)" : "\(e.repsMin)–\(e.repsMax)"
        return ([ "\(e.sets)×\(reps)", e.weight.isEmpty ? nil : e.weight, String(localized: "отдых \(restText(e.restS))")] as [String?])
            .compactMap { $0 }.joined(separator: " · ")
    }
}

func restText(_ s: Int) -> String { String(format: "%d:%02d", s / 60, s % 60) }

/// Everything the app keeps: the program (the only copy), the history (forever), the day names of each pushed
/// version and the cached pairing secret. One JSON file in Application Support.
@MainActor @Observable
final class AppStore {
    struct Saved: Codable {
        var days: [DayItem] = []
        var history: [HistoryEntry] = []
        var pushed: PushedPrograms = [:]
        var watchVersion: UInt32?
        var heldBack = false
        var lastSync: Date?
        /// starts of entries the owner deleted: the watch still has them, a sync must not bring them back
        var deleted: Set<UInt32>?
        /// each pushed version's exercise names per day (history names a trace's sets from them)
        var pushedExercises: [UInt32: [[String]]]?
        /// the workout in progress, on the phone, the watch or both (docs/specs/2026-11-14-shared-workout-design.md)
        var open: OpenWorkout?
        /// workouts finished on the phone the watch has not heard of yet (H or F at the next meeting)
        var unsent: [Entry]?
    }

    private(set) var saved: Saved
    private let url: URL

    init(url: URL = URL.applicationSupportDirectory.appending(path: "workout.json")) {
        self.url = url
        saved = (try? JSONDecoder().decode(Saved.self, from: Data(contentsOf: url))) ?? Saved()
    }

    var days: [DayItem] {
        get { saved.days }
        set { saved.days = newValue; save() }
    }

    var history: [HistoryEntry] { saved.history }
    var lastSync: Date? { saved.lastSync }
    var heldBack: Bool { saved.heldBack }
    var watchVersion: UInt32? { saved.watchVersion }

    /// The encoded program, or why the watch would not take it.
    var encoded: Result<Data, ProgramError> {
        Result { try encodeProgram(days.map(\.day)) }.mapError { $0 as! ProgramError }
    }

    /// The watch has an older program (or none) than the phone.
    var pending: Bool {
        guard case let .success(p) = encoded else { return false }
        return saved.watchVersion != programVersion(p)
    }

    func apply(_ r: SyncResult) {
        saved.history = merge(saved.history, r, pushed: saved.pushed, exercises: saved.pushedExercises ?? [:]).filter { !(saved.deleted ?? []).contains($0.start) }
        saved.watchVersion = r.pushed ?? r.watchVersion
        saved.heldBack = r.heldBack
        saved.lastSync = .now
        save()
    }

    var open: OpenWorkout? { saved.open }
    var unsent: [Entry] { saved.unsent ?? [] }

    /// The day the rotation offers: the one after the newest workout's (by name), else the first.
    var nextDay: Int {
        guard let last = history.first, let i = days.firstIndex(where: { $0.name == last.dayName }) else { return 0 }
        return (i + 1) % max(days.count, 1)
    }

    func planned(_ ex: Int) -> Int {
        guard let o = open, o.day < days.count, ex < days[o.day].exercises.count else { return 0 }
        return days[o.day].exercises[ex].exercise.sets
    }

    func startWorkout(day: Int) {
        guard case let .success(p) = encoded, day < days.count else { return }
        saved.open = OpenWorkout(start: watchSeconds(), day: day, version: programVersion(p))
        save()
    }

    /// The watch's workout (or the merged one) replaces the phone's view of it.
    func setOpen(_ w: OpenWorkout?) {
        saved.open = w
        save()
    }

    /// The next set of `ex` done now; nil when it has none left.
    func doneSet(_ ex: Int) -> SetMark? {
        guard var o = saved.open, o.done(ex) < planned(ex) else { return nil }
        let m = SetMark(ex: ex, t: max(0, Int(watchSeconds()) - Int(o.start)))
        o.union([m], planned: planned)
        saved.open = o
        save()
        return m
    }

    /// Ends the open workout: with sets it goes to the history (and, unless `told`, to the watch later with H).
    /// Returns its entry.
    @discardableResult
    func finishWorkout(told: Bool = false, end: UInt32 = watchSeconds()) -> Entry? {
        guard let o = saved.open else { return nil }
        saved.open = nil
        defer { save() }
        guard !o.sets.isEmpty else { return nil }
        let day = o.day < days.count ? days[o.day] : nil
        let e = Entry(start: o.start, durS: Int(end &- o.start), day: o.day, done: o.sets.count,
                      planned: day?.exercises.map(\.exercise.sets).reduce(0, +) ?? o.sets.count)
        if !history.contains(where: { $0.start == o.start }) {
            var h = HistoryEntry(start: e.start, durS: e.durS, dayName: day?.name ?? "День \(o.day + 1)", done: e.done,
                                 planned: e.planned)
            h.exercises = day?.exercises.map(\.exercise.name)
            h.trace = Trace(sets: o.sets, hr: [])
            saved.history = ([h] + saved.history).sorted { $0.start > $1.start }
        }
        if !told { saved.unsent = unsent + [e] }
        return e
    }

    /// When the rest after the newest set ends (nil: no set yet).
    var restEnd: Date? {
        guard let o = open, let last = o.sets.last, o.day < days.count, last.ex < days[o.day].exercises.count else { return nil }
        let rest = days[o.day].exercises[last.ex].exercise.restS
        return Entry(start: o.start &+ UInt32(last.t + rest), durS: 0, day: 0, done: 0, planned: 0).date
    }

    func markSent(_ start: UInt32) {
        saved.unsent = unsent.filter { $0.start != start }
        save()
    }

    /// Entries a sync must not fetch again: kept ones and deleted ones.
    var knownStarts: Set<UInt32> { Set(history.map(\.start)).union(saved.deleted ?? []) }
    /// Entries whose trace a sync need not fetch.
    var tracedStarts: Set<UInt32> {
        // a phone-made trace has no heart rate: the watch's, if it shared the workout, is better
        Set(history.filter { $0.trace.map { !$0.hr.isEmpty } ?? false }.map(\.start)).union(saved.deleted ?? [])
    }

    func deleteHistory(_ starts: [UInt32]) {
        saved.deleted = (saved.deleted ?? []).union(starts)
        saved.history.removeAll { starts.contains($0.start) }
        save()
    }

    /// Before a push: remember the day names of this version (history names its entries from them).
    func willPush(_ program: Data) {
        saved.pushed[programVersion(program)] = days.map(\.name)
        saved.pushedExercises = (saved.pushedExercises ?? [:])
            .merging([programVersion(program): days.map { $0.exercises.map(\.exercise.name) }]) { $1 }
        save()
    }

    func replaceProgram(_ imported: [Day]) {
        days = imported.map { d in DayItem(name: d.name, exercises: d.exercises.map { ExerciseItem(exercise: $0) }) }
    }

    #if DEBUG
    /// Made-up program and history for screenshots: launch with `-demo`.
    func loadDemo() {
        let ex = { (n: String, s: Int, a: Int, b: Int, w: String, r: Int) in
            ExerciseItem(exercise: Exercise(name: n, sets: s, repsMin: a, repsMax: b, weight: w, restS: r))
        }
        saved.days = [
            DayItem(name: "Жим и присед", exercises: [ex("Жим лёжа", 4, 5, 8, "60 кг", 120), ex("Присед", 4, 5, 5, "80 кг", 150),
                                                     ex("Скручивания", 3, 20, 20, "свой вес", 60)]),
            DayItem(name: "Тяга", exercises: [ex("Становая тяга", 3, 5, 5, "90 кг", 180), ex("Подтягивания", 3, 6, 10, "", 90)]),
            DayItem(name: "Ноги", exercises: [ex("Выпады", 3, 10, 12, "2×10 кг", 90)]),
        ]
        let base = UInt32(Date.now.timeIntervalSince1970 - 946_684_800 + Double(TimeZone.current.secondsFromGMT()))
        saved.history = (0..<9).map { i in
            HistoryEntry(start: base - UInt32(i) * 2 * 86_400 - 3600, durS: 3300 + i * 97,
                         dayName: saved.days[i % 3].name, done: 11 - i % 2, planned: 11)
        }
        saved.lastSync = .now.addingTimeInterval(-300)
        save()
    }
    #endif

    private func save() {
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? JSONEncoder().encode(saved).write(to: url, options: .atomic)
    }
}

extension ProgramError {
    var text: String {
        switch self {
        case let .days(n): n == 0 ? String(localized: "Добавьте хотя бы один день") : String(localized: "Дней \(n), часы берут до \(daysMax)")
        case let .exercises(day, n): n == 0 ? String(localized: "В дне «\(day)» нет упражнений") :
            String(localized: "В дне «\(day)» \(n) упражнений, часы берут до \(exercisesMax)")
        case let .range(name): String(localized: "«\(name)»: проверьте подходы и повторы")
        case let .size(n): String(localized: "Программа \(n) Б, часы берут \(programMax) Б: сократите названия")
        }
    }
}
