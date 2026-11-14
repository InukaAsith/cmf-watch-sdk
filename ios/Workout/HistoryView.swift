import Charts
import SwiftUI
import WireKit

struct HistoryView: View {
    @Environment(AppStore.self) private var store

    var body: some View {
        NavigationStack {
            ScrollView {
                if store.history.isEmpty {
                    ContentUnavailableView("Тренировок пока нет", systemImage: "figure.strengthtraining.traditional",
                                           description: Text("Они появятся после синхронизации с часами."))
                        .padding(.top, 120)
                } else {
                    VStack(spacing: 12) {
                        WeekStrip(history: store.history)
                        WeeksChart(history: store.history)
                        Tiles(history: store.history)
                        Entries()
                    }
                    .padding(.horizontal)
                    .padding(.bottom)
                }
            }
            .background(Color(.systemGroupedBackground))
            .navigationTitle("История")
        }
    }
}

/// This week, Monday to Sunday: a filled check for each day with a workout.
private struct WeekStrip: View {
    let history: [HistoryEntry]

    var body: some View {
        let cal = weekCalendar
        let today = cal.startOfDay(for: .now)
        let monday = cal.dateInterval(of: .weekOfYear, for: today)!.start
        let trained = Set(history.map { cal.startOfDay(for: $0.date) })
        HStack {
            ForEach(0..<7, id: \.self) { i in
                let day = cal.date(byAdding: .day, value: i, to: monday)!
                VStack(spacing: 8) {
                    Text(day.formatted(.dateTime.weekday(.abbreviated)).capitalized)
                        .font(.footnote.weight(day == today ? .bold : .regular))
                        .foregroundStyle(day == today ? .primary : .secondary)
                    ZStack {
                        if trained.contains(day) {
                            Circle().fill(Color.accentColor)
                            Image(systemName: "checkmark").font(.footnote.bold())
                                .foregroundStyle(Color(.secondarySystemGroupedBackground))
                        } else {
                            Circle().strokeBorder(day == today ? Color.accentColor : .clear, lineWidth: 1.5)
                            Text(day.formatted(.dateTime.day())).font(.subheadline).monospacedDigit()
                                .foregroundStyle(day > today ? .tertiary : .secondary)
                        }
                    }
                    .frame(width: 34, height: 34)
                }
                .frame(maxWidth: .infinity)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(day.formatted(.dateTime.weekday(.wide).day().month())
                                    + (trained.contains(day) ? ", тренировка" : ""))
            }
        }
        .card(padding: 12)
    }
}

/// Workouts per week over the last eight weeks; the current week is solid.
private struct WeeksChart: View {
    let history: [HistoryEntry]

    var body: some View {
        let cal = weekCalendar
        let now = cal.dateInterval(of: .weekOfYear, for: .now)!.start
        let weeks = (0..<8).reversed().map { cal.date(byAdding: .weekOfYear, value: -$0, to: now)! }
        let counts = Dictionary(grouping: history) { cal.dateInterval(of: .weekOfYear, for: $0.date)!.start }
            .mapValues(\.count)
        VStack(alignment: .leading, spacing: 16) {
            HStack(alignment: .firstTextBaseline) {
                Label("По неделям", systemImage: "chart.bar.fill").font(.headline)
                Spacer()
                Metric(value: "\(counts[now] ?? 0)", unit: "на этой неделе").fixedSize()
            }
            // categorical x: a date axis puts the labels off the bars
            Chart(Array(weeks.enumerated()), id: \.offset) { i, w in
                BarMark(x: .value("Неделя", "\(i)"),
                        y: .value("Тренировки", counts[w] ?? 0), width: .ratio(0.7))
                    .clipShape(.rect(cornerRadius: 8, style: .continuous))
                    .foregroundStyle(w == now ? Color.accentColor : Color.accentColor.opacity(0.2))
            }
            .chartXAxis {
                AxisMarks { v in
                    AxisValueLabel { Text(v.as(String.self).flatMap(Int.init).map { weeks[$0].formatted(.dateTime.day()) } ?? "") }
                }
            }
            .chartYAxis(.hidden)
            .frame(height: 140)
        }
        .card()
    }
}

/// Four numbers: this month, average length, sets done in the last 30 days, weeks in a row.
private struct Tiles: View {
    let history: [HistoryEntry]

