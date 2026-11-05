package pwm.app

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import pwm.kit.design.Tokens
import pwm.kit.ui.CredentialModel

/** PWM's screen for a Credential Manager entry (D61 §5-7): unlock or confirm, or what will be saved. */
@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun CredentialScreen(model: CredentialModel) {
    val state by model.state.collectAsState()
    BackHandler { model.cancel() }
    Surface(
        color = color(Tokens.Color.bg),
        // The device test finds these by id.
        modifier = Modifier.fillMaxSize().semantics { testTagsAsResourceId = true },
    ) {
        val unlock = state.unlock
        if (unlock != null) {
            if (state.confirming) Unlock(unlock, title = stringResource(R.string.credential_confirm), showSignOut = false)
            else Unlock(unlock)
            return@Surface
        }
        val save = state.save
        if (save != null) {
            val title = stringResource(if (save.updating) R.string.credential_update_title else R.string.credential_save_title)
            Page(title, save.where, tag = "saveCredential") {
                Text(save.username, style = Tokens.Text.body.compose(), color = color(Tokens.Color.text), modifier = Modifier.testTag("saveUsername"))
                ErrorLine(state.error)
                Primary(
                    stringResource(if (save.updating) R.string.credential_update else R.string.save),
                    enabled = !state.busy, tag = "saveLogin", onClick = model::save,
                )
                Secondary(stringResource(R.string.cancel), tag = "cancelSave", onClick = model::cancel)
            }
            return@Surface
        }
        val error = state.error
        if (error != null) {
            Page(stringResource(R.string.app_name), null, tag = "credentialError") {
                ErrorLine(error)
                Secondary(stringResource(R.string.cancel), tag = "cancelCredential", onClick = model::cancel)
            }
        }
    }
}
