package pwm.app

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringArrayResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import pwm.kit.Item
import pwm.kit.Redacted
import pwm.kit.design.Tokens
import pwm.kit.ui.AppModel
import pwm.kit.ui.GeneratorModel
import pwm.kit.ui.ItemEditModel
import pwm.kit.ui.VaultModel

// A new or edited item (D54 §1): every D18 field for its type, a folder
// picker that can make a folder, tags, one-time-code setup typed or scanned,
// and Generate beside the password.

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ItemEdit(app: AppModel, vault: VaultModel, model: ItemEditModel) {
    val state by model.state.collectAsState()
    val context = LocalContext.current
    var generator by remember { mutableStateOf<GeneratorModel?>(null) }
    var scanning by rememberSaveable { mutableStateOf(false) }
    var folderAsked by remember { mutableStateOf(false) }
    val camera = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted -> if (granted) scanning = true }
    if (scanning) {
        androidx.activity.compose.BackHandler { scanning = false }
        QrScanner(stringResource(R.string.scan_totp_hint), onCode = { text ->
            // The core checks it on save, like a pasted one (D54 §6).
            model.edit { it.copy(totp = Redacted(text)) }
            scanning = false
            true
        }, onClose = { scanning = false })
        return
    }
    generator?.let { sheet ->
        ModalBottomSheet(onDismissRequest = { generator = null }) {
            // Scrolls, so every option can be reached on a small screen.
            Column(
                Modifier.verticalScroll(rememberScrollState()).padding(Tokens.Space.s5.dp),
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.s3.dp),
            ) {
                val value by sheet.state.collectAsState()
                GeneratorOptions(sheet) {
                    Primary(stringResource(R.string.use_password), enabled = value.value != null, tag = "useGenerated") {
                        value.value?.let(model::use)
                        generator = null
                    }
                }
            }
        }
    }
    if (folderAsked) NewFolder(onCreate = { name -> folderAsked = false; model.newFolder(name) }, onCancel = { folderAsked = false })

    val title = stringResource(if (model.isNew) R.string.new_item_title else R.string.edit_item_title)
    Page(title, null, tag = "editScreen") {
        if (!model.isEditable) {
            Text(stringResource(R.string.read_only), style = Tokens.Text.body.compose(), color = color(Tokens.Color.textMuted))
        }
        val on = model.isEditable && !state.busy
        Field(stringResource(R.string.title), state.title, { v -> model.edit { it.copy(title = v) } }, tag = "title", enabled = on)
        when (model.kind) {
            Item.Kind.LOGIN -> {
                Field(stringResource(R.string.username), state.username, { v -> model.edit { it.copy(username = v) } }, tag = "username", enabled = on)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.weight(1f)) {
                        Field(
                            stringResource(R.string.password), state.password.value,
                            { v -> model.edit { it.copy(password = Redacted(v)) } }, tag = "password", secret = true, enabled = on,
                        )
                    }
                    TextButton(
                        onClick = { generator = model.makeGenerator().also { it.regenerate() } },
                        enabled = on, modifier = Modifier.testTag("generate"),
                    ) { Text(stringResource(R.string.generate)) }
                }
                Field(
                    stringResource(R.string.one_time_code), state.totp.value,
                    { v -> model.edit { it.copy(totp = Redacted(v)) } }, tag = "totp", secret = true, enabled = on,
                )
                Text(stringResource(R.string.totp_hint), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
                if (context.packageManager.hasSystemFeature(PackageManager.FEATURE_CAMERA_ANY)) {
                    OutlinedButton(onClick = {
                        val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
                        if (granted) scanning = true else camera.launch(Manifest.permission.CAMERA)
                    }, enabled = on, modifier = Modifier.testTag("scanTotp")) { Text(stringResource(R.string.scan_qr)) }
                }
                Websites(model, state, on)
            }
            Item.Kind.CARD -> {
                Field(stringResource(R.string.cardholder), state.cardholder, { v -> model.edit { it.copy(cardholder = v) } }, tag = "cardholder", enabled = on)
                Field(
                    stringResource(R.string.card_number), state.number.value, { v -> model.edit { it.copy(number = Redacted(v)) } },
                    tag = "number", secret = true, keyboard = KeyboardType.Number, enabled = on,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s2.dp)) {
                    Box(Modifier.weight(1f)) {
                        Field(stringResource(R.string.expiry_month), state.expiryMonth, { v -> model.edit { it.copy(expiryMonth = v) } }, tag = "expiryMonth", keyboard = KeyboardType.Number, enabled = on)
                    }
                    Box(Modifier.weight(1f)) {
                        Field(stringResource(R.string.expiry_year), state.expiryYear, { v -> model.edit { it.copy(expiryYear = v) } }, tag = "expiryYear", keyboard = KeyboardType.Number, enabled = on)
                    }
                }
                Field(
                    stringResource(R.string.security_code), state.securityCode.value, { v -> model.edit { it.copy(securityCode = Redacted(v)) } },
                    tag = "securityCode", secret = true, keyboard = KeyboardType.Number, enabled = on,
                )
                Field(stringResource(R.string.brand), state.brand, { v -> model.edit { it.copy(brand = v) } }, tag = "brand", enabled = on)
            }
            Item.Kind.IDENTITY -> {
                val labels = mapOf(
                    "first_name" to R.string.first_name, "middle_name" to R.string.middle_name, "last_name" to R.string.last_name,
                    "email" to R.string.email, "phone" to R.string.phone, "company" to R.string.company,
                )
                for ((key, label) in labels) IdentityField(model, state, key, stringResource(label), on)
                OutlinedTextField(
                    value = state.address, onValueChange = { v -> model.edit { it.copy(address = v) } }, enabled = on,
                    label = { Text(stringResource(R.string.address)) }, minLines = 2, modifier = Modifier.fillMaxWidth().testTag("address"),
                )
                val place = mapOf("city" to R.string.city, "region" to R.string.region, "postal_code" to R.string.postal_code, "country" to R.string.country)
                for ((key, label) in place) IdentityField(model, state, key, stringResource(label), on)
            }
            else -> Unit
        }
        OutlinedTextField(
            value = state.notes, onValueChange = { v -> model.edit { it.copy(notes = v) } }, enabled = on,
            label = { Text(stringResource(R.string.notes)) }, minLines = 3, modifier = Modifier.fillMaxWidth().testTag("notes"),
        )
        CustomFields(model, state, on)
        FolderPicker(model, state, on, onNew = { folderAsked = true })
        Field(stringResource(R.string.tags), state.tags, { v -> model.edit { it.copy(tags = v) } }, tag = "tags", enabled = on)
        Text(stringResource(R.string.tags_hint), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
            Text(stringResource(R.string.favorite), style = Tokens.Text.bodyStrong.compose(), modifier = Modifier.weight(1f))
            Switch(checked = state.favorite, onCheckedChange = { v -> model.edit { it.copy(favorite = v) } }, enabled = on, modifier = Modifier.testTag("favorite"))
        }
        ErrorLine(state.error)
        Primary(stringResource(if (state.busy) R.string.saving else R.string.save), enabled = model.canSave, tag = "save", onClick = model::save)
        Secondary(stringResource(R.string.cancel), tag = "cancelEdit", enabled = !state.busy) { vault.back() }
    }
}

