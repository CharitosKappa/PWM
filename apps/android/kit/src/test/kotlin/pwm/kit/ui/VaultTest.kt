package pwm.kit.ui

import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import pwm.kit.Item

/** The vault screens' models (D54 §9), over the fake account's in-memory vault. */
class VaultTest {
    private fun login(id: String, title: String, username: String = "", url: String? = null, folder: String? = null) =
        Item.new(Item.Kind.LOGIN, title, id, 1).edited {
            this.username = username
            if (url != null) setUrls(listOf(Item.Url(url)))
            folderId = folder
        }

    private fun card(id: String, title: String, number: String) = Item.new(Item.Kind.CARD, title, id, 1).edited { this.number = number }

    /** Unlocked on the vault, with [items] in it. */
    private fun TestScope.unlocked(vararg items: Item, prefs: MemoryPrefs = MemoryPrefs()): Harness {
        val h = Harness(this, prefs)
        h.nextAccount = { FakeAccount(it, signedIn = true).apply { items.forEach(::add) } }
        h.app.start()
        val unlock = (h.screen as AppModel.Screen.Unlock).model
        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        return h
    }

    private val Harness.visible get() = vault.state.value.visible.map { it.item.title }

    @Test
    fun searchMatchesTitleUsernameWebsiteAndLastFourAndSkipsTrashAndFolders() = runTest {
        val h = unlocked(
            login("1", "bank", username = "ada@example.com"),
            login("2", "Mail", url = "https://mail.example.org"),
            card("3", "Visa", "4111 1111 1111 9876"),
            Item.new(Item.Kind.FOLDER, "Bank folder", "4", 1),
            Item.new(Item.Kind.NOTE, "Alpha", "5", 1),
        )
        h.vault.setTrashed("5")
        assertEquals("by title, case aside", listOf("bank", "Mail", "Visa"), h.visible)
        h.vault.setQuery("  ADA ")
        assertEquals(listOf("bank"), h.visible)
        h.vault.setQuery("mail.example")
        assertEquals(listOf("Mail"), h.visible)
        h.vault.setQuery("9876")
        assertEquals(listOf("Visa"), h.visible)
        h.vault.setQuery("bank")
        assertEquals("folders aren't items", listOf("bank"), h.visible)
        h.vault.setQuery("alpha")
        assertEquals("trashed items aren't searched", emptyList<String>(), h.visible)
        assertEquals(listOf("Bank folder"), h.vault.state.value.folders.map { it.item.title })
    }

    private suspend fun VaultModel.setTrashed(id: String) {
        account.setTrashed(id, FakeAccount.VAULT, trashed = true)
        load()
    }

    @Test
    fun fiveThousandItemsFilterWithoutADelay() = runTest {
        val items = (1..5_000).map { login("id-$it", "Site $it", username = "user$it@example.com", url = "https://site$it.example") }
        val h = unlocked(*items.toTypedArray())
        assertEquals(5_000, h.vault.state.value.visible.size)
        val started = System.nanoTime()
        h.vault.setQuery("user4999@")
        val ms = (System.nanoTime() - started) / 1_000_000
        assertEquals(listOf("Site 4999"), h.visible)
        assertTrue("took $ms ms", ms < 500)
    }

    @Test
    fun sectionsFilterByFavoriteTypeFolderAndTrash() = runTest {
        val favorite = Item.new(Item.Kind.NOTE, "Fav", "1", 1).edited { this.favorite = true }
        val h = unlocked(favorite, login("2", "Inside", folder = "f1"), card("3", "Card", "1234"), Item.new(Item.Kind.FOLDER, "F", "f1", 1))
        h.vault.setSection(Section.Favorites)
        assertEquals(listOf("Fav"), h.visible)
        h.vault.setSection(Section.OfKind(Item.Kind.CARD))
        assertEquals(listOf("Card"), h.visible)
        h.vault.setSection(Section.Folder("f1"))
        assertEquals(listOf("Inside"), h.visible)
        h.vault.setSection(Section.Trash)
        assertEquals(emptyList<String>(), h.visible)
    }

