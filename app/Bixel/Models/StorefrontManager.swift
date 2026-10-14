// StorefrontManager.swift
//
// Manages App Store storefront awareness for regulatory compliance (e.g. Guideline 5.0.0).
// In China mainland (CHN), Deep Synthesis Technology (DST) regulations require
// specific MIIT permitting, so ChatGPT/Codex provider functionality is deactivated.

import Foundation
import StoreKit

@MainActor
final class StorefrontManager: ObservableObject {
    static let shared = StorefrontManager()

    /// Whether the active App Store storefront or device region is China mainland (CHN).
    @Published private(set) var isChinaStorefront: Bool = false

    private init() {
        evaluateStorefront()
        Task {
            await observeStoreKitStorefront()
        }
    }

    private func evaluateStorefront() {
        // Check system locale region code
        if let regionCode = Locale.current.region?.identifier, regionCode.uppercased() == "CN" {
            isChinaStorefront = true
        }

        #if os(macOS) || os(iOS)
        if let sf = SKPaymentQueue.default().storefront, sf.countryCode == "CHN" {
            isChinaStorefront = true
        }
        #endif
    }

    /// StoreKit 2 storefront detection
    func observeStoreKitStorefront() async {
        if let current = await Storefront.current {
            let isChina = (current.countryCode == "CHN")
            if isChinaStorefront != isChina {
                isChinaStorefront = isChina
            }
        }
    }

    #if DEBUG
    /// Helper for testing storefront gating in debug builds.
    func debugSetChinaStorefront(_ isChina: Bool) {
        self.isChinaStorefront = isChina
    }
    #endif
}
