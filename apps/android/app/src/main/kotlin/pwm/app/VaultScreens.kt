package pwm.app

import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AccountCircle
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.ShoppingCart
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.delay
import pwm.kit.Item
import pwm.kit.design.Tokens
import pwm.kit.ui.AppModel
import pwm.kit.ui.Entry
import pwm.kit.ui.HomeModel
import pwm.kit.ui.ItemDetailModel
import pwm.kit.ui.Section
import pwm.kit.ui.SyncNotice
import pwm.kit.ui.VaultModel

// The unlocked vault (D54 §1-3): three tabs, a bottom bar on phones and a
// side rail on wide screens, where the list and the open item sit side by
// side. Each tab keeps its place; the back gesture walks the vault's stack.

/** Wide enough for a side rail and two panes (600 dp, as Material's guidance). */
@Composable
internal fun isWide(): Boolean = LocalConfiguration.current.screenWidthDp >= 600

@Composable
fun Home(app: AppModel, home: HomeModel) {
    val state by home.state.collectAsState()
    val vaultState by home.vault.state.collectAsState()
    val settingsState by home.settings.state.collectAsState()
    val canGoBack = state.tab != HomeModel.Tab.VAULT || vaultState.stack.isNotEmpty() || settingsState.page != pwm.kit.ui.SettingsModel.Page.MAIN
    BackHandler(enabled = canGoBack) { home.back() }
    val tabs = listOf(
        Triple(HomeModel.Tab.VAULT, R.string.tab_vault, Icons.Filled.Lock),
        Triple(HomeModel.Tab.GENERATOR, R.string.tab_generator, Icons.Filled.Refresh),
        Triple(HomeModel.Tab.SETTINGS, R.string.tab_settings, Icons.Filled.Settings),
    )
    val content: @Composable (Modifier) -> Unit = { modifier ->
        Box(modifier.testTag("vaultScreen")) {
            when (state.tab) {
                HomeModel.Tab.VAULT -> VaultTab(app, home.vault)
                HomeModel.Tab.GENERATOR -> GeneratorTab(home.generator)
                HomeModel.Tab.SETTINGS -> SettingsTab(app, home.settings)
            }
        }
    }
    if (isWide()) {
        Row(Modifier.fillMaxSize().safeDrawingPadding()) {
            NavigationRail {
                for ((tab, label, icon) in tabs) {
                    NavigationRailItem(
                        selected = state.tab == tab, onClick = { home.setTab(tab) },
                        icon = { Icon(icon, contentDescription = null) }, label = { Text(stringResource(label)) },
                        modifier = Modifier.testTag("tab.${tab.name.lowercase()}"),
                    )
                }
            }
            content(Modifier.weight(1f).fillMaxHeight())
        }
    } else {
        Scaffold(
            bottomBar = {
                NavigationBar {
                    for ((tab, label, icon) in tabs) {
                        NavigationBarItem(
                            selected = state.tab == tab, onClick = { home.setTab(tab) },
                            icon = { Icon(icon, contentDescription = null) }, label = { Text(stringResource(label)) },
                            modifier = Modifier.testTag("tab.${tab.name.lowercase()}"),
                        )
                    }
                }
            },
            containerColor = color(Tokens.Color.bg),
        ) { padding -> content(Modifier.padding(padding).fillMaxSize()) }
    }
}

// --- The vault tab ---