    @Test
    fun aNewItemGoesIntoTheFolderShownAndSavesThenSyncs() = runTest {
        val h = unlocked(Item.new(Item.Kind.FOLDER, "Work", "f1", 1))
        h.vault.setSection(Section.Folder("f1"))
        h.vault.newItem(Item.Kind.LOGIN)
        advanceUntilIdle()
        val edit = (h.vault.state.value.stack.single() as VaultModel.Route.Edit).model
        assertTrue(edit.isNew)
        assertEquals("f1", edit.state.value.folderId)
        assertFalse("needs a title", edit.canSave)
        edit.edit { it.copy(title = "Payroll", username = "ada") }
        assertTrue(edit.canSave)
        val syncs = h.accounts.single().syncs
        edit.save()
        advanceUntilIdle()
        assertTrue("the edit closed", h.vault.state.value.stack.isEmpty())
        assertEquals(listOf("Payroll"), h.visible)
        assertTrue(h.accounts.single().syncs > syncs)
    }

    @Test
    fun editingALoginKeepsUnknownFieldsAndRefreshesTheDetail() = runTest {
        val stored = Item.parse(
            """{"id":"1","v":1,"type":"login","title":"Bank","x_future":1,"login":{"username":"ada","password":"pw","x_new":"keep"}}""",
        )
        val h = unlocked(stored)
        h.vault.open(h.vault.state.value.visible.single())
        val detail = (h.vault.state.value.stack.single() as VaultModel.Route.Detail).model
        h.vault.edit(detail.entry)
        val edit = (h.vault.state.value.stack.last() as VaultModel.Route.Edit).model
        assertFalse("nothing changed yet", edit.canSave)
        edit.edit { it.copy(username = "grace") }
        edit.save()
        advanceUntilIdle()
        val saved = h.accounts.single().items.getValue("1").json.toString()
        assertTrue(saved.contains("\"x_future\":1"))
        assertTrue(saved.contains("\"x_new\":\"keep\""))
        val shown = (h.vault.state.value.stack.single() as VaultModel.Route.Detail).model
        assertEquals("grace", shown.item.username)
    }

    @Test
    fun backAsksBeforeDiscardingChanges() = runTest {
        val h = unlocked(login("1", "Bank"))
        val entry = h.vault.state.value.visible.single()
        h.vault.edit(entry)
        assertTrue(h.app.back())
        assertTrue("no changes: it just closes", h.vault.state.value.stack.isEmpty())

        h.vault.edit(entry)
        val edit = (h.vault.state.value.stack.single() as VaultModel.Route.Edit).model
        edit.edit { it.copy(title = "Bank 2") }
        assertTrue(h.app.back())
        assertTrue(h.vault.state.value.discardAsked)
        h.vault.keepEditing()
        assertEquals(1, h.vault.state.value.stack.size)
        h.app.back()
        h.vault.confirmDiscard()
        assertTrue(h.vault.state.value.stack.isEmpty())
        assertEquals("Bank", h.accounts.single().items.getValue("1").title)
        assertFalse("back on the list is the system's", h.app.back())
    }

    @Test
    fun itemsFromANewerVersionAreReadOnly() = runTest {
        val newer = Item.parse("""{"id":"1","v":2,"type":"login","title":"New","login":{}}""")
        val h = unlocked(newer, Item.parse("""{"id":"2","v":1,"type":"hologram","title":"Odd"}"""))
        for (entry in h.vault.state.value.visible) {
            h.vault.edit(entry)
            val edit = (h.vault.state.value.stack.last() as VaultModel.Route.Edit).model
            assertFalse(edit.isEditable)
            edit.edit { it.copy(title = "Changed") }
            assertFalse(edit.canSave)
        }
    }

