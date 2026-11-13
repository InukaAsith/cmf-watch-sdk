import SwiftUI
import UniformTypeIdentifiers
import WireKit

struct ProgramView: View {
    @Environment(AppStore.self) private var store
    @State private var importing = false
    @State private var importError: String?

    var body: some View {
        @Bindable var store = store
        NavigationStack {
            List {
                if case let .failure(e) = store.encoded, !store.days.isEmpty {
                    Section { Label(e.text, systemImage: "exclamationmark.triangle").foregroundStyle(.orange) }
                }
                Section("Дни") {
                    ForEach($store.days) { $day in
                        NavigationLink(value: day.id) {
                            VStack(alignment: .leading) {
                                Text(day.name.isEmpty ? "Без названия" : day.name)
                                Text(plural(day.exercises.count, "упражнение", "упражнения", "упражнений")).font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    }
                    .onDelete { store.days.remove(atOffsets: $0) }
                    .onMove { store.days.move(fromOffsets: $0, toOffset: $1) }
                    if store.days.count < daysMax {
                        Button("Добавить день", systemImage: "plus") {
                            store.days.append(DayItem(name: "День \(store.days.count + 1)", exercises: []))
                        }
                    }
                }
            }
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
                        VStack(alignment: .leading) {
                            Text(ex.exercise.name.isEmpty ? "Без названия" : ex.exercise.name)
                            Text(ex.summary).font(.caption).foregroundStyle(.secondary)
                        }
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
        .navigationTitle(exercise.name.isEmpty ? "Упражнение" : exercise.name)
    }
}

/// A text field that shows the bytes left before the watch cuts the text (32 B UTF-8).
struct LimitedField: View {
    let title: String
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
