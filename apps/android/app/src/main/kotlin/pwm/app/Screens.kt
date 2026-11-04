package pwm.app

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringArrayResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import pwm.kit.BiometricState
import pwm.kit.design.Tokens
import pwm.kit.ui.AppModel
import pwm.kit.ui.CreateAccountModel
import pwm.kit.ui.SecretKeyModel
import pwm.kit.ui.SignInModel
import pwm.kit.ui.UnlockModel

// The onboarding screens (D53 §2), the same as the Apple app's (D42 §4).
// Each field and button has a test tag for the UI test.

@Composable
fun AppScreen(app: AppModel) {
    val state by app.state.collectAsState()
    val screen = state.screen
    BackHandler(enabled = screen is AppModel.Screen.CreateAccount || screen is AppModel.Screen.SignIn) { app.back() }
    Surface(color = color(Tokens.Color.bg), modifier = Modifier.fillMaxSize()) {
        when (screen) {
            AppModel.Screen.Loading -> Unit
            AppModel.Screen.Welcome -> Welcome(app, state)
            is AppModel.Screen.CreateAccount -> CreateAccount(app, screen.model)
            is AppModel.Screen.SecretKey -> SecretKey(app, screen.model)
            is AppModel.Screen.SignIn -> SignIn(app, state, screen.model)
            is AppModel.Screen.Unlock -> Unlock(screen.model)
            is AppModel.Screen.Vault -> Home(app, screen.model)
        }
    }
}

@Composable
internal fun Page(title: String, subtitle: String?, tag: String, content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .safeDrawingPadding()
            .imePadding()
            .verticalScroll(rememberScrollState())
            .padding(Tokens.Space.s5.dp)
            .testTag(tag),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.s3.dp),
    ) {
        Text(title, style = Tokens.Text.display.compose(), color = color(Tokens.Color.text))
        if (subtitle != null) Text(subtitle, style = Tokens.Text.body.compose(), color = color(Tokens.Color.textMuted))
        Spacer(Modifier.height(Tokens.Space.s2.dp))
        content()
    }
}

@Composable
internal fun ErrorLine(error: String?) {
    if (error != null) {
        Text(error, style = Tokens.Text.body.compose(), color = color(Tokens.Color.danger), modifier = Modifier.testTag("error"))
    }
}

@Composable
internal fun Field(
    label: String,
    value: String,
    onChange: (String) -> Unit,
    tag: String,
    secret: Boolean = false,
    keyboard: KeyboardType = KeyboardType.Text,
    enabled: Boolean = true,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        label = { Text(label) },
        singleLine = true,
        enabled = enabled,
        visualTransformation = if (secret) PasswordVisualTransformation() else VisualTransformation.None,
        keyboardOptions = KeyboardOptions(keyboardType = if (secret) KeyboardType.Password else keyboard, autoCorrectEnabled = false),
        modifier = Modifier.fillMaxWidth().testTag(tag),
    )
}

@Composable
internal fun Primary(title: String, enabled: Boolean, tag: String, onClick: () -> Unit) {
    Button(onClick = onClick, enabled = enabled, modifier = Modifier.fillMaxWidth().testTag(tag)) { Text(title) }
}

@Composable
internal fun Secondary(title: String, tag: String, enabled: Boolean = true, onClick: () -> Unit) {
    TextButton(onClick = onClick, enabled = enabled, modifier = Modifier.fillMaxWidth().testTag(tag)) { Text(title) }
}

// --- Welcome ---

@Composable
private fun Welcome(app: AppModel, state: AppModel.State) {
    Page(stringResource(R.string.welcome_title), stringResource(R.string.welcome_subtitle), tag = "welcome") {
        Primary(stringResource(R.string.create_account), enabled = true, tag = "createAccount") { app.showCreateAccount() }
        Secondary(stringResource(R.string.sign_in), tag = "signIn") { app.showSignIn() }
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
            Text(stringResource(R.string.other_server), style = Tokens.Text.bodyStrong.compose(), modifier = Modifier.weight(1f))
            Switch(
                checked = state.usesOtherServer,
                onCheckedChange = app::setUsesOtherServer,
                modifier = Modifier.testTag("otherServerSwitch"),
            )
        }
        if (state.usesOtherServer) {
            Field(
                stringResource(R.string.server_placeholder), state.otherServer, app::setOtherServer, tag = "otherServer",
                keyboard = KeyboardType.Uri,
            )
        }
        Text(stringResource(R.string.other_server_hint), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
        ErrorLine(state.error)
    }
}

// --- Create account ---

