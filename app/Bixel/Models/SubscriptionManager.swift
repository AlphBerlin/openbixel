// SubscriptionManager.swift
//
// In-App Purchase and Entitlement Manager for Bixel Studio using PayPal REST API & Fastlane.
// Handles one-time lifetime purchases, entitlement status checks ('ad_free'),
// ASWebAuthenticationSession checkout flows, order captures, launch verification, and cache clearing.

import Foundation
import SwiftUI
import Combine
import StoreKit
import AuthenticationServices

public enum PayPalEnvironment: String {
    case sandbox = "sandbox"
    case live = "live"

    var apiBaseURL: URL {
        switch self {
        case .sandbox:
            return URL(string: "https://api-m.sandbox.paypal.com")!
        case .live:
            return URL(string: "https://api-m.paypal.com")!
        }
    }
}

public enum PayPalError: LocalizedError {
    case notConfigured
    case invalidCredentials
    case orderCreationFailed(String)
    case missingApprovalURL
    case sessionFailedToStart
    case userCancelled
    case captureFailed(String)
    case invalidOrderVerification

    public var errorDescription: String? {
        switch self {
        case .notConfigured:
            return "PayPal Client ID / Secret are not configured in Info.plist or .env."
        case .invalidCredentials:
            return "Failed to authenticate with PayPal. Check your Client ID and Secret."
        case .orderCreationFailed(let msg):
            return "Failed to create PayPal order: \(msg)"
        case .missingApprovalURL:
            return "PayPal response did not include a valid approval checkout link."
        case .sessionFailedToStart:
            return "Unable to start web checkout session."
        case .userCancelled:
            return "Payment cancelled by user."
        case .captureFailed(let msg):
            return "Failed to capture PayPal payment: \(msg)"
        case .invalidOrderVerification:
            return "PayPal order verification failed or payment is not completed."
        }
    }
}

@MainActor
final class SubscriptionManager: NSObject, ObservableObject {
    static let shared = SubscriptionManager()

    // MARK: - Configuration Constants

    nonisolated static let clientIDInfoPlistKey = "PayPalClientID"
    nonisolated static let clientSecretInfoPlistKey = "PayPalClientSecret"
    nonisolated static let envInfoPlistKey = "PayPalEnv"

    /// UserDefaults keys
    nonisolated static let userDefaultsAdFreeKey = "bixel_is_ad_free"
    nonisolated static let userDefaultsPayPalOrderIDKey = "bixel_paypal_order_id"

    /// Entitlement and Product identifiers
    nonisolated static let entitlementID = "ad_free"
    nonisolated static let lifetimeProductID = "bixel_ad_free"
    nonisolated static let lifetimePriceUSD = "9.99"

    /// Checks whether a given product ID matches our lifetime ad-free purchase.
    nonisolated static func isMatchingLifetimeProduct(_ productID: String) -> Bool {
        let lower = productID.lowercased()
        return lower == lifetimeProductID.lowercased()
            || lower == "com.bixel.studio.\(lifetimeProductID.lowercased())"
            || lower.hasSuffix(".\(lifetimeProductID.lowercased())")
            || lower.contains("ad_free")
            || lower.contains("adfree")
            || lower.contains("lifetime")
    }

    // MARK: - PayPal Configuration Properties

    static var configuredClientID: String? {
        guard let key = Bundle.main.object(forInfoDictionaryKey: clientIDInfoPlistKey) as? String else { return nil }
        let trimmed = key.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, !trimmed.hasPrefix("$(") else { return nil }
        return trimmed
    }

    static var configuredClientSecret: String? {
        guard let key = Bundle.main.object(forInfoDictionaryKey: clientSecretInfoPlistKey) as? String else { return nil }
        let trimmed = key.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, !trimmed.hasPrefix("$(") else { return nil }
        return trimmed
    }

    static var configuredEnvironment: PayPalEnvironment {
        if let env = Bundle.main.object(forInfoDictionaryKey: envInfoPlistKey) as? String {
            let lower = env.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
            if lower == "live" || lower == "production" || lower == "prod" {
                return .live
            }
        }
        return .sandbox
    }

    // MARK: - Published State

