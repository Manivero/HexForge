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
        [cmd, sub, rest @ ..] if cmd == "plugin" => match sub.as_str() {
            "keygen" => {
                if !rest.is_empty() {
                    eprintln!("error: `plugin keygen` takes no arguments");
                    std::process::exit(2);
                }
                let (pubkey, signing_key) = hexforge_cli::plugin_keygen();
                println!("pubkey={pubkey}\nsigning_key={signing_key}");
                eprintln!("warning: keep signing_key secret; it signs your manifest.json");
            }
            "sign" => {
                // plugin sign <manifest.json> --key <signing_key_hex>
                let mut manifest: Option<&str> = None;
                let mut key: Option<&str> = None;
                let mut i = 0;
                while i < rest.len() {
                    match rest[i].as_str() {
                        "--key" => {
                            if i + 1 >= rest.len() {
                                eprintln!("error: --key requires a hex argument");
                                std::process::exit(2);
                            }
                            key = Some(&rest[i + 1]);
                            i += 2;
                        }
                        other => {
                            if manifest.is_some() {
                                eprintln!("error: unexpected argument '{other}'");
                                std::process::exit(2);
                            }
                            manifest = Some(other);
                            i += 1;
                        }
                    }
                }
                let (Some(manifest), Some(key)) = (manifest, key) else {
                    eprintln!("error: usage: hexforge-cli plugin sign <manifest.json> --key <hex>");
                    std::process::exit(2);
                };
                match hexforge_cli::plugin_sign_manifest(manifest, key) {
                    Ok(sig) => println!("signature={sig}"),
                    Err(message) => {
                        eprintln!("error: {message}");
                        std::process::exit(1);
                    }
                }
            }
            "validate" => {
                if rest.len() != 1 {
                    eprintln!("error: usage: hexforge-cli plugin validate <manifest.json>");
                    std::process::exit(2);
                }
                match hexforge_cli::plugin_validate_manifest(&rest[0]) {
                    Ok(msg) => println!("OK: {msg}"),
                    Err(message) => {
                        eprintln!("error: {message}");
                        std::process::exit(1);
                    }
                }
            }
            other => {
                eprintln!("error: unknown plugin subcommand '{other}' (keygen|sign|validate)");
                std::process::exit(2);
            }
        },
        _ => {
            eprintln!(
                "Usage:\n  hexforge-cli run <recipe.hexforge> --in <file> [--in <file> ...] --out <file>\n  hexforge-cli validate <recipe.hexforge>\n  hexforge-cli plugin keygen\n  hexforge-cli plugin sign <manifest.json> --key <hex>\n  hexforge-cli plugin validate <manifest.json>"
            );
            std::process::exit(2);
        }
    }
}
