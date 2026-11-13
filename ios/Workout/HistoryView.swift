import SwiftUI
import WireKit

struct HistoryView: View {
    @Environment(AppStore.self) private var store

    var body: some View {
        NavigationStack {
            let months = Dictionary(grouping: store.history) {
                Calendar.current.dateInterval(of: .month, for: $0.date)!.start
            }
            List {
                ForEach(months.keys.sorted(by: >), id: \.self) { month in
                    let entries = months[month]!
                    Section("\(month.formatted(.dateTime.month(.wide).year())) · "
                            + plural(entries.count, "тренировка", "тренировки", "тренировок")) {
                        ForEach(entries) { e in
                            VStack(alignment: .leading) {
                                Text(e.dayName)
                                Text("\(e.date.formatted(date: .abbreviated, time: .shortened)) · "
                                     + "\(String(format: "%d:%02d", e.durS / 3600, e.durS % 3600 / 60)) · подходов \(e.done) из \(e.planned)")
                                    .font(.caption).foregroundStyle(.secondary)
                            }
                            .accessibilityElement(children: .combine)
                        }
                        .onDelete { store.deleteHistory($0.map { entries[$0].start }) }
                    }
                }
            }
            .overlay {
                if store.history.isEmpty {
                    ContentUnavailableView("Тренировок пока нет", systemImage: "figure.strengthtraining.traditional",
                                           description: Text("Они появятся после синхронизации с часами."))
                }
            }
            .navigationTitle("История")
        }
    }
}
