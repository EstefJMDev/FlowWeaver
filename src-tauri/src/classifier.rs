/// Domain → category classifier (T-0a-003 + Capa A T-3-006).
/// Deterministic: same domain always produces same category.
/// Static tables, no network, no LLM, no state between calls.
///
/// Capa A (T-3-006, CR-004): tres pasos nuevos cuando la tabla exacta
/// devuelve `otro` — TLD inference, subdomain inference, keyword inference
/// sobre tokens del path y del título. Operación upstream del cifrado
/// AES-GCM en SQLCipher: `url` y `title` se reciben en claro como
/// argumentos y nunca se descifran de la BD (D1). Diccionario público,
/// auditable, ≤200 entradas (PG-A1..PG-A5).

use crate::episode_detector::{extract_url_tokens, tokenize};

pub struct Classified {
    pub domain: String,
    pub category: String,
}

/// Extract domain and assign category from a URL + optional title.
/// Returns `domain = "unknown"` and `category = "otro"` if the URL is malformed.
///
/// `title`: opcional. Cuando viene de `import_resource` o `add_capture`,
/// está disponible en claro upstream del cifrado. Capa A.3 lo tokeniza
/// para keyword inference. `title` nunca se descifra de SQLCipher.
pub fn classify(url: &str, title: Option<&str>) -> Classified {
    let domain = extract_domain(url);
    let path_tokens_owned = extract_url_tokens(url);
    let title_tokens_owned = title.map(tokenize).unwrap_or_default();
    let path_tokens: Vec<&str> = path_tokens_owned.iter().map(|s| s.as_str()).collect();
    let title_tokens: Vec<&str> = title_tokens_owned.iter().map(|s| s.as_str()).collect();
    let category = lookup_category(&domain, &path_tokens, &title_tokens).to_string();
    Classified { domain, category }
}

// TEMP: reclassify helper — eliminar tras reclasificación única.
// Se invoca con tokens vacíos: el reclasificador opera sobre `domain` ya
// almacenado (tabla + TLD + subdominio); Capa A.3 no aplica porque no
// disponemos del path ni del título originales tras la captura.
pub fn classify_domain(domain: &str) -> String {
    lookup_category(domain, &[], &[]).to_string()
}

fn extract_domain(url: &str) -> String {
    let s = if let Some(p) = url.find("://") { &url[p + 3..] } else { url };
    let host = s.split(|c| matches!(c, '/' | '?' | '#')).next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    let host = host.strip_prefix("www.").unwrap_or(host);
    if host.is_empty() {
        "unknown".to_string()
    } else {
        host.to_lowercase()
    }
}

/// Six-step lookup (T-3-006): exact (3 levels) → tld_inference →
/// subdomain_inference → keyword_inference → "otro".
/// Determinístico: cualquier paso != `otro` corta la cadena.
fn lookup_category(
    domain: &str,
    path_tokens: &[&str],
    title_tokens: &[&str],
) -> &'static str {
    if let Some(cat) = exact_lookup(domain) {
        return cat;
    }
    if let Some(root) = domain.find('.').map(|i| &domain[i + 1..]) {
        if let Some(cat) = exact_lookup(root) {
            return cat;
        }
        if let Some(root2) = root.find('.').map(|i| &root[i + 1..]) {
            if let Some(cat) = exact_lookup(root2) {
                return cat;
            }
        }
    }
    let cat = tld_inference(domain);
    if cat != "otro" {
        return cat;
    }
    let cat = subdomain_inference(domain);
    if cat != "otro" {
        return cat;
    }
    keyword_inference(path_tokens, title_tokens)
}

// ── Capa A.1 — TLD Inference (T-3-006) ───────────────────────────────────────

/// TLD-based inference. Tabla estática ordenada de más específico a menos
/// específico (`ends_with` lineal). Cota ≤ 30 entradas.
const TLD_INFERENCE: &[(&str, &str)] = &[
    (".gob.es",   "gobierno"),
    (".gov.uk",   "gobierno"),
    (".gov.fr",   "gobierno"),
    (".gov",      "gobierno"),
    (".ac.uk",    "educación"),
    (".ac.jp",    "educación"),
    (".edu.es",   "educación"),
    (".edu",      "educación"),
];

fn tld_inference(domain: &str) -> &'static str {
    for (suffix, category) in TLD_INFERENCE {
        if domain.ends_with(suffix) {
            return category;
        }
    }
    "otro"
}

// ── Capa A.2 — Subdomain Inference (T-3-006) ─────────────────────────────────

/// Subdomain-prefix inference. Cota ≤ 10 entradas.
const SUBDOMAIN_INFERENCE: &[(&str, &str)] = &[
    ("tienda.",    "comercio"),
    ("shop.",      "comercio"),
    ("store.",     "comercio"),
    ("blog.",      "artículos"),
    ("api.",       "desarrollo"),
    ("developer.", "desarrollo"),
    ("dev.",       "desarrollo"),
    ("docs.",      "educación"),
    ("wiki.",      "educación"),
];

