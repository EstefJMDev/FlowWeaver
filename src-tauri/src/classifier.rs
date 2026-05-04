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
        | "hub.docker.com" | "registry.hub.docker.com" => "desarrollo",

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
        | "todoist.com" | "ticktick.com" => "productividad",

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
        | "snapchat.com" => "social",

        // comercio
        "amazon.com" | "gumroad.com" | "stripe.com" | "shopify.com"
        | "etsy.com" | "ebay.com" | "paypal.com" | "paddle.com"
        | "lemonsqueezy.com" | "revenuecat.com" | "fastspring.com"
        | "milanuncios.com" | "wallapop.com" | "pccomponentes.com"
        | "mediamarkt.es" | "fnac.es" | "elcorteingles.es"
        | "zalando.es" | "shein.com" | "aliexpress.com" | "vinted.es" => "comercio",

        // investigación — note: scholar.google.com is listed here before google.com
        // so the exact match wins over the google.com → productividad fallback
        "arxiv.org" | "scholar.google.com" | "pubmed.ncbi.nlm.nih.gov"
        | "semanticscholar.org" | "researchgate.net" | "jstor.org"
        | "ncbi.nlm.nih.gov" | "nature.com" | "science.org"
        | "acm.org" | "ieee.org" | "springer.com" | "wiley.com"
        | "sciencedirect.com" | "plos.org" => "investigación",

        // entretenimiento
        "imdb.com" | "letterboxd.com" | "filmaffinity.com"
        | "rottentomatoes.com" | "themoviedb.org" | "justwatch.com"
        | "sensacine.com" | "trakt.tv" | "allocine.fr"
        | "youtube.com" | "youtu.be" | "netflix.com" | "vimeo.com"
        | "dailymotion.com" | "disneyplus.com" | "hbomax.com"
        | "primevideo.com" | "crunchyroll.com"
        | "fotogramas.es" | "espinof.com" | "cartelera.elpais.com"
        | "elseptimoarte.net" | "culturagenial.com" | "ecartelera.com"
        | "max.com" | "appletv.com" | "mubi.com" | "filmin.es" => "entretenimiento",

        // gaming
        "store.steampowered.com" | "epicgames.com" | "gog.com"
        | "ign.com" | "kotaku.com" | "polygon.com"
        | "gamespot.com" | "twitch.tv" | "nintendo.com"
        | "playstation.com" | "xbox.com" => "gaming",

        // noticias
        "bbc.com" | "cnn.com" | "reuters.com" | "elpais.com"
        | "elmundo.es" | "theguardian.com" | "nytimes.com"
        | "washingtonpost.com" | "apnews.com" | "rtve.es"
        | "20minutos.es" | "lavanguardia.com" | "abc.es"
        | "elconfidencial.com" | "eldiario.es" | "publico.es"
        | "expansion.com" | "eleconomista.es" | "cincodias.elpais.com"
        | "invertia.com" | "periodistadigital.com" | "huffingtonpost.es"
        | "vozpopuli.com" => "noticias",

        // educación
        "coursera.org" | "udemy.com" | "edx.org" | "khanacademy.org"
        | "udacity.com" | "skillshare.com" | "pluralsight.com"
        | "codecademy.com" | "freecodecamp.org" | "domestika.org"
        | "duolingo.com" | "brilliant.org" | "wolframalpha.com"
        | "linguee.com" | "wordreference.com" | "rae.es" => "educación",

        // música
        "spotify.com" | "soundcloud.com" | "bandcamp.com"
        | "music.apple.com" | "deezer.com" | "tidal.com"
        | "last.fm" | "genius.com" | "letras.com"
        | "musixmatch.com" | "letras.mus.br" | "azlyrics.com"
        | "shazam.com" => "música",

        // deportes
        "marca.com" | "as.com" | "sport.es" | "mundodeportivo.com"
        | "relevo.com" | "goal.com" | "besoccer.com" | "eurosport.es"
        | "espn.com" => "deportes",

        // tecnología
        "xataka.com" | "genbeta.com" | "hipertextual.com" | "applesfera.com"
        | "andro4all.com" | "computerhoy.com" | "elotrolado.net" | "hardzone.es"
        | "muycomputer.com" | "vidaextra.com" => "tecnología",

        // cocina
        "recetasdeescandalo.com" | "claudiaandjulia.com" | "canalcocina.es"
        | "directoalpaladar.com" | "webosfritos.es" | "elcomidista.es"
        | "petitchef.es" | "recetasgratis.net" | "pequerecetas.com"
        | "cocinatis.com" => "cocina",

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
        // Nuevas categorías
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
    }
}