    @Test
    fun cardsCheckTheExpiryAndCodesAreCheckedByTheCore() = runTest {
        val h = unlocked(card("1", "Visa", "4111"), login("2", "Mail"))
        val (mail, visa) = h.vault.state.value.visible
        h.vault.edit(visa)
        val cardEdit = (h.vault.state.value.stack.last() as VaultModel.Route.Edit).model
        cardEdit.edit { it.copy(expiryMonth = "13") }
        cardEdit.save()
        advanceUntilIdle()
        assertEquals("Enter the expiry month as a number from 1 to 12.", cardEdit.state.value.error)
        cardEdit.edit { it.copy(expiryMonth = "7", expiryYear = "2031") }
        cardEdit.save()
        advanceUntilIdle()
        assertEquals(7, h.accounts.single().items.getValue("1").card!!.expiryMonth)

        h.vault.edit(mail)
        val loginEdit = (h.vault.state.value.stack.last() as VaultModel.Route.Edit).model
        loginEdit.edit { it.copy(totp = pwm.kit.Redacted("not a key!")) }
        loginEdit.save()
        advanceUntilIdle()
        assertEquals(
            "That's not a one-time code setup key. Paste the key or scan the site's QR code.",
            loginEdit.state.value.error,
        )
        loginEdit.edit { it.copy(totp = pwm.kit.Redacted("JBSWY3DPEHPK3PXP")) }
        loginEdit.save()
        advanceUntilIdle()
        assertTrue(h.accounts.single().items.getValue("2").totp!!.startsWith("otpauth://totp/"))
    }

    @Test
    fun identitiesWebsitesAndOtherFields() = runTest {
        val h = unlocked(Item.new(Item.Kind.IDENTITY, "Me", "1", 1), login("2", "Mail"))
        val (mail, me) = h.vault.state.value.visible
        h.vault.edit(me)
        val identity = (h.vault.state.value.stack.last() as VaultModel.Route.Edit).model
        identity.edit { it.copy(identity = it.identity + ("first_name" to "Ada"), address = "1 Main St\n\n Flat 2 ") }
        identity.addField(Item.CustomField.Kind.HIDDEN)
        val row = identity.state.value.customFields.single()
        identity.edit { s -> s.copy(customFields = s.customFields.map { if (it.id == row.id) it.copy(name = "PIN", value = pwm.kit.Redacted("1234")) else it }) }
        identity.save()
        advanceUntilIdle()
        val saved = h.accounts.single().items.getValue("1")
        assertEquals("Ada", saved.identity!!.firstName)
        assertEquals(listOf("1 Main St", "Flat 2"), saved.identity!!.addressLines)
        assertEquals("PIN", saved.customFields.single().name)

        h.vault.edit(mail)
        val edit = (h.vault.state.value.stack.last() as VaultModel.Route.Edit).model
        edit.addUrl()
        edit.addUrl()
        val (first, second) = edit.state.value.urls
        edit.edit { s -> s.copy(urls = s.urls.map { if (it.id == first.id) it.copy(url = "https://mail.example", match = "host") else it }) }
        edit.removeUrl(second.id)
        edit.save()
        advanceUntilIdle()
        assertEquals(listOf(Item.Url("https://mail.example", "host")), h.accounts.single().items.getValue("2").urls)
    }

    @Test
    fun theFolderPickerMakesAFolder() = runTest {
        val h = unlocked(login("1", "Bank"))
        h.vault.edit(h.vault.state.value.visible.single())
        val edit = (h.vault.state.value.stack.single() as VaultModel.Route.Edit).model
        edit.newFolder("  Money ")
        advanceUntilIdle()
        val folder = edit.state.value.folders.single()
        assertEquals("Money", folder.item.title)
        assertEquals(folder.id, edit.state.value.folderId)
    }

    @Test
    fun trashRestoreAndDeleteForever() = runTest {
        val h = unlocked(login("1", "Bank"))
        h.vault.open(h.vault.state.value.visible.single())
        h.vault.trash(h.vault.state.value.visible.single())
        advanceUntilIdle()
        assertTrue("back to the list", h.vault.state.value.stack.isEmpty())
        assertEquals(emptyList<String>(), h.visible)
        h.vault.setSection(Section.Trash)
        val trashed = h.vault.state.value.visible.single()
        h.vault.restore(trashed)
        advanceUntilIdle()
        assertEquals(emptyList<String>(), h.visible)
        h.vault.setSection(Section.All)
        assertEquals(listOf("Bank"), h.visible)
        h.vault.trash(h.vault.state.value.visible.single())
        advanceUntilIdle()
        h.vault.deleteForever(h.vault.state.value.entries.single())
        advanceUntilIdle()
        assertTrue(h.accounts.single().items.isEmpty())
        assertTrue("old trash is purged after a sync", h.accounts.single().purges > 0)
    }

