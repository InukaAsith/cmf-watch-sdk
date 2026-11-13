import Foundation
import Testing
@testable import WireKit

/// The stub's install commands, scripted: `used` slots answer INFO with a header, writes are recorded.
final class FakeInstaller: Requester, @unchecked Sendable {
    var used: [Int: Data] = [:]
    var calls: [(UInt16, Data)] = []
    var beginStatus: Status = .ok
    var written = 0

    func call(_ c2: UInt16, _ payload: Data) async throws -> Data {
        calls.append((c2, payload))
        switch c2 {
        case Cmd.info: return Data([0]) + (used[Int(payload[payload.startIndex])] ?? Data())
        case Cmd.begin: return Data([beginStatus.rawValue])
        case Cmd.data:
            written = Int(payload.le(UInt32.self, at: 0)) + payload.count - 4
            var r = Data([0])
            r.le(UInt32(written))
            return r
        default: return Data([0])
        }
    }
}

let pkg = v.package
let header = hex(pkg.info_reply_hex).dropFirst()

@Test func headerMatchesPython() {
    #expect(parseHeader(Data(header)) == PackageHeader(kind: pkg.kind, name: pkg.name, version: pkg.version))
    #expect(parseHeader(hex(pkg.hex)) == PackageHeader(kind: pkg.kind, name: pkg.name, version: pkg.version))
    #expect(parseHeader(Data(count: 64)) == nil)
}

@Test func payloadsMatchPython() async throws {
    let f = FakeInstaller()
    f.used = [0: Data(header), 1: Data(header), 2: Data(header)]
    f.used[0]![8] = UInt8(ascii: "x") // another app in slot 0
    f.used[1]![8] = UInt8(ascii: "y")
    f.used[2]![8] = UInt8(ascii: "z")
    nonisolated(unsafe) var seen: [Double] = []
    let slot = try await install(f, package: hex(pkg.hex)) { seen.append($0) }
    #expect(slot == 3)
    let writes = f.calls.filter { $0.0 != Cmd.info }
    #expect(writes.map(\.1) == [hex(pkg.begin_hex)] + pkg.data_hex.map(hex) + [hex(pkg.end_hex)])
    #expect(seen.last == 1)
}

@Test func picksSameNameSlot() async throws {
    let f = FakeInstaller()
    f.used = [5: Data(header)]
    #expect(try await install(f, package: hex(pkg.hex)) { _ in } == 5)
}

@Test func installBusy() async throws {
    let f = FakeInstaller()
    f.beginStatus = .busy
    await #expect(throws: InstallError.busy) { try await install(f, package: hex(pkg.hex)) { _ in } }
    #expect(!f.calls.contains { $0.0 == Cmd.data }, "nothing half-written")
}

@Test func noFreeSlot() async throws {
    let f = FakeInstaller()
    for i in 0..<8 {
        var h = Data(header)
        h[8] = UInt8(ascii: "a") + UInt8(i)
        f.used[i] = h
    }
    await #expect(throws: InstallError.noFreeSlot) { try await install(f, package: hex(pkg.hex)) { _ in } }
}