@Composable
private fun IdentityField(model: ItemEditModel, state: ItemEditModel.State, key: String, label: String, enabled: Boolean) {
    Field(label, state.identity[key] ?: "", { v -> model.edit { it.copy(identity = it.identity + (key to v)) } }, tag = "identity.$key", enabled = enabled)
}

@Composable
private fun Websites(model: ItemEditModel, state: ItemEditModel.State, enabled: Boolean) {
    val names = stringArrayResource(R.array.match_names)
    for (row in state.urls) key(row.id) {
        var open by remember { mutableStateOf(false) }
        Column(Modifier.fillMaxWidth()) {
            Field(
                stringResource(R.string.website), row.url,
                { v -> model.edit { s -> s.copy(urls = s.urls.map { if (it.id == row.id) it.copy(url = v) else it }) } },
                tag = "url.${row.id}", keyboard = KeyboardType.Uri, enabled = enabled,
            )
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box {
                    val known = Item.Url.MATCHES.indexOf(row.match)
                    TextButton(onClick = { open = true }, enabled = enabled) {
                        // A mode from a newer app version shows as it is and is kept.
                        Text("${stringResource(R.string.match)}: ${if (known >= 0) names[known] else row.match}")
                    }
                    DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                        Item.Url.MATCHES.forEachIndexed { i, match ->
                            DropdownMenuItem(text = { Text(names[i]) }, onClick = {
                                open = false
                                model.edit { s -> s.copy(urls = s.urls.map { if (it.id == row.id) it.copy(match = match) else it }) }
                            })
                        }
                    }
                }
                TextButton(onClick = { model.removeUrl(row.id) }, enabled = enabled) { Text(stringResource(R.string.remove)) }
            }
        }
    }
    TextButton(onClick = model::addUrl, enabled = enabled, modifier = Modifier.testTag("addUrl")) { Text(stringResource(R.string.add_website)) }
}

