use std::env;
use std::process::exit;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[cfg(unix)]
use tokio::net::UnixStream;
#[cfg(windows)]
use tokio::net::windows::named_pipe::ClientOptions;

fn print_help() {
    println!(r#"🎵 Cadence CLI — Lightning-fast music & focus controller

Uso:
  cadence [comando] [argumentos]

Comandos de Reproducción:
  status                   Muestra el estado actual de la pista y del temporizador
  play <canción o url>     Busca y reproduce una canción o enlace directo
  pause                    Pausa la reproducción
  resume                   Reanuda la reproducción
  toggle                   Alterna entre play y pausa
  next                     Salta a la siguiente canción de la cola
  prev                     Salta a la canción anterior
  volume <0-100>           Ajusta el volumen (ej. cadence volume 80)

Audio FX & Filtros:
  preset <nombre>          Aplica un filtro: normal, lofi, bass_boost, loudnorm, slowed_reverb, nightcore
  speed <0.5-2.0>          Ajusta la velocidad de reproducción (ej. cadence speed 1.1)

Focus & Pomodoro:
  focus start              Inicia o reanuda la sesión de concentración
  focus pause              Pausa la sesión de concentración
  focus reset              Reinicia el temporizador
  focus skip               Avanza a la siguiente fase (Focus / Descanso)

Cola de Reproducción:
  queue                    Lista las canciones en la cola
  queue clear              Vacía la cola
"#);
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" || args[0] == "help" {
        print_help();
        return;
    }

    let cmd = args[0].to_lowercase();
    let payload = match cmd.as_str() {
        "status" => json!({ "cmd": "status" }),
        "pause" => json!({ "cmd": "pause" }),
        "resume" => json!({ "cmd": "resume" }),
        "toggle" => json!({ "cmd": "toggle" }),
        "next" => json!({ "cmd": "next" }),
        "prev" | "previous" => json!({ "cmd": "prev" }),
        "play" => {
            if args.len() < 2 {
                eprintln!("❌ Error: Especifica qué canción o URL reproducir. Ej: cadence play \"lofi hip hop\"");
                exit(1);
            }
            let query = args[1..].join(" ");
            json!({ "cmd": "play", "query": query })
        }
        "volume" | "vol" => {
            if args.len() < 2 {
                eprintln!("❌ Error: Especifica el volumen (0 a 100). Ej: cadence volume 75");
                exit(1);
            }
            let val: f64 = args[1].parse().unwrap_or(100.0);
            json!({ "cmd": "volume", "value": val })
        }
        "preset" | "filter" => {
            if args.len() < 2 {
                eprintln!("❌ Error: Especifica el preset (normal, lofi, bass_boost, loudnorm, slowed_reverb, nightcore)");
                exit(1);
            }
            json!({ "cmd": "preset", "value": args[1].to_lowercase() })
        }
        "speed" => {
            if args.len() < 2 {
                eprintln!("❌ Error: Especifica la velocidad (0.5 a 2.0). Ej: cadence speed 1.15");
                exit(1);
            }
            let val: f64 = args[1].parse().unwrap_or(1.0);
            json!({ "cmd": "speed", "value": val })
        }
        "focus" => {
            let action = if args.len() >= 2 {
                args[1].to_lowercase()
            } else {
                "start".to_string()
            };
            json!({ "cmd": "focus", "action": action })
        }
        "queue" => {
            if args.len() >= 2 && args[1] == "clear" {
                json!({ "cmd": "queue_clear" })
            } else {
                json!({ "cmd": "queue_list" })
            }
        }
        _ => {
            eprintln!("❌ Comando desconocido: '{}'", cmd);
            print_help();
            exit(1);
        }
    };

    let socket_path = cadence_lib::services::control_server::control_socket_path();

    #[cfg(unix)]
    let mut stream = match UnixStream::connect(&socket_path).await {
        Ok(s) => s,
        Err(_) => {
            eprintln!("⚠️ No se pudo conectar con Cadence. ¿Está la aplicación ejecutándose?");
            exit(2);
        }
    };

    #[cfg(windows)]
    let mut stream = {
        let pipe_name = socket_path.to_string_lossy();
        match ClientOptions::new().open(&pipe_name) {
            Ok(s) => s,
            Err(_) => {
                eprintln!("⚠️ No se pudo conectar con Cadence. ¿Está la aplicación ejecutándose?");
                exit(2);
            }
        }
    };

    let (reader, mut writer) = tokio::io::split(&mut stream);
    let mut msg_bytes = serde_json::to_vec(&payload).unwrap_or_default();
    msg_bytes.push(b'\n');

    if writer.write_all(&msg_bytes).await.is_err() || writer.flush().await.is_err() {
        eprintln!("❌ Error enviando comando a Cadence.");
        exit(1);
    }

    let mut response_line = String::new();
    let mut buf_reader = BufReader::new(reader);
    if buf_reader.read_line(&mut response_line).await.is_err() {
        eprintln!("❌ Error leyendo respuesta de Cadence.");
        exit(1);
    }

    match serde_json::from_str::<Value>(response_line.trim()) {
        Ok(val) => {
            if val["ok"].as_bool() == Some(true) {
                if let Some(data) = val.get("data") {
                    format_cli_output(&cmd, data);
                } else {
                    println!("✅ OK");
                }
            } else {
                let err_msg = val["error"].as_str().unwrap_or("Error desconocido");
                eprintln!("❌ Error de Cadence: {}", err_msg);
                exit(1);
            }
        }
        Err(_) => {
            println!("{}", response_line.trim());
        }
    }
}

