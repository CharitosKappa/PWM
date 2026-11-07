package pwm.app

import android.webkit.WebView
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import android.view.autofill.AutofillManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringArrayResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.compose.LifecycleResumeEffect
import pwm.kit.BiometricState
import pwm.kit.PwmError
import pwm.kit.design.Tokens
import pwm.kit.ui.AppModel
import pwm.kit.ui.GeneratorModel
import pwm.kit.ui.LockTimeout
import pwm.kit.ui.SettingsModel

// --- The generator tab (D43 §2) ---

@Composable
internal fun GeneratorTab(model: GeneratorModel) {
    Page(stringResource(R.string.tab_generator), null, tag = "generatorScreen") {
        GeneratorOptions(model)
    }
}

/**
 * The value, its strength, Copy and Regenerate, then [actions] (the sheet's
 * "Use this password", kept near the top so a small screen shows it), then
 * the options; for the tab and the password field's sheet.
 */
@Composable
internal fun GeneratorOptions(model: GeneratorModel, actions: @Composable () -> Unit = {}) {
    val state by model.state.collectAsState()
    Surface(color = color(Tokens.Color.surface), shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth()) {
        Text(
            state.value?.value ?: " ", style = Tokens.Text.secret.compose(), color = color(Tokens.Color.text),
            modifier = Modifier.padding(Tokens.Space.s4.dp).testTag("generated"),
        )
    }
    StrengthMeter(state.strength?.score)
    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s2.dp)) {
        OutlinedButton(onClick = model::copy, enabled = state.value != null, modifier = Modifier.testTag("copyGenerated")) {
            Text(stringResource(if (state.copied) R.string.copied else R.string.copy))
        }
        OutlinedButton(onClick = model::regenerate, modifier = Modifier.testTag("regenerate")) { Text(stringResource(R.string.regenerate)) }
    }
    ErrorLine(state.error)
    actions()
    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s2.dp)) {
        for ((mode, label) in listOf(GeneratorModel.Mode.PASSWORD to R.string.mode_password, GeneratorModel.Mode.PASSPHRASE to R.string.mode_passphrase)) {
            FilterChip(selected = state.mode == mode, onClick = { model.options { it.copy(mode = mode) } }, label = { Text(stringResource(label)) })
        }
    }
    when (state.mode) {
        GeneratorModel.Mode.PASSWORD -> {
            Text(stringResource(R.string.length, state.length), style = Tokens.Text.body.compose())
            val lengths = GeneratorModel.LENGTHS
            Slider(
                value = state.length.toFloat(),
                onValueChange = { v -> model.options { it.copy(length = v.toInt()) } },
                valueRange = lengths.first.toFloat()..lengths.last.toFloat(),
                steps = lengths.last - lengths.first - 1,
            )
            Toggle(stringResource(R.string.lowercase), state.lowercase) { v -> model.options { it.copy(lowercase = v) } }
            Toggle(stringResource(R.string.uppercase), state.uppercase) { v -> model.options { it.copy(uppercase = v) } }
            Toggle(stringResource(R.string.digits), state.digits) { v -> model.options { it.copy(digits = v) } }
            Toggle(stringResource(R.string.symbols), state.symbols) { v -> model.options { it.copy(symbols = v) } }
            Toggle(stringResource(R.string.avoid_lookalikes), state.avoidLookalikes) { v -> model.options { it.copy(avoidLookalikes = v) } }
        }
        GeneratorModel.Mode.PASSPHRASE -> {
            Text(stringResource(R.string.words, state.words), style = Tokens.Text.body.compose())
            val counts = GeneratorModel.WORD_COUNTS
            Slider(
                value = state.words.toFloat(),
                onValueChange = { v -> model.options { it.copy(words = v.toInt()) } },
                valueRange = counts.first.toFloat()..counts.last.toFloat(),
                steps = counts.last - counts.first - 1,
            )
            Text(stringResource(R.string.separator), style = Tokens.Text.body.compose())
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s2.dp)) {
                for (separator in GeneratorModel.SEPARATORS) {
                    FilterChip(
                        selected = state.separator == separator, onClick = { model.options { it.copy(separator = separator) } },
                        label = { Text(if (separator == " ") stringResource(R.string.separator_space) else separator) },
                    )
                }
            }
            Toggle(stringResource(R.string.capitalize), state.capitalize) { v -> model.options { it.copy(capitalize = v) } }
            Toggle(stringResource(R.string.add_digit), state.addDigit) { v -> model.options { it.copy(addDigit = v) } }
        }
    }
}

