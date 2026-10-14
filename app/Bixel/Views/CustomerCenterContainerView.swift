// CustomerCenterContainerView.swift
//
// Customer Center & Purchases presentation for Bixel Studio.
// Provides self-service purchase management, real-time PayPal verification,
// and cache clearing.

import SwiftUI

struct CustomerCenterContainerView: View {
    @ObservedObject private var subscriptionManager = SubscriptionManager.shared
    @Environment(\.dismiss) private var dismiss

    @State private var showRestorePrompt = false
    @State private var orderIDInput = ""
    @State private var showPaywall = false

    var body: some View {
        ZStack {
            StudioTheme.homeDark
                .ignoresSafeArea()

            VStack(spacing: 20) {
                // Header
                HStack {
                    Text("Purchases & Account")
                        .font(.system(size: 18, weight: .bold, design: .rounded))
                        .foregroundColor(.white)
                    Spacer()
                    Button {
                        dismiss()
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(.system(size: 18))
                            .foregroundColor(Color.white.opacity(0.6))
                    }
                    .buttonStyle(.plain)
                }

                // Current Plan Card
                VStack(alignment: .leading, spacing: 14) {
                    HStack {
                        VStack(alignment: .leading, spacing: 4) {
                            Text("Current Plan")
                                .font(.system(size: 11, weight: .medium))
                                .foregroundColor(StudioTheme.textSecondary)
                            Text(subscriptionManager.isAdFree ? "Lifetime Ad-Free (Unlocked)" : "Free Tier (Supported by Ads)")
                                .font(.system(size: 15, weight: .semibold))
                                .foregroundColor(.white)
                        }
                        Spacer()
                        Image(systemName: subscriptionManager.isAdFree ? "checkmark.seal.fill" : "person.crop.circle")
                            .font(.system(size: 26))
                            .foregroundColor(subscriptionManager.isAdFree ? StudioTheme.bixelGreen : StudioTheme.textDisabled)
                    }

                    Divider()
                        .background(StudioTheme.homeCardBorder)

                    // Details
                    HStack {
                        Text("Payment Rails")
                            .font(.system(size: 12))
                            .foregroundColor(StudioTheme.textSecondary)
                        Spacer()
                        Text("PayPal / Fastlane")
                            .font(.system(size: 12, weight: .medium))
                            .foregroundColor(.white)
                    }

                    HStack {
                        Text("Order ID")
                            .font(.system(size: 12))
                            .foregroundColor(StudioTheme.textSecondary)
                        Spacer()
                        if let orderID = subscriptionManager.lastPayPalOrderID {
                            Text(orderID)
                                .font(.system(size: 12, design: .monospaced))
                                .foregroundColor(StudioTheme.bixelGreen)
                        } else {
                            Text("Not linked")
                                .font(.system(size: 12))
                                .foregroundColor(StudioTheme.textDisabled)
                        }
                    }

                    HStack {
                        Text("Environment")
                            .font(.system(size: 12))
                            .foregroundColor(StudioTheme.textSecondary)
                        Spacer()
                        Text(subscriptionManager.environment == .live ? "Production (Live)" : "Sandbox")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundColor(subscriptionManager.environment == .live ? StudioTheme.bixelGreen : Color.yellow)
                    }
                }
                .padding(16)
                .background(StudioTheme.homeCard, in: RoundedRectangle(cornerRadius: 12))
                .overlay(
                    RoundedRectangle(cornerRadius: 12)
                        .strokeBorder(StudioTheme.homeCardBorder, lineWidth: 1)
                )

                if let notice = subscriptionManager.statusNotice {
                    Text(notice)
                        .font(.system(size: 12))
                        .foregroundColor(StudioTheme.bixelGreen)
                        .multilineTextAlignment(.center)
                }

                if let error = subscriptionManager.errorMessage {
                    Text(error)
                        .font(.system(size: 12))
                        .foregroundColor(.red)
                        .multilineTextAlignment(.center)
                }

                Spacer()

                // Actions
                VStack(spacing: 10) {
                    if !subscriptionManager.isAdFree {
                        Button {
                            showPaywall = true
                        } label: {
                            HStack {
                                Image(systemName: "crown.fill")
                                Text("Upgrade to Lifetime ($\(SubscriptionManager.lifetimePriceUSD))")
                                    .font(.system(size: 13, weight: .bold))
                            }
                            .foregroundColor(.white)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 10)
                            .background(
                                LinearGradient(
                                    colors: [Color(red: 0.0, green: 0.45, blue: 0.85), Color(red: 0.0, green: 0.3, blue: 0.7)],
                                    startPoint: .leading,
                                    endPoint: .trailing
                                ),
                                in: RoundedRectangle(cornerRadius: 8)
                            )
                        }
                        .buttonStyle(.plain)
                    }

                    HStack(spacing: 10) {
                        Button {
                            Task {
                                await subscriptionManager.restorePurchases()
                            }
                        } label: {
                            HStack {
                                if subscriptionManager.isRestoring {
                                    ProgressView().controlSize(.small)
                                }
                                Text("Verify / Restore")
                                    .font(.system(size: 12, weight: .medium))
                                    .foregroundColor(.white)
                            }
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 9)
                            .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
                        }
                        .buttonStyle(.plain)
                        .disabled(subscriptionManager.isRestoring)

                        Button {
                            subscriptionManager.clearCache()
                        } label: {
                            HStack {
                                Image(systemName: "trash")
                                Text("Clear Cache")
                                    .font(.system(size: 12, weight: .medium))
                            }
                            .foregroundColor(Color.red.opacity(0.85))
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 9)
                            .background(Color.red.opacity(0.12), in: RoundedRectangle(cornerRadius: 8))
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
            .padding(28)
        }
        .frame(width: 480, height: 480)
        .onAppear {
            Task {
                await subscriptionManager.verifyEntitlementOnLaunch()
            }
        }
        .sheet(isPresented: $showPaywall) {
            PaywallContainerView()
        }
    }
}
