package pwm.app

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringArrayResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import java.text.NumberFormat
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import pwm.kit.ColumnMap
import pwm.kit.ImportFile
import pwm.kit.ImportSummary
import pwm.kit.design.Tokens
import pwm.kit.ui.AppModel
import pwm.kit.ui.DeleteAccountModel
import pwm.kit.ui.Entry
import pwm.kit.ui.ExportModel
import pwm.kit.ui.HealthModel
import pwm.kit.ui.ImportModel
import pwm.kit.ui.SettingsModel

// Settings' data pages (D59 §5-7): the health report, export and import.

@Composable
internal fun HealthPage(settings: SettingsModel, model: HealthModel) {
    val state by model.state.collectAsState()
    // Read again each time the page shows, as on Apple; the leak results stay in memory only.
    LaunchedEffect(model) { model.load() }
    Page(stringResource(R.string.password_health), null, tag = "healthScreen") {
        TextButton(onClick = { settings.back() }, modifier = Modifier.testTag("back")) { Text(stringResource(R.string.back)) }
        if (state.checkingLeaks) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s2.dp)) {
                CircularProgressIndicator(modifier = Modifier.testTag("checkingLeaks"))
                Text(stringResource(R.string.health_checking), style = Tokens.Text.body.compose(), color = color(Tokens.Color.textMuted))
            }
        }
        val number = remember { NumberFormat.getIntegerInstance() }
        if (state.leaked.isNotEmpty()) {
            Group(stringResource(R.string.health_leaked), stringResource(R.string.health_leaked_hint))
            for (leak in state.leaked) {
                HealthRow(leak.entry, pluralStringResource(R.plurals.health_leak_count, leak.count.coerceAtMost(Int.MAX_VALUE.toLong()).toInt(), number.format(leak.count)), "leaked", model)
            }
        }
        if (state.weak.isNotEmpty()) {
            Group(stringResource(R.string.health_weak), null)
            for (entry in state.weak) HealthRow(entry, stringResource(R.string.health_weak_detail), "weak", model)
        }
        if (state.reused.isNotEmpty()) {
            Group(stringResource(R.string.health_reused), stringResource(R.string.health_reused_hint))
            for (group in state.reused) {
                for (entry in group) {
                    HealthRow(entry, pluralStringResource(R.plurals.health_reused_detail, group.size - 1, group.size - 1), "reused", model)
                }
                HorizontalDivider(color = color(Tokens.Color.border))
            }
        }
        if (state.isClean) {
            Text(stringResource(R.string.health_clean), style = Tokens.Text.bodyStrong.compose(), color = color(Tokens.Color.success), modifier = Modifier.testTag("healthClean"))
        }
        ErrorLine(state.error)
    }
}

@Composable
private fun Group(title: String, hint: String?) {
    Text(title, style = Tokens.Text.title.compose(), color = color(Tokens.Color.text))
    if (hint != null) Text(hint, style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
}

@Composable
private fun HealthRow(entry: Entry, detail: String, kind: String, model: HealthModel) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth().testTag("$kind.${entry.item.title}")) {
        Column(Modifier.weight(1f)) {
            Text(entry.item.title, style = Tokens.Text.bodyStrong.compose(), color = color(Tokens.Color.text))
            Text(detail, style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
        }
        TextButton(onClick = { model.edit(entry) }) { Text(stringResource(R.string.health_change), color = color(Tokens.Color.accent)) }
    }
}