fn subdomain_inference(domain: &str) -> &'static str {
    for (prefix, category) in SUBDOMAIN_INFERENCE {
        if domain.starts_with(prefix) {
            return category;
        }
    }
    "otro"
}

// ── Capa A.3 — Keyword Inference (T-3-006) ───────────────────────────────────

/// Keyword inference sobre tokens del path y del título. Diccionario
/// estático en código (PG-A1, PG-A5). Auditoría: nombres genéricos de
/// actividad/servicio, sin nombres propios ni sub-especialidades médicas
/// (PG-A2). Idiomas: ES (PG-A3). Cota dura: ≤ 200 entradas en total.
const KEYWORD_INFERENCE: &[(&str, &[&str])] = &[
    ("cocina", &[
        "receta", "recetas", "ingredientes", "cocinar", "plato",
        "horno", "guiso", "postre", "tapa", "menu",
    ]),
    ("deportes", &[
        "partido", "gol", "liga", "futbol", "baloncesto",
        "tenis", "formula1", "motogp", "marcador", "resultado",
    ]),
    ("entretenimiento", &[
        "pelicula", "peliculas", "serie", "series", "episodio",
        "temporada", "capitulo", "reparto", "estreno", "sinopsis",
    ]),
    ("gobierno", &[
        "ley", "decreto", "boe", "resolucion",
        "tramite", "expediente", "boja", "doe",
    ]),
    ("salud", &[
        "sintoma", "tratamiento", "consulta", "clinica",
        "diagnostico", "farmacia", "vacuna",
    ]),
];

fn keyword_inference(
    path_tokens: &[&str],
    title_tokens: &[&str],
) -> &'static str {
    use std::collections::HashSet;
    let all_tokens: HashSet<&str> = path_tokens.iter().chain(title_tokens.iter()).copied().collect();
    if all_tokens.is_empty() {
        return "otro";
    }
    let mut scores: Vec<(&'static str, usize)> = KEYWORD_INFERENCE
        .iter()
        .filter_map(|(category, keywords)| {
            let count = keywords.iter().filter(|k| all_tokens.contains(*k)).count();
            if count > 0 { Some((*category, count)) } else { None }
        })
        .collect();
    if scores.is_empty() {
        return "otro";
    }
    scores.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let (best_cat, best_count) = scores[0];
    if best_count < 2 {
        return "otro";
    }
    if scores.len() >= 2 && scores[0].1 == scores[1].1 {
        return "otro";
    }
    best_cat
}

