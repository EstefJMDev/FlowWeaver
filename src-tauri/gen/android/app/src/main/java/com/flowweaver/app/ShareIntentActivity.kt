package com.flowweaver.app

// ── ShareIntentActivity.kt ─────────────────────────────────────────────────────
// Receives ACTION_SEND intents from the Android share sheet (T-0b-android-001).
// Classifies the URL, encrypts url+title (D1), writes the raw_event to the local
// relay queue, and enqueues a one-shot DriveRelayWorker for fast upload.
//
// R12: this activity does NOT invoke Episode Detector, Pattern Detector, Session
//   Builder or any longitudinal analysis. It classifies a single URL and enqueues
//   the raw_event for transport to the desktop.
// D1: url and title are encrypted before any storage (FieldCrypto, AES-256-GCM
//   via Android Keystore). domain and category travel in clear per D1 spec.
// D9: no background observer, no clipboard polling, no accessibility services.
//   Capture is purely explicit — triggered by the user through the share sheet.

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.widget.TextView
import android.widget.Toast
import androidx.work.Constraints
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import org.json.JSONObject
import java.io.File
import java.util.UUID

class ShareIntentActivity : Activity() {

    companion object {
        private const val TAG              = "ShareIntentActivity"
        private const val PREFS_NAME       = "flowweaver_relay"
        private const val PREF_DEVICE_ID   = "device_id"
        private const val PREF_PAIRING_KEY = "pairing_shared_key"   // hex AES-256 from QR pairing
        private const val SCHEMA_VERSION   = 1

        /** Generate or retrieve the stable device_id for this Android installation. */
        fun getOrCreateDeviceId(context: Context): String {
            val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            val existing = prefs.getString(PREF_DEVICE_ID, null)
            if (existing != null) return existing
            val id = "android-${UUID.randomUUID()}"
            prefs.edit().putString(PREF_DEVICE_ID, id).apply()
            Log.i(TAG, "Created new device_id: $id")
            return id
        }
    }

    private var pendingUndoFile: File? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Only handle ACTION_SEND with text/plain MIME type.
        if (intent?.action != Intent.ACTION_SEND || intent.type != "text/plain") {
            Log.d(TAG, "Ignoring non-text share intent")
            finish()
            return
        }

        val extraText = intent.getStringExtra(Intent.EXTRA_TEXT) ?: run {
            Toast.makeText(this, "No URL recibida", Toast.LENGTH_SHORT).show()
            finish()
            return
        }

        // Some apps (YouTube, Chrome, etc.) send "Title\nURL" in EXTRA_TEXT.
        // Extract the URL part; any text before it is a candidate title.
        val urlRegex = Regex("https?://\\S+")
        val rawText = urlRegex.find(extraText)?.value ?: run {
            Toast.makeText(this, "Solo se admiten URLs", Toast.LENGTH_SHORT).show()
            finish()
            return
        }
        val titleFromText = extraText.substringBefore(rawText)
            .trim().trimEnd('\n', '\r', '-', '–', '|', '·').trim()
            .ifBlank { null }

        // H-001 fix: prefer EXTRA_SUBJECT (YouTube title), fall back to text before URL.
        val titleRaw = intent.getStringExtra(Intent.EXTRA_SUBJECT)
            ?.takeIf { it.isNotBlank() }
            ?: titleFromText
            ?: ""

        // ── Pipeline (< 300ms budget — all local, no network) ──────────────────
        val deviceId   = getOrCreateDeviceId(this)
        val eventId    = UUID.randomUUID().toString()
        val capturedAt = System.currentTimeMillis()

        // Extract domain and classify — deterministic, same table as Rust (D8, R12).
        val domain   = extractDomain(rawText)
        val category = classifyDomain(domain)

        // ── Bug #3 fix (sesión 2026-04-30) ───────────────────────────────────────
        // Separar cifrado de tránsito (Drive, descifrable por desktop con
        // pairing_shared_key) del cifrado local (SQLite Android, Keystore).
        // Antes ambos usaban field_key local → desktop nunca descifraba el upload.