fn format_cli_output(cmd: &str, data: &Value) {
    match cmd {
        "status" => {
            let player = &data["player"];
            let focus = &data["focus"];
            println!("🎵 ESTADO DE CADENCE");
            println!("──────────────────────────────────────────");
            let status = player["status"].as_str().unwrap_or("Stopped");
            println!("▶ Reproducción : {}", status);
            if let Some(curr) = player.get("current").filter(|c| !c.is_null()) {
                let title = curr["title"].as_str().unwrap_or("Desconocido");
                let artist = curr["artist"].as_str().unwrap_or("");
                println!("🎶 Pista        : {} - {}", title, artist);
                let pos = player["position_seconds"].as_f64().unwrap_or(0.0) as u32;
                let dur = player["duration_seconds"].as_f64().unwrap_or(0.0) as u32;
                println!("⏱️  Tiempo       : {:02}:{:02} / {:02}:{:02}", pos / 60, pos % 60, dur / 60, dur % 60);
            }
            println!("🔊 Volumen      : {}%", player["volume"].as_f64().unwrap_or(100.0) as u32);
            println!("🎛️  Preset Audio : {}", player["audio_preset"].as_str().unwrap_or("normal"));
            println!("⚡ Velocidad    : {:.2}x", player["speed"].as_f64().unwrap_or(1.0));
            println!("──────────────────────────────────────────");
            let f_phase = focus["phase"].as_str().unwrap_or("idle");
            let f_rem = focus["remaining_seconds"].as_u64().unwrap_or(0) as u32;
            let f_run = focus["is_running"].as_bool().unwrap_or(false);
            let f_comp = focus["sessions_completed"].as_u64().unwrap_or(0);
            println!("🎯 Focus Phase  : {} ({})", f_phase, if f_run { "Activo" } else { "Pausado" });
            println!("⏳ Tiempo Focus : {:02}:{:02}", f_rem / 60, f_rem % 60);
            println!("🏆 Sesiones     : {}", f_comp);
        }
        "play" => {
            if let Some(track) = data.get("playing") {
                let title = track["title"].as_str().unwrap_or("");
                let artist = track["artist"].as_str().unwrap_or("");
                println!("🎶 Reproduciendo ahora: {} {}", title, if artist.is_empty() { "".to_string() } else { format!("— {}", artist) });
            } else {
                println!("✅ Comando play enviado");
            }
        }
        "queue" => {
            if let Some(list) = data["queue"].as_array() {
                if list.is_empty() {
                    println!("📋 La cola de reproducción está vacía.");
                } else {
                    println!("📋 COLA DE REPRODUCCIÓN ({} pistas)", list.len());
                    for (i, item) in list.iter().enumerate() {
                        let t = &item["track"];
                        let title = t["title"].as_str().unwrap_or("");
                        let artist = t["artist"].as_str().unwrap_or("");
                        println!("  {}. {} {}", i + 1, title, if artist.is_empty() { "".to_string() } else { format!("— {}", artist) });
                    }
                }
            } else {
                println!("✅ Cola vaciada");
            }
        }
        _ => {
            println!("✅ {}", serde_json::to_string_pretty(data).unwrap_or_default());
        }
    }
}
