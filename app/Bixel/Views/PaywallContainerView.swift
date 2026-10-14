// PaywallContainerView.swift
//
// Modern SwiftUI Paywall presentation for Bixel Studio.
// Integrated with PayPal REST API & Fastlane accelerated guest checkout
// for Lifetime Ad-Free access.

import SwiftUI

struct PaywallContainerView: View {
    @ObservedObject private var subscriptionManager = SubscriptionManager.shared
    @Environment(\.dismiss) private var dismiss

    @State private var showRestoreSheet = false
    @State private var restoreOrderIDInput = ""

    var body: some View {
        ZStack {
            StudioTheme.homeDark
                .ignoresSafeArea()

            VStack(spacing: 22) {
                // Header Bar with Environment Badge & Close Button
                HStack {
                    HStack(spacing: 6) {
                        Circle()
                            .fill(subscriptionManager.environment == .live ? StudioTheme.bixelGreen : Color.yellow)
                            .frame(width: 8, height: 8)
                        Text(subscriptionManager.environment == .live ? "PayPal Live" : "PayPal Sandbox")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundColor(StudioTheme.textSecondary)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 4)
                    .background(Color.white.opacity(0.06), in: Capsule())

                    Spacer()

                    Button {
                        dismiss()
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(.system(size: 20))
                            .foregroundColor(Color.white.opacity(0.6))
                    }
                    .buttonStyle(.plain)
                }

                // Mascot / Icon
                ZStack {
                    Circle()
                        .fill(
                            LinearGradient(
                                colors: [Color(red: 0.0, green: 0.45, blue: 0.9).opacity(0.25), Color(red: 0.0, green: 0.15, blue: 0.5).opacity(0.15)],
                                startPoint: .topLeading,
                                endPoint: .bottomTrailing
                            )
                        )
                        .frame(width: 76, height: 76)

                    Image(systemName: "crown.fill")
                        .font(.system(size: 34))
                        .foregroundColor(Color(red: 0.2, green: 0.6, blue: 1.0))
                }

                // Headline & Subhead
                VStack(spacing: 6) {
                    Text("Unlock Bixel Studio Lifetime")
                        .font(.system(size: 24, weight: .bold, design: .rounded))
                        .foregroundColor(.white)

                    Text("Accelerated Checkout by PayPal Fastlane • One-time purchase")
                        .font(.system(size: 13))
                        .foregroundColor(StudioTheme.textSecondary)
                        .multilineTextAlignment(.center)
                }

                // Feature Highlights
                VStack(alignment: .leading, spacing: 14) {
                    featureRow(
                        icon: "shield.slash.fill",
                        title: "100% Ad-Free Canvas",
                        description: "Never see another banner or sponsor prompt while creating."
                    )
                    featureRow(
                        icon: "sparkles",
                        title: "Full Generative AI & Pixel Art Suites",
                        description: "Generate sprites, tilemaps, animations, and color palettes without limits."
                    )
                    featureRow(
                        icon: "bolt.fill",
                        title: "Fastlane 1-Click Guest Checkout",
                        description: "Pay with one click using your phone number or email—no password needed."
                    )
                    featureRow(
                        icon: "creditcard.fill",
                        title: "PayPal, Cards, or PYUSD",
                        description: "Flexible checkout powered directly by PayPal payment rails."
                    )
                }
                .padding(20)
                .background(
                    RoundedRectangle(cornerRadius: 14, style: .continuous)
                        .fill(StudioTheme.homeCard)
                        .overlay(
                            RoundedRectangle(cornerRadius: 14, style: .continuous)
                                .strokeBorder(StudioTheme.homeCardBorder, lineWidth: 1)
                        )
                )

                // Error / Status Message
                if let error = subscriptionManager.errorMessage {
                    Text(error)
                        .font(.system(size: 12))
                        .foregroundColor(.red)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal)
                } else if let notice = subscriptionManager.statusNotice {
                    Text(notice)
                        .font(.system(size: 12))
                        .foregroundColor(StudioTheme.bixelGreen)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal)
                }

                Spacer()

                // Primary PayPal Checkout Action
                Button {
                    Task {
                        let success = await subscriptionManager.purchaseLifetime()
                        if success {
                            try? await Task.sleep(nanoseconds: 700_000_000)
                            dismiss()
                        }
                    }
                } label: {
                    HStack(spacing: 10) {
                        if subscriptionManager.isPurchasing {
                            ProgressView()
                                .controlSize(.small)
                            Text("Connecting to PayPal…")
                                .font(.system(size: 14, weight: .bold, design: .rounded))
                        } else {
                            Image(systemName: "creditcard.circle.fill")
                                .font(.system(size: 16, weight: .bold))
                            Text("Pay with PayPal & Fastlane — $\(SubscriptionManager.lifetimePriceUSD)")
                                .font(.system(size: 14, weight: .bold, design: .rounded))
                        }
                    }
                    .foregroundColor(.white)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 13)
                    .background(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(
                                LinearGradient(
                                    colors: [Color(red: 0.0, green: 0.45, blue: 0.85), Color(red: 0.0, green: 0.3, blue: 0.7)],
                                    startPoint: .leading,
                                    endPoint: .trailing
                                )
                            )
                    )
                    .shadow(color: Color(red: 0.0, green: 0.45, blue: 0.85).opacity(0.35), radius: 8, y: 2)
                }
                .buttonStyle(.plain)
                .disabled(subscriptionManager.isPurchasing)

