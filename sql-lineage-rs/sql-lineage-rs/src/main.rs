use std::{env, fs, io::{self, Read}, process};
use sql_lineage_rs::{analyze, dot_output, json_output};

fn usage() {
    eprintln!("Usage : sql-lineage-rs [--format json|dot|text] <fichier.sql | ->");
    eprintln!("Exemple : cargo run -- --format dot examples/demo.sql");
    eprintln!("Remplacer le fichier par '-' pour lire depuis stdin.");
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut format = "json";
    let mut filename = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => { usage(); return Ok(()); }
            "--format" => {
                i += 1;
                format = args.get(i).ok_or("Valeur absente après --format")?.as_str();
                if !matches!(format, "json" | "dot" | "text") {
                    return Err("Formats possibles : json, dot, text".into());
                }
            }
            name if filename.is_none() => filename = Some(name.to_string()),
            _ => return Err("Trop d'arguments".into()),
        }
        i += 1;
    }
    let filename = filename.ok_or("Fichier SQL manquant")?;
    let sql = if filename == "-" {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer).map_err(|e| e.to_string())?;
        buffer
    } else {
        fs::read_to_string(filename).map_err(|e| format!("Lecture impossible : {e}"))?
    };
    let statements = analyze(&sql)?;
    match format {
        "dot" => print!("{}", dot_output(&statements)),
        "text" => {
            for (i, s) in statements.iter().enumerate() {
                println!("#{}, type={}, cible={}, sources={}", i + 1, s.kind,
                    s.target.as_deref().unwrap_or("(aucune)"), s.sources.join(", "));
            }
        }
        _ => println!("{}", json_output(&statements)),
    }
    Ok(())
}

fn main() {
    if let Err(message) = run() {
        eprintln!("Erreur : {message}");
        usage();
        process::exit(1);
    }
}
