//! Точка входа headless-режима. Только разбор аргументов; вся логика —
//! в библиотеке, чтобы оставаться тестируемой без спавна процессов.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, recipe] if cmd == "validate" => match hexforge_cli::validate_recipe(recipe) {
            Ok(msg) => println!("OK: {msg}"),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        },
        [cmd, recipe, rest @ ..] if cmd == "run" => {
            // Parse `--in <file>` (repeatable) + `--out <file>`
            let mut in_files: Vec<String> = Vec::new();
            let mut out_file: Option<String> = None;
            let mut i = 0;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--in" => {
                        if i + 1 >= rest.len() {
                            eprintln!("error: --in requires a file argument");
                            std::process::exit(2);
                        }
                        in_files.push(rest[i + 1].clone());
                        i += 2;
                    }
                    "--out" => {
                        if i + 1 >= rest.len() {
                            eprintln!("error: --out requires a file argument");
                            std::process::exit(2);
                        }
                        out_file = Some(rest[i + 1].clone());
                        i += 2;
                    }
                    other => {
                        eprintln!("error: unknown argument '{other}'");
                        std::process::exit(2);
                    }
                }
            }
            let Some(out) = out_file else {
                eprintln!("error: --out <file> is required");
                std::process::exit(2);
            };
            if in_files.is_empty() {
                eprintln!("error: at least one --in <file> is required");
                std::process::exit(2);
            }
            match hexforge_cli::run_recipe(recipe, &in_files, &out) {
                Ok(summary) => {
                    println!(
                        "OK: {} node(s), {} bytes written in {} ms",
                        summary.executed_nodes, summary.output_bytes, summary.duration_ms
                    );
                }
                Err(message) => {
                    eprintln!("error: {message}");
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!(
                "Usage:\n  hexforge-cli run <recipe.hexforge> --in <file> [--in <file> ...] --out <file>\n  hexforge-cli validate <recipe.hexforge>"
            );
            std::process::exit(2);
        }
    }
}
