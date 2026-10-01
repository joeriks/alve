package com.alve.local

import android.content.Intent
import android.os.Bundle
import android.view.WindowManager

class MainActivity : TauriActivity() {
    private var documentPickerActive = false
    private external fun alveOnResume()
    private external fun alveOnStop(documentPickerActive: Boolean)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
    }

    override fun onResume() {
        super.onResume()
        alveOnResume()
    }

    override fun onStop() {
        if (!isChangingConfigurations) alveOnStop(documentPickerActive)
        super.onStop()
    }

    @Suppress("DEPRECATION")
    override fun startActivityForResult(intent: Intent, requestCode: Int, options: Bundle?) {
        val action = if (intent.action == Intent.ACTION_CHOOSER) intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT)?.action else intent.action
        if (action in listOf(Intent.ACTION_CREATE_DOCUMENT, Intent.ACTION_OPEN_DOCUMENT, Intent.ACTION_GET_CONTENT)) {
            documentPickerActive = true
        }
        super.startActivityForResult(intent, requestCode, options)
    }

    @Suppress("DEPRECATION")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        documentPickerActive = false
        super.onActivityResult(requestCode, resultCode, data)
    }
}