    @Test
    fun theDetailMasksCopiesAndShowsTheCode() = runTest {
        val h = unlocked(login("1", "Bank").edited { password = "hunter2"; totp = "otpauth://totp/Bank?secret=JBSWY3DPEHPK3PXP" })
        h.vault.open(h.vault.state.value.visible.single())
        val detail = (h.vault.state.value.stack.single() as VaultModel.Route.Detail).model
        assertEquals("••••••••", ItemDetailModel.masked("hunter2"))
        detail.toggle("password")
        assertTrue("password" in detail.state.value.revealed)
        detail.copy(detail.item.password, "password")
        assertEquals("hunter2", h.copied.single().value)
        assertEquals(30_000L, h.copied.single().clearAfterMs)
        detail.refreshCode()
        assertEquals("123456", detail.state.value.code!!.code.value)
        detail.copyCode()
        assertEquals("123456", h.copied.last().value)
        h.app.back()
        assertTrue("masked again when it goes", detail.state.value.revealed.isEmpty())
        assertNull(detail.state.value.code)
    }

    /** On Free the screen says codes are part of Premium, not that the code is unsupported (D73 §1). */
    @Test
    fun onFreeTheCodeNeedsAPlan() = runTest {
        val h = unlocked(login("1", "Bank").edited { totp = "otpauth://totp/Bank?secret=JBSWY3DPEHPK3PXP" })
        h.accounts.single().codeNeedsPlan = true
        h.vault.open(h.vault.state.value.visible.single())
        val detail = (h.vault.state.value.stack.single() as VaultModel.Route.Detail).model
        detail.refreshCode()
        assertNull(detail.state.value.code)
        assertTrue(detail.state.value.codeNeedsPlan)
        assertFalse(detail.state.value.codeUnsupported)
        detail.copyCode()
        assertTrue(h.copied.isEmpty())
        h.accounts.single().codeNeedsPlan = false
        detail.refreshCode()
        assertFalse(detail.state.value.codeNeedsPlan)
        assertEquals("123456", detail.state.value.code!!.code.value)
    }

    @Test
    fun theGeneratorsOptions() = runTest {
        val h = unlocked()
        val generator = h.home.generator
        assertEquals(20, generator.state.value.value!!.value.length)
        generator.options { it.copy(length = 32, symbols = false) }
        val value = generator.state.value.value!!.value
        assertEquals(32, value.length)
        assertTrue(value.all(Char::isLetterOrDigit))
        generator.options { it.copy(lowercase = false, uppercase = false, digits = false) }
        assertNull(generator.state.value.value)
        assertEquals("Choose at least one kind of character.", generator.state.value.error)
        generator.options { it.copy(mode = GeneratorModel.Mode.PASSPHRASE, words = 4, separator = ".") }
        assertEquals(4, generator.state.value.value!!.value.split('.').size)
        advanceUntilIdle()
        assertTrue(generator.state.value.strength != null)
        generator.copy()
        assertTrue(generator.state.value.copied)
        assertFalse(generator.state.value.toString().contains(generator.state.value.value!!.value))
    }

    @Test
    fun theEditorsGeneratorFillsThePassword() = runTest {
        val h = unlocked(login("1", "Bank"))
        h.vault.edit(h.vault.state.value.visible.single())
        val edit = (h.vault.state.value.stack.single() as VaultModel.Route.Edit).model
        val generator = edit.makeGenerator()
        generator.regenerate()
        edit.use(generator.state.value.value!!)
        assertEquals(20, edit.state.value.password.value.length)
        assertTrue(edit.canSave)
    }

    @Test
    fun theLockTimeoutCountsTimeInTheBackground() = runTest {
        val h = unlocked()
        assertEquals(LockTimeout.STANDARD, h.home.settings.state.value.lockTimeout)
        h.app.appBackgrounded()
        h.now += 14 * 60_000L
        h.app.appForegrounded()
        assertTrue("14 minutes: still open", h.screen is AppModel.Screen.Vault)
        h.app.appBackgrounded()
        h.now += 15 * 60_000L
        h.app.appForegrounded()
        advanceUntilIdle()
        assertTrue(h.screen is AppModel.Screen.Unlock)
        assertFalse(h.accounts.single().unlocked)
    }

