package pwm.app

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.unit.dp
import pwm.kit.Caller
import pwm.kit.design.Tokens
import pwm.kit.ui.FillModel

/** PWM's fill screen (D58 §6): unlock first if needed, then the matches and a search over every login. */
@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun FillScreen(model: FillModel) {
    val state by model.state.collectAsState()
    BackHandler { model.cancel() }
    Surface(
        color = color(Tokens.Color.bg),
        // The device test finds these by id.
        modifier = Modifier.fillMaxSize().semantics { testTagsAsResourceId = true },
    ) {
        val unlock = state.unlock
        if (unlock != null) {
            Unlock(unlock)
            return@Surface
        }
        val site = when (val caller = model.caller) {
            is Caller.Web -> caller.url.removePrefix("https://").removePrefix("http://").trimEnd('/')
            is Caller.App -> caller.packageName
        }
        Page(stringResource(R.string.fill_title), stringResource(R.string.fill_for, site), tag = "fillScreen") {
            for (match in state.matches) {
                Choice(match.title, match.username, "fill.${match.title}") { model.pick(match) }
            }
            Field(stringResource(R.string.search), state.query, model::setQuery, tag = "fillSearch")
            for (entry in state.results) {
                Choice(entry.item.title, entry.item.username, "result.${entry.item.title}") { model.pick(entry) }
            }
            if (model.canLink && state.query.isNotBlank()) {
                Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
                    Text(stringResource(R.string.use_for_this_app), style = Tokens.Text.body.compose(), modifier = Modifier.weight(1f))
                    Switch(checked = state.linkForApp, onCheckedChange = model::setLinkForApp, modifier = Modifier.testTag("linkForApp"))
                }
            }
            ErrorLine(state.error)
            Secondary(stringResource(R.string.cancel), tag = "cancelFill", onClick = model::cancel)
        }
    }
}

@Composable
private fun Choice(title: String, username: String, tag: String, onClick: () -> Unit) {
    Column(Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = Tokens.Space.s3.dp).testTag(tag)) {
        Text(title, style = Tokens.Text.bodyStrong.compose(), color = color(Tokens.Color.text))
        if (username.isNotEmpty()) Text(username, style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
    }
    HorizontalDivider(color = color(Tokens.Color.border))
}
