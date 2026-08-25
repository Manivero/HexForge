//! Точка входа headless-режима. Только разбор аргументов; вся логика —
//! в библиотеке, чтобы оставаться тестируемой без спавна процессов.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, recipe, flag_in, input, flag_out, output] if cmd == "run" && flag_in == "--in" && flag_out == "--out" =>
        {
            match hexforge_cli::run_recipe(recipe, input, output) {
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
                "Usage: hexforge-cli run <recipe.hexforge> --in <input-file> --out <output-file>"
            );
            std::process::exit(2);
        }
    }
}