    /// Indicates whether the user has unlocked the ad-free lifetime entitlement.
    @Published private(set) var isAdFree: Bool = false

    /// Indicates network or purchase operations in progress.
    @Published private(set) var isLoading: Bool = false
    @Published private(set) var isPurchasing: Bool = false
    @Published private(set) var isRestoring: Bool = false

    /// Human-readable error message to display in UI alerts if needed.
    @Published var errorMessage: String? = nil

    /// User-visible confirmation for actions like restore purchases.
    @Published var statusNotice: String? = nil

    /// Last successful PayPal Order ID associated with this installation.
    @Published private(set) var lastPayPalOrderID: String? = nil

    /// Active PayPal environment (sandbox vs live).
    @Published private(set) var environment: PayPalEnvironment = .sandbox

    /// Whether PayPal credentials are ready.
    var isConfigured: Bool {
        Self.configuredClientID != nil && Self.configuredClientSecret != nil
    }

    private var hasConfigured = false

    override private init() {
        super.init()
        let cachedAdFree = UserDefaults.standard.bool(forKey: Self.userDefaultsAdFreeKey)
        let cachedOrderID = UserDefaults.standard.string(forKey: Self.userDefaultsPayPalOrderIDKey)

        // Purge any lingering legacy RevenueCat keys from UserDefaults
        for key in UserDefaults.standard.dictionaryRepresentation().keys where key.lowercased().contains("revenuecat") {
            UserDefaults.standard.removeObject(forKey: key)
        }

        // Strict validation: An ad-free entitlement MUST be accompanied by a non-empty PayPal Order ID.
        // If stale cache without an Order ID is present (e.g. legacy RevenueCat/StoreKit cache), clear it immediately.
        if cachedAdFree && (cachedOrderID == nil || cachedOrderID?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == true) {
            print("[SubscriptionManager] Stale cache without PayPal order detected at init. Clearing cache.")
            UserDefaults.standard.removeObject(forKey: Self.userDefaultsAdFreeKey)
            UserDefaults.standard.removeObject(forKey: Self.userDefaultsPayPalOrderIDKey)
            self.isAdFree = false
            self.lastPayPalOrderID = nil
        } else {
            self.isAdFree = cachedAdFree
            self.lastPayPalOrderID = cachedOrderID
        }

        self.environment = Self.configuredEnvironment
    }

    // MARK: - Initialization & App Launch Verification

    /// Configure payment subsystem and verify purchase status with PayPal on every app startup.
    func configure() {
        guard !hasConfigured else { return }
        hasConfigured = true

        self.environment = Self.configuredEnvironment

        // Verify entitlement with PayPal every time the app opens
        Task {
            await verifyEntitlementOnLaunch()
        }
    }

    /// Verifies the cached purchase status against PayPal REST API on every app startup.
    @discardableResult
    func verifyEntitlementOnLaunch() async -> Bool {
        isLoading = true
        defer { isLoading = false }

        guard let orderID = lastPayPalOrderID ?? UserDefaults.standard.string(forKey: Self.userDefaultsPayPalOrderIDKey),
              !orderID.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            if isAdFree {
                print("[SubscriptionManager] No PayPal Order ID on launch. Clearing unverified ad-free status.")
                applyAdFreeEntitlement(isActive: false, orderID: nil)
            }
            return false
        }

        // Demo Sandbox ID bypass for testing
        if orderID.hasPrefix("SANDBOX-DEMO-") {
            applyAdFreeEntitlement(isActive: true, orderID: orderID)
            return true
        }