@Composable
private fun VaultTab(app: AppModel, vault: VaultModel) {
    val state by vault.state.collectAsState()
    val top = state.stack.lastOrNull()
    if (state.discardAsked) {
        AlertDialog(
            onDismissRequest = vault::keepEditing,
            text = { Text(stringResource(R.string.discard_ask)) },
            confirmButton = { TextButton(onClick = vault::confirmDiscard, modifier = Modifier.testTag("discard")) { Text(stringResource(R.string.discard)) } },
            dismissButton = { TextButton(onClick = vault::keepEditing) { Text(stringResource(R.string.keep_editing)) } },
        )
    }
    if (isWide()) {
        Row(Modifier.fillMaxSize()) {
            VaultList(vault, state, Modifier.width(360.dp).fillMaxHeight())
            Box(Modifier.weight(1f).fillMaxHeight()) {
                when (top) {
                    null -> Text(
                        stringResource(R.string.select_item), style = Tokens.Text.body.compose(), color = color(Tokens.Color.textMuted),
                        modifier = Modifier.align(Alignment.Center),
                    )
                    is VaultModel.Route.Detail -> ItemDetail(vault, top.model)
                    is VaultModel.Route.Edit -> ItemEdit(app, vault, top.model)
                }
            }
        }
    } else {
        when (top) {
            null -> VaultList(vault, state, Modifier.fillMaxSize())
            is VaultModel.Route.Detail -> ItemDetail(vault, top.model)
            is VaultModel.Route.Edit -> ItemEdit(app, vault, top.model)
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun VaultList(vault: VaultModel, state: VaultModel.State, modifier: Modifier) {
    var menu by remember { mutableStateOf(false) }
    Box(modifier) {
        Column(Modifier.fillMaxSize()) {
            OutlinedTextField(
                value = state.query, onValueChange = vault::setQuery, singleLine = true,
                label = { Text(stringResource(R.string.search)) },
                modifier = Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.s4.dp, vertical = Tokens.Space.s2.dp).testTag("search"),
            )
            Sections(vault, state)
            val notice = when (state.notice) {
                SyncNotice.SOME_FAILED -> stringResource(R.string.notice_some_failed)
                SyncNotice.OFFLINE -> stringResource(R.string.notice_offline)
                SyncNotice.SIGNED_OUT -> pwm.kit.PwmError("signed_out").userMessage
                null -> null
            }
            for (line in listOfNotNull(notice, state.error)) {
                Text(
                    line, style = Tokens.Text.caption.compose(), color = color(Tokens.Color.warning),
                    modifier = Modifier.padding(horizontal = Tokens.Space.s4.dp).testTag("notice"),
                )
            }
            PullToRefreshBox(isRefreshing = state.syncing, onRefresh = vault::refresh, modifier = Modifier.fillMaxSize()) {
                if (state.loaded && state.visible.isEmpty()) {
                    val empty = when {
                        state.query.isNotBlank() -> R.string.empty_search
                        state.section == Section.Trash -> R.string.empty_trash
                        else -> R.string.empty_vault
                    }
                    Text(
                        stringResource(empty), style = Tokens.Text.body.compose(), color = color(Tokens.Color.textMuted),
                        modifier = Modifier.padding(Tokens.Space.s5.dp).fillMaxWidth().testTag("emptyList"),
                    )
                }
                LazyColumn(Modifier.fillMaxSize().testTag("itemList")) {
                    items(state.visible, key = { "${it.vaultId}/${it.id}" }) { entry -> ItemRow(entry) { vault.open(entry) } }
                }
            }
        }
        if (state.section != Section.Trash) {
            Box(Modifier.align(Alignment.BottomEnd).padding(Tokens.Space.s5.dp)) {
                FloatingActionButton(onClick = { menu = true }, modifier = Modifier.testTag("newItem")) {
                    Icon(Icons.Filled.Add, contentDescription = stringResource(R.string.new_item))
                }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    for ((kind, label) in NEW_KINDS) {
                        DropdownMenuItem(
                            text = { Text(stringResource(label)) },
                            onClick = {
                                menu = false
                                vault.newItem(kind)
                            },
                            modifier = Modifier.testTag("newItem.${kind.type}"),
                        )
                    }
                }
            }
        }
    }
}

private val NEW_KINDS = listOf(
    Item.Kind.LOGIN to R.string.new_login,
    Item.Kind.CARD to R.string.new_card,
    Item.Kind.IDENTITY to R.string.new_identity,
    Item.Kind.NOTE to R.string.new_note,
)

@Composable
private fun Sections(vault: VaultModel, state: VaultModel.State) {
    val sections = buildList {
        add(Section.All to stringResource(R.string.section_all))
        add(Section.Favorites to stringResource(R.string.section_favorites))
        add(Section.OfKind(Item.Kind.LOGIN) to stringResource(R.string.kind_login))
        add(Section.OfKind(Item.Kind.CARD) to stringResource(R.string.kind_card))
        add(Section.OfKind(Item.Kind.IDENTITY) to stringResource(R.string.kind_identity))
        add(Section.OfKind(Item.Kind.NOTE) to stringResource(R.string.kind_note))
        for (folder in state.folders) add(Section.Folder(folder.id) to folder.item.title)
        add(Section.Trash to stringResource(R.string.section_trash))
    }
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = Tokens.Space.s4.dp),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s2.dp),
    ) {
        for ((section, label) in sections) {
            FilterChip(
                selected = state.section == section, onClick = { vault.setSection(section) }, label = { Text(label) },
                modifier = Modifier.testTag("section.${sectionTag(section)}"),
            )
        }
    }
}

