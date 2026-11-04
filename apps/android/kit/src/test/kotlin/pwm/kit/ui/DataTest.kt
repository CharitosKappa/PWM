package pwm.kit.ui

import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import pwm.kit.ColumnMap
import pwm.kit.ImportFile
import pwm.kit.ImportGuess
import pwm.kit.Item
import pwm.kit.PasswordHealth
import pwm.kit.PwmError

/** Settings' data screens (D59 §8): health, export and import. */
class DataTest {
    private fun TestScope.unlocked(prefs: MemoryPrefs = MemoryPrefs()): Harness {
        val h = Harness(this, prefs)
        h.nextAccount = { FakeAccount(it, signedIn = true) }
        h.app.start()
        val unlock = (h.screen as AppModel.Screen.Unlock).model
        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        return h
    }

    private fun login(id: String, title: String) = Item.new(Item.Kind.LOGIN, title, id, 1).edited { username = "ada" }

    @Test
    fun healthListsWeakReusedAndLeakedAndChangeOpensTheEditor() = runTest {
        val h = unlocked()
        val account = h.accounts.single()
        listOf(login("1", "Bank"), login("2", "Mail"), login("3", "Shop"), login("4", "Forum")).forEach(account::add)
        account.health = PasswordHealth(mapOf("4" to 1, "gone" to 0), listOf(listOf("2", "1"), listOf("3", "gone")))
        account.breached = mapOf("3" to 1200L)

        h.home.setTab(HomeModel.Tab.SETTINGS)
        h.home.settings.showHealth()
        val health = h.home.settings.state.value.health!!
        health.load()
        advanceUntilIdle()
        val s = health.state.value
        assertEquals(listOf("Forum"), s.weak.map { it.item.title })
        assertEquals("a group with one item left is no group", listOf(listOf("Bank", "Mail")), s.reused.map { g -> g.map { it.item.title } })
        assertEquals(listOf("Shop" to 1200L), s.leaked.map { it.entry.item.title to it.count })
        assertTrue(s.checkedLeaks)
        assertFalse(s.isClean)

        health.edit(s.weak.single())
        assertEquals("Change goes to the vault tab", HomeModel.Tab.VAULT, h.home.state.value.tab)
        assertTrue(h.vault.state.value.stack.last() is VaultModel.Route.Edit)
    }

    @Test
    fun theLeakCheckRunsOnlyWhenSwitchedOnAndItsFailureKeepsTheRest() = runTest {
        val h = unlocked()
        val account = h.accounts.single()
        account.add(login("1", "Bank"))
        h.home.settings.setBreachCheck(false)
        assertFalse(h.prefs.breachCheck)
        h.home.settings.showHealth()
        h.home.settings.state.value.health!!.load()
        advanceUntilIdle()
        assertEquals(0, account.breachChecks)
        assertTrue("nothing found", h.home.settings.state.value.health!!.state.value.isClean)
        assertTrue(h.home.settings.back())
        assertNull(h.home.settings.state.value.health)

        h.home.settings.setBreachCheck(true)
        account.health = PasswordHealth(mapOf("1" to 0), emptyList())
        account.breachError = PwmError("breach_check")
        h.home.settings.showHealth()
        val health = h.home.settings.state.value.health!!
        health.load()
        advanceUntilIdle()
        assertEquals(1, account.breachChecks)
        assertEquals(1, health.state.value.weak.size)
        assertFalse(health.state.value.checkedLeaks)
        assertEquals("The leak check is not answering. Try again later.", health.state.value.error)
    }

    @Test
    fun exportAsksTheMasterPasswordAndChecksEachFormat() = runTest {
        val h = unlocked()
        val account = h.accounts.single()
        h.home.settings.showExport()
        val export = h.home.settings.state.value.export!!

        // Encrypted: a strong export password, twice.
        export.setMasterPassword(FakeAccount.PASSWORD)
        export.setExportPassword("short")
        export.setConfirm("short")
        advanceUntilIdle()
        assertFalse("too weak", export.state.value.canExport)
        export.setExportPassword("violet tractor quiz lantern 42")
        advanceUntilIdle()
        assertFalse("not the same twice", export.state.value.canExport)
        export.setConfirm("violet tractor quiz lantern 42")
        assertTrue(export.state.value.canExport)

        // A wrong master password: nothing made, and the field empties.
        export.setMasterPassword("wrong")
        export.export()
        advanceUntilIdle()
        assertNull(export.state.value.file)
        assertEquals("Wrong master password.", export.state.value.error)
        assertEquals("", export.state.value.masterPassword.value)

        export.setMasterPassword(FakeAccount.PASSWORD)
        export.export()
        advanceUntilIdle()
        val made = export.state.value
        assertTrue("export pwmx" in account.calls)
        assertTrue(made.fileName.matches(Regex("""PWM export \d{4}-\d{2}-\d{2}\.pwmx""")))
        assertEquals("", made.masterPassword.value)
        assertEquals("", made.exportPassword.value)
        export.didSave()
        assertNull("the bytes go once saved", export.state.value.file)
        assertTrue(export.state.value.saved)

        // Plaintext: only after the warning, and the master password again.
        export.setChoice(ExportModel.Choice.CSV)
        export.setMasterPassword(FakeAccount.PASSWORD)
        assertFalse(export.state.value.canExport)
        export.setUnderstood(true)
        export.export()
        advanceUntilIdle()
        assertTrue("export csv" in account.calls)
        assertEquals("text/csv", export.state.value.mimeType)
        export.saveFailed()
        assertEquals("The export couldn't be saved. Try again.", export.state.value.error)
        assertTrue("kept for another try", export.state.value.file != null)

        // Leaving drops the file.
        assertTrue(h.home.settings.back())
        assertNull(export.state.value.file)
    }

