package pwm.kit

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The item fields in Kotlin (D54 §4) against D18 JSON, with fields from a newer version kept. */
class ItemTest {
    private val card = Item.parse(
        """{"id":"c1","v":1,"type":"card","title":"Visa","card":{"cardholder":"Ada","number":"4111 1111 1111 1234",
           "expiry_month":7,"expiry_year":2031,"security_code":"123","brand":"visa","x_future":"kept"},
           "custom_fields":[{"name":"PIN","value":"9876","kind":"hidden","x_new":1},{"name":"Seen","value":"x","kind":"hologram"}],
           "x_top":{"a":1}}""",
    )

    @Test
    fun cardsReadEveryFieldAndTheLastFour() {
        val c = card.card!!
        assertEquals("Ada", c.cardholder)
        assertEquals("1234", c.lastFour)
        assertEquals(7, c.expiryMonth)
        assertEquals(2031, c.expiryYear)
        assertEquals("123", c.securityCode)
        assertEquals("Card(<redacted>)", c.toString())
        assertEquals(listOf("PIN", "Seen"), card.customFields.map { it.name })
        assertEquals(Item.CustomField.Kind.HIDDEN, card.customFields[0].kind)
        assertNull("a kind from a newer version", card.customFields[1].kind)
        assertTrue(card.customFields[1].isSecret)
    }

    @Test
    fun editingKeepsFieldsANewerVersionWrote() {
        val edited = card.edited {
            number = "5555 4444 3333 2222"
            expiryMonth = null
            setCustomFields(listOf(Item.CustomField("PIN", "1111", Item.CustomField.Kind.HIDDEN), card.customFields[1]))
        }
        val json = edited.json.toString()
        assertTrue(json.contains("\"x_future\":\"kept\""))
        assertTrue(json.contains("\"x_top\""))
        assertTrue(json.contains("\"x_new\":1"))
        assertTrue("an unknown kind stays", json.contains("\"kind\":\"hologram\""))
        assertEquals("2222", edited.card!!.lastFour)
        assertNull(edited.card!!.expiryMonth)
        assertEquals("1111", edited.customFields[0].value)
    }

    @Test
    fun identitiesAndTheirAddress() {
        val item = Item.new(Item.Kind.IDENTITY, "Me", "i1", 1).edited {
            setIdentity("first_name", "Ada")
            setIdentity("postal_code", "10557")
            setAddressLines(listOf("1 Main St", "Flat 2"))
        }
        val identity = item.identity!!
        assertEquals("Ada", identity.firstName)
        assertEquals("10557", identity.postalCode)
        assertEquals(listOf("1 Main St", "Flat 2"), identity.addressLines)
        assertTrue(runCatching { item.edited { setIdentity("nickname", "x") } }.isFailure)
    }

    @Test
    fun loginsHavePasskeysAndAPasswordHistoryNewestFirst() {
        val login = Item.parse(
            """{"id":"l1","v":1,"type":"login","title":"Bank","login":{"username":"ada","password":"third",
               "passkeys":[{"rp_id":"bank.example","credential_id":"x"}]},
               "history":[{"saved_at":30,"login":{"password":"second"}},{"saved_at":20,"login":{"password":"second"}},
                          {"saved_at":10,"login":{"password":"first"}}]}""",
        )
        assertEquals(listOf("bank.example"), login.passkeySites)
        assertEquals(listOf("second", "first"), login.passwordHistory.map { it.password })
        assertEquals(30L, login.passwordHistory[0].replacedAtMs)
        assertFalse(login.passwordHistory.toString().contains("second"))
    }

    @Test
    fun emptyAndAbsentFieldsAreTheSame() {
        val item = Item.new(Item.Kind.LOGIN, "Bank", "l1", 1)
        val touched = item.edited {
            notes = ""
            favorite = false
            tags = emptyList()
            username = ""
            folderId = null
        }
        assertTrue(touched.sameFields(item))
        assertFalse(item.edited { username = "ada" }.sameFields(item))
    }

    @Test
    fun aNoteHasNoLoginFieldsToSet() {
        val note = Item.new(Item.Kind.NOTE, "Note", "n1", 1)
        assertEquals("invalid_item", (runCatching { note.edited { username = "x" } }.exceptionOrNull() as PwmError).code)
    }
}