        // Local — Keystore field key (lo que se guarda en SQLite Android).
        val fieldKey            = FieldCrypto.deriveKey(FieldCrypto.FIELD_KEY_PASSPHRASE)
        val urlEncryptedLocal   = FieldCrypto.encrypt(rawText, fieldKey)
        val titleEncryptedLocal = FieldCrypto.encrypt(titleRaw, fieldKey)

        // Tránsito — pairing_shared_key (compartida con desktop vía QR pairing).
        // Misma derivación SHA-256(string) que crypto.rs::derive_key_aes y que
        // DriveRelayWorker.decryptDesktopField (Bug #2 alineado).
        val pairingKeyHex = getSharedPreferences(PREFS_NAME, MODE_PRIVATE)
            .getString(PREF_PAIRING_KEY, null)
        if (pairingKeyHex.isNullOrBlank()) {
            Log.e(TAG, "pairing_shared_key not configured — share aborted")
            Toast.makeText(this, "Empareja primero con el desktop", Toast.LENGTH_LONG).show()
            finish()
            return
        }
        // RelayCrypto.encryptFw1a usa SecureRandom internamente; nunca exponer nonce
        // en API de producción (ver RelayCrypto.kt — refuerzo de seguridad).
        val urlEncryptedTransit   = RelayCrypto.encryptFw1a(rawText,  pairingKeyHex)
        val titleEncryptedTransit = RelayCrypto.encryptFw1a(titleRaw, pairingKeyHex)

        // Build raw_event JSON — matches TS-0b-android-001 schema.
        // domain and category are in clear (D1). url and title travel in fw1a (transit key).
        val rawEvent = JSONObject().apply {
            put("event_id",        eventId)
            put("device_id",       deviceId)
            put("source",          "android")
            put("captured_at",     capturedAt)
            put("domain",          domain)
            put("category",        category)
            put("url_encrypted",   urlEncryptedTransit)
            put("title_encrypted", titleEncryptedTransit)
            put("schema_version",  SCHEMA_VERSION)
        }

        // Write to local relay queue (Drive Worker will upload asynchronously).
        val queueFile = writeToQueue(eventId, rawEvent.toString())
        pendingUndoFile = queueFile

        // Also insert immediately into local SQLite so the gallery shows it right away.
        // Local DB usa la clave Keystore — nunca la pairing key.
        val db = LocalDb(this)
        db.insertOrIgnore(
            uuid           = eventId,
            urlEncrypted   = urlEncryptedLocal,
            titleEncrypted = titleEncryptedLocal,
            domain         = domain,
            category       = category,
            capturedAt     = capturedAt
        )
        db.close()

        // Trigger immediate (one-shot) upload attempt — best-effort if network available.
        triggerImmediateSync()