fn exact_lookup(d: &str) -> Option<&'static str> {
    Some(match d {
        // desarrollo
        "github.com" | "gitlab.com" | "bitbucket.org" | "stackoverflow.com"
        | "stackexchange.com" | "npmjs.com" | "crates.io" | "pypi.org"
        | "docs.rs" | "pkg.go.dev" | "codepen.io" | "replit.com"
        | "jsfiddle.net" | "codesandbox.io" | "leetcode.com"
        | "hackerrank.com" | "codewars.com" | "rust-lang.org"
        | "golang.org" | "python.org" | "developer.apple.com"
        | "developer.android.com" | "developer.mozilla.org"
        | "hub.docker.com" | "registry.hub.docker.com"
        | "vercel.com" | "netlify.com" | "supabase.com" | "cloudflare.com"
        | "digitalocean.com" | "railway.app" | "render.com" | "fly.io"
        | "heroku.com" | "gitbook.com" | "swagger.io" | "developer.chrome.com"
        | "web.dev" | "caniuse.com" | "bundlephobia.com" | "packagephobia.com"
        | "regex101.com" | "regexr.com" | "devdocs.io" | "roadmap.sh"
        | "excalidraw.com" | "dbdiagram.io" | "drawio.com" => "desarrollo",

        // notas
        "notion.so" | "notionhq.com" | "obsidian.md" | "roamresearch.com"
        | "craft.do" | "evernote.com" | "onenote.com" | "bear.app"
        | "logseq.com" | "remnote.com" | "workflowy.com" => "notas",

        // diseño
        "figma.com" | "dribbble.com" | "behance.net" | "sketch.com"
        | "invisionapp.com" | "zeplin.io" | "canva.com" | "adobe.com"
        | "framer.com" | "webflow.com" | "storybook.js.org"
        | "coolors.co" | "fontawesome.com" | "fonts.google.com" => "diseño",

        // vídeo (herramientas profesionales de grabación y screencast)
        "wistia.com" | "loom.com" | "screencast.com" => "vídeo",

        // productividad — note: google.com root catches all unmatched google.* subdomains
        "google.com" | "airtable.com" | "trello.com" | "asana.com"
        | "monday.com" | "linear.app" | "atlassian.com" | "slack.com"
        | "discord.com" | "zoom.us" | "microsoft.com" | "office.com"
        | "outlook.com" | "clickup.com" | "basecamp.com"
        | "todoist.com" | "ticktick.com"
        | "dropbox.com" | "box.com" | "wetransfer.com" | "scribd.com"
        | "issuu.com" | "slideshare.net" | "miro.com" | "mural.co"
        | "calendly.com" | "doodle.com" | "typeform.com" | "jotform.com"
        | "zapier.com" | "make.com" | "n8n.io" | "reclaim.ai"
        | "clockify.com" | "toggl.com" | "harvest.com" => "productividad",

        // artículos
        "medium.com" | "substack.com" | "dev.to" | "hashnode.com"
        | "hackernoon.com" | "techcrunch.com" | "theverge.com"
        | "wired.com" | "news.ycombinator.com" | "lobste.rs"
        | "indiehackers.com" | "smashingmagazine.com" | "css-tricks.com"
        | "alistapart.com" | "increment.com" => "artículos",

        // social
        "twitter.com" | "x.com" | "linkedin.com" | "reddit.com"
        | "facebook.com" | "instagram.com" | "pinterest.com"
        | "mastodon.social" | "threads.net" | "bsky.app"
        | "tiktok.com" | "telegram.org" | "t.me" | "whatsapp.com"
        | "snapchat.com"
        | "kick.com" | "tumblr.com" | "flickr.com" | "vk.com"
        | "meetup.com" | "nextdoor.com" => "social",

        // comercio
        "amazon.com" | "gumroad.com" | "stripe.com" | "shopify.com"
        | "etsy.com" | "ebay.com" | "paypal.com" | "paddle.com"
        | "lemonsqueezy.com" | "revenuecat.com" | "fastspring.com"
        | "milanuncios.com" | "wallapop.com" | "pccomponentes.com"
        | "mediamarkt.es" | "fnac.es" | "elcorteingles.es"
        | "zalando.es" | "shein.com" | "aliexpress.com" | "vinted.es"
        | "amazon.es" | "temu.com" | "privalia.com" | "zara.com"
        | "mango.com" | "asos.com" | "decathlon.es" | "leroy-merlin.es"
        | "ikea.com" | "carrefour.es" | "mercadona.es" | "lidl.es"
        | "dia.es" | "hipercor.es" | "primor.com" | "druni.es"
        | "sephora.es" | "sprinter.es" | "jdsports.es" | "footlocker.es"
        | "game.es" | "worten.es" | "coolmod.com" | "ldlc.com"
        | "alternate.es" | "bargento.es" => "comercio",

        // investigación — note: scholar.google.com is listed here before google.com
        // so the exact match wins over the google.com → productividad fallback
        "arxiv.org" | "scholar.google.com" | "pubmed.ncbi.nlm.nih.gov"
        | "semanticscholar.org" | "researchgate.net" | "jstor.org"
        | "ncbi.nlm.nih.gov" | "nature.com" | "science.org"
        | "acm.org" | "ieee.org" | "springer.com" | "wiley.com"
        | "sciencedirect.com" | "plos.org"
        | "dialnet.unirioja.es" | "redalyc.org" | "scielo.org"
        | "ssrn.com" | "biorxiv.org" | "medrxiv.org"
        | "philpapers.org" | "plato.stanford.edu"
        | "worldcat.org" | "europeana.eu" => "investigación",

        // entretenimiento — residuo: portales de vídeo general (UGC/mixto).
        "youtube.com" | "youtu.be" | "vimeo.com" | "dailymotion.com" => "entretenimiento",

        // cine — sitios cuyo contenido primario es información sobre películas
        // (bases de datos, reseñas, carteleras, agregadores). No plataformas de
        // visionado — esas van a "streaming".
        "imdb.com" | "letterboxd.com" | "filmaffinity.com"
        | "rottentomatoes.com" | "themoviedb.org" | "justwatch.com"
        | "sensacine.com" | "trakt.tv" | "allocine.fr"
        | "fotogramas.es" | "espinof.com" | "cartelera.elpais.com"
        | "elseptimoarte.net" | "culturagenial.com" | "ecartelera.com" => "cine",

        // streaming — plataformas SVOD/AVOD + portales web de canales TV.
        "netflix.com" | "hulu.com" | "disneyplus.com" | "hbomax.com" | "max.com"
        | "appletv.com" | "primevideo.com" | "crunchyroll.com" | "funimation.com"
        | "mubi.com" | "filmin.es"
        | "atresplayer.com" | "mitele.es" | "movistarplus.es"
        | "plex.tv" | "stremio.com" | "cuevana.io" | "cuevana3.io"
        | "pelisplus.io" | "repelis.tv" | "telecinco.es"
        | "antena3.com" | "cuatro.com" | "lasexta.com"
        | "clan.rtve.es" | "playz.es" | "dazn.com" | "rakuten.tv"
        | "pluto.tv" | "tubi.tv" | "paramount.plus" => "streaming",

        // gaming — vidaextra.com queda en tecnología (regla "categoría más
        // específica gana"); theverge.com queda en artículos. No re-añadir.
        "store.steampowered.com" | "steampowered.com" | "epicgames.com" | "gog.com"
        | "ign.com" | "kotaku.com" | "polygon.com"
        | "gamespot.com" | "twitch.tv" | "nintendo.com"
        | "playstation.com" | "xbox.com" | "itch.io" | "origin.com"
        | "3djuegos.com" | "vandal.net" | "meristation.com"
        | "hobbyconsolas.com" | "metacritic.com" | "steamcommunity.com"
        | "ea.com" | "ubisoft.com" | "riotgames.com" | "blizzard.com"
        | "areajugones.com" | "g2a.com" | "cdkeys.com"
        | "isthereanydeal.com" | "pcgamer.com" | "rockpapershotgun.com"
        | "eurogamer.net" | "igdb.com" | "rawg.io"
        | "howlongtobeat.com" => "gaming",

        // noticias — telecinco/antena3/cuatro/lasexta van a streaming
        // (split entretenimiento → streaming; canales TV con portal de visionado).
        "bbc.com" | "bbc.co.uk" | "cnn.com" | "reuters.com" | "elpais.com"
        | "elmundo.es" | "theguardian.com" | "nytimes.com"
        | "washingtonpost.com" | "apnews.com" | "rtve.es"
        | "20minutos.es" | "lavanguardia.com" | "abc.es"
        | "lemonde.fr" | "spiegel.de"
        | "elconfidencial.com" | "eldiario.es" | "publico.es"
        | "expansion.com" | "eleconomista.es" | "cincodias.elpais.com"
        | "invertia.com" | "periodistadigital.com" | "huffingtonpost.es"
        | "vozpopuli.com"
        | "elespanol.com" | "okdiario.com" | "infolibre.es"
        | "elindependiente.com" | "europapress.es" | "agenciaefe.com"
        | "voz.us" | "actualidad.rt.com" | "infobae.com"
        | "elperiodico.com" | "ara.cat" | "naciodigital.cat"
        | "diariovasco.com" | "elcorreo.com" | "hoy.es" => "noticias",

        // educación
        "coursera.org" | "udemy.com" | "edx.org" | "khanacademy.org"
        | "udacity.com" | "skillshare.com" | "pluralsight.com" | "lynda.com"
        | "codecademy.com" | "freecodecamp.org" | "domestika.org"
        | "duolingo.com" | "brilliant.org" | "wolframalpha.com"
        | "linguee.com" | "wordreference.com" | "rae.es"
        | "deepl.com" | "fundeu.es" | "merriam-webster.com"
        | "cambridge.org" | "babbel.com" | "busuu.com"
        | "classgap.com" | "superprof.es" | "mit.edu" | "stanford.edu"
        | "coursehero.com" | "quizlet.com" => "educación",

        // música
        "spotify.com" | "soundcloud.com" | "bandcamp.com"
        | "music.apple.com" | "deezer.com" | "tidal.com"
        | "last.fm" | "genius.com" | "letras.com"
        | "musixmatch.com" | "letras.mus.br" | "azlyrics.com"
        | "shazam.com"
        | "mixcloud.com" | "audiomack.com" | "beatport.com"
        | "songkick.com" | "setlist.fm" | "discogs.com"
        | "letssingit.com" | "lyrics.com" | "musica.com" => "música",

        // deportes
        "marca.com" | "as.com" | "sport.es" | "mundodeportivo.com"
        | "relevo.com" | "goal.com" | "besoccer.com" | "eurosport.es"
        | "espn.com"
        | "transfermarkt.es" | "sofascore.com" | "flashscore.es"
        | "soccerway.com" | "resultados-futbol.com"
        | "estadiodeportivo.com" | "superdeporte.es" | "vavel.com"
        | "sportytrader.es" | "todo-lineas.com" | "bein.com"
        | "diariogol.com" | "futbolme.com" | "laliga.com"
        | "rfef.es" | "motogp.com" | "formula1.com"
        | "nba.com" | "nfl.com" => "deportes",

        // tecnología — theverge.com permanece en artículos (no duplicar aquí).
        "xataka.com" | "genbeta.com" | "hipertextual.com" | "applesfera.com"
        | "andro4all.com" | "computerhoy.com" | "elotrolado.net" | "hardzone.es"
        | "muycomputer.com" | "vidaextra.com"
        | "adslzone.net" | "bandaancha.eu" | "redeszone.net"
        | "arstechnica.com" | "tomshardware.com" | "gsmarena.com"
        | "notebookcheck.net" | "rtings.com" | "9to5mac.com"
        | "9to5google.com" | "androidpolice.com" | "macrumors.com"
        | "slashgear.com" | "techradar.com" => "tecnología",

        // cocina
        "recetasdeescandalo.com" | "claudiaandjulia.com" | "canalcocina.es"
        | "directoalpaladar.com" | "webosfritos.es" | "elcomidista.es"
        | "petitchef.es" | "recetasgratis.net" | "pequerecetas.com"
        | "cocinatis.com"
        | "tasty.co" | "allrecipes.com" | "bbcgoodfood.com"
        | "cookpad.com" | "nestlecocina.es" | "recetario.es"
        | "hogarmania.com" | "mundorecetas.com" | "recetasderechupete.com"
        | "cocina-casera.com" | "mis-recetas.es" | "kiwilimon.com"
        | "bonviveur.es" => "cocina",

        // gobierno
        "boe.es" | "gob.es" | "agenciatributaria.gob.es"
        | "sede.agenciatributaria.gob.es" | "seg-social.es"
        | "importass.seg-social.es" | "mjusticia.gob.es" | "europa.eu"
        | "ine.es" | "ipyme.org" | "sepe.gob.es" | "dgt.es"
        | "map.gob.es" | "hacienda.gob.es" | "mites.gob.es"
        | "exteriores.gob.es" | "interior.gob.es" | "congreso.es"
        | "senado.es" | "poderjudicial.es" | "notariado.org"
        | "registradores.org" | "administracion.gob.es" | "sede.gob.es"
        | "correos.es" | "catastro.meh.es" | "juntadeandalucia.es"
        | "gencat.cat" | "madrid.org" | "comunidad.madrid" => "gobierno",

        // salud
        "sanidad.gob.es" | "aemps.gob.es" | "who.int" | "cdc.gov"
        | "ecdc.europa.eu" | "webconsultas.com" | "mayoclinic.org"
        | "healthline.com" | "medlineplus.gov" | "doctoralia.es"
        | "tuotromedico.com" | "saludemia.com" | "medscape.com"
        | "infosalus.com" | "consumer.es" | "cinfasalud.com"
        | "farmacias.com" | "elmedicointeractivo.com" | "diarioenfermero.es"
        | "redaccionmedica.com" | "semfyc.es" | "fisterra.com"
        | "salusplanet.net" | "iqb.es" | "vademecum.es"
        | "farmaceuticos.com" | "cofm.es" => "salud",

        // viajes
        "booking.com" | "airbnb.com" | "airbnb.es"
        | "tripadvisor.com" | "tripadvisor.es"
        | "skyscanner.es" | "skyscanner.com"
        | "renfe.com" | "aena.es" | "iberia.com"
        | "rome2rio.com" | "kayak.es" | "kayak.com"
        | "civitatis.com" | "expedia.es" | "expedia.com"
        | "hotels.com" | "edreams.es" | "rumbo.es"
        | "logitravel.com" | "ryanair.com" | "vueling.com"
        | "easyjet.com" | "alsa.es" | "blablacar.es"
        | "ouigo.es" | "flixbus.es" | "interrail.eu"
        | "viajalagoaspana.info" | "toprural.com"
        | "rusticae.es" | "paradores.es" | "lowcostviajes.es"
        | "despegar.com" | "travelgenio.com" | "atrapalo.com" => "viajes",

        // finanzas — cincodias.elpais.com queda en noticias (no duplicar).
        "ing.es" | "bbva.es" | "santander.es" | "caixabank.es"
        | "bankinter.com" | "sabadell.com" | "unicaja.es"
        | "openbank.es" | "myinvestor.es" | "indexacapital.com"
        | "finizens.com" | "coinbase.com" | "binance.com" | "kraken.com"
        | "investing.com" | "morningstar.com" | "rankia.com" | "finect.com"
        | "revolut.com" | "n26.com" | "wise.com" | "transferwise.com"
        | "bnpparibas.es" | "ibercaja.es" | "kutxabank.es"
        | "abanca.com" | "triodos.es" | "selfbank.es" | "degiro.es"
        | "interactive-brokers.com" | "etoro.com"
        | "expansiondirecto.com" => "finanzas",

        // inmobiliario
        "idealista.com" | "fotocasa.es" | "habitaclia.com" | "pisos.com"
        | "yaencontre.com" | "housfy.com" | "servihabitat.com" | "solvia.es"
        | "enalquiler.com" | "alquiler.net" | "casaktua.com" | "donpiso.com"
        | "remax.es" | "century21.es" | "api.es" | "tecnocasa.es"
        | "inmopc.com" | "segundamano.es" => "inmobiliario",

        // IA — adobe.com permanece en diseño.
        "claude.ai" | "chat.openai.com" | "chatgpt.com"
        | "perplexity.ai" | "midjourney.com" | "huggingface.co"
        | "replicate.com" | "gemini.google.com" | "copilot.microsoft.com"
        | "cursor.sh" | "v0.dev" | "bolt.new" | "gamma.app"
        | "elevenlabs.io" | "runway.ml" | "suno.ai" | "udio.com"
        | "stability.ai" | "leonardo.ai" | "poe.com" | "character.ai"
        | "groq.com" | "mistral.ai" | "cohere.com" | "together.ai"
        | "fireworks.ai" | "notdiamond.ai" | "krea.ai"
        | "ideogram.ai" => "IA",

        // ciencia
        "nationalgeographic.com" | "ted.com" | "quantamagazine.org"
        | "scientificamerican.com" | "naukas.com" | "microsiervos.com"
        | "muyinteresante.es" | "investigacionyciencia.es"
        | "space.com" | "livescience.com" | "bbcearth.com"
        | "newscientist.com" | "agenciasinc.es" | "tendencias21.net"
        | "madrimasd.org" | "csic.es" | "nasa.gov" | "esa.int"
        | "nhm.ac.uk" | "smithsonianmag.com" | "discovermagazine.com"
        | "popularmechanics.com" => "ciencia",

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_domain() {
        assert_eq!(extract_domain("https://github.com/user/repo"), "github.com");
        assert_eq!(extract_domain("https://www.notion.so/page"), "notion.so");
        assert_eq!(extract_domain("https://mail.google.com/mail/u/0/"), "mail.google.com");
        assert_eq!(extract_domain("https://scholar.google.com/scholar?q=rust"), "scholar.google.com");
        assert_eq!(extract_domain("not-a-url"), "not-a-url");
    }

    #[test]
    fn test_classify() {
        assert_eq!(classify("https://github.com/foo/bar", None).category, "desarrollo");
        assert_eq!(classify("https://notion.so/page", None).category, "notas");
        assert_eq!(classify("https://scholar.google.com/", None).category, "investigación");
        assert_eq!(classify("https://mail.google.com/", None).category, "productividad");
        assert_eq!(classify("https://unknown-domain.xyz/", None).category, "otro");
        assert_eq!(classify("https://youtube.com/watch?v=x", None).category, "entretenimiento");
        assert_eq!(classify("https://twitch.tv/channel", None).category, "gaming");
        assert_eq!(classify("https://elpais.com/", None).category, "noticias");
        assert_eq!(classify("https://udemy.com/course/x", None).category, "educación");
        assert_eq!(classify("https://spotify.com/track/x", None).category, "música");
        // Nuevas categorías (3 previas)
        assert_eq!(classify("https://marca.com/futbol/", None).category, "deportes");
        assert_eq!(classify("https://xataka.com/", None).category, "tecnología");
        assert_eq!(classify("https://directoalpaladar.com/receta", None).category, "cocina");
        // Cine — sitios de info/reseñas de películas (split de entretenimiento).
        assert_eq!(classify("https://filmaffinity.com/es/film1.html", None).category, "cine");
        assert_eq!(classify("https://fotogramas.es/peliculas/", None).category, "cine");
        assert_eq!(classify("https://imdb.com/title/x", None).category, "cine");
        // Entretenimiento residuo — portales de vídeo general (UGC/mixto).
        assert_eq!(classify("https://youtube.com/watch?v=x", None).category, "entretenimiento");
        assert_eq!(classify("https://vimeo.com/123", None).category, "entretenimiento");
        // Noticias ES ampliado
        assert_eq!(classify("https://elconfidencial.com/", None).category, "noticias");
        // Comercio ES
        assert_eq!(classify("https://wallapop.com/item/x", None).category, "comercio");
        // Deportes
        assert_eq!(classify("https://sofascore.com/es/", None).category, "deportes");
        assert_eq!(classify("https://laliga.com/", None).category, "deportes");
        // Tecnología — theverge.com permanece en artículos (no se modifica
        // entrada existente); arstechnica representa la verificación de la
        // nueva categoría.
        assert_eq!(classify("https://xataka.com/moviles/", None).category, "tecnología");
        assert_eq!(classify("https://gsmarena.com/samsung", None).category, "tecnología");
        assert_eq!(classify("https://arstechnica.com/tech", None).category, "tecnología");
        // Cocina ampliado
        assert_eq!(classify("https://webosfritos.es/receta", None).category, "cocina");
        assert_eq!(classify("https://tasty.co/recipe/x", None).category, "cocina");
        // Gobierno
        assert_eq!(classify("https://boe.es/buscar/doc.php", None).category, "gobierno");
        assert_eq!(classify("https://agenciatributaria.gob.es/", None).category, "gobierno");
        assert_eq!(classify("https://congreso.es/", None).category, "gobierno");
        assert_eq!(classify("https://sepe.gob.es/", None).category, "gobierno");
        // Salud
        assert_eq!(classify("https://sanidad.gob.es/", None).category, "salud");
        assert_eq!(classify("https://doctoralia.es/medico/x", None).category, "salud");
        assert_eq!(classify("https://aemps.gob.es/", None).category, "salud");
        assert_eq!(classify("https://who.int/es/", None).category, "salud");
        // Viajes
        assert_eq!(classify("https://booking.com/hotel/es/x", None).category, "viajes");
        assert_eq!(classify("https://airbnb.es/rooms/x", None).category, "viajes");
        assert_eq!(classify("https://ryanair.com/es/es/", None).category, "viajes");
        assert_eq!(classify("https://renfe.com/es/es", None).category, "viajes");
        assert_eq!(classify("https://civitatis.com/es/madrid/", None).category, "viajes");
        // Finanzas
        assert_eq!(classify("https://investing.com/crypto/bitcoin", None).category, "finanzas");
        assert_eq!(classify("https://myinvestor.es/", None).category, "finanzas");
        assert_eq!(classify("https://wise.com/es/", None).category, "finanzas");
        assert_eq!(classify("https://revolut.com/es-ES/", None).category, "finanzas");
        // Inmobiliario
        assert_eq!(classify("https://idealista.com/inmueble/x", None).category, "inmobiliario");
        assert_eq!(classify("https://fotocasa.es/es/", None).category, "inmobiliario");
        assert_eq!(classify("https://habitaclia.com/", None).category, "inmobiliario");
        // IA
        assert_eq!(classify("https://claude.ai/chat", None).category, "IA");
        assert_eq!(classify("https://chat.openai.com/", None).category, "IA");
        assert_eq!(classify("https://perplexity.ai/search", None).category, "IA");
        assert_eq!(classify("https://bolt.new/", None).category, "IA");
        assert_eq!(classify("https://gemini.google.com/", None).category, "IA");
        // Ciencia
        assert_eq!(classify("https://ted.com/talks/x", None).category, "ciencia");
        assert_eq!(classify("https://naukas.com/", None).category, "ciencia");
        assert_eq!(classify("https://nasa.gov/", None).category, "ciencia");
        // Streaming — plataformas SVOD/AVOD + portales de canales TV.
        assert_eq!(classify("https://cuevana.io/pelicula/x", None).category, "streaming");
        assert_eq!(classify("https://filmin.es/", None).category, "streaming");
        assert_eq!(classify("https://atresplayer.com/", None).category, "streaming");
        assert_eq!(classify("https://netflix.com/title/x", None).category, "streaming");
        assert_eq!(classify("https://disneyplus.com/", None).category, "streaming");
        // Gaming ampliado
        assert_eq!(classify("https://3djuegos.com/juego/x", None).category, "gaming");
        assert_eq!(classify("https://meristation.com/", None).category, "gaming");
        // Comercio ampliado
        assert_eq!(classify("https://pccomponentes.com/", None).category, "comercio");
        assert_eq!(classify("https://mediamarkt.es/", None).category, "comercio");
        assert_eq!(classify("https://temu.com/", None).category, "comercio");
        // Noticias ampliado
        assert_eq!(classify("https://eldiario.es/", None).category, "noticias");
        // Educación ampliado
        assert_eq!(classify("https://deepl.com/translator", None).category, "educación");
        assert_eq!(classify("https://rae.es/dpd/", None).category, "educación");
        // Social ampliado
        assert_eq!(classify("https://tiktok.com/@user", None).category, "social");
        assert_eq!(classify("https://t.me/channel", None).category, "social");
        // Desarrollo ampliado
        assert_eq!(classify("https://vercel.com/dashboard", None).category, "desarrollo");
        assert_eq!(classify("https://excalidraw.com/", None).category, "desarrollo");
        // Productividad ampliado
        assert_eq!(classify("https://miro.com/app/board/x", None).category, "productividad");
        assert_eq!(classify("https://dropbox.com/home", None).category, "productividad");
        // Música ampliado
        assert_eq!(classify("https://shazam.com/track/x", None).category, "música");
        assert_eq!(classify("https://discogs.com/release/x", None).category, "música");
    }

    /// Verifica que un subdominio explícito gana sobre el strip-1-level.
    /// scholar.google.com → investigación (no productividad por google.com).
    /// gemini.google.com → IA (no productividad).
    /// cartelera.elpais.com → cine (no noticias por elpais.com; tras split
    /// entretenimiento → cine/streaming, los sitios de info de películas
    /// pasaron a "cine").
    /// chat.openai.com → IA (no fallback porque openai.com no está mapeado).
    #[test]
    fn explicit_subdomain_wins_over_root_fallback() {
        assert_eq!(classify("https://scholar.google.com/", None).category, "investigación");
        assert_eq!(classify("https://gemini.google.com/", None).category, "IA");
        assert_eq!(classify("https://cartelera.elpais.com/", None).category, "cine");
        assert_eq!(classify("https://chat.openai.com/", None).category, "IA");
        // Sub-dominio sin entrada explícita → strip a root.
        assert_eq!(classify("https://mail.google.com/", None).category, "productividad");
        assert_eq!(classify("https://es.elpais.com/", None).category, "noticias");
    }

    /// Verifica que la regla "categoría más específica gana" se respeta para
    /// dominios que aparecen en múltiples temas: vidaextra (gaming/tecnología
    /// según el contexto del task) queda en tecnología; theverge.com queda en
    /// artículos; adobe.com en diseño. No hay duplicación silenciosa.
    #[test]
    fn no_silent_cross_category_duplication() {
        assert_eq!(classify("https://vidaextra.com/", None).category, "tecnología");
        assert_eq!(classify("https://theverge.com/", None).category, "artículos");
        assert_eq!(classify("https://adobe.com/", None).category, "diseño");
        // amazon.com (US) y amazon.es (ES) → ambos comercio, distintos exact.
        assert_eq!(classify("https://amazon.com/dp/x", None).category, "comercio");
        assert_eq!(classify("https://amazon.es/dp/x", None).category, "comercio");

    }

    // ── Capa A — T-3-006 (TS-3-006 §"Casos de test obligatorios") ──────────

    /// T1: dominio en `exact_lookup` → corta en paso 1.
    #[test]
    fn capa_a_t1_exact_lookup_wins() {
        assert_eq!(classify("https://github.com/foo/bar", Some("repo")).category, "desarrollo");
    }

    /// T2: subdominio resuelto por strip-1-level (mail.google.com → google.com).
    #[test]
    fn capa_a_t2_strip_one_subdomain() {
        assert_eq!(classify("https://mail.google.com/x", None).category, "productividad");
    }

    /// T3: strip-1-level sobre noticias (es.elpais.com → elpais.com).
    #[test]
    fn capa_a_t3_strip_one_subdomain_news() {
        assert_eq!(classify("https://es.elpais.com/", None).category, "noticias");
    }

    /// T4: subdominio explícito gana sobre strip-1-level (sede.juntadeandalucia.es).
    /// juntadeandalucia.es está en exact_lookup como "gobierno".
    #[test]
    fn capa_a_t4_subdomain_resolves_to_government() {
        assert_eq!(classify("https://sede.juntadeandalucia.es/tramite/x", Some("Trámite")).category, "gobierno");
    }

    /// T5: subdomain_inference (shop.* → comercio) cuando el dominio raíz no está.
    #[test]
    fn capa_a_t5_subdomain_inference_shop() {
        assert_eq!(classify("https://shop.misitio.io/", None).category, "comercio");
    }

    /// T6: subdomain_inference (api.* → desarrollo).
    #[test]
    fn capa_a_t6_subdomain_inference_api() {
        assert_eq!(classify("https://api.misitio.com/", None).category, "desarrollo");
    }

    /// T7: keyword_inference cocina con count ≥ 2 (path + title).
    #[test]
    fn capa_a_t7_keyword_inference_cocina() {
        assert_eq!(
            classify(
                "https://desconocido.com/recetas/tarta-queso",
                Some("Receta tarta de queso ingredientes")
            ).category,
            "cocina"
        );
    }

    /// T8: keyword_inference deportes.
    #[test]
    fn capa_a_t8_keyword_inference_deportes() {
        assert_eq!(
            classify(
                "https://desconocido.com/futbol/liga",
                Some("Partido liga gol resumen")
            ).category,
            "deportes"
        );
    }

    /// T9: sin coincidencias → fallback "otro".
    #[test]
    fn capa_a_t9_no_match_fallback() {
        assert_eq!(classify("https://desconocido.com/abc", Some("Hola mundo")).category, "otro");
    }

    /// T10: empate en keyword_inference → "otro".
    #[test]
    fn capa_a_t10_keyword_tie_returns_otro() {
        // 1 token cocina (receta) + 1 token deportes (futbol) en path → empate
        assert_eq!(classify("https://desconocido.com/receta-futbol", None).category, "otro");
    }

    /// T11: exact_lookup gana sobre keyword incluso con título sugestivo.
    #[test]
    fn capa_a_t11_exact_lookup_dominates_over_keyword() {
        assert_eq!(
            classify("https://github.com/x", Some("receta cocinar plato")).category,
            "desarrollo"
        );
    }

    /// T12: path vacío + title None → "otro" sin pánico.
    #[test]
    fn capa_a_t12_empty_inputs_return_otro() {
        assert_eq!(classify("https://desconocido.com/", None).category, "otro");
    }

    // ── Capa A.3 — tests específicos de keyword_inference ───────────────────

    /// K1: un único keyword coincidente → "otro" (umbral mínimo 2).
    #[test]
    fn capa_a_k1_single_keyword_below_threshold() {
        assert_eq!(keyword_inference(&["receta"], &[]), "otro");
    }

    /// K2: dos keywords distintos misma categoría → categoría.
    #[test]
    fn capa_a_k2_two_distinct_keywords_match() {
        assert_eq!(keyword_inference(&["receta", "ingredientes"], &[]), "cocina");
    }

    /// K3: empate 2-vs-2 entre dos categorías → "otro".
    #[test]
    fn capa_a_k3_tie_returns_otro() {
        assert_eq!(
            keyword_inference(&["receta", "ingredientes"], &["partido", "gol"]),
            "otro"
        );
    }

    /// K4: token duplicado entre path y title cuenta como 1.
    #[test]
    fn capa_a_k4_duplicate_token_counts_once() {
        // un solo token (count=1) → "otro" por umbral mínimo 2.
        assert_eq!(keyword_inference(&["receta"], &["receta"]), "otro");
    }

    // ── Capa A.1 — TLD inference ───────────────────────────────────────────

    /// TLD .gov → gobierno cuando el dominio no está en tabla.
    #[test]
    fn capa_a_tld_gov_inference() {
        assert_eq!(classify("https://nuevo-portal.gov/", None).category, "gobierno");
    }

    /// TLD .edu → educación cuando el dominio no está en tabla.
    #[test]
    fn capa_a_tld_edu_inference() {
        assert_eq!(classify("https://nueva-universidad.edu/cursos", None).category, "educación");
    }
}