@Composable
private fun Toggle(label: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
        Text(label, style = Tokens.Text.body.compose(), color = color(Tokens.Color.text), modifier = Modifier.weight(1f))
        Switch(checked = checked, onCheckedChange = onChange)
    }
}

// --- The Settings tab (D43 §2) ---

@Composable
internal fun SettingsTab(app: AppModel, model: SettingsModel) {
    val state by model.state.collectAsState()
    when (state.page) {
        SettingsModel.Page.NOTICES -> return Notices(model)
        SettingsModel.Page.DEVICES -> return state.devices?.let { DevicesPage(model, it) } ?: Unit
        SettingsModel.Page.APPROVALS -> return state.approvals?.let { ApprovalsPage(model, it) } ?: Unit
        SettingsModel.Page.HEALTH -> return state.health?.let { HealthPage(model, it) } ?: Unit
        SettingsModel.Page.EXPORT -> return state.export?.let { ExportPage(app, model, it) } ?: Unit
        SettingsModel.Page.IMPORT -> return state.importing?.let { ImportPage(app, model, it) } ?: Unit
        SettingsModel.Page.MAIN -> Unit
    }
    LaunchedEffect(model) { model.prepareKit() }
    if (state.signOutAsked) {
        AlertDialog(
            onDismissRequest = model::keepSignedIn,
            text = { Text(stringResource(R.string.sign_out_ask)) },
            confirmButton = {
                TextButton(onClick = model::signOut, modifier = Modifier.testTag("confirmSignOut")) {
                    Text(stringResource(R.string.sign_out), color = color(Tokens.Color.danger))
                }
            },
            dismissButton = { TextButton(onClick = model::keepSignedIn) { Text(stringResource(R.string.cancel)) } },
        )
    }
    val context = LocalContext.current
    val version = remember {
        runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull() ?: ""
    }
    Page(stringResource(R.string.tab_settings), null, tag = "settingsScreen") {
        AutofillCard()
        PasskeyCard()
        Heading(stringResource(R.string.account))
        Text(state.email ?: "", style = Tokens.Text.body.compose(), color = color(Tokens.Color.text))
        Text(
            "${stringResource(R.string.server)}: ${state.server ?: stringResource(R.string.hosted_server)}",
            style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted),
        )

        // The plan and where it's managed (D6).
        Heading(stringResource(R.string.plan))
        PlanSection(model.plan)

        Heading(stringResource(R.string.lock_timeout))
        val names = stringArrayResource(R.array.lock_timeouts)
        LockTimeout.entries.forEachIndexed { i, timeout ->
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth().clickable { model.setLockTimeout(timeout) }.testTag("lockTimeout.${timeout.name}"),
            ) {
                RadioButton(selected = state.lockTimeout == timeout, onClick = { model.setLockTimeout(timeout) })
                Text(names[i], style = Tokens.Text.body.compose(), color = color(Tokens.Color.text))
            }
        }
        Text(stringResource(R.string.lock_timeout_hint), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
        Secondary(stringResource(R.string.lock_now), tag = "lockNow", onClick = model::lockNow)

        // Screenshots and screen sharing (D56 §2).
        Heading(stringResource(R.string.hide_from_capture))
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
            Text(
                stringResource(
                    if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU) R.string.hide_from_capture_hint
                    else R.string.hide_from_capture_hint_old,
                ),
                style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted), modifier = Modifier.weight(1f),
            )
            Switch(
                checked = state.hideFromCapture, onCheckedChange = model::setHideFromCapture,
                modifier = Modifier.testTag("hideFromCaptureSwitch"),
            )
        }

        // Fingerprint and face unlock (D55): the switch, or why it can't be used.
        Heading(stringResource(R.string.biometric_switch))
        when (val biometric = state.biometric) {
            is BiometricState.Unavailable -> Text(
                PwmError(biometric.code).userMessage, style = Tokens.Text.body.compose(), color = color(Tokens.Color.textMuted),
                modifier = Modifier.testTag("biometricUnavailable"),
            )
            BiometricState.On, BiometricState.Off -> Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.biometric_hint), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted), modifier = Modifier.weight(1f))
                Switch(
                    checked = biometric == BiometricState.On, onCheckedChange = model::setBiometric, enabled = !state.biometricBusy,
                    modifier = Modifier.testTag("biometricSwitch"),
                )
            }
            null -> Unit
        }

        Heading(stringResource(R.string.emergency_kit))
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s2.dp), modifier = Modifier.fillMaxWidth()) {
            KitButtons(app, state.kit?.value, onSaved = model::didSaveKit, onFailed = model::saveFailed, modifier = Modifier.weight(1f))
        }
        if (state.kitSaved) Text(stringResource(R.string.kit_saved), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.success))
        ErrorLine(state.error)

        // The data pages (D59 §5-7); export is on every plan.
        Heading(stringResource(R.string.your_data))
        Secondary(stringResource(R.string.password_health), tag = "health", onClick = model::showHealth)
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.breach_check_switch), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text))
                Text(stringResource(R.string.breach_check_hint), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
            }
            Switch(checked = state.breachCheck, onCheckedChange = model::setBreachCheck, modifier = Modifier.testTag("breachCheckSwitch"))
        }
        Secondary(stringResource(R.string.export_title), tag = "export", onClick = model::showExport)
        Secondary(stringResource(R.string.import_title), tag = "import", onClick = model::showImport)

        Heading(stringResource(R.string.devices))
        Secondary(stringResource(R.string.devices), tag = "devices", onClick = model::showDevices)
        Secondary(stringResource(R.string.approve_device), tag = "approvals", onClick = model::showApprovals)
        if (state.pushAvailable) ApprovalNoticesSwitch(model, state)

        Secondary(stringResource(R.string.notices), tag = "notices", onClick = model::showNotices)
        Secondary(stringResource(R.string.sign_out), tag = "signOut", onClick = model::askSignOut)
        Text(stringResource(R.string.version, version), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
    }
}