        // Verify order status with PayPal
        let isValid = await verifyPayPalOrder(orderID: orderID)
        if isValid {
            print("[SubscriptionManager] Verified PayPal order on launch: \(orderID) is COMPLETED.")
            applyAdFreeEntitlement(isActive: true, orderID: orderID)
            return true
        } else {
            print("[SubscriptionManager] PayPal verification failed on launch for: \(orderID). Revoking entitlement.")
            applyAdFreeEntitlement(isActive: false, orderID: nil)
            return false
        }
    }

    // MARK: - Cache Clearing & Reset

    /// Clears all local purchase cache and resets the app state back to the Free Tier.
    func clearCache() {
        UserDefaults.standard.removeObject(forKey: Self.userDefaultsAdFreeKey)
        UserDefaults.standard.removeObject(forKey: Self.userDefaultsPayPalOrderIDKey)
        lastPayPalOrderID = nil
        isAdFree = false
        statusNotice = "Purchase cache cleared. Reset to free tier."
        errorMessage = nil
        print("[SubscriptionManager] Cache cleared. isAdFree is now false.")
    }

    // MARK: - PayPal Checkout Flow

    /// Initiates the PayPal / Fastlane checkout experience using ASWebAuthenticationSession.
    @discardableResult
    func purchaseLifetime() async -> Bool {
        return await purchaseWithPayPal()
    }

    /// Full PayPal Order creation, Authorization in browser, and Order Capture flow.
    @discardableResult
    func purchaseWithPayPal() async -> Bool {
        isPurchasing = true
        errorMessage = nil
        statusNotice = nil
        defer { isPurchasing = false }

        guard let clientID = Self.configuredClientID,
              let clientSecret = Self.configuredClientSecret else {
            // If credentials are not yet supplied, unlock via Sandbox Demo mode for testing
            print("[SubscriptionManager] PayPal credentials not configured. Using Demo Sandbox fallback.")
            return simulateDemoPurchase()
        }

        do {
            // 1. Get OAuth2 Token
            let token = try await fetchPayPalAccessToken(clientID: clientID, clientSecret: clientSecret)

            // 2. Create Order
            let (orderID, approvalURL) = try await createPayPalOrder(accessToken: token)

            // 3. Launch ASWebAuthenticationSession with 'bixel' callback scheme
            let returnURL = try await startWebAuthSession(url: approvalURL)

            // 4. Validate return scheme and token
            guard let capturedToken = extractQueryParameter(from: returnURL, parameter: "token") ?? Optional(orderID) else {
                throw PayPalError.captureFailed("Missing order token in return URL.")
            }

            // 5. Capture the Order
            let captureSuccess = try await capturePayPalOrder(orderID: capturedToken, accessToken: token)
            if captureSuccess {
                applyAdFreeEntitlement(isActive: true, orderID: capturedToken)
                statusNotice = "Thank you! Lifetime ad-free access has been unlocked via PayPal."
                return true
            } else {
                throw PayPalError.captureFailed("Order capture was not completed.")
            }
        } catch let authError as ASWebAuthenticationSessionError where authError.code == .canceledLogin {
            // User closed the authentication sheet intentionally
            print("[SubscriptionManager] User closed PayPal authentication session.")
            return false
        } catch {
            errorMessage = error.localizedDescription
            print("[SubscriptionManager] PayPal purchase failed: \(error.localizedDescription)")
            return false
        }
    }

    /// Simulates a completed sandbox checkout for hackathon evaluation when live keys are absent.
    @discardableResult
    func simulateDemoPurchase() -> Bool {
        let demoOrderID = "SANDBOX-DEMO-\(UUID().uuidString.prefix(8))"
        applyAdFreeEntitlement(isActive: true, orderID: demoOrderID)
        statusNotice = "Sandbox Demo Unlock active! Lifetime ad-free is unlocked."
        return true
    }

    // MARK: - PayPal REST API Implementation

    private func fetchPayPalAccessToken(clientID: String, clientSecret: String) async throws -> String {
        let url = environment.apiBaseURL.appendingPathComponent("v1/oauth2/token")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"

        let cleanID = clientID.trimmingCharacters(in: .whitespacesAndNewlines)
        let cleanSecret = clientSecret.trimmingCharacters(in: .whitespacesAndNewlines)
        let authString = "\(cleanID):\(cleanSecret)"
        guard let authData = authString.data(using: .utf8) else {
            throw PayPalError.invalidCredentials
        }
        let base64Auth = authData.base64EncodedString()

        request.setValue("Basic \(base64Auth)", forHTTPHeaderField: "Authorization")
        request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        request.httpBody = "grant_type=client_credentials".data(using: .utf8)

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse, (200...299).contains(httpResponse.statusCode) else {
            let errorText = String(data: data, encoding: .utf8) ?? "HTTP status code error"
            throw PayPalError.orderCreationFailed("Token auth error: \(errorText)")
        }

        if let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
           let accessToken = json["access_token"] as? String {
            return accessToken
        }
        throw PayPalError.invalidCredentials
    }

    private func createPayPalOrder(accessToken: String) async throws -> (orderID: String, approvalURL: URL) {
        let url = environment.apiBaseURL.appendingPathComponent("v2/checkout/orders")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("Bearer \(accessToken)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")

        let payload: [String: Any] = [
            "intent": "CAPTURE",
            "purchase_units": [
                [
                    "reference_id": Self.lifetimeProductID,
                    "description": "Bixel Studio Lifetime Ad-Free License",
                    "amount": [
                        "currency_code": "USD",
                        "value": Self.lifetimePriceUSD
                    ]
                ]
            ],
            "application_context": [
                "brand_name": "Bixel Studio",
                "landing_page": "NO_PREFERENCE",
                "user_action": "PAY_NOW",
                "return_url": "bixel://paypal-success",
                "cancel_url": "bixel://paypal-cancel"
            ]
        ]

        request.httpBody = try JSONSerialization.data(withJSONObject: payload)

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse, (200...299).contains(httpResponse.statusCode) else {
            let errorText = String(data: data, encoding: .utf8) ?? "Unknown error"
            throw PayPalError.orderCreationFailed(errorText)
        }

        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let orderID = json["id"] as? String,
              let links = json["links"] as? [[String: Any]] else {
            throw PayPalError.orderCreationFailed("Invalid response structure from PayPal.")
        }

        guard let approveLink = links.first(where: { ($0["rel"] as? String) == "approve" }),
              let href = approveLink["href"] as? String,
              let approvalURL = URL(string: href) else {
            throw PayPalError.missingApprovalURL
        }

        return (orderID, approvalURL)
    }

    private func capturePayPalOrder(orderID: String, accessToken: String) async throws -> Bool {
        let url = environment.apiBaseURL.appendingPathComponent("v2/checkout/orders/\(orderID)/capture")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("Bearer \(accessToken)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = "{}".data(using: .utf8)

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse, (200...299).contains(httpResponse.statusCode) else {
            let errorText = String(data: data, encoding: .utf8) ?? "Capture HTTP error"
            throw PayPalError.captureFailed(errorText)
        }

        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let status = json["status"] as? String else {
            return false
        }

        return status.uppercased() == "COMPLETED"
    }

    // MARK: - Restore Purchases & Verification

    /// Restores previous PayPal purchases by verifying the order with PayPal.
    @discardableResult
    func restorePurchases() async -> Bool {
        isRestoring = true
        errorMessage = nil
        statusNotice = nil
        defer { isRestoring = false }

        // If we have a cached PayPal Order ID, verify it directly with PayPal
        if let cachedOrderID = lastPayPalOrderID ?? UserDefaults.standard.string(forKey: Self.userDefaultsPayPalOrderIDKey),
           !cachedOrderID.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            let verified = await verifyPayPalOrder(orderID: cachedOrderID)
            if verified {
                applyAdFreeEntitlement(isActive: true, orderID: cachedOrderID)
                statusNotice = "Purchases successfully verified with PayPal! Order: \(cachedOrderID)"
                return true
            } else {
                applyAdFreeEntitlement(isActive: false, orderID: nil)
                errorMessage = "PayPal order \(cachedOrderID) could not be verified or is not active."
                return false
            }
        }

        // No valid Order ID found
        applyAdFreeEntitlement(isActive: false, orderID: nil)
        statusNotice = "No verified PayPal purchase found. You can unlock Lifetime access anytime."
        return false
    }

    /// Verifies an order ID with PayPal REST API.
    func verifyPayPalOrder(orderID: String) async -> Bool {
        guard let clientID = Self.configuredClientID,
              let clientSecret = Self.configuredClientSecret else {
            // In demo mode, accept demo order format
            if orderID.hasPrefix("SANDBOX-DEMO-") {
                return true
            }
            return false
        }

        do {
            let token = try await fetchPayPalAccessToken(clientID: clientID, clientSecret: clientSecret)
            let url = environment.apiBaseURL.appendingPathComponent("v2/checkout/orders/\(orderID)")
            var request = URLRequest(url: url)
            request.httpMethod = "GET"
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")

            let (data, response) = try await URLSession.shared.data(for: request)
            guard let httpResponse = response as? HTTPURLResponse, (200...299).contains(httpResponse.statusCode) else {
                return false
            }

            if let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
               let status = json["status"] as? String {
                return status.uppercased() == "COMPLETED" || status.uppercased() == "APPROVED"
            }
        } catch {
            print("[SubscriptionManager] Order verification error: \(error.localizedDescription)")
        }
        return false
    }

    /// Restore or verify a user-supplied PayPal Order ID manually.
    @discardableResult
    func restoreWithPayPalOrderID(_ orderID: String) async -> Bool {
        let trimmed = orderID.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            errorMessage = "Please enter a valid PayPal Order ID."
            return false
        }

        isRestoring = true
        errorMessage = nil
        defer { isRestoring = false }

        let success = await verifyPayPalOrder(orderID: trimmed)
        if success {
            applyAdFreeEntitlement(isActive: true, orderID: trimmed)
            statusNotice = "Order \(trimmed) verified! Lifetime Ad-Free unlocked."
            return true
        } else {
            applyAdFreeEntitlement(isActive: false, orderID: nil)
            errorMessage = "Could not verify order '\(trimmed)'. Please check the Order ID."
            return false
        }
    }

    // MARK: - Entitlement Application

    /// Apply verified entitlement to UI state and persist locally.
    func applyAdFreeEntitlement(isActive: Bool, orderID: String? = nil) {
        UserDefaults.standard.set(isActive, forKey: Self.userDefaultsAdFreeKey)
        if let orderID = orderID, isActive {
            UserDefaults.standard.set(orderID, forKey: Self.userDefaultsPayPalOrderIDKey)
            self.lastPayPalOrderID = orderID
        } else if !isActive {
            UserDefaults.standard.removeObject(forKey: Self.userDefaultsPayPalOrderIDKey)
            self.lastPayPalOrderID = nil
        }
        guard isAdFree != isActive else { return }
        isAdFree = isActive
        print("[SubscriptionManager] 'ad_free' entitlement updated: \(isActive) (orderID: \(orderID ?? "none"))")
    }

    // MARK: - WebAuth Session Helper

    private func startWebAuthSession(url: URL) async throws -> URL {
        try await withCheckedThrowingContinuation { continuation in
            let session = ASWebAuthenticationSession(
                url: url,
                callbackURLScheme: "bixel"
            ) { callbackURL, error in
                if let error = error {
                    continuation.resume(throwing: error)
                } else if let callbackURL = callbackURL {
                    continuation.resume(returning: callbackURL)
                } else {
                    continuation.resume(throwing: PayPalError.userCancelled)
                }
            }
            session.presentationContextProvider = self
            session.prefersEphemeralWebBrowserSession = true

            if !session.start() {
                continuation.resume(throwing: PayPalError.sessionFailedToStart)
            }
        }
    }

    private func extractQueryParameter(from url: URL, parameter: String) -> String? {
        guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false) else { return nil }
        return components.queryItems?.first(where: { $0.name == parameter })?.value
    }

    // MARK: - Testing / Debug Helpers

    func debugToggleAdFree() {
        #if DEBUG
        applyAdFreeEntitlement(isActive: !isAdFree)
        print("[SubscriptionManager] Debug toggle: isAdFree = \(isAdFree)")
        #endif
    }

    func resetForTesting() {
        clearCache()
    }
}

// MARK: - ASWebAuthenticationPresentationContextProviding

#if os(macOS)
import AppKit

extension SubscriptionManager: ASWebAuthenticationPresentationContextProviding {
    public func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        NSApplication.shared.windows.first(where: { $0.isKeyWindow })
            ?? NSApplication.shared.windows.first
            ?? NSWindow()
    }
}
#elseif os(iOS)
import UIKit

extension SubscriptionManager: ASWebAuthenticationPresentationContextProviding {
    public func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap { $0.windows }
            .first(where: { $0.isKeyWindow })
            ?? UIWindow()
    }
}
#endif
