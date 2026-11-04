// Generated from design/messages.json by scripts/messages-kotlin.mjs. Do not edit.
package pwm.kit

/** What to show for each of the core's error codes; never a value. */
internal object Messages {
    const val FALLBACK = "Something went wrong. Try again."

    val byCode: Map<String, String> = mapOf(
        "wrong_password" to "Wrong master password.",
        "invalid_credentials" to "Wrong email, master password or Secret Key.",
        "invalid_secret_key" to "That Secret Key is not valid. Check it on your Emergency Kit.",
        "weak_password" to "Choose a stronger master password.",
        "network" to "Can't reach the server. Check your connection.",
        "rate_limited" to "Too many tries. Wait a minute and try again.",
        "account_exists" to "There is already an account with this email.",
        "server" to "The server had a problem. Try again later.",
        "device_revoked" to "This device was signed out from another device.",
        "signed_out" to "You were signed out. Sign in again.",
        "locked" to "Unlock first.",
        "busy" to "Still working on the last request.",
        "read_only_item" to "This item was made by a newer version of the app. Update to edit it.",
        "item_too_large" to "This item is too large to save.",
        "invalid_item" to "This item could not be saved.",
        "invalid_totp" to "That's not a one-time code setup key. Paste the key or scan the site's QR code.",
        "unknown_item" to "This item was deleted.",
        "unknown_vault" to "This device has no vault yet. Pull down to sync.",
        "invalid_expiry_month" to "Enter the expiry month as a number from 1 to 12.",
        "invalid_expiry_year" to "Enter the expiry year with 4 digits.",
        "no_characters" to "Choose at least one kind of character.",
        "unknown_device" to "That device was removed already.",
        "approval_ended" to "The request ended. Ask again, or sign in with your Secret Key.",
        "unknown_approval" to "That request ended. Ask the new device to try again.",
        "approval_failed" to "The approval didn't arrive safely. Sign in with your Secret Key.",
        "biometric_cancelled" to "Unlock with your master password, or try again.",
        "biometric_changed" to "Face ID or Touch ID changed. Enter your master password to turn it back on.",
        "biometric_unavailable" to "Face ID or Touch ID is not available on this device.",
        "biometric_unlock_failed" to "Enter your master password.",
        "master_password_due" to "Enter your master password. It's asked every 14 days so you don't forget it.",
        "invalid_device_name" to "Use a shorter name, up to 63 characters.",
        "breach_check" to "The leak check is not answering. Try again later.",
        "export_not_authorized" to "Enter your master password to export.",
        "export_expired" to "Enter your master password again to export.",
        "weak_export_password" to "Choose a stronger export password.",
        "not_signed_in" to "Sign in on this device first.",
        "storage" to "The vault on this device could not be opened.",
        "keychain" to "The keychain refused to store this device's keys.",
        "invalid_email" to "Enter a valid email address.",
        "invalid_server" to "Enter the server's address, starting with https://.",
        "not_a_kit" to "That's not an Emergency Kit code.",
        "kit_too_new" to "This Emergency Kit is from a newer version of the app. Update the app to scan it.",
        "camera" to "Allow camera access in Settings to scan your Emergency Kit.",
    )
}