@Composable
private fun Heading(text: String) {
    Text(text, style = Tokens.Text.title.compose(), color = color(Tokens.Color.text), modifier = Modifier.padding(top = Tokens.Space.s3.dp))
}

/**
 * Third-party notices (D54 §7), bundled under `assets/notices/` by the
 * build. Shown in a WebView that runs no JavaScript and loads nothing from
 * the network: only the app's own files.
 */
@Composable
private fun Notices(model: SettingsModel) {
    val context = LocalContext.current
    val files = remember { runCatching { context.assets.list("notices")?.sorted() }.getOrNull().orEmpty() }
    var open by remember { mutableStateOf<String?>(null) }
    Column(Modifier.fillMaxSize().testTag("noticesScreen")) {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(Tokens.Space.s3.dp)) {
            TextButton(onClick = { if (open != null) open = null else model.back() }) { Text(stringResource(R.string.back)) }
            Text(stringResource(R.string.notices), style = Tokens.Text.title.compose(), color = color(Tokens.Color.text))
        }
        val file = open
        when {
            files.isEmpty() -> Text(
                stringResource(R.string.notices_missing), style = Tokens.Text.body.compose(), color = color(Tokens.Color.textMuted),
                modifier = Modifier.padding(Tokens.Space.s5.dp),
            )
            file == null -> for (name in files) {
                Text(
                    name, style = Tokens.Text.body.compose(), color = color(Tokens.Color.accent),
                    modifier = Modifier.fillMaxWidth().clickable { open = name }.padding(Tokens.Space.s4.dp),
                )
            }
            else -> AndroidView(
                factory = { ctx ->
                    WebView(ctx).apply {
                        settings.javaScriptEnabled = false
                        settings.blockNetworkLoads = true
                        settings.allowContentAccess = false
                        settings.allowFileAccess = false
                        loadUrl("file:///android_asset/notices/$file")
                    }
                },
                modifier = Modifier.fillMaxSize(),
            )
        }
    }
}

