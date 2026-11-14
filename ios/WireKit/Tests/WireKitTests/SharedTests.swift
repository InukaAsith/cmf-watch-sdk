import Foundation
import Testing
@testable import WireKit

@Test func watchSecondsRoundTrip() {
    let s = watchSeconds()
    #expect(watchSeconds(Entry(start: s, durS: 0, day: 0, done: 0, planned: 0).date) == s)
}

@Test func encodesDFH() {
    let w = OpenWorkout(start: 0x0A0B_0C0D, day: 1, version: 0x0102_0304)
    #expect(setMessage(w, SetMark(ex: 2, t: 30)) == Data("w1D".utf8) + Data([0, 0x0D, 0x0C, 0x0B, 0x0A, 1, 4, 3, 2, 1, 2, 30, 0]))
    #expect(setMessage(w, nil).suffix(3) == Data([0xFF, 0, 0]))
    #expect(finishMessage(start: 0x0A0B_0C0D, end: 1) == Data("w1F".utf8) + Data([0, 0x0D, 0x0C, 0x0B, 0x0A, 1, 0, 0, 0]))
    #expect(entryMessage(Entry(start: 0x0A0B_0C0D, durS: 0x0102, day: 2, done: 7, planned: 9))
            == Data("w1H".utf8) + Data([0, 0x0D, 0x0C, 0x0B, 0x0A, 2, 1, 2, 7, 9]))
}

@Test func unionDedupesAndCaps() {
    var w = OpenWorkout(start: 1, day: 0, version: 1, sets: [SetMark(ex: 0, t: 10)])
    let added = w.union([SetMark(ex: 0, t: 10), SetMark(ex: 1, t: 5), SetMark(ex: 0, t: 20), SetMark(ex: 0, t: 30)]) { _ in 2 }
    #expect(added == [SetMark(ex: 1, t: 5), SetMark(ex: 0, t: 20)])
    #expect(w.sets.map(\.t) == [5, 10, 20])
}

@Test func meetings() {
    let p = OpenWorkout(start: 100, day: 0, version: 7, sets: [SetMark(ex: 0, t: 1), SetMark(ex: 0, t: 2)])
    #expect(meet(phone: nil, watch: nil, watchVersion: 7) == .nothing)
    #expect(meet(phone: p, watch: nil, watchVersion: 7) == .pushOwn)
    #expect(meet(phone: p, watch: nil, watchVersion: 8) == .independent)
    #expect(meet(phone: nil, watch: WatchWorkout(start: 5, day: 0, done: 0), watchVersion: 7) == .adoptWatch)
    #expect(meet(phone: p, watch: WatchWorkout(start: 100, day: 0, done: 9), watchVersion: 7) == .exchange)
    #expect(meet(phone: p, watch: WatchWorkout(start: 200, day: 0, done: 1), watchVersion: 7) == .replaceWatch)
    #expect(meet(phone: p, watch: WatchWorkout(start: 200, day: 0, done: 2), watchVersion: 7) == .replaceWatch, "tie: earlier")
    #expect(meet(phone: p, watch: WatchWorkout(start: 50, day: 0, done: 2), watchVersion: 7) == .yieldToWatch)
    #expect(meet(phone: p, watch: WatchWorkout(start: 200, day: 0, done: 0), watchVersion: 8) == .independent)
}

@Test func undoKeepsItOut() {
    var w = OpenWorkout(start: 0x0A0B_0C0D, day: 0, version: 1, sets: [SetMark(ex: 2, t: 30)])
    #expect(w.undoLast() == SetMark(ex: 2, t: 30))
    #expect(w.union([SetMark(ex: 2, t: 30)]) { _ in 3 }.isEmpty, "the watch's copy does not bring it back")
    #expect(unsetMessage(w, SetMark(ex: 2, t: 30)) == Data("w1U".utf8) + Data([0, 0x0D, 0x0C, 0x0B, 0x0A, 2, 30, 0]))
}

@Test func zonesByAgeAndReserve() {
    let z = HeartZones(age: 30)
    #expect(z.max == 187)
    #expect(z.lows == [94, 112, 131, 150, 168])
    #expect([z.zone(0), z.zone(80), z.zone(94), z.zone(140), z.zone(200)] as [Int] == [0, 0, 1, 3, 5])
    #expect(HeartZones(age: 30, resting: 60).lows == [124, 136, 149, 162, 174])
    #expect(zonesMessage(z) == Data("w1Z".utf8) + Data([0, 94, 112, 131, 150, 168]))
}
