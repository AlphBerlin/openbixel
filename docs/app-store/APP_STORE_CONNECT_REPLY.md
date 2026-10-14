# App Store Connect — Review Resolution Center Response

Copy and paste the following reply into App Store Connect under **Resolution Center** for Submission **baef6394-dcb8-468b-b92d-6df58be1c774**:

---

Dear App Review Team,

Thank you for your review and feedback regarding OpenBixel (Version 1.0). We have thoroughly addressed both rejection items (Guideline 5.0.0 and Guideline 2.1(b)) and uploaded a new binary for your review.

---

### 1. Guideline 5 - Legal: Deep Synthesis Technologies / China Mainland Compliance

In accordance with local regulations and Apple's guidelines regarding deep synthesis and generative AI technologies:

1. **Metadata Sanitization**: All references to "ChatGPT", "GPT", and "OpenAI" have been completely removed from all App Store metadata fields, including promotional text, app description, feature lists, and reviewer notes.
2. **Runtime Storefront Gating / Suppression**: We integrated StoreKit's storefront detection (`Storefront.current?.countryCode == "CHN"` and `SKPaymentQueue.default().storefront?.countryCode == "CHN"`). When the app is accessed on the China mainland storefront:
   - ChatGPT / Codex provider functionality is completely deactivated and suppressed from Settings and the AI panel.
   - Users cannot connect or sign in to ChatGPT services in China.
   - Any prior connection is automatically switched to a neutral local or custom API configuration.
3. **App Store Connect Availability**: As advised, we have also ensured that if China mainland distribution is deselected in App Store Connect (**Pricing and Availability → Availability → Manage Availability**), the app remains compliant worldwide without altering functionality outside of China.

---

### 2. Guideline 2.1(b) - Performance: In-App Purchase "Remove Ads"

We investigated the issue where ads continued to be displayed after purchasing "Remove Ads" in the sandbox environment, and resolved all underlying causes:

1. **StoreKit 2 Sandbox Transaction Verification**:
   - In macOS sandbox testing, Apple transactions frequently return `.unverified` because sandbox intermediate certificates are evaluated locally.
   - The app now extracts the underlying payload (`verification.unsafePayloadValue`), verifying that the transaction belongs to our product (`bixel_ad_free` or bundle-prefixed variants) and has not been revoked (`transaction.revocationDate == nil`). This ensures sandbox test purchases are recognized immediately upon completion.
2. **Immediate & Unconditional Paywall Unlock**:
   - In `PaywallContainerView`, the completion callback now immediately grants the `ad_free` lifetime entitlement and closes the sheet. It no longer relies on delayed third-party server synchronization, preventing any race condition where an incomplete cached record could falsely report unentitled status.
3. **Permanent Non-Consumable Entitlement Retention**:
   - Lifetime Remove Ads is a non-consumable purchase that never expires. We updated entitlement synchronization so that an existing ad-free status is never revoked due to network latency or server timeouts. It can only ever be revoked if StoreKit explicitly issues a revocation record (`transaction.revocationDate != nil`).
4. **Immediate Ad Surface Tear-down**:
   - Setting `isAdFree = true` synchronously updates `AdManager.shared.shouldShowAds = false`, clears the ad inventory, cancels rotation timers, and tears down all ad surfaces (`AdBannerView`, `ProjectLoadingAdView`, and bottom canvas chrome) on the MainActor without delay.

---

### Instructions for Reviewer to Test "Remove Ads":

1. Open OpenBixel and launch or open any canvas project.
2. Click the green **"Remove Ads"** button on the banner at the bottom of the canvas, or open the action menu (wrench / crown) and select **"Unlock Lifetime Ad-Free…"**.
3. Complete the purchase in the sandbox environment.
4. When the Apple Sandbox confirmation alert (*"You're all set. Your purchase was successful."*) appears, dismiss it.
5. **Result**: The ad banner at the bottom disappears immediately, the "Remove Ads" button is gone, interstitial loading ads are completely skipped when opening projects, and the app reflects Lifetime Ad-Free status across all windows and future launches.
6. Selecting **"Restore Purchases"** in the menu also verifies the entitlement directly with StoreKit and keeps ad-free active.

Thank you again for your assistance. Please let us know if you have any questions or require further information.

Best regards,  
Ajith Berlin A  
OpenBixel Development Team