    var body: some View {
        let cal = weekCalendar
        let month = history.filter { cal.isDate($0.date, equalTo: .now, toGranularity: .month) }.count
        let avgMin = history.map(\.durS).reduce(0, +) / max(history.count, 1) / 60
        let recent = history.filter { $0.date > .now.addingTimeInterval(-30 * 86_400) }
        let planned = recent.map(\.planned).reduce(0, +)
        let done = Double(recent.map(\.done).reduce(0, +)) / Double(max(planned, 1))
        let streak = weekStreak(history)
        LazyVGrid(columns: [GridItem(.flexible(), spacing: 12), GridItem(.flexible())], spacing: 12) {
            Tile(title: "Тренировки", icon: "calendar", value: "\(month)",
                 unit: "в этом месяце")
            Tile(title: "Время", icon: "timer", value: "\(avgMin)", unit: "мин в среднем")
            Tile(title: "Подходы", icon: "checklist", value: "\(Int(done * 100))%", unit: "за 30 дней") {
                Ring(value: done).frame(width: 44, height: 44)
            }
            Tile(title: "Серия", icon: "flame.fill", value: "\(streak)",
                 unit: "нед. подряд")
        }
    }
}

/// Weeks in a row with at least one workout, counting back from this week (or the last one, if this week is still empty).
func weekStreak(_ history: [HistoryEntry], now: Date = .now) -> Int {
    let cal = weekCalendar
    let weeks = Set(history.map { cal.dateInterval(of: .weekOfYear, for: $0.date)!.start })
    var w = cal.dateInterval(of: .weekOfYear, for: now)!.start
    if !weeks.contains(w) { w = cal.date(byAdding: .weekOfYear, value: -1, to: w)! }
    var n = 0
    while weeks.contains(w) { n += 1; w = cal.date(byAdding: .weekOfYear, value: -1, to: w)! }
    return n
}

private struct Tile<Accessory: View>: View {
    let title: LocalizedStringKey
    let icon: String
    let value: String
    let unit: LocalizedStringKey
    @ViewBuilder var accessory: Accessory

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label(title, systemImage: icon).font(.subheadline.weight(.medium))
            HStack(alignment: .bottom) {
                Metric(value: value, unit: unit)
                Spacer(minLength: 0)
                accessory
            }
        }
        .card()
        .accessibilityElement(children: .combine)
    }
}

extension Tile where Accessory == EmptyView {
    init(title: LocalizedStringKey, icon: String, value: String, unit: LocalizedStringKey) {
        self.init(title: title, icon: icon, value: value, unit: unit) { EmptyView() }
    }
}

/// Every workout, newest first, grouped by month.
private struct Entries: View {
    @Environment(AppStore.self) private var store

    var body: some View {
        let months = Dictionary(grouping: store.history) { Calendar.current.dateInterval(of: .month, for: $0.date)!.start }
        ForEach(months.keys.sorted(by: >), id: \.self) { month in
            let entries = months[month]!.sorted { $0.date > $1.date }
            VStack(alignment: .leading, spacing: 8) {
                Text(month.formatted(.dateTime.month(.wide).year()).capitalized)
                    .font(.title3.bold()).padding(.top, 12).padding(.leading, 4)
                ForEach(entries) { e in
                    NavigationLink { HistoryDetail(entry: e) } label: { EntryRow(entry: e) }
                        .buttonStyle(.plain)
                        .contextMenu {
                            Button("Удалить", systemImage: "trash", role: .destructive) { store.deleteHistory([e.start]) }
                        }
                }
            }
        }
    }
}

private struct EntryRow: View {
    let entry: HistoryEntry

    var body: some View {
        let e = entry
        HStack(spacing: 14) {
            VStack(spacing: 0) {
                Text(e.date.formatted(.dateTime.day())).font(.system(.title3, design: .rounded, weight: .bold))
                Text(e.date.formatted(.dateTime.month(.abbreviated))).font(.caption2).foregroundStyle(.secondary)
            }
            .frame(width: 48, height: 48)
            .background(Color(.tertiarySystemGroupedBackground), in: .rect(cornerRadius: 14, style: .continuous))
            VStack(alignment: .leading, spacing: 2) {
                Text(e.dayName).font(.body.weight(.semibold))
                Text("\(e.date.formatted(date: .omitted, time: .shortened)) · "
                     + String(format: "%d:%02d", e.durS / 3600, e.durS % 3600 / 60))
                    .font(.subheadline).foregroundStyle(.secondary).monospacedDigit()
            }
            Spacer()
            ZStack {
                Ring(value: Double(e.done) / Double(max(e.planned, 1)), width: 4)
                Text("\(e.done)").font(.caption.bold()).monospacedDigit()
            }
            .frame(width: 40, height: 40)
        }
        .card(padding: 12)
        .accessibilityElement(children: .combine)
        .accessibilityValue("подходов \(e.done) из \(e.planned)")
    }
}