@Composable
private fun CreateAccount(app: AppModel, model: CreateAccountModel) {
    val state by model.state.collectAsState()
    Page(stringResource(R.string.create_account), stringResource(R.string.create_subtitle), tag = "createAccountScreen") {
        Field(stringResource(R.string.email), state.email, model::setEmail, tag = "email", keyboard = KeyboardType.Email)
        Field(stringResource(R.string.master_password), state.password.value, model::setPassword, tag = "password", secret = true)
        StrengthMeter(state.strength?.score)
        Field(stringResource(R.string.master_password_again), state.confirm.value, model::setConfirm, tag = "confirm", secret = true)
        if (state.confirm.value.isNotEmpty() && !state.passwordsMatch) {
            Text(stringResource(R.string.passwords_differ), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.danger))
        }
        ErrorLine(state.error)
        Primary(
            stringResource(if (state.busy) R.string.creating else R.string.create_account),
            enabled = state.canSubmit, tag = "create", onClick = model::submit,
        )
        Secondary(stringResource(R.string.back), tag = "back", enabled = !state.busy) { app.showWelcome() }
    }
}

@Composable
internal fun StrengthMeter(score: Int?) {
    val words = stringArrayResource(R.array.strength_words)
    val shown = score?.coerceIn(0, 4)
    val word = shown?.let { words[it] } ?: ""
    val tint = when {
        shown == null -> color(Tokens.Color.border)
        shown >= CreateAccountModel.MINIMUM_SCORE -> color(Tokens.Color.success)
        shown == 2 -> color(Tokens.Color.warning)
        else -> color(Tokens.Color.danger)
    }
    Column(Modifier.semantics { contentDescription = word }.testTag("strength")) {
        LinearProgressIndicator(
            progress = { if (shown == null) 0f else (shown + 1) / 5f },
            color = tint,
            trackColor = color(Tokens.Color.surface),
            modifier = Modifier.fillMaxWidth(),
        )
        if (shown != null) {
            Text(stringResource(R.string.strength, word), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
        }
    }
}

// --- The Secret Key ---

@Composable
private fun SecretKey(app: AppModel, model: SecretKeyModel) {
    val state by model.state.collectAsState()
    // No screenshots or recordings of it: the Activity sets that for this screen (D56 §1).
    Page(stringResource(R.string.secret_key_title), stringResource(R.string.secret_key_subtitle), tag = "secretKeyScreen") {
        Surface(color = color(Tokens.Color.surface), shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth()) {
            Text(
                model.secretKey.value,
                // The secret style, at headline size, so it reads well off the screen.
                style = Tokens.Text.secret.compose().copy(fontSize = Tokens.Text.headline.size.sp),
                color = color(Tokens.Color.text),
                modifier = Modifier.padding(Tokens.Space.s4.dp).testTag("secretKey"),
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s2.dp), modifier = Modifier.fillMaxWidth()) {
            KitButtons(app, state.kit?.value, onSaved = model::didSaveKit, onFailed = model::saveFailed, modifier = Modifier.weight(1f))
            OutlinedButton(onClick = model::copy, modifier = Modifier.testTag("copy")) {
                Text(stringResource(if (state.copied) R.string.copied else R.string.copy))
            }
        }
        Text(
            stringResource(if (state.copied) R.string.clipboard_clears else R.string.keep_it_offline),
            style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted),
        )
        ErrorLine(state.error)
        Primary(stringResource(R.string.i_saved_it), enabled = state.canContinue, tag = "saved", onClick = model::done)
    }
}

/**
 * Save Emergency Kit and Print (D53 §2), for onboarding and Settings. The
 * app is told first, since both screens put it in the background (D54 §5).
 */
@Composable
internal fun KitButtons(app: AppModel, kit: ByteArray?, onSaved: () -> Unit, onFailed: () -> Unit, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/pdf")) { uri ->
        if (uri == null || kit == null) return@rememberLauncherForActivityResult
        scope.launch {
            val written = withContext(Dispatchers.IO) { KitFiles.write(context, uri, kit) }
            if (written) onSaved() else onFailed()
        }
    }
    val fileName = stringResource(R.string.kit_file_name)
    val jobName = stringResource(R.string.kit_print_job)
    OutlinedButton(
        onClick = {
            app.openingSystemScreen()
            save.launch(fileName)
        },
        enabled = kit != null,
        modifier = modifier.testTag("saveKit"),
    ) { Text(stringResource(R.string.save_kit)) }
    OutlinedButton(
        onClick = {
            if (kit != null) {
                app.openingSystemScreen()
                KitFiles.print(context, jobName, kit, onSaved, onFailed)
            }
        },
        enabled = kit != null,
        modifier = Modifier.testTag("printKit"),
    ) { Text(stringResource(R.string.print_kit)) }
}

// --- Sign in ---