private fun sectionTag(section: Section): String = when (section) {
    Section.All -> "all"
    Section.Favorites -> "favorites"
    Section.Trash -> "trash"
    is Section.OfKind -> section.kind.type
    is Section.Folder -> "folder"
}

/** A row: the type's icon, the title and the username or a card's last 4 digits. No website icons (D43 §2). */
@Composable
private fun ItemRow(entry: Entry, onClick: () -> Unit) {
    val item = entry.item
    val subtitle = when (item.kind) {
        Item.Kind.LOGIN -> item.username
        Item.Kind.CARD -> item.card?.lastFour?.takeIf { it.isNotEmpty() }?.let { stringResource(R.string.card_ending, it) } ?: ""
        Item.Kind.IDENTITY -> item.identity?.let { listOf(it.firstName, it.lastName).filter(String::isNotBlank).joinToString(" ") } ?: ""
        else -> ""
    }
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = Tokens.Space.s4.dp, vertical = Tokens.Space.s3.dp)
            .testTag("item.${item.title}"),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s3.dp),
    ) {
        Icon(kindIcon(item.kind), contentDescription = null, tint = color(Tokens.Color.textMuted))
        Column(Modifier.weight(1f)) {
            Text(item.title, style = Tokens.Text.bodyStrong.compose(), color = color(Tokens.Color.text))
            if (subtitle.isNotEmpty()) Text(subtitle, style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
        }
        if (item.favorite) Icon(Icons.Filled.Star, contentDescription = stringResource(R.string.favorite), tint = color(Tokens.Color.warning))
    }
    HorizontalDivider(color = color(Tokens.Color.border))
}

private fun kindIcon(kind: Item.Kind?): ImageVector = when (kind) {
    Item.Kind.LOGIN -> Icons.Filled.AccountCircle
    Item.Kind.CARD -> Icons.Filled.ShoppingCart
    Item.Kind.IDENTITY -> Icons.Filled.Person
    Item.Kind.NOTE -> Icons.Filled.Edit
    else -> Icons.Filled.Info
}

// --- An item's detail ---