/** Export (D24): the master password, then the format's own checks, then "Save to". */
@Composable
internal fun ExportPage(app: AppModel, settings: SettingsModel, model: ExportModel) {
    val state by model.state.collectAsState()
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument(state.mimeType.ifEmpty { "application/octet-stream" })) { uri ->
        val file = model.state.value.file
        if (uri == null || file == null) return@rememberLauncherForActivityResult
        scope.launch {
            val written = withContext(Dispatchers.IO) { DataFiles.write(context, uri, file.value) }
            if (written) model.didSave() else model.saveFailed()
        }
    }
    Page(stringResource(R.string.export_title), stringResource(R.string.export_hint), tag = "exportScreen") {
        TextButton(onClick = { settings.back() }, modifier = Modifier.testTag("back")) { Text(stringResource(R.string.back)) }
        val choices = listOf(
            Triple(ExportModel.Choice.ENCRYPTED, R.string.export_encrypted, R.string.export_encrypted_hint),
            Triple(ExportModel.Choice.JSON, R.string.export_json, R.string.export_json_hint),
            Triple(ExportModel.Choice.CSV, R.string.export_csv, R.string.export_csv_hint),
        )
        for ((choice, label, _) in choices) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth().clickable { model.setChoice(choice) }.testTag("export.${choice.name}"),
            ) {
                RadioButton(selected = state.choice == choice, onClick = { model.setChoice(choice) })
                Text(stringResource(label), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text))
            }
        }
        Text(stringResource(choices.first { it.first == state.choice }.third), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))

        Field(stringResource(R.string.master_password), state.masterPassword.value, model::setMasterPassword, tag = "exportMasterPassword", secret = true)
        Text(stringResource(R.string.export_master_hint), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
        when (state.choice) {
            ExportModel.Choice.ENCRYPTED -> {
                Field(stringResource(R.string.export_password), state.exportPassword.value, model::setExportPassword, tag = "exportPassword", secret = true)
                StrengthMeter(state.strength?.score)
                Field(stringResource(R.string.export_password_again), state.confirm.value, model::setConfirm, tag = "exportConfirm", secret = true)
            }
            ExportModel.Choice.JSON, ExportModel.Choice.CSV -> Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth().clickable { model.setUnderstood(!state.understood) }.testTag("exportUnderstood"),
            ) {
                Checkbox(checked = state.understood, onCheckedChange = model::setUnderstood)
                Text(stringResource(R.string.export_understood), style = Tokens.Text.body.compose(), color = color(Tokens.Color.warning))
            }
        }
        val file = state.file
        if (file == null) {
            Primary(
                stringResource(if (state.busy) R.string.export_working else R.string.export_button),
                enabled = state.canExport, tag = "exportNow", onClick = model::export,
            )
        } else {
            Primary(stringResource(R.string.export_save), enabled = true, tag = "saveExport") {
                app.openingSystemScreen()
                save.launch(state.fileName)
            }
        }
        if (state.saved) Text(stringResource(R.string.export_saved), style = Tokens.Text.body.compose(), color = color(Tokens.Color.success), modifier = Modifier.testTag("exportSaved"))
        ErrorLine(state.error)
    }
}

