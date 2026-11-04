package pwm.kit

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.longOrNull

/**
 * A vault item as the core gives it: D18 JSON with its id (D52 §4). Typed
 * reads for what the screens show; edits rewrite only the fields they
 * change, so fields this version doesn't know are kept, as D18 requires
 * (D54 §4). `toString` never shows a value.
 */
class Item private constructor(val json: JsonObject) {
    enum class Kind(val type: String) {
        LOGIN("login"), NOTE("note"), CARD("card"), IDENTITY("identity"), FOLDER("folder");

        companion object {
            fun of(type: String?): Kind? = entries.firstOrNull { it.type == type }
        }
    }

    /** A login's website and how it matches (`AUTOFILL.md`). */
    data class Url(val url: String, val match: String = "domain") {
        override fun toString(): String = "Url(<redacted>)"

        companion object {
            /** The match modes, in the order the editor offers them. */
            val MATCHES = listOf("domain", "host", "starts_with", "exact", "regex", "never")
        }
    }

    /** The `card` fields. Number and security code are secrets. */
    class Card internal constructor(private val o: JsonObject) {
        val cardholder: String get() = o.text("cardholder")
        val number: String get() = o.text("number")
        val expiryMonth: Int? get() = o.int("expiry_month")
        val expiryYear: Int? get() = o.int("expiry_year")
        val securityCode: String get() = o.text("security_code")
        val brand: String get() = o.text("brand")

        /** The last 4 digits, for lists and search. */
        val lastFour: String get() = number.filter(Char::isDigit).takeLast(4)

        override fun toString(): String = "Card(<redacted>)"
    }

    /** The `identity` fields. */
    class Identity internal constructor(private val o: JsonObject) {
        val firstName: String get() = o.text("first_name")
        val middleName: String get() = o.text("middle_name")
        val lastName: String get() = o.text("last_name")
        val email: String get() = o.text("email")
        val phone: String get() = o.text("phone")
        val company: String get() = o.text("company")
        val addressLines: List<String> get() = strings(o["address_lines"])
        val city: String get() = o.text("city")
        val region: String get() = o.text("region")
        val postalCode: String get() = o.text("postal_code")
        val country: String get() = o.text("country")

        override fun toString(): String = "Identity(<redacted>)"
    }

    /** A custom field of any item. A `hidden` value, or one of an unknown kind, is a secret. */
    class CustomField(val name: String, val value: String, val kind: Kind?) {
        enum class Kind(val key: String) {
            TEXT("text"), HIDDEN("hidden"), URL("url"), DATE("date");

            companion object {
                /** A missing kind is text, as in the core; an unknown one is null. */
                fun of(key: String?): Kind? = if (key == null) TEXT else entries.firstOrNull { it.key == key }
            }
        }

        val isSecret: Boolean get() = kind == null || kind == Kind.HIDDEN

        override fun toString(): String = "CustomField(<redacted>)"
    }

    /** A password a login had before, with when it was replaced. A secret. */
    class PreviousPassword(val password: String, val replacedAtMs: Long?) {
        override fun toString(): String = "PreviousPassword(<redacted>)"
    }

    val id: String? get() = json.textOrNull("id")
    val type: String get() = json.textOrNull("type") ?: ""
    /** Null for a type from a newer app version: shown read-only. */
    val kind: Kind? get() = Kind.of(type)
    val title: String get() = json.text("title")
    val notes: String get() = json.text("notes")
    val favorite: Boolean get() = (json["favorite"] as? JsonPrimitive)?.booleanOrNull ?: false
    val folderId: String? get() = json.textOrNull("folder_id")
    val tags: List<String> get() = strings(json["tags"])
    val createdAtMs: Long? get() = json.long("created_at")
    val updatedAtMs: Long? get() = json.long("updated_at")
    val trashedAtMs: Long? get() = json.long("trashed_at")
    val schemaVersion: Long get() = json.long("v") ?: 0
    /** Items from a newer schema, or of an unknown type, are shown read-only. */
    val isEditable: Boolean get() = schemaVersion <= SCHEMA_VERSION && kind != null

    private val login: JsonObject? get() = json["login"] as? JsonObject
    val username: String get() = login?.text("username") ?: ""
    val password: String get() = login?.text("password") ?: ""
    /** An `otpauth://` URI, when the login has a one-time code. */
    val totp: String? get() = login?.textOrNull("totp")
    val urls: List<Url>
        get() = (login?.get("urls") as? JsonArray).orEmpty().mapNotNull { element ->
            val o = element as? JsonObject ?: return@mapNotNull null
            val url = o.textOrNull("url") ?: return@mapNotNull null
            Url(url, o.textOrNull("match") ?: "domain")
        }