@Composable
private fun ItemDetail(vault: VaultModel, model: ItemDetailModel) {
    val state by model.state.collectAsState()
    val item = model.item
    val trashed = item.trashedAtMs != null
    var deleteAsked by remember { mutableStateOf(false) }
    // The code, every second while this shows.
    if (!item.totp.isNullOrEmpty()) {
        LaunchedEffect(model) {
            while (true) {
                model.refreshCode()
                delay(1_000)
            }
        }
    }
    if (deleteAsked) {
        AlertDialog(
            onDismissRequest = { deleteAsked = false },
            text = { Text(stringResource(R.string.delete_forever_ask)) },
            confirmButton = {
                TextButton(onClick = { deleteAsked = false; vault.deleteForever(model.entry) }, modifier = Modifier.testTag("confirmDelete")) {
                    Text(stringResource(R.string.delete_forever))
                }
            },
            dismissButton = { TextButton(onClick = { deleteAsked = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
    Page(item.title, null, tag = "detailScreen") {
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s2.dp)) {
            TextButton(onClick = { vault.back() }, modifier = Modifier.testTag("back")) { Text(stringResource(R.string.back)) }
            Spacer(Modifier.weight(1f))
            if (!trashed) TextButton(onClick = { vault.edit(model.entry) }, modifier = Modifier.testTag("edit")) { Text(stringResource(R.string.edit)) }
        }
        when (item.kind) {
            Item.Kind.LOGIN -> LoginFields(model, state)
            Item.Kind.CARD -> CardFields(model, state)
            Item.Kind.IDENTITY -> IdentityFields(model, state)
            else -> Unit
        }
        if (item.notes.isNotEmpty()) FieldRow(model, state, stringResource(R.string.notes), item.notes, "notes")
        item.customFields.forEachIndexed { i, field ->
            FieldRow(model, state, field.name, field.value, "field.$i", secret = field.isSecret)
        }
        if (item.tags.isNotEmpty()) Plain(stringResource(R.string.tags), item.tags.joinToString(", "))
        if (!item.isEditable) Text(stringResource(R.string.read_only), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
        if (trashed) {
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s2.dp)) {
                TextButton(onClick = { vault.restore(model.entry) }, modifier = Modifier.testTag("restore")) { Text(stringResource(R.string.restore)) }
                TextButton(onClick = { deleteAsked = true }, modifier = Modifier.testTag("deleteForever")) {
                    Text(stringResource(R.string.delete_forever), color = color(Tokens.Color.danger))
                }
            }
        } else {
            TextButton(onClick = { vault.trash(model.entry) }, modifier = Modifier.testTag("trash")) {
                Text(stringResource(R.string.move_to_trash), color = color(Tokens.Color.danger))
            }
        }
    }
}

@Composable
private fun LoginFields(model: ItemDetailModel, state: ItemDetailModel.State) {
    val item = model.item
    val context = LocalContext.current
    if (item.username.isNotEmpty()) FieldRow(model, state, stringResource(R.string.username), item.username, "username")
    if (item.password.isNotEmpty()) FieldRow(model, state, stringResource(R.string.password), item.password, "password", secret = true, monospace = true)
    if (!item.totp.isNullOrEmpty()) {
        val code = state.code
        Column(Modifier.fillMaxWidth()) {
            Label(stringResource(R.string.one_time_code))
            if (code != null) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        code.code.value.chunked(3).joinToString(" "), style = Tokens.Text.headline.compose().copy(fontFamily = FontFamily.Monospace),
                        color = color(Tokens.Color.text), modifier = Modifier.weight(1f).testTag("code"),
                    )
                    Text(stringResource(R.string.code_seconds, code.secondsLeft.toInt()), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
                    CopyButton(state.copied == "code", "copy.code", model::copyCode)
                }
            } else if (state.codeUnsupported) {
                Text(stringResource(R.string.code_unsupported), style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
            }
        }
    }
    for (url in item.urls) {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
            Column(Modifier.weight(1f)) {
                Label(stringResource(R.string.website))
                Text(url.url, style = Tokens.Text.body.compose(), color = color(Tokens.Color.text))
            }
            if (url.url.startsWith("https://") || url.url.startsWith("http://")) {
                TextButton(onClick = {
                    try {
                        context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url.url)))
                    } catch (e: ActivityNotFoundException) {
                        // No browser: nothing to open it with.
                    }
                }) { Text(stringResource(R.string.open)) }
            }
        }
    }
    // Read-only: a passkey's keys never leave the core.
    for (site in item.passkeySites) {
        Text(stringResource(R.string.passkey_for, site), style = Tokens.Text.body.compose(), color = color(Tokens.Color.text))
    }
    val history = item.passwordHistory
    if (history.isNotEmpty()) {
        Label(stringResource(R.string.password_history))
        history.forEachIndexed { i, previous ->
            val at = previous.replacedAtMs?.let { DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(it)) }
            FieldRow(model, state, at?.let { stringResource(R.string.replaced_on, it) } ?: "", previous.password, "history.$i", secret = true, monospace = true)
        }
    }
}

