import Foundation
@preconcurrency import PWMCore

/// The sync servers an account can live on.
public enum Server {
    /// The hosted service: a placeholder until the product is named (D1).
    public static let hosted = URL(string: "https://api.pwm.invalid")!
}

/// The Emergency Kit (D42): a PDF drawn by the core, the same on every
/// platform, and the QR code printed on it.
public enum EmergencyKit {
    public enum Paper: Sendable, Equatable {
        case a4, letter

        /// Regions that print on US Letter; everywhere else uses A4.
        static let letterRegions: Set<String> = [
            "US", "CA", "MX", "PH", "CL", "CO", "VE", "GT", "CR", "PA", "DO", "SV", "NI", "PR", "BZ",
        ]

        public static func of(_ locale: Locale) -> Paper {
            guard let region = locale.region?.identifier else { return .a4 }
            return letterRegions.contains(region) ? .letter : .a4
        }
    }

    /// What a scanned kit fills in on the sign-in screen: never a password.
    public struct Scanned: Sendable, Equatable {
        public let email: String
        public let secretKey: Redacted<String>
        /// A self-hosted server; nil for the hosted service.
        public let server: URL?
    }

    /// Reads a scanned QR code's text; `not_a_kit` for any other code.
    public static func scan(_ text: String) throws -> Scanned {
        let kit = try core { try scanKit(text: text) }
        return Scanned(email: kit.email, secretKey: Redacted(kit.secretKey), server: kit.server.flatMap(URL.init(string:)))
    }

    /// The kit as PDF bytes. They hold the Secret Key: pass them to the share
    /// sheet or printer and drop them, never to a file of the app's own.
    static func pdf(
        email: String, secretKey: String, server: URL, accountId: String, created: Date, paper: Paper,
        calendar: Calendar = .current
    ) throws -> Redacted<Data> {
        let parts = calendar.dateComponents([.year, .month, .day], from: created)
        let date = String(format: "%04d-%02d-%02d", parts.year ?? 0, parts.month ?? 0, parts.day ?? 0)
        let bytes = try core {
            try emergencyKit(
                email: email, secretKey: secretKey,
                server: server == Server.hosted ? nil : server.absoluteString,
                accountId: accountId, created: date, paper: paper == .a4 ? .a4 : .letter
            )
        }
        return Redacted(bytes)
    }
}
