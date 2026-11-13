import Foundation
import Testing
@testable import WireKit

/// Byte-for-byte equal to the Python host (tests/ios_vectors.py).
struct Vectors: Decodable {
    struct Seal: Decodable { let key: String; let plain: String; let cipher: String }
    struct Key: Decodable { let secret: String; let key: String }
    struct FrameV: Decodable { let c2: UInt16; let payload: String; let key: String; let hex: String }
    struct EntryV: Decodable {
        let hex: String; let index: Int; let count: Int; let start: UInt32; let durS: Int; let day: Int
        let done: Int; let planned: Int
    }
    struct Req: Decodable { let hex: String; let version: UInt32; let running: Bool }
    let days: [Day]
    let program_hex: String
    let version: UInt32
    let messages_hex: [String]
    let query_hex: String
    let seal: Seal
    let channel_key: Key
    let frame: FrameV
    let entry: EntryV
    let empty_history_hex: String
    let requests: [Req]
    let ack_hex: String
    let dry_run_json: String

    static let shared: Vectors = {
        let url = Bundle.module.url(forResource: "vectors", withExtension: "json")!
        return try! JSONDecoder().decode(Vectors.self, from: Data(contentsOf: url))
    }()
}

func hex(_ s: String) -> Data {
    var d = Data()
    var i = s.startIndex
    while i < s.endIndex {
        let j = s.index(i, offsetBy: 2)
        d.append(UInt8(s[i..<j], radix: 16)!)
        i = j
    }
    return d
}

let v = Vectors.shared

@Test func encodeMatchesPython() throws {
    #expect(try encodeProgram(v.days) == hex(v.program_hex))
    #expect(programVersion(hex(v.program_hex)) == v.version)
}

@Test func messagesMatchPython() {
    #expect(programMessages(hex(v.program_hex)) == v.messages_hex.map(hex))
    #expect(historyQuery() == hex(v.query_hex))
}

@Test func sealOpen() {
    let key = hex(v.seal.key)
    #expect(seal(key, hex(v.seal.plain)) == hex(v.seal.cipher))
    #expect(open(key, hex(v.seal.cipher)) == hex(v.seal.plain))
    var bad = hex(v.seal.cipher)
    bad[0] ^= 1
    #expect(open(key, bad) == nil)
    #expect(open(Data(repeating: 1, count: 16), hex(v.seal.cipher)) == nil)
}

@Test func channelKeyFromSecret() {
    #expect(channelKey(secretHex: v.channel_key.secret) == hex(v.channel_key.key))
    #expect(channelKey(secretHex: "zz") == nil)
}

@Test func frameMatchesPython() {
    let f = frame(c2: v.frame.c2, payload: hex(v.frame.payload), key: hex(v.frame.key))
    #expect(f == hex(v.frame.hex))
    var p = FrameParser()
    let raw = hex(v.frame.hex)
    #expect(p.feed(raw.prefix(5)).isEmpty)
    let got = p.feed(raw.dropFirst(5))
    #expect(got.count == 1)
    #expect(got.first?.c1 == 0xFFFF && got.first?.c2 == v.frame.c2)
    #expect(got.first.flatMap { open(hex(v.frame.key), $0.body) } == hex(v.frame.payload))
}

@Test func parseEntry() {
    let e = v.entry
    #expect(parseIncoming(hex(e.hex)) == .entry(index: e.index, count: e.count, Entry(
        start: e.start, durS: e.durS, day: e.day, done: e.done, planned: e.planned)))
    #expect(parseIncoming(hex(v.empty_history_hex)) == .entry(index: 0, count: 0, nil))
    #expect(parseIncoming(Data("h1Y".utf8)) == nil)
}

@Test func parseRequest7And8() {
    for r in v.requests {
        #expect(parseIncoming(hex(r.hex)) == .request(version: r.version, running: r.running))
    }
    #expect(parseIncoming(hex(v.ack_hex)) == .ack(0x0A0B_0C0D))
}

@Test func importDryRunMatchesPython() throws {
    let days = try importDryRun(Data(v.dry_run_json.utf8))
    #expect(days == v.days)
}

@Test func limits() {
    let ex = Exercise(name: "Жим", sets: 3, repsMin: 5, repsMax: 8, weight: "", restS: 90)
    let day = Day(name: "Д", exercises: [ex])
    #expect(throws: ProgramError.days(0)) { try encodeProgram([]) }
    #expect(throws: ProgramError.days(8)) { try encodeProgram(Array(repeating: day, count: 8)) }
    #expect(throws: ProgramError.exercises(day: "Д", 13)) {
        try encodeProgram([Day(name: "Д", exercises: Array(repeating: ex, count: 13))])
    }
    var bad = ex
    bad.repsMin = 9
    #expect(throws: ProgramError.range("Жим")) { try encodeProgram([Day(name: "Д", exercises: [bad])]) }
    let long = String(repeating: "Ж", count: 16)
    let big = Day(name: long, exercises: Array(repeating: Exercise(name: long, sets: 1, repsMin: 1, repsMax: 1,
                                                                   weight: long, restS: 1), count: 12))
    #expect(throws: ProgramError.size(3545)) { try encodeProgram(Array(repeating: big, count: 4)) }
}

@Test func clipCutsOnCharBoundary() {
    #expect(clip("Ёж", 32) == Data("Еж".utf8))
    #expect(clip(String(repeating: "ж", count: 20), 33).count == 32)
    #expect(clip("abc", 2) == Data("ab".utf8))
}