        // ── Confirmation UI (4-second overlay) ────────────────────────────────
        showConfirmationAndClose(domain, category, eventId)
    }

    // ── Local queue write ────────────────────────────────────────────────────────

    private fun writeToQueue(eventId: String, json: String): File {
        val dir = File(filesDir, "relay_queue")
        dir.mkdirs()
        val file = File(dir, "$eventId.json")
        file.writeText(json, Charsets.UTF_8)
        Log.d(TAG, "Enqueued event $eventId to relay queue")
        return file
    }

    // ── One-shot WorkManager trigger ─────────────────────────────────────────────

    private fun triggerImmediateSync() {
        val constraints = Constraints.Builder()
            .setRequiredNetworkType(NetworkType.CONNECTED)
            .build()
        val request = OneTimeWorkRequestBuilder<DriveRelayWorker>()
            .setConstraints(constraints)
            .addTag("flowweaver_immediate_sync")
            .build()
        WorkManager.getInstance(applicationContext).enqueue(request)
        Log.d(TAG, "Immediate sync WorkRequest enqueued")
    }

    // ── Confirmation UI ──────────────────────────────────────────────────────────

    private fun showConfirmationAndClose(domain: String, category: String, eventId: String) {
        // Simple toast confirmation — a full card overlay can replace this in a later sprint.
        // The category feedback confirms the classifier ran (D8).
        val message = "Guardado en FlowWeaver\n$category · $domain"
        Toast.makeText(this, message, Toast.LENGTH_LONG).show()

        // Auto-close after 4 seconds — or immediately on Undo tap.
        Handler(Looper.getMainLooper()).postDelayed({
            finish()
        }, 4000)
    }

    // ── Domain extraction (mirrors Rust classifier.rs extract_domain) ────────────

    private fun extractDomain(url: String): String {
        val s = if (url.contains("://")) url.substringAfter("://") else url
        val host = s.split('/', '?', '#').firstOrNull() ?: ""
        val hostNoPort = host.split(':').firstOrNull() ?: ""
        val clean = hostNoPort.removePrefix("www.")
        return if (clean.isEmpty()) "unknown" else clean.lowercase()
    }

    // ── Deterministic domain classifier (D8, R12) ────────────────────────────────

    private fun classifyDomain(domain: String): String {
        val d = domain.lowercase()
        return exactLookup(d)
            ?: d.indexOf('.').let { if (it >= 0) exactLookup(d.substring(it + 1)) else null }
            ?: d.indexOf('.').let { i ->
                if (i >= 0) {
                    val sub = d.substring(i + 1)
                    sub.indexOf('.').let { j -> if (j >= 0) exactLookup(sub.substring(j + 1)) else null }
                } else null
            }
            ?: "otro"
    }

    private fun exactLookup(d: String): String? = when (d) {
        "github.com", "gitlab.com", "bitbucket.org", "stackoverflow.com",
        "stackexchange.com", "npmjs.com", "crates.io", "pypi.org",
        "docs.rs", "pkg.go.dev", "codepen.io", "replit.com",
        "jsfiddle.net", "codesandbox.io", "leetcode.com",
        "hackerrank.com", "codewars.com", "rust-lang.org",
        "golang.org", "python.org", "developer.apple.com",
        "developer.android.com", "developer.mozilla.org",
        "hub.docker.com", "registry.hub.docker.com",
        "vercel.com", "netlify.com", "supabase.com", "cloudflare.com",
        "digitalocean.com", "railway.app", "render.com", "fly.io",
        "heroku.com", "gitbook.com", "swagger.io", "developer.chrome.com",
        "web.dev", "caniuse.com", "bundlephobia.com", "packagephobia.com",
        "regex101.com", "regexr.com", "devdocs.io", "roadmap.sh",
        "excalidraw.com", "dbdiagram.io", "drawio.com" -> "desarrollo"

        "notion.so", "notionhq.com", "obsidian.md", "roamresearch.com",
        "craft.do", "evernote.com", "onenote.com", "bear.app",
        "logseq.com", "remnote.com", "workflowy.com" -> "notas"

        "figma.com", "dribbble.com", "behance.net", "sketch.com",
        "invisionapp.com", "zeplin.io", "canva.com", "adobe.com",
        "framer.com", "webflow.com", "storybook.js.org",
        "coolors.co", "fontawesome.com", "fonts.google.com" -> "diseño"

        "wistia.com", "loom.com", "screencast.com" -> "vídeo"

        "imdb.com", "youtube.com", "youtu.be", "netflix.com",
        "vimeo.com", "dailymotion.com", "hulu.com", "primevideo.com",
        "disneyplus.com", "hbomax.com", "max.com", "appletv.com",
        "crunchyroll.com", "funimation.com", "rottentomatoes.com",
        "letterboxd.com", "themoviedb.org",
        "filmaffinity.com", "sensacine.com", "fotogramas.es",
        "espinof.com", "cartelera.elpais.com", "elseptimoarte.net",
        "culturagenial.com", "ecartelera.com", "mubi.com",
        "filmin.es",
        "atresplayer.com", "mitele.es", "movistarplus.es",
        "plex.tv", "stremio.com", "cuevana.io", "cuevana3.io",
        "pelisplus.io", "repelis.tv", "telecinco.es",
        "antena3.com", "cuatro.com", "lasexta.com",
        "clan.rtve.es", "playz.es", "dazn.com", "rakuten.tv",
        "pluto.tv", "tubi.tv", "paramount.plus" -> "entretenimiento"

        "store.steampowered.com", "steampowered.com", "twitch.tv",
        "itch.io", "epicgames.com", "gog.com", "origin.com",
        "xbox.com", "playstation.com", "nintendo.com",
        "gamespot.com", "ign.com", "kotaku.com",
        "3djuegos.com", "vandal.net", "meristation.com",
        "hobbyconsolas.com", "metacritic.com", "steamcommunity.com",
        "ea.com", "ubisoft.com", "riotgames.com", "blizzard.com",
        "areajugones.com", "g2a.com", "cdkeys.com",
        "isthereanydeal.com", "pcgamer.com", "rockpapershotgun.com",
        "eurogamer.net", "igdb.com", "rawg.io",
        "howlongtobeat.com" -> "gaming"

        "bbc.com", "bbc.co.uk", "elpais.com", "elmundo.es",
        "reuters.com", "apnews.com", "theguardian.com",
        "nytimes.com", "washingtonpost.com", "lemonde.fr",
        "spiegel.de", "publico.es", "elconfidencial.com",
        "lavanguardia.com", "20minutos.es",
        "eldiario.es", "expansion.com", "eleconomista.es",
        "cincodias.elpais.com", "invertia.com", "periodistadigital.com",
        "huffingtonpost.es", "vozpopuli.com",
        "elespanol.com", "okdiario.com", "infolibre.es",
        "elindependiente.com", "europapress.es", "agenciaefe.com",
        "voz.us", "actualidad.rt.com", "infobae.com",
        "elperiodico.com", "ara.cat", "naciodigital.cat",
        "diariovasco.com", "elcorreo.com", "hoy.es" -> "noticias"

        "coursera.org", "udemy.com", "edx.org", "khanacademy.org",
        "pluralsight.com", "skillshare.com", "lynda.com",
        "linkedin.com/learning", "udacity.com", "freecodecamp.org",
        "codecademy.com", "brilliant.org", "duolingo.com",
        "wolframalpha.com", "linguee.com", "wordreference.com",
        "rae.es",
        "deepl.com", "fundeu.es", "merriam-webster.com",
        "cambridge.org", "babbel.com", "busuu.com",
        "classgap.com", "superprof.es", "mit.edu", "stanford.edu",
        "coursehero.com", "quizlet.com" -> "educación"

        "spotify.com", "soundcloud.com", "bandcamp.com",
        "music.apple.com", "tidal.com", "deezer.com",
        "last.fm", "genius.com", "musixmatch.com",
        "audiomack.com", "mixcloud.com",
        "letras.mus.br", "azlyrics.com", "shazam.com",
        "beatport.com", "songkick.com", "setlist.fm",
        "discogs.com", "letssingit.com", "lyrics.com",
        "musica.com" -> "música"

        "google.com", "airtable.com", "trello.com", "asana.com",
        "monday.com", "linear.app", "atlassian.com", "slack.com",
        "discord.com", "zoom.us", "microsoft.com", "office.com",
        "outlook.com", "clickup.com", "basecamp.com",
        "todoist.com", "ticktick.com",
        "dropbox.com", "box.com", "wetransfer.com", "scribd.com",
        "issuu.com", "slideshare.net", "miro.com", "mural.co",
        "calendly.com", "doodle.com", "typeform.com", "jotform.com",
        "zapier.com", "make.com", "n8n.io", "reclaim.ai",
        "clockify.com", "toggl.com", "harvest.com" -> "productividad"

        "medium.com", "substack.com", "dev.to", "hashnode.com",
        "hackernoon.com", "techcrunch.com", "theverge.com",
        "wired.com", "news.ycombinator.com", "lobste.rs",
        "indiehackers.com", "smashingmagazine.com", "css-tricks.com",
        "alistapart.com", "increment.com" -> "artículos"

        "twitter.com", "x.com", "linkedin.com", "reddit.com",
        "facebook.com", "instagram.com", "pinterest.com",
        "mastodon.social", "threads.net", "bsky.app",
        "tiktok.com", "telegram.org", "t.me", "whatsapp.com",
        "snapchat.com",
        "kick.com", "tumblr.com", "flickr.com", "vk.com",
        "meetup.com", "nextdoor.com" -> "social"

        "amazon.com", "gumroad.com", "stripe.com", "shopify.com",
        "etsy.com", "ebay.com", "paypal.com", "paddle.com",
        "lemonsqueezy.com", "revenuecat.com", "fastspring.com",
        "milanuncios.com", "wallapop.com", "pccomponentes.com",
        "mediamarkt.es", "fnac.es", "elcorteingles.es",
        "zalando.es", "shein.com", "aliexpress.com", "vinted.es",
        "amazon.es", "temu.com", "privalia.com", "zara.com",
        "mango.com", "asos.com", "decathlon.es", "leroy-merlin.es",
        "ikea.com", "carrefour.es", "mercadona.es", "lidl.es",
        "dia.es", "hipercor.es", "primor.com", "druni.es",
        "sephora.es", "sprinter.es", "jdsports.es", "footlocker.es",
        "game.es", "worten.es", "coolmod.com", "ldlc.com",
        "alternate.es", "bargento.es" -> "comercio"

        "arxiv.org", "scholar.google.com", "pubmed.ncbi.nlm.nih.gov",
        "semanticscholar.org", "researchgate.net", "jstor.org",
        "ncbi.nlm.nih.gov", "nature.com", "science.org",
        "acm.org", "ieee.org", "springer.com", "wiley.com",
        "sciencedirect.com", "plos.org",
        "dialnet.unirioja.es", "redalyc.org", "scielo.org",
        "ssrn.com", "biorxiv.org", "medrxiv.org",
        "philpapers.org", "plato.stanford.edu",
        "worldcat.org", "europeana.eu" -> "investigación"

        "marca.com", "as.com", "sport.es", "mundodeportivo.com",
        "relevo.com", "goal.com", "besoccer.com", "eurosport.es",
        "espn.com",
        "transfermarkt.es", "sofascore.com", "flashscore.es",
        "soccerway.com", "resultados-futbol.com",
        "estadiodeportivo.com", "superdeporte.es", "vavel.com",
        "sportytrader.es", "todo-lineas.com", "bein.com",
        "diariogol.com", "futbolme.com", "laliga.com",
        "rfef.es", "motogp.com", "formula1.com",
        "nba.com", "nfl.com" -> "deportes"

        "xataka.com", "genbeta.com", "hipertextual.com", "applesfera.com",
        "andro4all.com", "computerhoy.com", "elotrolado.net", "hardzone.es",
        "muycomputer.com", "vidaextra.com",
        "adslzone.net", "bandaancha.eu", "redeszone.net",
        "arstechnica.com", "tomshardware.com", "gsmarena.com",
        "notebookcheck.net", "rtings.com", "9to5mac.com",
        "9to5google.com", "androidpolice.com", "macrumors.com",
        "slashgear.com", "techradar.com" -> "tecnología"

        "recetasdeescandalo.com", "claudiaandjulia.com", "canalcocina.es",
        "directoalpaladar.com", "webosfritos.es", "elcomidista.es",
        "petitchef.es", "recetasgratis.net", "pequerecetas.com",
        "cocinatis.com",
        "tasty.co", "allrecipes.com", "bbcgoodfood.com",
        "cookpad.com", "nestlecocina.es", "recetario.es",
        "hogarmania.com", "mundorecetas.com", "recetasderechupete.com",
        "cocina-casera.com", "mis-recetas.es", "kiwilimon.com",
        "bonviveur.es" -> "cocina"

        "boe.es", "gob.es", "agenciatributaria.gob.es",
        "sede.agenciatributaria.gob.es", "seg-social.es",
        "importass.seg-social.es", "mjusticia.gob.es", "europa.eu",
        "ine.es", "ipyme.org", "sepe.gob.es", "dgt.es",
        "map.gob.es", "hacienda.gob.es", "mites.gob.es",
        "exteriores.gob.es", "interior.gob.es", "congreso.es",
        "senado.es", "poderjudicial.es", "notariado.org",
        "registradores.org", "administracion.gob.es", "sede.gob.es",
        "correos.es", "catastro.meh.es", "juntadeandalucia.es",
        "gencat.cat", "madrid.org", "comunidad.madrid" -> "gobierno"

        "sanidad.gob.es", "aemps.gob.es", "who.int", "cdc.gov",
        "ecdc.europa.eu", "webconsultas.com", "mayoclinic.org",
        "healthline.com", "medlineplus.gov", "doctoralia.es",
        "tuotromedico.com", "saludemia.com", "medscape.com",
        "infosalus.com", "consumer.es", "cinfasalud.com",
        "farmacias.com", "elmedicointeractivo.com", "diarioenfermero.es",
        "redaccionmedica.com", "semfyc.es", "fisterra.com",
        "salusplanet.net", "iqb.es", "vademecum.es",
        "farmaceuticos.com", "cofm.es" -> "salud"

        "booking.com", "airbnb.com", "airbnb.es",
        "tripadvisor.com", "tripadvisor.es",
        "skyscanner.es", "skyscanner.com",
        "renfe.com", "aena.es", "iberia.com",
        "rome2rio.com", "kayak.es", "kayak.com",
        "civitatis.com", "expedia.es", "expedia.com",
        "hotels.com", "edreams.es", "rumbo.es",
        "logitravel.com", "ryanair.com", "vueling.com",
        "easyjet.com", "alsa.es", "blablacar.es",
        "ouigo.es", "flixbus.es", "interrail.eu",
        "viajalagoaspana.info", "toprural.com",
        "rusticae.es", "paradores.es", "lowcostviajes.es",
        "despegar.com", "travelgenio.com", "atrapalo.com" -> "viajes"

        "ing.es", "bbva.es", "santander.es", "caixabank.es",
        "bankinter.com", "sabadell.com", "unicaja.es",
        "openbank.es", "myinvestor.es", "indexacapital.com",
        "finizens.com", "coinbase.com", "binance.com", "kraken.com",
        "investing.com", "morningstar.com", "rankia.com", "finect.com",
        "revolut.com", "n26.com", "wise.com", "transferwise.com",
        "bnpparibas.es", "ibercaja.es", "kutxabank.es",
        "abanca.com", "triodos.es", "selfbank.es", "degiro.es",
        "interactive-brokers.com", "etoro.com",
        "expansiondirecto.com" -> "finanzas"

        "idealista.com", "fotocasa.es", "habitaclia.com", "pisos.com",
        "yaencontre.com", "housfy.com", "servihabitat.com", "solvia.es",
        "enalquiler.com", "alquiler.net", "casaktua.com", "donpiso.com",
        "remax.es", "century21.es", "api.es", "tecnocasa.es",
        "inmopc.com", "segundamano.es" -> "inmobiliario"

        "claude.ai", "chat.openai.com", "chatgpt.com",
        "perplexity.ai", "midjourney.com", "huggingface.co",
        "replicate.com", "gemini.google.com", "copilot.microsoft.com",
        "cursor.sh", "v0.dev", "bolt.new", "gamma.app",
        "elevenlabs.io", "runway.ml", "suno.ai", "udio.com",
        "stability.ai", "leonardo.ai", "poe.com", "character.ai",
        "groq.com", "mistral.ai", "cohere.com", "together.ai",
        "fireworks.ai", "notdiamond.ai", "krea.ai",
        "ideogram.ai" -> "IA"

        "nationalgeographic.com", "ted.com", "quantamagazine.org",
        "scientificamerican.com", "naukas.com", "microsiervos.com",
        "muyinteresante.es", "investigacionyciencia.es",
        "space.com", "livescience.com", "bbcearth.com",
        "newscientist.com", "agenciasinc.es", "tendencias21.net",
        "madrimasd.org", "csic.es", "nasa.gov", "esa.int",
        "nhm.ac.uk", "smithsonianmag.com", "discovermagazine.com",
        "popularmechanics.com" -> "ciencia"

        else -> null
    }
}