/** Import (D51 §9): pick, check where it's from, its password or columns, the preview, then Import. */
@Composable
internal fun ImportPage(app: AppModel, settings: SettingsModel, model: ImportModel) {
    val state by model.state.collectAsState()
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri -> uri?.let { model.picked(it.toString()) } }
    val pickKey = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri -> uri?.let { model.pickedKeyFile(it.toString()) } }
    Page(stringResource(R.string.import_title), stringResource(R.string.import_hint), tag = "importScreen") {
        TextButton(onClick = { settings.back() }, modifier = Modifier.testTag("back")) { Text(stringResource(R.string.back)) }
        OutlinedButton(
            onClick = {
                app.openingSystemScreen()
                pick.launch(arrayOf("*/*"))
            },
            enabled = !state.working,
            modifier = Modifier.fillMaxWidth().testTag("chooseImport"),
        ) { Text(stringResource(if (state.fileName == null) R.string.import_choose else R.string.import_choose_another)) }
        val name = state.fileName
        if (name != null && state.done == null) {
            Text(stringResource(R.string.import_file, name), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text), modifier = Modifier.testTag("importFile"))
            val labels = stringArrayResource(R.array.import_formats)
            val formatLabel = ImportFile.FORMATS.indexOf(state.format).takeIf { it >= 0 }?.let(labels::get)
                ?: stringResource(R.string.import_format_choose)
            Choice(stringResource(R.string.import_format), formatLabel, labels.toList(), tag = "importFormat") { model.setFormat(ImportFile.FORMATS[it]) }
            if (state.asksPassword) {
                Field(
                    stringResource(if (state.asksKeyFile) R.string.master_password else R.string.export_password),
                    state.password.value, model::setPassword, tag = "importPassword", secret = true,
                )
                if (state.asksKeyFile) {
                    OutlinedButton(onClick = {
                        app.openingSystemScreen()
                        pickKey.launch(arrayOf("*/*"))
                    }, modifier = Modifier.fillMaxWidth()) {
                        Text(state.keyFileName?.let { stringResource(R.string.import_key_file_named, it) } ?: stringResource(R.string.import_key_file))
                    }
                }
                Text(
                    stringResource(if (state.asksKeyFile) R.string.import_keepass_hint else R.string.import_password_hint),
                    style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted),
                )
            }
            state.columns?.takeIf { state.format == "csv" }?.let { columns ->
                Text(stringResource(R.string.import_columns), style = Tokens.Text.title.compose(), color = color(Tokens.Color.text))
                val none = stringResource(R.string.import_column_none)
                val headers = columns.headers.mapIndexed { i, h -> h.ifBlank { stringResource(R.string.import_column_n, i + 1) } }
                for ((field, label) in COLUMN_FIELDS) {
                    val current = state.columnMap[field]?.let { headers.getOrNull(it) } ?: none
                    Choice(stringResource(label), current, listOf(none) + headers, tag = "column.${field.name}") { i ->
                        model.setColumn(field, if (i == 0) null else i - 1)
                    }
                }
            }
            val summary = state.summary
            if (summary == null) {
                Primary(stringResource(if (state.working) R.string.import_reading else R.string.import_check), enabled = state.canCheck, tag = "checkImport", onClick = model::check)
            } else {
                Text(stringResource(R.string.import_ready, counts(summary)), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text), modifier = Modifier.testTag("importReady"))
                if (summary.trashed > 0) {
                    Text(pluralStringResource(R.plurals.import_trashed, summary.trashed, summary.trashed), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
                }
                LeftOut(summary)
                Primary(stringResource(if (state.working) R.string.import_working else R.string.import_button), enabled = !state.working, tag = "importNow", onClick = model::importNow)
            }
        }
        state.done?.let { done ->
            Text(stringResource(R.string.import_done, counts(done)), style = Tokens.Text.body.compose(), color = color(Tokens.Color.success), modifier = Modifier.testTag("importDone"))
            LeftOut(done)
            if (state.deleted) {
                Text(stringResource(R.string.import_deleted), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text), modifier = Modifier.testTag("importDeleted"))
            } else if (state.plaintext) {
                Text(stringResource(R.string.import_plaintext), style = Tokens.Text.body.compose(), color = color(Tokens.Color.warning))
                if (state.canDelete) {
                    TextButton(onClick = model::deleteFile, modifier = Modifier.testTag("deleteImport")) {
                        Text(stringResource(R.string.import_delete), color = color(Tokens.Color.danger))
                    }
                }
            }
        }
        ErrorLine(state.error)
    }
}

private val COLUMN_FIELDS = listOf(
    ColumnMap.Field.TITLE to R.string.title,
    ColumnMap.Field.URL to R.string.website,
    ColumnMap.Field.USERNAME to R.string.username,
    ColumnMap.Field.PASSWORD to R.string.password,
    ColumnMap.Field.NOTES to R.string.notes,
    ColumnMap.Field.TOTP to R.string.one_time_code,
    ColumnMap.Field.FOLDER to R.string.folder,
)