    @Test
    fun importReadsACsvChecksItAndOffersToDeleteThePlainFile() = runTest {
        val h = unlocked()
        val account = h.accounts.single()
        h.files.files["content://picked/1"] = FakeFiles.File("logins.csv", "name,url,username,password\n".toByteArray())
        h.home.settings.showImport()
        val import = h.home.settings.state.value.importing!!

        assertFalse(import.state.value.canCheck)
        import.picked("content://picked/1")
        advanceUntilIdle()
        val picked = import.state.value
        assertEquals("logins.csv", picked.fileName)
        assertEquals("csv", picked.format)
        assertEquals(ColumnMap(title = 0, url = 1, username = 2, password = 3), picked.columnMap)
        assertFalse(picked.asksPassword)

        import.setColumn(ColumnMap.Field.TITLE, null)
        import.check()
        advanceUntilIdle()
        assertEquals("csv" to ColumnMap(url = 1, username = 2, password = 3), account.prepared)
        assertEquals(2, import.state.value.summary!!.logins)
        import.setColumn(ColumnMap.Field.TITLE, 0)
        assertNull("a change asks to check again", import.state.value.summary)
        import.check()
        advanceUntilIdle()

        import.importNow()
        advanceUntilIdle()
        val done = import.state.value
        assertTrue("finish" in account.calls)
        assertEquals(2, done.done!!.logins)
        assertEquals(listOf(ImportModel.Kind.LOGINS to 2), ImportModel.counts(done.done!!))
        assertTrue(done.plaintext)
        assertTrue(done.canDelete)

        // A place that can't delete says so; one that can, deletes.
        h.files.deletable = false
        import.deleteFile()
        advanceUntilIdle()
        assertEquals("The file couldn't be deleted. Delete it yourself in your Files app.", import.state.value.error)
        h.files.deletable = true
        import.deleteFile()
        advanceUntilIdle()
        assertTrue(import.state.value.deleted)
        assertFalse(import.state.value.canDelete)
        assertTrue(h.files.files.isEmpty())
    }

    @Test
    fun anEncryptedExportNeedsItsPasswordAndALargeFileIsNeverRead() = runTest {
        val h = unlocked()
        val account = h.accounts.single()
        account.guess = ImportGuess("bitwarden", needsPassword = true)
        account.importPassword = "export pass"
        h.files.files["content://big"] = FakeFiles.File("huge.csv", ByteArray(1), size = ImportFile.MAX_BYTES + 1)
        h.files.files["content://bw"] = FakeFiles.File("bitwarden.json", "{}".toByteArray())
        h.home.settings.showImport()
        val import = h.home.settings.state.value.importing!!

        import.picked("content://big")
        advanceUntilIdle()
        assertEquals("This file is too large to import.", import.state.value.error)
        assertFalse("refused before reading", "content://big" in h.files.reads)
        assertFalse(import.state.value.canCheck)

        import.picked("content://bw")
        advanceUntilIdle()
        assertNull(import.state.value.error)
        assertTrue(import.state.value.asksPassword)
        import.check()
        advanceUntilIdle()
        assertNull(import.state.value.summary)
        assertTrue(import.state.value.error != null)
        import.setPassword("export pass")
        import.check()
        advanceUntilIdle()
        assertTrue(import.state.value.summary != null)

        // Locking while a preview shows drops it.
        h.home.settings.lockNow()
        advanceUntilIdle()
        assertTrue("cancelImport" in account.calls)
        assertNull(import.state.value.summary)
        assertTrue(h.screen is AppModel.Screen.Unlock)
    }
}
