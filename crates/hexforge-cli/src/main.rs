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
            "bind" => {
                // plugin bind <manifest.json> <plugin.wasm> — writes wasm_sha256
                // into the manifest (in place). Sign AFTER binding.
                if rest.len() != 2 {
                    eprintln!(
                        "error: usage: hexforge-cli plugin bind <manifest.json> <plugin.wasm>"
                    );
                    std::process::exit(2);
                }
                match hexforge_cli::plugin_bind_artifact(&rest[0], &rest[1]) {
                    Ok(msg) => println!("OK: {msg}"),
                    Err(message) => {
                        eprintln!("error: {message}");
                        std::process::exit(1);
                    }
                }
            }
            "new" => {
                let mut dir: Option<&str> = None;
                let mut id: Option<&str> = None;
                let mut name: Option<&str> = None;
                let mut i = 0;
                while i < rest.len() {
                    match rest[i].as_str() {
                        "--id" => {
                            if i + 1 >= rest.len() {
                                eprintln!("error: --id requires an argument");
                                std::process::exit(2);
                            }
                            id = Some(&rest[i + 1]);
                            i += 2;
                        }
                        "--name" => {
                            if i + 1 >= rest.len() {
                                eprintln!("error: --name requires an argument");
                                std::process::exit(2);
                            }
                            name = Some(&rest[i + 1]);
                            i += 2;
                        }
                        other => {
                            if dir.is_some() {
                                eprintln!("error: unexpected argument '{other}'");
                                std::process::exit(2);
                            }
                            dir = Some(other);
                            i += 1;
                        }
                    }
                }
                let Some(dir) = dir else {
                    eprintln!(
                        "error: usage: hexforge-cli plugin new <dir> [--id <id>] [--name <name>]"
                    );
                    std::process::exit(2);
                };
                match hexforge_cli::plugin_new(dir, id, name) {
                    Ok(msg) => println!("{msg}"),
                    Err(message) => {
                        eprintln!("error: {message}");
                        std::process::exit(1);
                    }
                }
            }
            "install" => {
                // plugin install <plugin.wasm> <manifest.json> --root <dir> [--sig <hex> --pub <hex>]
                let mut positional: Vec<&str> = Vec::new();
                let mut root: Option<&str> = None;
                let mut sig: Option<&str> = None;
                let mut pubkey: Option<&str> = None;
                let mut i = 0;
                while i < rest.len() {
                    match rest[i].as_str() {
                        "--root" | "--sig" | "--pub" => {
                            if i + 1 >= rest.len() {
                                eprintln!("error: {} requires an argument", rest[i]);
                                std::process::exit(2);
                            }
                            match rest[i].as_str() {
                                "--root" => root = Some(&rest[i + 1]),
                                "--sig" => sig = Some(&rest[i + 1]),
                                _ => pubkey = Some(&rest[i + 1]),
                            }
                            i += 2;
                        }
                        other => {
                            positional.push(other);
                            i += 1;
                        }
                    }
                }
                if positional.len() != 2 {
                    eprintln!("error: usage: hexforge-cli plugin install <plugin.wasm> <manifest.json> --root <dir> [--sig <hex> --pub <hex>]");
                    std::process::exit(2);
                }
                let Some(root) = root else {
                    eprintln!("error: --root <dir> is required (installs never write to an implicit location)");
                    std::process::exit(2);
                };
                match hexforge_cli::plugin_install(positional[0], positional[1], root, sig, pubkey)
                {
                    Ok(msg) => println!("{msg}"),
                    Err(message) => {
                        eprintln!("error: {message}");
                        std::process::exit(1);
                    }
                }
            }
            "grant" | "revoke" => {
                // plugin grant|revoke <id> --root <dir> --cap <cap>
                let verb = sub.as_str();
                let mut id: Option<&str> = None;
                let mut root: Option<&str> = None;
                let mut cap: Option<&str> = None;
                let mut i = 0;
                while i < rest.len() {
                    match rest[i].as_str() {
                        "--root" | "--cap" => {
                            if i + 1 >= rest.len() {
                                eprintln!("error: {} requires an argument", rest[i]);
                                std::process::exit(2);
                            }
                            if rest[i] == "--root" {
                                root = Some(&rest[i + 1]);
                            } else {
                                cap = Some(&rest[i + 1]);
                            }
                            i += 2;
                        }
                        other => {
                            if id.is_some() {
                                eprintln!("error: unexpected argument '{other}'");
                                std::process::exit(2);
                            }
                            id = Some(other);
                            i += 1;
                        }
                    }
                }
                let (Some(id), Some(root), Some(cap)) = (id, root, cap) else {
                    eprintln!(
                        "error: usage: hexforge-cli plugin {verb} <id> --root <dir> --cap <cap>"
                    );
                    std::process::exit(2);
                };
                let result = if verb == "grant" {
                    hexforge_cli::plugin_grant(id, root, cap)
                } else {
                    hexforge_cli::plugin_revoke(id, root, cap)
                };
                match result {
                    Ok(msg) => println!("{msg}"),
                    Err(message) => {
                        eprintln!("error: {message}");
                        std::process::exit(1);
                    }
                }
            }
            "run" => {
                // plugin run <id> --root <dir> --in <file> --out <file>
                let mut id: Option<&str> = None;
                let mut root: Option<&str> = None;
                let mut input: Option<&str> = None;
                let mut output: Option<&str> = None;
                let mut i = 0;
                while i < rest.len() {
                    match rest[i].as_str() {
                        "--root" | "--in" | "--out" => {
                            if i + 1 >= rest.len() {
                                eprintln!("error: {} requires an argument", rest[i]);
                                std::process::exit(2);
                            }
                            match rest[i].as_str() {
                                "--root" => root = Some(&rest[i + 1]),
                                "--in" => input = Some(&rest[i + 1]),
                                _ => output = Some(&rest[i + 1]),
                            }
                            i += 2;
                        }
                        other => {
                            if id.is_some() {
                                eprintln!("error: unexpected argument '{other}'");
                                std::process::exit(2);
                            }
                            id = Some(other);
                            i += 1;
                        }
                    }
                }
                let (Some(id), Some(root), Some(input), Some(output)) = (id, root, input, output)
                else {
                    eprintln!("error: usage: hexforge-cli plugin run <id> --root <dir> --in <file> --out <file>");
                    std::process::exit(2);
                };
                match hexforge_cli::plugin_run(id, root, input, output) {
                    Ok(msg) => println!("{msg}"),
                    Err(message) => {
                        eprintln!("error: {message}");
                        std::process::exit(1);
                    }
                }
            }
            other => {
                eprintln!("error: unknown plugin subcommand '{other}' (keygen|bind|sign|validate|new|install|grant|revoke|run)");
                std::process::exit(2);
            }
        },
        _ => {
            eprintln!(
                "Usage:\n  hexforge-cli run <recipe.hexforge> --in <file> [--in <file> ...] --out <file>\n  hexforge-cli validate <recipe.hexforge>\n  hexforge-cli plugin keygen\n  hexforge-cli plugin new <dir> [--id <id>] [--name <name>]\n  hexforge-cli plugin bind <manifest.json> <plugin.wasm>\n  hexforge-cli plugin sign <manifest.json> --key <hex>\n  hexforge-cli plugin validate <manifest.json>\n  hexforge-cli plugin install <plugin.wasm> <manifest.json> --root <dir> [--sig <hex> --pub <hex>]\n  hexforge-cli plugin grant|revoke <id> --root <dir> --cap <cap>\n  hexforge-cli plugin run <id> --root <dir> --in <file> --out <file>"
            );
            std::process::exit(2);
        }
    }
}