/** "2 logins, 1 card", or "nothing new". */
@Composable
private fun counts(s: ImportSummary): String {
    val parts = ImportModel.counts(s).map { (kind, n) ->
        val id = when (kind) {
            ImportModel.Kind.LOGINS -> R.plurals.count_logins
            ImportModel.Kind.NOTES -> R.plurals.count_notes
            ImportModel.Kind.CARDS -> R.plurals.count_cards
            ImportModel.Kind.IDENTITIES -> R.plurals.count_identities
            ImportModel.Kind.FOLDERS -> R.plurals.count_folders
        }
        pluralStringResource(id, n, n)
    }
    return if (parts.isEmpty()) stringResource(R.string.import_nothing_new) else parts.joinToString(", ")
}

@Composable
private fun LeftOut(s: ImportSummary) {
    for (l in s.leftOut) {
        val what = when (l.reason) {
            "attachments" -> stringResource(R.string.left_out_attachments)
            "ssh_keys" -> stringResource(R.string.left_out_ssh_keys)
            "aliases" -> stringResource(R.string.left_out_aliases)
            "passkeys" -> stringResource(R.string.left_out_passkeys)
            "unknown_types" -> stringResource(R.string.left_out_unknown_types)
            "empty" -> stringResource(R.string.left_out_empty)
            "too_large" -> stringResource(R.string.left_out_too_large)
            "duplicates" -> stringResource(R.string.left_out_duplicates)
            else -> l.reason
        }
        Text(stringResource(R.string.left_out, l.count, what), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
    }
}

/** A labelled menu of choices. */
@Composable
private fun Choice(label: String, current: String, options: List<String>, tag: String, onPick: (Int) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
        Text(label, style = Tokens.Text.body.compose(), color = color(Tokens.Color.text), modifier = Modifier.weight(1f))
        Box {
            OutlinedButton(onClick = { open = true }, modifier = Modifier.testTag(tag)) { Text(current) }
            DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                options.forEachIndexed { i, option ->
                    DropdownMenuItem(text = { Text(option) }, onClick = {
                        open = false
                        onPick(i)
                    })
                }
            }
        }
    }
}

/** Deleting the account (D74 §7): what will happen, with export first; the plan, when there is one; then the master password. */
@Composable
internal fun DeleteAccountPage(settings: SettingsModel, model: DeleteAccountModel) {
    val state by model.state.collectAsState()
    Page(stringResource(R.string.delete_account), null, tag = "deleteAccountScreen") {
        TextButton(onClick = { settings.back() }, modifier = Modifier.testTag("back")) { Text(stringResource(R.string.back)) }
        when (state.step) {
            DeleteAccountModel.Step.INTRO -> {
                Text(stringResource(R.string.delete_intro), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text))
                Secondary(stringResource(R.string.delete_export_first), tag = "deleteExportFirst", onClick = settings::exportFirst)
                Primary(stringResource(R.string.delete_continue), enabled = true, tag = "deleteContinue", onClick = model::next)
            }
            DeleteAccountModel.Step.PLAN -> {
                if (state.endsWithRefund) {
                    Text(stringResource(R.string.delete_plan_refund), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text))
                }
                if (state.appleBills) {
                    Text(
                        stringResource(R.string.delete_plan_apple), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text),
                        modifier = Modifier.testTag("deleteAppleBills"),
                    )
                    Text(stringResource(R.string.delete_plan_apple_how), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
                }
                Primary(stringResource(R.string.delete_continue), enabled = true, tag = "deletePlanContinue", onClick = model::next)
            }
            DeleteAccountModel.Step.PASSWORD -> {
                Field(stringResource(R.string.master_password), state.password.value, model::setPassword, tag = "deletePassword", secret = true)
                Button(
                    onClick = model::delete,
                    enabled = state.canDelete,
                    colors = ButtonDefaults.buttonColors(containerColor = color(Tokens.Color.danger)),
                    modifier = Modifier.fillMaxWidth().testTag("deleteConfirm"),
                ) { Text(stringResource(if (state.busy) R.string.delete_working else R.string.delete_account)) }
            }
        }
        ErrorLine(state.error)
    }
}
