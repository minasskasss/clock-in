package io.github.minasskasss.clockin.alarm

import android.content.Context
import android.content.SharedPreferences
import android.os.UserManager
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The device secret and the local database key (ARCHITECTURE §5.7):
 * AES-256-GCM with a key that never leaves Android Keystore, ciphertext in
 * app-private, **credential-encrypted** storage. Unlike the plan ([Store]),
 * they are unreadable after a reboot until the phone is first unlocked; the
 * pre-alarm check then has no secret and the alarm just rings (fail loud).
 * Rust reads and writes them through the plugin; the background code reads
 * the device secret directly.
 *
 * Nothing here logs or returns a secret in an error.
 */
internal object Secrets {
    private const val FILE = "clockin_secrets"
    private const val ALIAS = "io.github.minasskasss.clockin.secrets"
    private const val TRANSFORMATION = "AES/GCM/NoPadding"
    private const val IV_BYTES = 12
    private const val TAG_BITS = 128

    /** The name Rust stores the device secret under (src-tauri/src/state.rs). */
    const val DEVICE_SECRET = "device-secret"

    /** False after a reboot until the phone is first unlocked. */
    fun available(context: Context): Boolean =
        context.applicationContext.getSystemService(UserManager::class.java)?.isUserUnlocked != false

    /** Credential-encrypted storage, or null while it is still locked. */
    private fun prefs(context: Context): SharedPreferences? =
        if (available(context)) {
            context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)
        } else {
            null
        }

    private fun key(): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (keyStore.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    /** @throws IllegalStateException before the first unlock. */
    fun put(context: Context, name: String, value: String) {
        synchronized(Store.LOCK) {
            val prefs = prefs(context) ?: throw IllegalStateException("locked")
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, key())
            val sealed = cipher.iv + cipher.doFinal(value.toByteArray(Charsets.UTF_8))
            prefs.edit().putString(name, Base64.encodeToString(sealed, Base64.NO_WRAP)).commit()
        }
    }

    /**
     * The secret, or null if there is none, it can't be decrypted, or the
     * phone hasn't been unlocked since it started.
     */
    fun get(context: Context, name: String): String? {
        val stored = prefs(context)?.getString(name, null) ?: return null
        return try {
            val sealed = Base64.decode(stored, Base64.NO_WRAP)
            if (sealed.size <= IV_BYTES) return null
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, sealed, 0, IV_BYTES))
            String(cipher.doFinal(sealed, IV_BYTES, sealed.size - IV_BYTES), Charsets.UTF_8)
        } catch (e: Exception) {
            // Key lost (e.g. restored onto another phone): the caller starts afresh.
            null
        }
    }

    /** @throws IllegalStateException before the first unlock. */
    fun delete(context: Context, name: String) {
        synchronized(Store.LOCK) {
            val prefs = prefs(context) ?: throw IllegalStateException("locked")
            prefs.edit().remove(name).commit()
        }
    }
}
