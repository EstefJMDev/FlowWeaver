/// Domain → category classifier (T-0a-003).
/// Deterministic: same domain always produces same category.
/// Static table, no network, no LLM, no state between calls.

pub struct Classified {
    pub domain: String,
    pub category: String,
}

/// Extract domain and assign category from a URL.
/// Returns `domain = "unknown"` and `category = "otro"` if the URL is malformed.
pub fn classify(url: &str) -> Classified {
    let domain = extract_domain(url);
    let category = lookup_category(&domain).to_string();
    Classified { domain, category }
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

/// Two-pass lookup: exact domain first, then strip one subdomain level, then two.
fn lookup_category(domain: &str) -> &'static str {
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
    "otro"
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

        // entretenimiento
        "imdb.com" | "letterboxd.com" | "filmaffinity.com"
        | "rottentomatoes.com" | "themoviedb.org" | "justwatch.com"
        | "sensacine.com" | "trakt.tv" | "allocine.fr"
        | "youtube.com" | "youtu.be" | "netflix.com" | "vimeo.com"
        | "dailymotion.com" | "disneyplus.com" | "hbomax.com"
        | "primevideo.com" | "crunchyroll.com"
        | "fotogramas.es" | "espinof.com" | "cartelera.elpais.com"
        | "elseptimoarte.net" | "culturagenial.com" | "ecartelera.com"
        | "max.com" | "appletv.com" | "mubi.com" | "filmin.es"
        | "atresplayer.com" | "mitele.es" | "movistarplus.es"
        | "plex.tv" | "stremio.com" | "cuevana.io" | "cuevana3.io"
        | "pelisplus.io" | "repelis.tv" | "telecinco.es"
        | "antena3.com" | "cuatro.com" | "lasexta.com"
        | "clan.rtve.es" | "playz.es" | "dazn.com" | "rakuten.tv"
        | "pluto.tv" | "tubi.tv" | "paramount.plus" => "entretenimiento",

        // gaming — vidaextra.com queda en tecnología (regla "categoría más
        // específica gana"); theverge.com queda en artículos. No re-añadir.
        "store.steampowered.com" | "epicgames.com" | "gog.com"
        | "ign.com" | "kotaku.com" | "polygon.com"
        | "gamespot.com" | "twitch.tv" | "nintendo.com"
        | "playstation.com" | "xbox.com"
        | "3djuegos.com" | "vandal.net" | "meristation.com"
        | "hobbyconsolas.com" | "metacritic.com" | "steamcommunity.com"
        | "ea.com" | "ubisoft.com" | "riotgames.com" | "blizzard.com"
        | "areajugones.com" | "g2a.com" | "cdkeys.com"
        | "isthereanydeal.com" | "pcgamer.com" | "rockpapershotgun.com"
        | "eurogamer.net" | "igdb.com" | "rawg.io"
        | "howlongtobeat.com" => "gaming",

        // noticias — telecinco/antena3/cuatro/lasexta van a entretenimiento
        // (regla del task: canales TV en entretenimiento, no aquí).
        "bbc.com" | "cnn.com" | "reuters.com" | "elpais.com"
        | "elmundo.es" | "theguardian.com" | "nytimes.com"
        | "washingtonpost.com" | "apnews.com" | "rtve.es"
        | "20minutos.es" | "lavanguardia.com" | "abc.es"
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
        | "udacity.com" | "skillshare.com" | "pluralsight.com"
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
        assert_eq!(classify("https://github.com/foo/bar").category, "desarrollo");
        assert_eq!(classify("https://notion.so/page").category, "notas");
        assert_eq!(classify("https://scholar.google.com/").category, "investigación");
        assert_eq!(classify("https://mail.google.com/").category, "productividad");
        assert_eq!(classify("https://unknown-domain.xyz/").category, "otro");
        assert_eq!(classify("https://youtube.com/watch?v=x").category, "entretenimiento");
        assert_eq!(classify("https://twitch.tv/channel").category, "gaming");
        assert_eq!(classify("https://elpais.com/").category, "noticias");
        assert_eq!(classify("https://udemy.com/course/x").category, "educación");
        assert_eq!(classify("https://spotify.com/track/x").category, "música");
        // Nuevas categorías (3 previas)
        assert_eq!(classify("https://marca.com/futbol/").category, "deportes");
        assert_eq!(classify("https://xataka.com/").category, "tecnología");
        assert_eq!(classify("https://directoalpaladar.com/receta").category, "cocina");
        // Entretenimiento ampliado
        assert_eq!(classify("https://filmaffinity.com/es/film1.html").category, "entretenimiento");
        assert_eq!(classify("https://fotogramas.es/peliculas/").category, "entretenimiento");
        // Noticias ES ampliado
        assert_eq!(classify("https://elconfidencial.com/").category, "noticias");
        // Comercio ES
        assert_eq!(classify("https://wallapop.com/item/x").category, "comercio");
        // Deportes
        assert_eq!(classify("https://sofascore.com/es/").category, "deportes");
        assert_eq!(classify("https://laliga.com/").category, "deportes");
        // Tecnología — theverge.com permanece en artículos (no se modifica
        // entrada existente); arstechnica representa la verificación de la
        // nueva categoría.
        assert_eq!(classify("https://xataka.com/moviles/").category, "tecnología");
        assert_eq!(classify("https://gsmarena.com/samsung").category, "tecnología");
        assert_eq!(classify("https://arstechnica.com/tech").category, "tecnología");
        // Cocina ampliado
        assert_eq!(classify("https://webosfritos.es/receta").category, "cocina");
        assert_eq!(classify("https://tasty.co/recipe/x").category, "cocina");
        // Gobierno
        assert_eq!(classify("https://boe.es/buscar/doc.php").category, "gobierno");
        assert_eq!(classify("https://agenciatributaria.gob.es/").category, "gobierno");
        assert_eq!(classify("https://congreso.es/").category, "gobierno");
        assert_eq!(classify("https://sepe.gob.es/").category, "gobierno");
        // Salud
        assert_eq!(classify("https://sanidad.gob.es/").category, "salud");
        assert_eq!(classify("https://doctoralia.es/medico/x").category, "salud");
        assert_eq!(classify("https://aemps.gob.es/").category, "salud");
        assert_eq!(classify("https://who.int/es/").category, "salud");
        // Viajes
        assert_eq!(classify("https://booking.com/hotel/es/x").category, "viajes");
        assert_eq!(classify("https://airbnb.es/rooms/x").category, "viajes");
        assert_eq!(classify("https://ryanair.com/es/es/").category, "viajes");
        assert_eq!(classify("https://renfe.com/es/es").category, "viajes");
        assert_eq!(classify("https://civitatis.com/es/madrid/").category, "viajes");
        // Finanzas
        assert_eq!(classify("https://investing.com/crypto/bitcoin").category, "finanzas");
        assert_eq!(classify("https://myinvestor.es/").category, "finanzas");
        assert_eq!(classify("https://wise.com/es/").category, "finanzas");
        assert_eq!(classify("https://revolut.com/es-ES/").category, "finanzas");
        // Inmobiliario
        assert_eq!(classify("https://idealista.com/inmueble/x").category, "inmobiliario");
        assert_eq!(classify("https://fotocasa.es/es/").category, "inmobiliario");
        assert_eq!(classify("https://habitaclia.com/").category, "inmobiliario");
        // IA
        assert_eq!(classify("https://claude.ai/chat").category, "IA");
        assert_eq!(classify("https://chat.openai.com/").category, "IA");
        assert_eq!(classify("https://perplexity.ai/search").category, "IA");
        assert_eq!(classify("https://bolt.new/").category, "IA");
        assert_eq!(classify("https://gemini.google.com/").category, "IA");
        // Ciencia
        assert_eq!(classify("https://ted.com/talks/x").category, "ciencia");
        assert_eq!(classify("https://naukas.com/").category, "ciencia");
        assert_eq!(classify("https://nasa.gov/").category, "ciencia");
        // Entretenimiento ampliado (extras)
        assert_eq!(classify("https://cuevana.io/pelicula/x").category, "entretenimiento");
        assert_eq!(classify("https://filmin.es/").category, "entretenimiento");
        assert_eq!(classify("https://atresplayer.com/").category, "entretenimiento");
        // Gaming ampliado
        assert_eq!(classify("https://3djuegos.com/juego/x").category, "gaming");
        assert_eq!(classify("https://meristation.com/").category, "gaming");
        // Comercio ampliado
        assert_eq!(classify("https://pccomponentes.com/").category, "comercio");
        assert_eq!(classify("https://mediamarkt.es/").category, "comercio");
        assert_eq!(classify("https://temu.com/").category, "comercio");
        // Noticias ampliado
        assert_eq!(classify("https://eldiario.es/").category, "noticias");
        // Educación ampliado
        assert_eq!(classify("https://deepl.com/translator").category, "educación");
        assert_eq!(classify("https://rae.es/dpd/").category, "educación");
        // Social ampliado
        assert_eq!(classify("https://tiktok.com/@user").category, "social");
        assert_eq!(classify("https://t.me/channel").category, "social");
        // Desarrollo ampliado
        assert_eq!(classify("https://vercel.com/dashboard").category, "desarrollo");
        assert_eq!(classify("https://excalidraw.com/").category, "desarrollo");
        // Productividad ampliado
        assert_eq!(classify("https://miro.com/app/board/x").category, "productividad");
        assert_eq!(classify("https://dropbox.com/home").category, "productividad");
        // Música ampliado
        assert_eq!(classify("https://shazam.com/track/x").category, "música");
        assert_eq!(classify("https://discogs.com/release/x").category, "música");
    }

    /// Verifica que un subdominio explícito gana sobre el strip-1-level.
    /// scholar.google.com → investigación (no productividad por google.com).
    /// gemini.google.com → IA (no productividad).
    /// cartelera.elpais.com → entretenimiento (no noticias por elpais.com).
    /// chat.openai.com → IA (no fallback porque openai.com no está mapeado).
    #[test]
    fn explicit_subdomain_wins_over_root_fallback() {
        assert_eq!(classify("https://scholar.google.com/").category, "investigación");
        assert_eq!(classify("https://gemini.google.com/").category, "IA");
        assert_eq!(classify("https://cartelera.elpais.com/").category, "entretenimiento");
        assert_eq!(classify("https://chat.openai.com/").category, "IA");
        // Sub-dominio sin entrada explícita → strip a root.
        assert_eq!(classify("https://mail.google.com/").category, "productividad");
        assert_eq!(classify("https://es.elpais.com/").category, "noticias");
    }

    /// Verifica que la regla "categoría más específica gana" se respeta para
    /// dominios que aparecen en múltiples temas: vidaextra (gaming/tecnología
    /// según el contexto del task) queda en tecnología; theverge.com queda en
    /// artículos; adobe.com en diseño. No hay duplicación silenciosa.
    #[test]
    fn no_silent_cross_category_duplication() {
        assert_eq!(classify("https://vidaextra.com/").category, "tecnología");
        assert_eq!(classify("https://theverge.com/").category, "artículos");
        assert_eq!(classify("https://adobe.com/").category, "diseño");
        // amazon.com (US) y amazon.es (ES) → ambos comercio, distintos exact.
        assert_eq!(classify("https://amazon.com/dp/x").category, "comercio");
        assert_eq!(classify("https://amazon.es/dp/x").category, "comercio");
    }
}
