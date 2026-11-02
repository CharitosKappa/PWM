// Scanning the Emergency Kit's QR code with the camera (D42 §4), through
// VisionKit. The text goes to the core's parser and nowhere else; M5.9's
// device approval reuses this scanner.
#if canImport(SwiftUI)
import SwiftUI

#if os(iOS)
import VisionKit

struct KitScanner: UIViewControllerRepresentable {
    /// Called with each QR code's text until the sheet is closed.
    let found: @MainActor (String) -> Void

    /// Hidden on devices without a camera or the scanner (and in the simulator).
    @MainActor static var isAvailable: Bool {
        DataScannerViewController.isSupported && DataScannerViewController.isAvailable
    }

    func makeUIViewController(context: Context) -> DataScannerViewController {
        let scanner = DataScannerViewController(
            recognizedDataTypes: [.barcode(symbologies: [.qr])],
            qualityLevel: .balanced,
            isHighlightingEnabled: true
        )
        scanner.delegate = context.coordinator
        try? scanner.startScanning()
        return scanner
    }

    func updateUIViewController(_ scanner: DataScannerViewController, context: Context) {}

    static func dismantleUIViewController(_ scanner: DataScannerViewController, coordinator: Coordinator) {
        scanner.stopScanning()
    }

    func makeCoordinator() -> Coordinator {
        Coordinator(found: found)
    }

    final class Coordinator: NSObject, DataScannerViewControllerDelegate {
        let found: @MainActor (String) -> Void

        init(found: @escaping @MainActor (String) -> Void) {
            self.found = found
        }

        func dataScanner(_ scanner: DataScannerViewController, didAdd items: [RecognizedItem], allItems: [RecognizedItem]) {
            for case let .barcode(code) in items {
                if let text = code.payloadStringValue { found(text) }
            }
        }
    }
}
#else
/// No camera scanner on this platform yet; the Secret Key is typed.
struct KitScanner: View {
    let found: @MainActor (String) -> Void

    static var isAvailable: Bool { false }

    var body: some View { EmptyView() }
}
#endif
#endif