@Composable
private fun SignIn(app: AppModel, appState: AppModel.State, model: SignInModel) {
    val state by model.state.collectAsState()
    val context = LocalContext.current
    var scanning by rememberSaveable { mutableStateOf(false) }
    val hasCamera = context.packageManager.hasSystemFeature(PackageManager.FEATURE_CAMERA_ANY)
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        if (granted) scanning = true else model.cameraRefused()
    }
    if (scanning) {
        BackHandler { scanning = false }
        QrScanner(stringResource(R.string.scan_kit_hint), onCode = { text -> model.scanned(text).also { if (it) scanning = false } }, onClose = { scanning = false })
        return
    }
    Page(stringResource(R.string.sign_in), stringResource(R.string.sign_in_subtitle), tag = "signInScreen") {
        if (hasCamera) {
            OutlinedButton(
                onClick = {
                    val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
                    if (granted) scanning = true else permission.launch(Manifest.permission.CAMERA)
                },
                enabled = !state.busy && !state.asking,
                modifier = Modifier.fillMaxWidth().testTag("scanKit"),
            ) { Text(stringResource(R.string.scan_kit)) }
        }
        Field(stringResource(R.string.email), state.email, model::setEmail, tag = "email", keyboard = KeyboardType.Email, enabled = !state.asking)
        Field(stringResource(R.string.master_password), state.password.value, model::setPassword, tag = "password", secret = true)
        if (state.approved) {
            Text(stringResource(R.string.approved), style = Tokens.Text.body.compose(), color = color(Tokens.Color.success), modifier = Modifier.testTag("approved"))
        } else if (state.asking) {
            Asking(state.approvalCode, model::cancelAsking)
        } else {
            Field(stringResource(R.string.secret_key_label), state.secretKey.value, model::setSecretKey, tag = "secretKey", secret = true)
            Secondary(stringResource(R.string.ask_another_device), tag = "askAnotherDevice", enabled = state.canAsk, onClick = model::startAsking)
        }
        if (appState.usesOtherServer) {
            Text(stringResource(R.string.server_shown, appState.otherServer), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
        }
        ErrorLine(state.error)
        Primary(
            stringResource(if (state.busy) R.string.signing_in else R.string.sign_in),
            enabled = state.canSubmit, tag = "submit", onClick = model::submit,
        )
        Secondary(stringResource(R.string.back), tag = "back", enabled = !state.busy) { app.back() }
    }
}

@Composable
private fun Asking(code: String?, onCancel: () -> Unit) {
    Surface(color = color(Tokens.Color.surface), shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(Tokens.Space.s4.dp), verticalArrangement = Arrangement.spacedBy(Tokens.Space.s2.dp)) {
            Text(stringResource(R.string.ask_instructions), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text))
            if (code != null) {
                Text(stringResource(R.string.ask_check_code), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text))
                Text(code, style = Tokens.Text.display.compose().copy(fontFamily = FontFamily.Monospace), color = color(Tokens.Color.text), modifier = Modifier.testTag("approvalCode"))
            }
            Text(stringResource(R.string.ask_waiting), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
            TextButton(onClick = onCancel, modifier = Modifier.testTag("cancelAsking")) { Text(stringResource(R.string.cancel)) }
        }
    }
}

// --- Unlock ---

@Composable
private fun Unlock(model: UnlockModel) {
    val state by model.state.collectAsState()
    // The finger or face once by itself (D45, D55 §3); a cancel leaves the password.
    LaunchedEffect(model) { model.tryBiometricsOnce() }
    if (state.offer) {
        AlertDialog(
            onDismissRequest = model::declineOffer,
            title = { Text(stringResource(R.string.biometric_offer)) },
            text = { Text(stringResource(R.string.biometric_hint)) },
            confirmButton = {
                TextButton(onClick = model::acceptOffer, modifier = Modifier.testTag("acceptBiometric")) { Text(stringResource(R.string.turn_on)) }
            },
            dismissButton = {
                TextButton(onClick = model::declineOffer, modifier = Modifier.testTag("declineBiometric")) { Text(stringResource(R.string.not_now)) }
            },
        )
    }
    Page(stringResource(R.string.unlock_title), model.email, tag = "unlockScreen") {
        if (state.biometric == BiometricState.On) {
            OutlinedButton(
                onClick = model::unlockWithBiometrics, enabled = state.canUseBiometrics,
                modifier = Modifier.fillMaxWidth().testTag("unlockBiometric"),
            ) { Text(stringResource(R.string.unlock_with_biometric)) }
        }
        Field(stringResource(R.string.master_password), state.password.value, model::setPassword, tag = "password", secret = true)
        ErrorLine(state.error)
        Primary(stringResource(if (state.busy) R.string.unlocking else R.string.unlock), enabled = state.canSubmit, tag = "unlock", onClick = model::submit)
        Secondary(stringResource(R.string.sign_out), tag = "signOut", enabled = !state.busy, onClick = model::signOut)
    }
}
