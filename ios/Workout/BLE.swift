@preconcurrency import CoreBluetooth
import Foundation
import WireKit

/// The watch over CoreBluetooth, as WireKit's `Link`. Takes the connection iOS already holds for Nothing X
/// (`retrieveConnectedPeripherals`) or connects itself; a pending connect completes whenever the watch is in range,
/// also in the background (`bluetooth-central`, state restoration).
final class BLE: NSObject, Link, @unchecked Sendable {
    static let cmdService = CBUUID(string: "FFF0")
    static let shellService = CBUUID(string: "77D4E67C-2FE2-2334-0D35-9CCD078F529C")
    static let dataService = CBUUID(string: "02F00000-0000-0000-0000-00000000FFE0")
    static let dataNotify = CBUUID(string: "02F00000-0000-0000-0000-00000000FFE2")
    private static let notified = [CBUUID(string: Uuid.cmdNotify), CBUUID(string: Uuid.shellNotify), dataNotify]

    enum State: Equatable { case off, searching, connecting, ready, disconnected }

    /// Main queue: every callback and every property below lives there.
    private var central: CBCentralManager!
    private var peripheral: CBPeripheral?
    private var chars: [CBUUID: CBCharacteristic] = [:]
    private var handler: (@Sendable (String, Data) -> Void)?
    private var writes: [CheckedContinuation<Void, Error>] = []
    var onState: ((State) -> Void)?
    private(set) var state = State.off { didSet { onState?(state) } }

    override init() {
        super.init()
        central = CBCentralManager(delegate: self, queue: .main,
                                   options: [CBCentralManagerOptionRestoreIdentifierKey: "wp2.workout"])
    }

    func onNotify(_ handler: @escaping @Sendable (String, Data) -> Void) {
        DispatchQueue.main.async { self.handler = handler }
    }

    func write(_ uuid: String, _ data: Data) async throws {
        try await withCheckedThrowingContinuation { (c: CheckedContinuation<Void, Error>) in
            DispatchQueue.main.async {
                guard let p = self.peripheral, let ch = self.chars[CBUUID(string: uuid)] else {
                    return c.resume(throwing: WireError.closed)
                }
                // longer than MTU-3: a write with response, which iOS sends as a long write (as blegatt.Gatt)
                if data.count > p.maximumWriteValueLength(for: .withoutResponse) {
                    self.writes.append(c)
                    p.writeValue(data, for: ch, type: .withResponse)
                } else {
                    p.writeValue(data, for: ch, type: .withoutResponse)
                    c.resume()
                }
            }
        }
    }

    private func find() {
        guard central.state == .poweredOn, peripheral == nil else { return }
        let held = central.retrieveConnectedPeripherals(withServices: [Self.cmdService])
        if let p = held.first(where: { ($0.name ?? "").lowercased().contains("cmf") }) ?? held.first {
            attach(p)
        } else {
            state = .searching
            central.scanForPeripherals(withServices: nil)
        }
    }

    private func attach(_ p: CBPeripheral) {
        central.stopScan()
        peripheral = p
        p.delegate = self
        UserDefaults.standard.set(p.identifier.uuidString, forKey: "watch")
        state = .connecting
        if p.state == .connected { discover(p) } else { central.connect(p) }
    }

    private func discover(_ p: CBPeripheral) {
        chars = [:]
        p.discoverServices([Self.cmdService, Self.shellService, Self.dataService])
    }
}

extension BLE: CBCentralManagerDelegate {
    func centralManagerDidUpdateState(_ c: CBCentralManager) {
        if c.state == .poweredOn {
            if let p = peripheral { attach(p) } else { find() }
        } else {
            state = .off
        }
    }

    func centralManager(_ c: CBCentralManager, willRestoreState dict: [String: Any]) {
        if let p = (dict[CBCentralManagerRestoredStatePeripheralsKey] as? [CBPeripheral])?.first {
            peripheral = p
            p.delegate = self
        }
    }

    func centralManager(_ c: CBCentralManager, didDiscover p: CBPeripheral, advertisementData: [String: Any],
                        rssi: NSNumber) {
        let name = (advertisementData[CBAdvertisementDataLocalNameKey] as? String) ?? p.name ?? ""
        if name.lowercased().contains("cmf") { attach(p) }
    }

    func centralManager(_ c: CBCentralManager, didConnect p: CBPeripheral) { discover(p) }

    func centralManager(_ c: CBCentralManager, didDisconnectPeripheral p: CBPeripheral, error: Error?) {
        state = .disconnected
        writes.forEach { $0.resume(throwing: WireError.closed) }
        writes = []
        c.connect(p) // pending: completes when the watch is back
    }

    func centralManager(_ c: CBCentralManager, didFailToConnect p: CBPeripheral, error: Error?) {
        c.connect(p)
    }
}

extension BLE: CBPeripheralDelegate {
    func peripheral(_ p: CBPeripheral, didDiscoverServices error: Error?) {
        p.services?.forEach { p.discoverCharacteristics(nil, for: $0) }
    }

    func peripheral(_ p: CBPeripheral, didDiscoverCharacteristicsFor s: CBService, error: Error?) {
        for ch in s.characteristics ?? [] {
            chars[ch.uuid] = ch
            if Self.notified.contains(ch.uuid) { p.setNotifyValue(true, for: ch) }
        }
        if Self.notified.allSatisfy({ chars[$0] != nil }), chars[CBUUID(string: Uuid.cmdWrite)] != nil,
           chars[CBUUID(string: Uuid.shellWrite)] != nil {
            // the stock answers the shell only after the third subscription ("phone ready"): give it a moment
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) { self.state = .ready }
        }
    }

    func peripheral(_ p: CBPeripheral, didUpdateValueFor ch: CBCharacteristic, error: Error?) {
        // 16-bit UUIDs print short ("FFF1"): WireKit compares the 128-bit form
        let id = ch.uuid.uuidString.count == 4 ? "0000\(ch.uuid.uuidString)-0000-1000-8000-00805F9B34FB" : ch.uuid.uuidString
        if let v = ch.value { handler?(id, v) }
    }

    func peripheral(_ p: CBPeripheral, didWriteValueFor ch: CBCharacteristic, error: Error?) {
        guard !writes.isEmpty else { return }
        let c = writes.removeFirst()
        if let error { c.resume(throwing: error) } else { c.resume() }
    }
}
