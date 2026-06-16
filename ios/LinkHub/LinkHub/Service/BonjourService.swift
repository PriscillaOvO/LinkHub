import Foundation
import Network

/// Bonjour (mDNS) discovery for LinkHub.
/// Registers and browses `_linkhub._tcp.` services.
class BonjourService: NSObject, NetServiceDelegate, NetServiceBrowserDelegate {
    private var service: NetService?
    private var browser: NetServiceBrowser?
    var discoveredDevices: [(name: String, id: String, fingerprint: String, port: Int)] = []
    var onDeviceDiscovered: (((String, String, String, Int)) -> Void)?

    func startAdvertising(deviceName: String, deviceId: String, fingerprint: String, port: Int32) {
        let txt: [String: Data] = [
            "lh": "1".data(using: .utf8)!,
            "id": deviceId.data(using: .utf8)!,
            "name": deviceName.data(using: .utf8)!,
            "fp": fingerprint.data(using: .utf8)!,
            "port": "\(port)".data(using: .utf8)!,
        ]
        service = NetService(domain: "local.", type: "_linkhub._tcp.", name: "\(deviceName)-\(deviceId)", port: port)
        service?.setTXTRecord(NetService.data(fromTXTRecord: txt))
        service?.delegate = self
        service?.publish()
    }

    func startBrowsing() {
        browser = NetServiceBrowser()
        browser?.delegate = self
        browser?.searchForServices(ofType: "_linkhub._tcp.", inDomain: "local.")
    }

    func stopAll() {
        service?.stop()
        browser?.stop()
    }
}
