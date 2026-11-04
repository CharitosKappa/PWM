package pwm.kit

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull

/**
 * A vault item as the core gives it: D18 JSON with its id (D52 §4). Typed
 * reads for what the screens show; edits rewrite only the fields they
 * change, so fields this version doesn't know are kept, as D18 requires.
 * `toString` never shows a value.
 */
class Item private constructor(val json: JsonObject) {
    enum class Kind(val type: String) {
        LOGIN("login"), NOTE("note"), CARD("card"), IDENTITY("identity"), FOLDER("folder");

        companion object {
            fun of(type: String?): Kind? = entries.firstOrNull { it.type == type }
        }
    }

    /** A login's website and how it matches (`AUTOFILL.md`). */
    data class Url(val url: String, val match: String = "domain")

    val id: String? get() = text("id")
    val type: String get() = text("type") ?: ""
    /** Null for a type from a newer app version: shown read-only. */
    val kind: Kind? get() = Kind.of(type)
    val title: String get() = text("title") ?: ""
    val notes: String get() = text("notes") ?: ""
    val favorite: Boolean get() = json["favorite"]?.jsonPrimitive?.booleanOrNull ?: false
    val folderId: String? get() = text("folder_id")
    val tags: List<String> get() = strings(json["tags"])
    val trashedAtMs: Long? get() = (json["trashed_at"] as? JsonPrimitive)?.longOrNull
    val schemaVersion: Long get() = (json["v"] as? JsonPrimitive)?.longOrNull ?: 0
    /** Items from a newer schema, or of an unknown type, are shown read-only. */
    val isEditable: Boolean get() = schemaVersion <= SCHEMA_VERSION && kind != null

    private val login: JsonObject? get() = json["login"] as? JsonObject
    val username: String get() = login?.get("username")?.jsonPrimitive?.contentOrNull ?: ""
    val password: String get() = login?.get("password")?.jsonPrimitive?.contentOrNull ?: ""
    val totp: String? get() = login?.get("totp")?.let { (it as? JsonPrimitive)?.contentOrNull }
    val urls: List<Url>
        get() = (login?.get("urls") as? JsonArray).orEmpty().mapNotNull { element ->
            val o = element as? JsonObject ?: return@mapNotNull null
            val url = o["url"]?.jsonPrimitive?.contentOrNull ?: return@mapNotNull null
            Url(url, o["match"]?.jsonPrimitive?.contentOrNull ?: "domain")
        }

    /** The JSON the core takes back. */
    fun toJson(): Redacted<String> = Redacted(json.toString())

    /** A copy with these changes; everything else is kept as it was. */
    fun edited(change: Draft.() -> Unit): Item {
        val draft = Draft(json.toMutableMap(), (json["login"] as? JsonObject)?.toMutableMap())
        draft.change()
        return Item(draft.build())
    }

    /** The fields the screens edit. Unset ones stay as they are. */
    class Draft internal constructor(
        private val fields: MutableMap<String, JsonElement>,
        private val loginFields: MutableMap<String, JsonElement>?,
    ) {
        var title: String
            get() = fields["title"]?.jsonPrimitive?.contentOrNull ?: ""
            set(v) { fields["title"] = JsonPrimitive(v) }
        var notes: String
            get() = fields["notes"]?.jsonPrimitive?.contentOrNull ?: ""
            set(v) { fields["notes"] = JsonPrimitive(v) }
        var favorite: Boolean
            get() = fields["favorite"]?.jsonPrimitive?.booleanOrNull ?: false
            set(v) { fields["favorite"] = JsonPrimitive(v) }
        var folderId: String?
            get() = fields["folder_id"]?.jsonPrimitive?.contentOrNull
            set(v) { fields["folder_id"] = v?.let(::JsonPrimitive) ?: JsonNull }

        var username: String
            get() = loginFields?.get("username")?.jsonPrimitive?.contentOrNull ?: ""
            set(v) { requireLogin()["username"] = JsonPrimitive(v) }
        var password: String
            get() = loginFields?.get("password")?.jsonPrimitive?.contentOrNull ?: ""
            set(v) { requireLogin()["password"] = JsonPrimitive(v) }
        var totp: String?
            get() = loginFields?.get("totp")?.let { (it as? JsonPrimitive)?.contentOrNull }
            set(v) { requireLogin()["totp"] = v?.let(::JsonPrimitive) ?: JsonNull }

        /** Replaces the websites, keeping each one's unknown fields by position. */
        fun setUrls(urls: List<Url>) {
            val login = requireLogin()
            val old = (login["urls"] as? JsonArray).orEmpty()
            login["urls"] = JsonArray(
                urls.mapIndexed { i, u ->
                    val kept = (old.getOrNull(i) as? JsonObject)?.toMutableMap() ?: mutableMapOf()
                    kept["url"] = JsonPrimitive(u.url)
                    kept["match"] = JsonPrimitive(u.match)
                    JsonObject(kept)
                },
            )
        }

        private fun requireLogin(): MutableMap<String, JsonElement> =
            loginFields ?: throw PwmError("invalid_item")

        internal fun build(): JsonObject {
            loginFields?.let { fields["login"] = JsonObject(it) }
            return JsonObject(fields)
        }
    }

    override fun toString(): String = "Item($type, <redacted>)"

    override fun equals(other: Any?): Boolean = other is Item && other.json == json

    override fun hashCode(): Int = json.hashCode()

    private fun text(key: String): String? = (json[key] as? JsonPrimitive)?.contentOrNull

    private fun strings(element: JsonElement?): List<String> =
        (element as? JsonArray).orEmpty().mapNotNull { (it as? JsonPrimitive)?.contentOrNull }

    companion object {
        /** D18 schema version this build writes. */
        const val SCHEMA_VERSION = 1L

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
    }
}

private fun JsonArray?.orEmpty(): List<JsonElement> = this ?: emptyList()
