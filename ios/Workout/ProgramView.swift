import SwiftUI
import UniformTypeIdentifiers
import WireKit

struct ProgramView: View {
    @Environment(AppStore.self) private var store
    @Environment(SyncController.self) private var sync
    @State private var importing = false
    @State private var importError: String?

    var body: some View {
        @Bindable var store = store
        NavigationStack {
            List {
                if let live = sync.live { ActiveSection(live: live) }
                if case let .failure(e) = store.encoded, !store.days.isEmpty {
                    Section { Label(e.text, systemImage: "exclamationmark.triangle").foregroundStyle(.orange) }
                }
                Section("Дни") {
                    ForEach(Array(store.days.enumerated()), id: \.element.id) { i, day in
                        NavigationLink(value: day.id) { DayRow(index: i + 1, day: day) }
                    }
                    .onDelete { store.days.remove(atOffsets: $0) }
                    .onMove { store.days.move(fromOffsets: $0, toOffset: $1) }
                    if store.days.count < daysMax {
                        Button("Добавить день", systemImage: "plus") {
                            store.days.append(DayItem(name: String(localized: "День \(store.days.count + 1)"), exercises: []))
                        }
                    }
                }
            }
            .listRowSpacing(10)
            .navigationTitle("Программа")
            .navigationDestination(for: UUID.self) { id in
                if let i = store.days.firstIndex(where: { $0.id == id }) { DayView(day: $store.days[i]) }
            }
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Импорт", systemImage: "square.and.arrow.down") { importing = true }
                }
                ToolbarItem(placement: .topBarTrailing) { EditButton() }
            }
            .fileImporter(isPresented: $importing, allowedContentTypes: [.json]) { result in
                do {
                    let url = try result.get()
                    let ok = url.startAccessingSecurityScopedResource()
                    defer { if ok { url.stopAccessingSecurityScopedResource() } }
                    store.replaceProgram(try importDryRun(Data(contentsOf: url)))
                } catch {
                    importError = "Не удалось прочитать файл: \(error.localizedDescription)"
                }
            }
            .alert("Импорт", isPresented: .constant(importError != nil)) {
                Button("OK") { importError = nil }
            } message: {
                Text(importError ?? "")
            }
        }
    }
}

/// A day of the program: its number in a filled badge, the name and the exercises in it.
private struct DayRow: View {
    let index: Int
    let day: DayItem

    var body: some View {
        HStack(spacing: 14) {
            Text("\(index)")
                .font(.system(.title3, design: .rounded, weight: .bold))
                .foregroundStyle(Color(.secondarySystemGroupedBackground))
                .frame(width: 44, height: 44)
                .background(Color.accentColor, in: .rect(cornerRadius: 14, style: .continuous))
            VStack(alignment: .leading, spacing: 2) {
                Text(day.name.isEmpty ? String(localized: "Без названия") : day.name).font(.body.weight(.semibold))
                Text(day.exercises.isEmpty ? String(localized: "Нет упражнений")
                     : day.exercises.map(\.exercise.name).joined(separator: " · "))
                    .font(.subheadline).foregroundStyle(.secondary).lineLimit(2)
            }
        }
        .padding(.vertical, 4)
    }
}

struct DayView: View {
    @Binding var day: DayItem

    var body: some View {
        List {
            Section("Название") {
                LimitedField(title: "Название дня", text: $day.name)
            }
            Section("Упражнения") {
                ForEach($day.exercises) { $ex in
                    NavigationLink {
                        ExerciseView(exercise: $ex.exercise)
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(ex.exercise.name.isEmpty ? String(localized: "Без названия") : ex.exercise.name).font(.body.weight(.semibold))
                            Text(ex.summary).font(.subheadline).foregroundStyle(.secondary).monospacedDigit()
                        }
                        .padding(.vertical, 4)
                    }
                }
                .onDelete { day.exercises.remove(atOffsets: $0) }
                .onMove { day.exercises.move(fromOffsets: $0, toOffset: $1) }
                if day.exercises.count < exercisesMax {
                    Button("Упражнение", systemImage: "plus") {
                        day.exercises.append(ExerciseItem(exercise: Exercise(name: "", sets: 3, repsMin: 8, repsMax: 12,
                                                                             weight: "")))
                    }
                }
            }
        }
        .listRowSpacing(10)
        .navigationTitle(day.name)
        .toolbar { EditButton() }
    }
}

struct ExerciseView: View {
    @Binding var exercise: Exercise

    var body: some View {
        Form {
            LimitedField(title: "Название", text: $exercise.name)
            Stepper("Подходы: \(exercise.sets)", value: $exercise.sets, in: 1...20)
            Stepper("Повторы от: \(exercise.repsMin)", value: Binding(
                get: { exercise.repsMin },
                set: { exercise.repsMin = $0; exercise.repsMax = max(exercise.repsMax, $0) }), in: 1...100)
            Stepper("Повторы до: \(exercise.repsMax)", value: Binding(
                get: { exercise.repsMax },
                set: { exercise.repsMax = $0; exercise.repsMin = min(exercise.repsMin, $0) }), in: 1...100)
            Section {
                LimitedField(title: "Вес", text: $exercise.weight)
            } footer: {
                Text("Свободный текст: «90 кг», «свой вес», «50–60 кг».")
            }
            Stepper("Отдых: \(restText(exercise.restS))", value: $exercise.restS, in: 0...600, step: 15)
        }
        .navigationTitle(exercise.name.isEmpty ? String(localized: "Упражнение") : exercise.name)
    }
}

/// A text field that shows the bytes left before the watch cuts the text (32 B UTF-8).
struct LimitedField: View {
    let title: LocalizedStringKey
    @Binding var text: String

    var body: some View {
        let left = bytesLeft(text)
        HStack {
            TextField(title, text: $text)
            if left < 10 {
                Text("\(left)").monospacedDigit().foregroundStyle(left < 0 ? .red : .secondary)
                    .accessibilityLabel(left < 0 ? "Не влезет на часы" : "Осталось \(left) байт")
            }
        }
    }
}