@Composable
private fun CustomFields(model: ItemEditModel, state: ItemEditModel.State, enabled: Boolean) {
    val kinds = stringArrayResource(R.array.field_kinds)
    if (state.customFields.isNotEmpty()) Text(stringResource(R.string.other_fields), style = Tokens.Text.bodyStrong.compose())
    for (row in state.customFields) key(row.id) {
        fun change(f: (ItemEditModel.FieldRow) -> ItemEditModel.FieldRow) =
            model.edit { s -> s.copy(customFields = s.customFields.map { if (it.id == row.id) f(it) else it }) }
        Field(stringResource(R.string.field_name), row.name, { v -> change { it.copy(name = v) } }, tag = "fieldName.${row.id}", enabled = enabled)
        val secret = row.kind == null || row.kind == Item.CustomField.Kind.HIDDEN
        Field(
            stringResource(R.string.field_value), row.value.value, { v -> change { it.copy(value = Redacted(v)) } },
            tag = "fieldValue.${row.id}", secret = secret, enabled = enabled && row.kind != null,
        )
        TextButton(onClick = { model.removeField(row.id) }, enabled = enabled) { Text(stringResource(R.string.remove)) }
    }
    var open by remember { mutableStateOf(false) }
    Box {
        TextButton(onClick = { open = true }, enabled = enabled, modifier = Modifier.testTag("addField")) { Text(stringResource(R.string.add_field)) }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            Item.CustomField.Kind.entries.forEachIndexed { i, kind ->
                DropdownMenuItem(text = { Text(kinds[i]) }, onClick = { open = false; model.addField(kind) })
            }
        }
    }
}

@Composable
private fun FolderPicker(model: ItemEditModel, state: ItemEditModel.State, enabled: Boolean, onNew: () -> Unit) {
    var open by remember { mutableStateOf(false) }
    val current = state.folders.firstOrNull { it.id == state.folderId }?.item?.title ?: stringResource(R.string.no_folder)
    Box {
        TextButton(onClick = { open = true }, enabled = enabled, modifier = Modifier.testTag("folder")) {
            Text("${stringResource(R.string.folder)}: $current")
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(text = { Text(stringResource(R.string.no_folder)) }, onClick = { open = false; model.edit { it.copy(folderId = null) } })
            for (folder in state.folders) {
                DropdownMenuItem(text = { Text(folder.item.title) }, onClick = { open = false; model.edit { it.copy(folderId = folder.id) } })
            }
            DropdownMenuItem(text = { Text(stringResource(R.string.new_folder)) }, onClick = { open = false; onNew() })
        }
    }
}

@Composable
private fun NewFolder(onCreate: (String) -> Unit, onCancel: () -> Unit) {
    var name by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onCancel,
        title = { Text(stringResource(R.string.new_folder)) },
        text = { Field(stringResource(R.string.folder_name), name, { name = it }, tag = "folderName") },
        confirmButton = { TextButton(onClick = { onCreate(name) }, enabled = name.isNotBlank()) { Text(stringResource(R.string.create)) } },
        dismissButton = { TextButton(onClick = onCancel) { Text(stringResource(R.string.cancel)) } },
    )
}
