/// Herramienta de diagnóstico: extrae y descifra síntesis de entretenimiento de la BD local.
/// Solo lectura (SELECT). No modifica ningún dato.
/// Uso: cargo run --example audit_syntheses > ../audit-syntheses-entretenimiento.md
fn main() {
    use chrono::{TimeZone, Utc};

    // SQLCipher PRAGMA key: data_local_dir/flowweaver (igual que lib.rs)
    let data_dir = dirs_next::data_local_dir()
        .expect("no se pudo resolver data_local_dir")
        .join("flowweaver");
    let pragma_key = format!("fw-{}", data_dir.to_string_lossy());
    let db_path = data_dir.join("resources.db");

    eprintln!("[audit] BD: {}", db_path.display());
    eprintln!("[audit] PRAGMA key derivada desde: {}", data_dir.display());

    // Field-level AES key: app_data_dir (commands.rs::db_key para desktop).
    // Tauri en Windows puede resolver a Roaming\com.flowweaver.app o Local\com.flowweaver.app.
    // Probamos ambos y usamos el que descifra correctamente.
    let candidate_dirs: Vec<std::path::PathBuf> = {
        let mut v = vec![];
        if let Some(roaming) = dirs_next::data_dir() {  // AppData\Roaming
            v.push(roaming.join("com.flowweaver.app"));
        }
        if let Some(local) = dirs_next::data_local_dir() {  // AppData\Local
            v.push(local.join("com.flowweaver.app"));
        }
        // Fallback: misma key que SQLCipher (improbable pero cubre el caso)
        v.push(data_dir.clone());
        v
    };
    eprintln!("[audit] Candidatos para field-level key:");
    for (i, c) in candidate_dirs.iter().enumerate() {
        eprintln!("  [{i}] fw-{}", c.display());
    }

    let conn = rusqlite::Connection::open(&db_path)
        .expect("no se pudo abrir la BD");
    conn.pragma_update(None, "key", &pragma_key)
        .expect("PRAGMA key falló — key SQLCipher incorrecta");

    // Verificar tabla syntheses
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='syntheses'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0) > 0;

    let now_str = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();

    if !table_exists {
        println!("# Auditoría síntesis entretenimiento — {now_str}");
        println!();
        println!("Total síntesis encontradas: 0");
        println!();
        println!("La tabla `syntheses` no existe en esta BD.");
        return;
    }

    let mut stmt = conn
        .prepare(
            "SELECT anchor_key, anchor_type, category, synthesis_type,
                    content_encrypted, generated_at
             FROM syntheses
             WHERE synthesis_type = 'entretenimiento'
             ORDER BY generated_at ASC",
        )
        .expect("error preparando SELECT");

    struct Row {
        anchor_key: String,
        anchor_type: String,
        category: String,
        synthesis_type: String,
        content_encrypted: String,
        generated_at: i64,
    }

    let rows: Vec<Row> = stmt
        .query_map([], |r| {
            Ok(Row {
                anchor_key:        r.get(0)?,
                anchor_type:       r.get(1)?,
                category:          r.get(2)?,
                synthesis_type:    r.get(3)?,
                content_encrypted: r.get(4)?,
                generated_at:      r.get(5)?,
            })
        })
        .expect("error ejecutando query")
        .filter_map(|r| r.ok())
        .collect();

    // Detectar qué field-level key funciona descifrado con la primera fila que tengamos
    let field_key = if rows.is_empty() {
        pragma_key.clone()
    } else {
        let test_ciphertext = &rows[0].content_encrypted;
        let mut found_key = None;
        for dir in &candidate_dirs {
            let k = format!("fw-{}", dir.to_string_lossy());
            if flowweaver_lib::crypto::decrypt_any(test_ciphertext, &k).is_some() {
                eprintln!("[audit] Field-level key válida: fw-{}", dir.display());
                found_key = Some(k);
                break;
            }
        }
        match found_key {
            Some(k) => k,
            None => {
                eprintln!("[audit] WARN: ninguna candidate key descifró la primera fila. Usando PRAGMA key como fallback.");
                pragma_key.clone()
            }
        }
    };

    println!("# Auditoría síntesis entretenimiento — {now_str}");
    println!();
    println!("Total síntesis encontradas: {}", rows.len());
    println!();

    for (i, row) in rows.iter().enumerate() {
        let fecha = Utc
            .timestamp_opt(row.generated_at, 0)
            .single()
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| format!("timestamp inválido ({})", row.generated_at));

        println!("## Síntesis #{} — {fecha}", i + 1);
        println!();
        println!("- anchor_key: {}", row.anchor_key);
        println!("- anchor_type: {}", row.anchor_type);
        println!("- category original: {}", row.category);
        println!("- synthesis_type: {}", row.synthesis_type);
        println!("- generated_at (Unix): {}", row.generated_at);
        println!();
        println!("### Contenido descifrado");
        println!();

        match flowweaver_lib::crypto::decrypt_any(&row.content_encrypted, &field_key) {
            Some(content) => println!("{content}"),
            None => println!("**ERROR DESCIFRADO** — ciphertext no compatible con ninguna key candidata."),
        }

        println!();
        println!("---");
        println!();
    }
}