    /** The site of each passkey (its relying party). Their keys never leave the core. */
    val passkeySites: List<String>
        get() = (login?.get("passkeys") as? JsonArray).orEmpty().mapNotNull { (it as? JsonObject)?.textOrNull("rp_id") }

    val card: Card? get() = (json["card"] as? JsonObject)?.let(::Card)
    val identity: Identity? get() = (json["identity"] as? JsonObject)?.let(::Identity)

    val customFields: List<CustomField>
        get() = (json["custom_fields"] as? JsonArray).orEmpty().mapNotNull { element ->
            val o = element as? JsonObject ?: return@mapNotNull null
            CustomField(o.text("name"), o.text("value"), CustomField.Kind.of(o.textOrNull("kind")))
        }

    /** A login's earlier passwords from its history (D19), newest first. Read-only: the core keeps history. */
    val passwordHistory: List<PreviousPassword>
        get() {
            var newer = password
            val out = mutableListOf<PreviousPassword>()
            for (version in (json["history"] as? JsonArray).orEmpty()) {
                val o = version as? JsonObject ?: continue
                val previous = (o["login"] as? JsonObject)?.textOrNull("password") ?: continue
                if (previous.isNotEmpty() && previous != newer) out += PreviousPassword(previous, o.long("saved_at"))
                newer = previous
            }
            return out
        }

    /** The JSON the core takes back. */
    fun toJson(): Redacted<String> = Redacted(json.toString())

    /** A copy with these changes; everything else is kept as it was. */
    fun edited(change: Draft.() -> Unit): Item {
        val draft = Draft(json.toMutableMap())
        draft.change()
        return Item(draft.build())
    }

    /**
     * The same fields as [other], once fields that were absent and are now
     * empty are ignored, so opening and leaving an item unchanged is not an edit.
     */
    fun sameFields(other: Item): Boolean = normalized(json) == normalized(other.json)

    /**
     * The fields the screens edit. Unset ones stay as they are; the type's
     * own object (`login`, `card`, `identity`) must exist, as on a new item.
     */
    class Draft internal constructor(private val fields: MutableMap<String, JsonElement>) {
        private val sections = mutableMapOf<String, MutableMap<String, JsonElement>>()

        private fun section(name: String): MutableMap<String, JsonElement> = sections.getOrPut(name) {
            (fields[name] as? JsonObject)?.toMutableMap() ?: throw PwmError("invalid_item")
        }

        private fun peek(name: String): JsonObject? = sections[name]?.let(::JsonObject) ?: fields[name] as? JsonObject

        var title: String
            get() = JsonObject(fields).text("title")
            set(v) { fields["title"] = JsonPrimitive(v) }
        var notes: String
            get() = JsonObject(fields).text("notes")
            set(v) { fields["notes"] = JsonPrimitive(v) }
        var favorite: Boolean
            get() = (fields["favorite"] as? JsonPrimitive)?.booleanOrNull ?: false
            set(v) { fields["favorite"] = JsonPrimitive(v) }
        var folderId: String?
            get() = JsonObject(fields).textOrNull("folder_id")
            set(v) { fields["folder_id"] = v?.let(::JsonPrimitive) ?: JsonNull }
        var tags: List<String>
            get() = strings(fields["tags"])
            set(v) { fields["tags"] = JsonArray(v.map(::JsonPrimitive)) }

        // --- login ---

        var username: String
            get() = peek("login")?.text("username") ?: ""
            set(v) { section("login")["username"] = JsonPrimitive(v) }
        var password: String
            get() = peek("login")?.text("password") ?: ""
            set(v) { section("login")["password"] = JsonPrimitive(v) }
        var totp: String?
            get() = peek("login")?.textOrNull("totp")
            set(v) { section("login")["totp"] = v?.let(::JsonPrimitive) ?: JsonNull }

        /** Replaces the websites, keeping each one's unknown fields by position. */
        fun setUrls(urls: List<Url>) {
            val login = section("login")
            login["urls"] = keptByPosition(login["urls"], urls) { kept, u ->
                kept["url"] = JsonPrimitive(u.url)
                kept["match"] = JsonPrimitive(u.match)
            }
        }

        // --- card ---

        var cardholder: String
            get() = peek("card")?.text("cardholder") ?: ""
            set(v) { section("card")["cardholder"] = JsonPrimitive(v) }
        var number: String
            get() = peek("card")?.text("number") ?: ""
            set(v) { section("card")["number"] = JsonPrimitive(v) }
        var expiryMonth: Int?
            get() = peek("card")?.int("expiry_month")
            set(v) { section("card")["expiry_month"] = v?.let(::JsonPrimitive) ?: JsonNull }
        var expiryYear: Int?
            get() = peek("card")?.int("expiry_year")
            set(v) { section("card")["expiry_year"] = v?.let(::JsonPrimitive) ?: JsonNull }
        var securityCode: String
            get() = peek("card")?.text("security_code") ?: ""
            set(v) { section("card")["security_code"] = JsonPrimitive(v) }
        var brand: String
            get() = peek("card")?.text("brand") ?: ""
            set(v) { section("card")["brand"] = JsonPrimitive(v) }

