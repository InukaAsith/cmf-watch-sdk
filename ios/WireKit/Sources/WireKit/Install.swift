import Foundation

// Installing an app package from the phone over K (src/wp2apps/install.py): INFO, BEGIN, DATA…, END.

public let slots = 8
public let loaderSlot = 0xFF
let headerSize = 64, dataMax = 128

/// What install needs from a session: one request, the plain reply. KSession in the app, a fake in the tests.
public protocol Requester: Sendable {
    func call(_ c2: UInt16, _ payload: Data) async throws -> Data
}

extension KSession: Requester {}

public struct PackageHeader: Equatable, Sendable {
    public var kind: Int
    public var name: String
    public var version: UInt32
}

public enum InstallError: Error, Equatable {
    case busy, noFreeSlot, notAPackage
    case refused(UInt8)
}

/// The package header (`package.HEADER`: magic "WP2A", abi, kind, abi_minor, name[24], version, …).
public func parseHeader(_ d: Data) -> PackageHeader? {
    guard d.count >= headerSize, d.prefix(4) == Data("WP2A".utf8) else { return nil }
    let name = d[d.startIndex + 8..<d.startIndex + 32].prefix { $0 != 0 }
    return PackageHeader(kind: Int(d[d.startIndex + 6]), name: String(decoding: name, as: UTF8.self),
                         version: d.le(UInt32.self, at: 32))
}

func check(_ r: Data) throws -> Data {
    guard let s = r.first else { throw InstallError.refused(0xFF) }
    if s == Status.busy.rawValue { throw InstallError.busy }
    guard s == Status.ok.rawValue else { throw InstallError.refused(s) }
    return Data(r.dropFirst())
}

/// Headers of the occupied slots (0…7 and the loader).
public func installed(_ s: Requester) async throws -> [Int: PackageHeader] {
    var out: [Int: PackageHeader] = [:]
    for slot in Array(0..<slots) + [loaderSlot] {
        if let h = parseHeader(try check(try await s.call(Cmd.info, Data([UInt8(slot)])))) { out[slot] = h }
    }
    return out
}

/// Installs an app package into the slot of the same name, else the first free one, and a loader package
/// (kind 1) into slot L; returns the slot. `busy` while that app (or the loader's menu) runs on the watch.
public func install(_ s: Requester, package: Data, progress: @Sendable (Double) -> Void) async throws -> Int {
    guard let pkg = parseHeader(package), pkg.kind == 0 || pkg.kind == 1 else { throw InstallError.notAPackage }
    let slot: Int
    if pkg.kind == 1 {
        slot = loaderSlot
    } else {
        let used = try await installed(s)
        guard let free = used.first(where: { $0.key < slots && $0.value.name == pkg.name })?.key
            ?? (0..<slots).first(where: { used[$0] == nil }) else { throw InstallError.noFreeSlot }
        slot = free
    }
    var begin = Data([UInt8(slot)])
    begin.le(UInt32(package.count))
    begin.le(crc32(package))
    _ = try check(try await s.call(Cmd.begin, begin))
    for off in stride(from: 0, to: package.count, by: dataMax) {
        var d = Data()
        d.le(UInt32(off))
        let end = min(off + dataMax, package.count)
        d += package[package.startIndex + off..<package.startIndex + end]
        let r = try check(try await s.call(Cmd.data, d))
        guard r.count == 4, r.le(UInt32.self, at: 0) == UInt32(end) else { throw InstallError.refused(0xFE) }
        progress(Double(end) / Double(package.count))
    }
    _ = try check(try await s.call(Cmd.end, Data([0])))
    return slot
}