    @Test
    fun immediatelyLocksOnLeavingExceptForTheAppsOwnSystemScreens() = runTest {
        val h = unlocked(prefs = MemoryPrefs(lockTimeoutMinutes = 0))
        assertEquals(LockTimeout.IMMEDIATELY, h.home.settings.state.value.lockTimeout)
        h.app.openingSystemScreen()
        h.app.appBackgrounded()
        h.now += 60_000L
        h.app.appForegrounded()
        assertTrue("Save to or Print doesn't count", h.screen is AppModel.Screen.Vault)
        h.app.openingSystemScreen()
        h.app.appBackgrounded()
        h.now += 11 * 60_000L
        h.app.appForegrounded()
        assertTrue("unless it stays a long time", h.screen is AppModel.Screen.Unlock)
    }

    @Test
    fun immediatelyAndScreenOffLock() = runTest {
        val h = unlocked()
        h.home.settings.setLockTimeout(LockTimeout.IMMEDIATELY)
        assertEquals(0, h.prefs.lockTimeoutMinutes)
        h.app.appBackgrounded()
        assertTrue(h.screen is AppModel.Screen.Unlock)

        val again = (h.screen as AppModel.Screen.Unlock).model
        again.setPassword(FakeAccount.PASSWORD)
        again.submit()
        advanceUntilIdle()
        h.home.settings.setLockTimeout(LockTimeout.EIGHT_HOURS)
        h.app.screenOff()
        assertTrue("the screen turning off always locks", h.screen is AppModel.Screen.Unlock)
    }

    @Test
    fun tabsKeepTheirPlaceAndBackWalksThem() = runTest {
        val h = unlocked(login("1", "Bank"))
        h.vault.open(h.vault.state.value.visible.single())
        h.home.setTab(HomeModel.Tab.SETTINGS)
        h.home.settings.showNotices()
        assertTrue(h.app.back())
        assertEquals(SettingsModel.Page.MAIN, h.home.settings.state.value.page)
        assertTrue(h.app.back())
        assertEquals(HomeModel.Tab.VAULT, h.home.state.value.tab)
        assertEquals("the vault kept its place", 1, h.vault.state.value.stack.size)
        h.home.setTab(HomeModel.Tab.GENERATOR)
        assertTrue(h.app.back())
        assertEquals(HomeModel.Tab.VAULT, h.home.state.value.tab)
    }

    @Test
    fun settingsSignsOutAfterAskingAndSavesTheKitAgain() = runTest {
        val h = unlocked()
        val settings = h.home.settings
        assertEquals("ada@example.com", settings.state.value.email)
        settings.prepareKit()
        advanceUntilIdle()
        assertTrue(settings.state.value.kit != null)
        settings.didSaveKit()
        assertTrue(settings.state.value.kitSaved)
        settings.askSignOut()
        assertTrue(settings.state.value.signOutAsked)
        settings.keepSignedIn()
        assertFalse(settings.state.value.signOutAsked)
        settings.signOut()
        advanceUntilIdle()
        assertEquals(AppModel.Screen.Welcome, h.screen)
    }

    @Test
    fun aRevokedDeviceIsSignedOutAndAFailedSyncSaysSo() = runTest {
        val h = unlocked()
        h.accounts.single().syncResult = pwm.kit.SyncResult.Retry(1_000)
        h.vault.refresh()
        advanceUntilIdle()
        assertEquals(SyncNotice.OFFLINE, h.vault.state.value.notice)
        h.accounts.single().syncResult = pwm.kit.SyncResult.DeviceRevoked
        h.vault.refresh()
        advanceUntilIdle()
        assertEquals(AppModel.Screen.Welcome, h.screen)
        // The welcome screen says why (D74 §5), until the user moves on.
        assertEquals("This device was signed out from another device.", h.app.state.value.ended)
        h.app.showSignIn()
        assertNull(h.app.state.value.ended)
    }

    /** D74 §5: the account was deleted from another device. */
    @Test
    fun aDeletedAccountsDeviceIsWipedAndTheWelcomeScreenSaysSo() = runTest {
        val h = unlocked()
        h.accounts.single().syncResult = pwm.kit.SyncResult.AccountDeleted
        h.vault.refresh()
        advanceUntilIdle()
        assertEquals(AppModel.Screen.Welcome, h.screen)
        assertEquals("This account was deleted.", h.app.state.value.ended)
        assertFalse(h.accounts.single().isSignedIn())
    }
}