        // --- identity ---

        /** Sets an identity field by its D18 key, such as `first_name`. */
        fun setIdentity(key: String, value: String) {
            require(key in IDENTITY_KEYS) { "not an identity field" }
            section("identity")[key] = JsonPrimitive(value)
        }

        fun setAddressLines(lines: List<String>) {
            section("identity")["address_lines"] = JsonArray(lines.map(::JsonPrimitive))
        }

        // --- custom fields ---

        /** Replaces the custom fields, keeping each one's unknown fields by position. */
        fun setCustomFields(custom: List<CustomField>) {
            fields["custom_fields"] = keptByPosition(fields["custom_fields"], custom) { kept, f ->
                kept["name"] = JsonPrimitive(f.name)
                kept["value"] = JsonPrimitive(f.value)
                // A kind from a newer app version is kept as it was.
                if (f.kind != null) kept["kind"] = JsonPrimitive(f.kind.key)
            }
        }

        internal fun build(): JsonObject {
            for ((name, section) in sections) fields[name] = JsonObject(section)
            return JsonObject(fields)
        }

        private fun <T> keptByPosition(
            old: JsonElement?,
            values: List<T>,
            write: (MutableMap<String, JsonElement>, T) -> Unit,
        ): JsonArray {
            val before = (old as? JsonArray).orEmpty()
            return JsonArray(
                values.mapIndexed { i, value ->
                    val kept = (before.getOrNull(i) as? JsonObject)?.toMutableMap() ?: mutableMapOf()
                    write(kept, value)
                    JsonObject(kept)
                },
            )
        }
    }

    override fun toString(): String = "Item($type, <redacted>)"

    override fun equals(other: Any?): Boolean = other is Item && other.json == json

    override fun hashCode(): Int = json.hashCode()

    companion object {
        /** D18 schema version this build writes. */
        const val SCHEMA_VERSION = 1L

        /** The identity fields' D18 keys, besides the address lines. */
        val IDENTITY_KEYS = listOf(
            "first_name", "middle_name", "last_name", "email", "phone", "company", "city", "region", "postal_code", "country",
        )

        /** A new item, not saved until [Session.save]. */
        fun new(kind: Kind, title: String, id: String, nowMs: Long): Item {
            val fields = mutableMapOf<String, JsonElement>(
                "id" to JsonPrimitive(id),
                "v" to JsonPrimitive(SCHEMA_VERSION),
                "type" to JsonPrimitive(kind.type),
                "title" to JsonPrimitive(title),
                "created_at" to JsonPrimitive(nowMs),
                "updated_at" to JsonPrimitive(nowMs),
            )
            if (kind != Kind.NOTE) fields[kind.type] = JsonObject(emptyMap())
            return Item(JsonObject(fields))
        }

        /** Item JSON with its id, as the core lists items. */
        fun parse(json: String): Item {
            val element = try {
                Json.parseToJsonElement(json)
            } catch (e: Exception) {
                throw PwmError("invalid_item")
            }
            return Item(element as? JsonObject ?: throw PwmError("invalid_item"))
        }

        private fun normalized(element: JsonElement): JsonElement? = when (element) {
            JsonNull -> null
            is JsonPrimitive -> when {
                element.isString && element.content.isEmpty() -> null
                !element.isString && element.booleanOrNull == false -> null
                else -> element
            }
            is JsonArray -> if (element.isEmpty()) null else JsonArray(element.map { normalized(it) ?: JsonNull })
            is JsonObject -> JsonObject(element.mapNotNull { (k, v) -> normalized(v)?.let { k to it } }.toMap())
        }
    }
}

private fun JsonArray?.orEmpty(): List<JsonElement> = this ?: emptyList()

private fun JsonObject.textOrNull(key: String): String? = (this[key] as? JsonPrimitive)?.takeIf { it !is JsonNull }?.contentOrNull

private fun JsonObject.text(key: String): String = textOrNull(key) ?: ""

private fun JsonObject.long(key: String): Long? = (this[key] as? JsonPrimitive)?.longOrNull

private fun JsonObject.int(key: String): Int? = long(key)?.toInt()

private fun strings(element: JsonElement?): List<String> =
    (element as? JsonArray).orEmpty().mapNotNull { (it as? JsonPrimitive)?.contentOrNull }