@Composable
private fun CardFields(model: ItemDetailModel, state: ItemDetailModel.State) {
    val card = model.item.card ?: return
    if (card.cardholder.isNotEmpty()) FieldRow(model, state, stringResource(R.string.cardholder), card.cardholder, "cardholder")
    if (card.number.isNotEmpty()) FieldRow(model, state, stringResource(R.string.card_number), card.number, "number", secret = true, monospace = true)
    if (card.expiryMonth != null || card.expiryYear != null) {
        val month = card.expiryMonth?.toString()?.padStart(2, '0') ?: "--"
        Plain(stringResource(R.string.expiry), "$month / ${card.expiryYear ?: "----"}")
    }
    if (card.securityCode.isNotEmpty()) FieldRow(model, state, stringResource(R.string.security_code), card.securityCode, "securityCode", secret = true, monospace = true)
    if (card.brand.isNotEmpty()) Plain(stringResource(R.string.brand), card.brand)
}

@Composable
private fun IdentityFields(model: ItemDetailModel, state: ItemDetailModel.State) {
    val identity = model.item.identity ?: return
    val name = listOf(identity.firstName, identity.middleName, identity.lastName).filter(String::isNotBlank).joinToString(" ")
    if (name.isNotEmpty()) FieldRow(model, state, stringResource(R.string.first_name), name, "name")
    if (identity.email.isNotEmpty()) FieldRow(model, state, stringResource(R.string.email), identity.email, "email")
    if (identity.phone.isNotEmpty()) FieldRow(model, state, stringResource(R.string.phone), identity.phone, "phone")
    if (identity.company.isNotEmpty()) FieldRow(model, state, stringResource(R.string.company), identity.company, "company")
    val address = (identity.addressLines + listOf(
        listOf(identity.postalCode, identity.city).filter(String::isNotBlank).joinToString(" "),
        identity.region,
        identity.country,
    )).filter(String::isNotBlank)
    if (address.isNotEmpty()) FieldRow(model, state, stringResource(R.string.address), address.joinToString("\n"), "address")
}

@Composable
private fun Label(text: String) {
    if (text.isNotEmpty()) Text(text, style = Tokens.Text.caption.compose(), color = color(Tokens.Color.textMuted))
}

@Composable
private fun Plain(label: String, value: String?) {
    Column(Modifier.fillMaxWidth()) {
        Label(label)
        if (value != null) Text(value, style = Tokens.Text.body.compose(), color = color(Tokens.Color.text))
    }
}

/** A field with Copy, and Show / Hide for a secret one. */
@Composable
private fun FieldRow(
    model: ItemDetailModel,
    state: ItemDetailModel.State,
    label: String,
    value: String,
    key: String,
    secret: Boolean = false,
    monospace: Boolean = false,
) {
    val shown = !secret || key in state.revealed
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.weight(1f)) {
            Label(label)
            Text(
                if (shown) value else ItemDetailModel.masked(value),
                style = (if (monospace) Tokens.Text.secret else Tokens.Text.body).compose(),
                color = color(Tokens.Color.text),
                modifier = Modifier.testTag("value.$key"),
            )
        }
        if (secret) {
            TextButton(onClick = { model.toggle(key) }, modifier = Modifier.testTag("reveal.$key")) {
                Text(stringResource(if (shown) R.string.hide else R.string.show))
            }
        }
        CopyButton(state.copied == key, "copy.$key") { model.copy(value, key) }
    }
}

@Composable
internal fun CopyButton(copied: Boolean, tag: String, onClick: () -> Unit) {
    TextButton(onClick = onClick, modifier = Modifier.testTag(tag)) {
        Text(stringResource(if (copied) R.string.copied else R.string.copy))
    }
}