/**
 * "Turn on PWM for autofill" (D58 §2): shown until PWM is the autofill
 * service, with a button to the system's choice. No prompt anywhere else.
 */
@Composable
private fun AutofillCard() {
    val context = LocalContext.current
    val manager = remember { context.getSystemService(AutofillManager::class.java) }
    var enabled by remember { mutableStateOf(true) }
    // Checked again each time Settings comes back, as after the system's screen.
    LifecycleResumeEffect(Unit) {
        enabled = manager == null || !manager.isAutofillSupported || manager.hasEnabledAutofillServices()
        onPauseOrDispose {}
    }
    if (enabled) return
    Surface(color = color(Tokens.Color.surface), shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth().testTag("autofillCard")) {
        Column(Modifier.padding(Tokens.Space.s4.dp), verticalArrangement = Arrangement.spacedBy(Tokens.Space.s2.dp)) {
            Text(stringResource(R.string.autofill_card), style = Tokens.Text.bodyStrong.compose(), color = color(Tokens.Color.text))
            Text(stringResource(R.string.autofill_card_hint), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
            OutlinedButton(onClick = {
                runCatching {
                    context.startActivity(
                        Intent(Settings.ACTION_REQUEST_SET_AUTOFILL_SERVICE, Uri.parse("package:${context.packageName}")),
                    )
                }
            }) { Text(stringResource(R.string.autofill_turn_on)) }
        }
    }
}

/**
 * "Turn on PWM for passkeys" (D61 §8), Android 14+: the system's preferred
 * service choice turns on both autofill and the credential provider, so this
 * shows only when autofill is PWM's and the provider was turned off on its own.
 */
@Composable
private fun PasskeyCard() {
    if (android.os.Build.VERSION.SDK_INT < 34) return
    val context = LocalContext.current
    var shown by remember { mutableStateOf(false) }
    LifecycleResumeEffect(Unit) {
        shown = PasskeySettings.autofillOnProviderOff(context)
        onPauseOrDispose {}
    }
    if (!shown) return
    Surface(color = color(Tokens.Color.surface), shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth().testTag("passkeyCard")) {
        Column(Modifier.padding(Tokens.Space.s4.dp), verticalArrangement = Arrangement.spacedBy(Tokens.Space.s2.dp)) {
            Text(stringResource(R.string.passkey_card), style = Tokens.Text.bodyStrong.compose(), color = color(Tokens.Color.text))
            Text(stringResource(R.string.passkey_card_hint), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
            OutlinedButton(onClick = { PasskeySettings.open(context) }) { Text(stringResource(R.string.autofill_turn_on)) }
        }
    }
}

/**
 * "Notify me when a new device asks to sign in" (D60 §4): off until turned
 * on, and only then does Android ask for the permission (13+). A refusal
 * leaves it off and says where to allow it.
 */
@Composable
private fun ApprovalNoticesSwitch(model: SettingsModel, state: SettingsModel.State) {
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        model.noticePermissionAnswered(granted)
    }
    LaunchedEffect(state.askNoticePermission) {
        if (!state.askNoticePermission) return@LaunchedEffect
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU) {
            ask.launch(android.Manifest.permission.POST_NOTIFICATIONS)
        } else {
            // Below 13 there's nothing to ask: notifications are off in system settings.
            model.noticePermissionAnswered(false)
        }
    }
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
        Text(stringResource(R.string.approval_notices), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text), modifier = Modifier.weight(1f))
        Switch(checked = state.approvalNotices, onCheckedChange = model::setApprovalNotices, modifier = Modifier.testTag("approvalNoticesSwitch"))
    }
    if (state.noticesRefused) {
        Text(stringResource(R.string.approval_notices_refused), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.warning), modifier = Modifier.testTag("noticesRefused"))
    }
}