                // Secondary Action: Restore Purchases
                HStack(spacing: 16) {
                    Button {
                        showRestoreSheet = true
                    } label: {
                        Text("Restore with Order ID")
                            .font(.system(size: 12, weight: .medium))
                            .foregroundColor(StudioTheme.textSecondary)
                    }
                    .buttonStyle(.plain)

                    Text("•")
                        .foregroundColor(Color.white.opacity(0.2))

                    Button {
                        Task {
                            let success = await subscriptionManager.restorePurchases()
                            if success {
                                try? await Task.sleep(nanoseconds: 700_000_000)
                                dismiss()
                            }
                        }
                    } label: {
                        if subscriptionManager.isRestoring {
                            ProgressView()
                                .controlSize(.small)
                        } else {
                            Text("Auto-Restore Account")
                                .font(.system(size: 12, weight: .medium))
                                .foregroundColor(StudioTheme.textSecondary)
                        }
                    }
                    .buttonStyle(.plain)
                    .disabled(subscriptionManager.isRestoring)
                }
            }
            .padding(32)
        }
        .frame(width: 520, height: 640)
        .sheet(isPresented: $showRestoreSheet) {
            restoreSheetView
        }
    }

    private var restoreSheetView: some View {
        ZStack {
            StudioTheme.homeDark
                .ignoresSafeArea()

            VStack(spacing: 18) {
                HStack {
                    Text("Restore PayPal Order")
                        .font(.system(size: 16, weight: .bold, design: .rounded))
                        .foregroundColor(.white)
                    Spacer()
                    Button {
                        showRestoreSheet = false
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundColor(Color.white.opacity(0.6))
                    }
                    .buttonStyle(.plain)
                }

                Text("Enter the PayPal Order ID or Transaction ID from your purchase receipt.")
                    .font(.system(size: 12))
                    .foregroundColor(StudioTheme.textSecondary)
                    .frame(maxWidth: .infinity, alignment: .leading)

                TextField("e.g. 5O190127TN364715T", text: $restoreOrderIDInput)
                    .textFieldStyle(.plain)
                    .padding(10)
                    .background(StudioTheme.homeCard, in: RoundedRectangle(cornerRadius: 8))
                    .overlay(
                        RoundedRectangle(cornerRadius: 8)
                            .strokeBorder(StudioTheme.homeCardBorder, lineWidth: 1)
                    )
                    .foregroundColor(.white)

                if let error = subscriptionManager.errorMessage {
                    Text(error)
                        .font(.system(size: 11))
                        .foregroundColor(.red)
                }

                Spacer()

                Button {
                    Task {
                        let success = await subscriptionManager.restoreWithPayPalOrderID(restoreOrderIDInput)
                        if success {
                            showRestoreSheet = false
                            try? await Task.sleep(nanoseconds: 500_000_000)
                            dismiss()
                        }
                    }
                } label: {
                    HStack {
                        if subscriptionManager.isRestoring {
                            ProgressView().controlSize(.small)
                        }
                        Text("Verify & Unlock")
                            .font(.system(size: 13, weight: .bold))
                    }
                    .foregroundColor(.white)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 10)
                    .background(StudioTheme.bixelGreen, in: RoundedRectangle(cornerRadius: 8))
                }
                .buttonStyle(.plain)
                .disabled(restoreOrderIDInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
            .padding(24)
        }
        .frame(width: 400, height: 260)
    }

    private func featureRow(icon: String, title: String, description: String) -> some View {
        HStack(alignment: .top, spacing: 14) {
            Image(systemName: icon)
                .font(.system(size: 16, weight: .semibold))
                .foregroundColor(Color(red: 0.25, green: 0.65, blue: 1.0))
                .frame(width: 22)

            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundColor(.white)
                Text(description)
                    .font(.system(size: 11))
                    .foregroundColor(StudioTheme.textSecondary)
            }
            Spacer()
        }
    }
}
