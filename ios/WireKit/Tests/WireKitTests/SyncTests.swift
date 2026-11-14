import Foundation
import Testing
@testable import WireKit

/// The watch app's side of w1, scripted: answers Q with `history`, B…E with A, records what it got.
final class FakeWatch: Channel, @unchecked Sendable {
    let inbox = Inbox<Data>()
    var sent: [Data] = []
    var history: [Entry]
    var stored: UInt32
    var running: Bool
    var ackPushes = true
    var trace: [Data] = []

    init(stored: UInt32 = 0, running: Bool = false, history: [Entry] = []) {
        (self.stored, self.running, self.history) = (stored, running, history)
    }

    func ask() async {
        var r = Data("w1R".utf8)
        r.le(stored)
        r.append(running ? 1 : 0)
        await inbox.put(r)
    }

    func send(_ data: Data) async throws {
        sent.append(data)
        if data == historyQuery() {
            if history.isEmpty { await inbox.put(Data("w1Y".utf8) + Data([0, 0])) }
            for (i, e) in history.enumerated() {
                var y = Data("w1Y".utf8) + Data([UInt8(i), UInt8(history.count)])
                y.le(e.start)
                y.le(UInt16(e.durS))
                y += Data([UInt8(e.day), UInt8(e.done), UInt8(e.planned)])
                await inbox.put(y)
            }
        } else if data == traceQuery() {
            for x in trace { await inbox.put(x) }
        } else if data.starts(with: Data("w1E".utf8)), ackPushes {
            stored = data.le(UInt32.self, at: 4)
            var a = Data("w1A".utf8)
            a.le(stored)
            await inbox.put(a)
        }
    }

    func recv(timeout: Duration) async throws -> Data {
        guard let d = await inbox.next(timeout: timeout) else { throw WireError.timeout("no message") }
        return d
    }

    var pushed: [Data] { sent.filter { $0.starts(with: Data("w1".utf8)) && $0 != historyQuery() } }
}

let program = try! encodeProgram([Day(name: "А", exercises: [Exercise(name: "Жим", sets: 3, repsMin: 5, repsMax: 8,
                                                                       weight: "60 кг")])])
let e1 = Entry(start: 100, durS: 60, day: 0, done: 3, planned: 3)
let e2 = Entry(start: 200, durS: 70, day: 1, done: 2, planned: 3)
let e3 = Entry(start: 300, durS: 80, day: 0, done: 1, planned: 3)

@Test func pushesWhenVersionDiffers() async throws {
    let w = FakeWatch(stored: 7)
    await w.ask()
    let r = try await sync(channel: w, program: program, knownStarts: [])
    #expect(r.pushed == programVersion(program))
    #expect(w.pushed == programMessages(program))
    #expect(r.watchVersion == 7)
    #expect(!r.heldBack)
}

@Test func holdsProgramWhileWorkoutRuns() async throws {
    let w = FakeWatch(stored: 7, running: true, history: [e1])
    await w.ask()
    let r = try await sync(channel: w, program: program, knownStarts: [])
    #expect(r.pushed == nil && r.heldBack)
    #expect(w.pushed.isEmpty)
    #expect(r.newEntries == [e1], "history is still read while a workout runs")
}

@Test func skipsWhenSame() async throws {
    let w = FakeWatch(stored: programVersion(program))
    await w.ask()
    let r = try await sync(channel: w, program: program, knownStarts: [])
    #expect(r.pushed == nil && !r.heldBack && w.pushed.isEmpty)
}

@Test func mergeKeepsExistingAndSkipsDuplicates() async throws {
    let w = FakeWatch(stored: programVersion(program), history: [e3, e2, e1])
    await w.ask()
    let r = try await sync(channel: w, program: program, knownStarts: [e2.start])
    #expect(Set(r.newEntries) == [e1, e3])
}

@Test func emptyHistory() async throws {
    let w = FakeWatch(stored: programVersion(program))
    await w.ask()
    #expect(try await sync(channel: w, program: program, knownStarts: []).newEntries.isEmpty)
}

@Test func nilProgramNeverPushes() async throws {
    let w = FakeWatch(stored: 0)
    await w.ask()
    let r = try await sync(channel: w, program: nil, knownStarts: [])
    #expect(r.pushed == nil && w.pushed.isEmpty)
}

@Test func usesTheRequestThatWokeUs() async throws {
    let w = FakeWatch(stored: 7)
    let r = try await sync(channel: w, program: program, knownStarts: [],
                           firstRequest: .request(version: 7, running: false))
    #expect(r.pushed == programVersion(program))
}

@Test func missingAckTimesOut() async throws {
    let w = FakeWatch(stored: 7)
    w.ackPushes = false
    await w.ask()
    await #expect(throws: WireError.self) {
        try await sync(channel: w, program: program, knownStarts: [], ackWait: .milliseconds(200))
    }
}

@Test func fetchesTheNewestTraceOnce() async throws {
    let w = FakeWatch(history: [e3, e2])
    // trace of e3: one set of exercise 1 at 40 s, bpm 70, none, 90; sent in two parts
    let payload = Data([1, 1, 40, 0, 3, 0, 70, 0, 90])
    w.trace = [Data("w1X".utf8) + Data([0, 2]) + le32(300) + payload.prefix(5),
               Data("w1X".utf8) + Data([1, 2]) + le32(300) + payload.dropFirst(5)]
    await w.ask()
    let r = try await sync(channel: w, program: nil, knownStarts: [], tracedStarts: [])
    #expect(r.trace?.start == 300)
    #expect(r.trace?.1 == Trace(sets: [SetMark(ex: 1, t: 40)], hr: [70, 0, 90]))
    let merged = merge([], r, pushed: [:])
    #expect(merged.first { $0.start == 300 }?.trace?.hr == [70, 0, 90])
    // already traced, or a workout running: no T
    await w.ask()
    _ = try await sync(channel: w, program: nil, knownStarts: [300, 200], tracedStarts: [300])
    #expect(w.sent.filter { $0 == traceQuery() }.count == 1)
}

@Test func parsesPulse() {
    var p = Data("w1P".utf8) + le32(300)
    p += Data([1, 72, 0xFF, 5])
    #expect(parseIncoming(p) == .pulse(start: 300, day: 1, bpm: 72, ex: nil, done: 5))
    var q = Data("w1P".utf8) + le32(300)
    q += Data([1, 72, 2, 5, 1, 0x02, 0x01])
    #expect(parseIncoming(q) == .pulse(start: 300, day: 1, bpm: 72, ex: 2, done: 5, exDone: 1, restLeft: 0x0102))
    #expect(parseTrace(Data([0, 0, 0])) == Trace(sets: [], hr: []))
    #expect(parseTrace(Data([1, 0])) == nil)
}

func le32(_ v: UInt32) -> Data { var d = Data(); d.le(v); return d }
