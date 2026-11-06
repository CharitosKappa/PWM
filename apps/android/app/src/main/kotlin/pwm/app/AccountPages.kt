package pwm.app

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.delay
import pwm.kit.DeviceEntry
import pwm.kit.design.Tokens
import pwm.kit.ui.ApprovalsModel
import pwm.kit.ui.DevicesModel
import pwm.kit.ui.SettingsModel
import pwm.kit.ui.SignInAgainModel

// Settings' account pages (D59 §2-4): devices, approving a new device, and
// the "Sign in again to sync" line over the vault.

private fun date(ms: Long): String = DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(ms))

@Composable
internal fun DevicesPage(settings: SettingsModel, model: DevicesModel) {
    val state by model.state.collectAsState()
    var renaming by remember { mutableStateOf<DeviceEntry?>(null) }
    renaming?.let { device ->
        var name by remember(device.id) { mutableStateOf(device.name ?: "") }
        AlertDialog(
            onDismissRequest = { renaming = null },
            title = { Text(stringResource(R.string.rename_device)) },
            text = { Field(stringResource(R.string.device_name), name, { name = it }, tag = "deviceName") },
            confirmButton = {
                TextButton(onClick = { model.rename(device, name); renaming = null }, enabled = name.isNotBlank(), modifier = Modifier.testTag("confirmRename")) {
                    Text(stringResource(R.string.rename))
                }
            },
            dismissButton = { TextButton(onClick = { renaming = null }) { Text(stringResource(R.string.cancel)) } },
        )
    }
    state.revokeAsked?.let { device ->
        AlertDialog(
            onDismissRequest = model::keepDevice,
            text = { Text(stringResource(R.string.revoke_ask, device.name ?: stringResource(R.string.unknown_device))) },
            confirmButton = {
                TextButton(onClick = model::revoke, modifier = Modifier.testTag("confirmRevoke")) {
                    Text(stringResource(R.string.revoke), color = color(Tokens.Color.danger))
                }
            },
            dismissButton = { TextButton(onClick = model::keepDevice) { Text(stringResource(R.string.cancel)) } },
        )
    }
    Page(stringResource(R.string.devices), null, tag = "devicesScreen") {
        TextButton(onClick = { settings.back() }, modifier = Modifier.testTag("back")) { Text(stringResource(R.string.back)) }
        for (device in state.devices) {
            Column(Modifier.fillMaxWidth().testTag(if (device.isCurrent) "device.current" else "device.${device.id}")) {
                Text(
                    device.name ?: stringResource(R.string.unknown_device),
                    style = Tokens.Text.bodyStrong.compose(), color = color(Tokens.Color.text),
                    modifier = Modifier.testTag(if (device.isCurrent) "device.current.name" else "device.${device.id}.name"),
                )
                val where = if (device.isCurrent) stringResource(R.string.this_device) else device.platform
                Text(
                    stringResource(R.string.device_seen, where, date(device.lastSeenAtMs), date(device.createdAtMs)),
                    style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted),
                )
                Row {
                    TextButton(onClick = { renaming = device }, enabled = !state.busy, modifier = Modifier.testTag("rename.${if (device.isCurrent) "current" else device.id}")) {
                        Text(stringResource(R.string.rename))
                    }
                    if (!device.isCurrent) {
                        TextButton(onClick = { model.askRevoke(device) }, enabled = !state.busy) {
                            Text(stringResource(R.string.revoke), color = color(Tokens.Color.danger))
                        }
                    }
                }
            }
            HorizontalDivider(color = color(Tokens.Color.border))
        }
        ErrorLine(state.error)
    }
}

@Composable
internal fun ApprovalsPage(settings: SettingsModel, model: ApprovalsModel) {
    val state by model.state.collectAsState()
    // No push on Android yet (D57): the list is read again while this shows.
    LaunchedEffect(model) {
        while (true) {
            model.load()
            delay(3_000)
        }
    }
    Page(stringResource(R.string.approve_device), stringResource(R.string.approve_hint), tag = "approvalsScreen") {
        TextButton(onClick = { settings.back() }, modifier = Modifier.testTag("back")) { Text(stringResource(R.string.back)) }
        if (state.done) Text(stringResource(R.string.approved_done), style = Tokens.Text.body.compose(), color = color(Tokens.Color.success))
        val code = state.code
        if (state.approving != null) {
            Surface(color = color(Tokens.Color.surface), shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth()) {
                Column(Modifier.padding(Tokens.Space.s4.dp), verticalArrangement = Arrangement.spacedBy(Tokens.Space.s2.dp)) {
                    if (code == null) {
                        Text(stringResource(R.string.approve_waiting), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text))
                    } else {
                        Text(stringResource(R.string.approve_compare), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text))
                        Text(code, style = Tokens.Text.display.compose().copy(fontFamily = GeistMono), color = color(Tokens.Color.text), modifier = Modifier.testTag("approveCode"))
                        Primary(stringResource(R.string.codes_match), enabled = !state.busy, tag = "codesMatch", onClick = model::codesMatch)
                    }
                    TextButton(onClick = { state.approving?.let(model::deny) }, enabled = !state.busy) {
                        Text(stringResource(R.string.deny), color = color(Tokens.Color.danger))
                    }
                }
            }
        } else if (state.loaded && state.requests.isEmpty()) {
            Text(stringResource(R.string.no_requests), style = Tokens.Text.body.compose(), color = color(Tokens.Color.textMuted), modifier = Modifier.testTag("noRequests"))
        } else {
            for (request in state.requests) {
                Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
                    Column(Modifier.weight(1f)) {
                        Text(stringResource(R.string.request_from, request.platform), style = Tokens.Text.bodyStrong.compose(), color = color(Tokens.Color.text))
                        Text(date(request.createdAtMs), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
                    }
                    TextButton(onClick = { model.approve(request) }, enabled = !state.busy) { Text(stringResource(R.string.approve)) }
                    TextButton(onClick = { model.deny(request.id) }, enabled = !state.busy) { Text(stringResource(R.string.deny), color = color(Tokens.Color.danger)) }
                }
            }
        }
        ErrorLine(state.error)
    }
}

/** "Sign in again to sync" (D50 §5): a line over the vault list, and a dialog asking for the master password. */
@Composable
internal fun SignInAgainLine(model: SignInAgainModel) {
    val state by model.state.collectAsState()
    if (!state.needed) return
    TextButton(onClick = model::open, modifier = Modifier.fillMaxWidth().testTag("signInAgain")) {
        Text(stringResource(R.string.sign_in_again), color = color(Tokens.Color.warning))
    }
    if (state.asking) {
        AlertDialog(
            onDismissRequest = model::close,
            title = { Text(stringResource(R.string.sign_in_again)) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.s2.dp)) {
                    Text(stringResource(R.string.sign_in_again_hint), style = Tokens.Text.body.compose())
                    Field(stringResource(R.string.master_password), state.password.value, model::setPassword, tag = "signInAgainPassword", secret = true)
                    ErrorLine(state.error)
                }
            },
            confirmButton = {
                TextButton(onClick = model::signIn, enabled = state.canSignIn, modifier = Modifier.testTag("confirmSignInAgain")) {
                    Text(stringResource(R.string.sign_in))
                }
            },
            dismissButton = { TextButton(onClick = model::close) { Text(stringResource(R.string.cancel)) } },
        )
    }
}
