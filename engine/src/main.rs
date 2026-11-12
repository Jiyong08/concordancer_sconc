mod builder;
mod editor;
mod handlers;
mod jamo_table;
mod models;
mod normalizer;
mod search;

pub use models::{DbBuildManager, DbBuildProgress};
pub use normalizer::Normalizer;

use editor::{handle_open_editor, handle_source_jump};
use handlers::{
    handle_build_db, handle_build_db_cancel, handle_build_db_status, handle_build_db_stop,
    handle_document_delete, handle_overview, handle_pick_folder, handle_segment_detail,
    handle_segment_edit,
};
use models::{parse_query_params, AppState, MemoryProfile};
use normalizer::find_bracket_query_match;
use search::{handle_search, handle_search_cancel, handle_search_stream};

const EMBEDDED_INDEX_HTML: &[u8] = include_bytes!("../../web/index.html");
const EMBEDDED_STYLES_CSS: &[u8] = include_bytes!("../../web/styles.css");
const EMBEDDED_HANGUL_AUTOMATA_JS: &[u8] = include_bytes!("../../web/hangul_automata.js");
const EMBEDDED_APP_JS: &[u8] = include_bytes!("../../web/app.js");


use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Method, Response, Server, StatusCode};

fn serve_static_file(path: &Path, content_type: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    serve_static_or_embedded(path, None, content_type)
}

fn serve_static_or_embedded(
    path: &Path,
    embedded: Option<&'static [u8]>,
    content_type: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let bytes_opt = fs::read(path)
        .ok()
        .or_else(|| embedded.map(|b| b.to_vec()));
    match bytes_opt {
        Some(bytes) => {
            let header =
                Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()).unwrap();
            let no_cache =
                Header::from_bytes(&b"Cache-Control"[..], &b"no-cache, no-store, must-revalidate"[..]).unwrap();
            let pragma =
                Header::from_bytes(&b"Pragma"[..], &b"no-cache"[..]).unwrap();
            Response::from_data(bytes)
                .with_header(header)
                .with_header(no_cache)
                .with_header(pragma)
        }
        None => Response::from_string("404 Not Found").with_status_code(StatusCode(404)),
    }
}

fn find_root_dir() -> PathBuf {
    if let Ok(val) = env::var("SCONC_HOME") {
        let p = PathBuf::from(val);
        if p.exists() {
            return p;
        }
    }
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if cwd.join("web").join("index.html").exists() || cwd.join("db").exists() {
        return cwd;
    }
    if let Some(parent) = cwd.parent() {
        if parent.join("web").join("index.html").exists() || parent.join("db").exists() {
            return parent.to_path_buf();
        }
    }
    if let Ok(exe_path) = env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let mut anc = exe_dir;
            loop {
                if anc.join("web").join("index.html").exists() || anc.join("db").exists() {
                    return anc.to_path_buf();
                }
                if let Some(p) = anc.parent() {
                    anc = p;
                } else {
                    break;
                }
            }
            return exe_dir.to_path_buf();
        }
    }
    cwd
}


#[derive(Serialize, Deserialize, Clone, Debug)]
struct BrowserInfo {
    id: String,
    name: String,
    path: String,
    installed: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct SconcConfig {
    #[serde(default = "default_system_browser")]
    default_browser: String,
    #[serde(default)]
    browser_path: String,
}

fn default_system_browser() -> String {
    "system".to_string()
}

fn get_sconc_config_path(root_dir: &Path) -> PathBuf {
    root_dir.join("sconc_config.json")
}

fn load_sconc_config(root_dir: &Path) -> SconcConfig {
    let p = get_sconc_config_path(root_dir);
    if let Ok(content) = fs::read_to_string(&p) {
        if let Ok(cfg) = serde_json::from_str::<SconcConfig>(&content) {
            return cfg;
        }
    }
    SconcConfig {
        default_browser: "system".to_string(),
        browser_path: "".to_string(),
    }
}

fn save_sconc_config(root_dir: &Path, cfg: &SconcConfig) {
    let p = get_sconc_config_path(root_dir);
    if let Ok(content) = serde_json::to_string_pretty(cfg) {
        let _ = fs::write(p, content);
    }
}

fn detect_installed_browsers() -> Vec<BrowserInfo> {
    let mut list = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let local_appdata = env::var("LOCALAPPDATA").unwrap_or_default();
        let program_files = env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
        let program_files_x86 = env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".to_string());
        let user_profile = env::var("USERPROFILE").unwrap_or_default();

        let candidates = [
            (
                "msedge",
                "Microsoft Edge (독립 앱 창 모드 권장)",
                vec![
                    format!("{PF86}\\Microsoft\\Edge\\Application\\msedge.exe", PF86=program_files_x86),
                    format!("{PF}\\Microsoft\\Edge\\Application\\msedge.exe", PF=program_files),
                    format!("{LocalAppdata}\\Microsoft\\Edge\\Application\\msedge.exe", LocalAppdata=local_appdata),
                    format!("{UP}\\AppData\\Local\\Microsoft\\Edge\\Application\\msedge.exe", UP=user_profile),
                ],
            ),
            (
                "chrome",
                "Google Chrome (독립 앱 창 모드 지원)",
                vec![
                    format!("{PF}\\Google\\Chrome\\Application\\chrome.exe", PF=program_files),
                    format!("{PF86}\\Google\\Chrome\\Application\\chrome.exe", PF86=program_files_x86),
                    format!("{LocalAppdata}\\Google\\Chrome\\Application\\chrome.exe", LocalAppdata=local_appdata),
                    format!("{UP}\\AppData\\Local\\Google\\Chrome\\Application\\chrome.exe", UP=user_profile),
                ],
            ),
            (
                "whale",
                "Naver Whale (네이버 웨일)",
                vec![
                    format!("{PF}\\Naver\\Naver Whale\\Application\\whale.exe", PF=program_files),
                    format!("{PF86}\\Naver\\Naver Whale\\Application\\whale.exe", PF86=program_files_x86),
                    format!("{LocalAppdata}\\Naver\\Naver Whale\\Application\\whale.exe", LocalAppdata=local_appdata),
                    format!("{UP}\\AppData\\Local\\Naver\\Naver Whale\\Application\\whale.exe", UP=user_profile),
                ],
            ),
            (
                "firefox",
                "Mozilla Firefox",
                vec![
                    format!("{PF}\\Mozilla Firefox\\firefox.exe", PF=program_files),
                    format!("{PF86}\\Mozilla Firefox\\firefox.exe", PF86=program_files_x86),
                ],
            ),
            (
                "brave",
                "Brave Browser",
                vec![
                    format!("{PF}\\BraveSoftware\\Brave-Browser\\Application\\brave.exe", PF=program_files),
                    format!("{LocalAppdata}\\BraveSoftware\\Brave-Browser\\Application\\brave.exe", LocalAppdata=local_appdata),
                ],
            ),
        ];

        for (id, name, path_candidates) in candidates {
            let mut found_path = "".to_string();
            let mut is_installed = false;
            for p in path_candidates {
                if Path::new(&p).exists() {
                    found_path = p;
                    is_installed = true;
                    break;
                }
            }
            list.push(BrowserInfo {
                id: id.to_string(),
                name: name.to_string(),
                path: found_path,
                installed: is_installed,
            });
        }
    }

    // Windows 시스템 기본 브라우저 옵션 (항상 사용 가능)
    list.push(BrowserInfo {
        id: "system".to_string(),
        name: "Windows 시스템 기본 브라우저".to_string(),
        path: "".to_string(),
        installed: true,
    });

    list
}

fn is_chromium_based(path_str: &str) -> bool {
    let low = path_str.to_lowercase();
    low.contains("msedge.exe")
        || low.contains("chrome.exe")
        || low.contains("whale.exe")
        || low.contains("brave.exe")
}

fn launch_specific_browser(browser_path: &str, url: &str, app_mode: bool) {
    if !browser_path.is_empty() && Path::new(browser_path).exists() {
        if app_mode && is_chromium_based(browser_path) {
            let _ = Command::new(browser_path)
                .args([
                    &format!("--app={}", url),
                    "--no-proxy-server",
                    "--window-size=1480,920",
                ])
                .spawn();
        } else if is_chromium_based(browser_path) {
            let _ = Command::new(browser_path)
                .args(["--no-proxy-server", url])
                .spawn();
        } else {
            let _ = Command::new(browser_path).arg(url).spawn();
        }
        return;
    }

    // browser_path가 지정되지 않은 경우, 독립 실행형 데스크톱 창(App Mode)으로
    // 공용 네트워크 프록시 차단 없이 실행하기 위해 설치된 Edge/Chrome/Whale을 우선 탐색
    if app_mode {
        for b in detect_installed_browsers() {
            if b.installed && !b.path.is_empty() && is_chromium_based(&b.path) {
                if Command::new(&b.path)
                    .args([
                        &format!("--app={}", url),
                        "--no-proxy-server",
                        "--window-size=1480,920",
                    ])
                    .spawn()
                    .is_ok()
                {
                    return;
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("xdg-open").arg(url).spawn();
    }
}

fn open_browser_with_root(url: &str, root_dir: &Path) {
    let cfg = load_sconc_config(root_dir);
    let use_app_mode = cfg.default_browser != "system" || cfg.browser_path.is_empty();
    launch_specific_browser(&cfg.browser_path, url, use_app_mode);
}

fn handle_installed_browsers(state: &AppState) -> Response<std::io::Cursor<Vec<u8>>> {
    let browsers = detect_installed_browsers();
    let cfg = load_sconc_config(&state.root_dir);

    let json = serde_json::json!({
        "status": "ok",
        "current_browser": cfg.default_browser,
        "current_path": cfg.browser_path,
        "browsers": browsers
    });
    Response::from_string(json.to_string())
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..]).unwrap())
}

fn handle_switch_browser(state: &AppState, params: &HashMap<String, String>) -> Response<std::io::Cursor<Vec<u8>>> {
    let browser_id = params.get("browser_id").map(|s| s.as_str()).unwrap_or("system");
    let browser_path = params.get("browser_path").map(|s| s.as_str()).unwrap_or("");
    let set_default = params.get("set_default").map(|s| s == "1" || s == "true").unwrap_or(true);
    let open_now = params.get("open_now").map(|s| s == "1" || s == "true").unwrap_or(true);
    let port = state.port;
    let url = format!("http://127.0.0.1:{}", port);

    if set_default {
        let cfg = SconcConfig {
            default_browser: browser_id.to_string(),
            browser_path: browser_path.to_string(),
        };
        save_sconc_config(&state.root_dir, &cfg);
    }

    if open_now {
        launch_specific_browser(browser_path, &url, browser_id != "system");
    }

    let json = serde_json::json!({
        "status": "ok",
        "message": format!("브라우저 설정이 적용되었습니다. (id={}, set_default={})", browser_id, set_default),
        "browser_id": browser_id,
        "browser_path": browser_path,
        "set_default": set_default
    });
    Response::from_string(json.to_string())
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..]).unwrap())
}


fn main() {
    let args: Vec<String> = env::args().collect();
    let mut port: u16 = 8787;
    // 단독 실행 파일(sconc.exe)을 더블클릭하여 실행할 때 즉시 창이 열리도록 기본값을 true로 설정
    let mut auto_open = true;
    for i in 1..args.len() {
        if args[i] == "--port" && i + 1 < args.len() {
            port = args[i + 1].parse().unwrap_or(8787);
        }
        if args[i] == "--open" {
            auto_open = true;
        }
        if args[i] == "--no-open" || args[i] == "--headless" {
            auto_open = false;
        }
    }

    let root_dir = find_root_dir();
    let _ = fs::create_dir_all(root_dir.join("db"));
    let db_path = root_dir.join("db").join("concordance.corpdb");
    let web_dir = root_dir.join("web");

    // DB 스키마 자동 초기화 (외부 설치/스크립트 없이도 빈 DB가 즉시 준비되도록 보장)
    if let Ok(conn) = Connection::open(&db_path) {
        let _ = builder::init_db_schema(&conn);
    }

    let pua_path = {
        let p1 = root_dir.join("assets").join("data").join("pua_table.json");
        if p1.exists() {
            p1
        } else {
            root_dir
                .join("HNI_MMK_Kug_1.2")
                .join("Old_Hangul")
                .join("program")
                .join("app")
                .join("lib")
                .join("pua_table.json")
        }
    };

    let hanbatang_path = {
        let p1 = root_dir.join("assets").join("fonts").join("HANBatang.ttf");
        if p1.exists() {
            p1
        } else {
            root_dir
                .join("HNI_MMK_Kug_1.2")
                .join("Reviewer_Fonts")
                .join("HANBatang.ttf")
        }
    };

    let kopub_ttf_path = {
        let p1 = root_dir.join("assets").join("fonts").join("KoPubWorldDotumMedium.ttf");
        if p1.exists() {
            p1
        } else {
            root_dir.join("KoPubWorld Dotum Medium.ttf")
        }
    };

    let kopub_otf_path = {
        let p1 = root_dir.join("assets").join("fonts").join("KoPubWorldDotumProMedium.otf");
        if p1.exists() {
            p1
        } else {
            root_dir.join("KoPubWorld Dotum_Pro Medium.otf")
        }
    };

    let mem_profile = MemoryProfile::detect();
    let normalizer = Normalizer::new(&pua_path);

    // 만약 DB가 비어 있고 sample 폴더에 말뭉치 파일이 존재한다면 자동으로 초기 DB 구축 수행
    if let Ok(conn) = Connection::open(&db_path) {
        let doc_cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
            .unwrap_or(0);
        let sample_dir = root_dir.join("sample");
        if doc_cnt == 0 && sample_dir.exists() {
            println!("  [초기화] DB가 비어 있어 sample 폴더의 말뭉치를 내장 파서로 자동 구축합니다...");
            let tmp_mgr = Arc::new(Mutex::new(DbBuildManager::new()));
            {
                let mut m = tmp_mgr.lock().unwrap();
                m.progress.is_running = true;
                m.progress.status = "RUNNING".to_string();
            }
            let _ = builder::build_folder_into_db_native(&sample_dir, &db_path, &normalizer, &tmp_mgr);
        }
    }

    let state = Arc::new(AppState {
        root_dir: root_dir.clone(),
        db_path: db_path.clone(),
        web_dir,
        hanbatang_path,
        kopub_ttf_path,
        kopub_otf_path,
        normalizer,
        memory_profile: mem_profile.clone(),
        latest_search_seq: AtomicU64::new(0),
        cancel_search_seq: AtomicU64::new(0),
        db_builder: Arc::new(Mutex::new(DbBuildManager::new())),
        port,
    });

    if args.iter().any(|a| a == "--self-test") {
        let conn = Connection::open(&state.db_path).expect("DB open failed");
        let doc_cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
            .unwrap_or(0);
        let file_cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM source_files", [], |r| r.get(0))
            .unwrap_or(0);
        let seg_cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM segments", [], |r| r.get(0))
            .unwrap_or(0);
        println!(
            "[SELF-TEST] DB verified: documents={}, source_files={}, segments={}",
            doc_cnt, file_cnt, seg_cnt
        );
        // Test Bracket Jamo Search [ㄱ/ㅏ/ㅇ], [ㄱ/*/ㅇ], [ㅎ/ㅏ/][ㄴ/ㆍ/ㄹ]
        for pat in &["[ㄱ/ㅏ/ㅇ]", "[ㄱ/*/ㅇ]", "[?/ㅏ/+]", "[ㅎ/ㅏ/][ㄴ/ㆍ/ㄹ]"] {
            let b_toks = state
                .normalizer
                .parse_bracket_query(pat)
                .expect("Failed to parse bracket query");
            let sample_text = "아름다운 금강산과 공산에서 하\u{1102}\u{119E}\u{11AF}을 보았다.";
            let units = state.normalizer.tokenize_text_units(sample_text);
            let matched = find_bracket_query_match(&b_toks, &units);
            println!("[SELF-TEST Bracket] {} -> matched={:?}", pat, matched);
        }
        return;
    }

    let bind_addr = format!("127.0.0.1:{}", port);
    let url = format!("http://127.0.0.1:{}", port);

    println!("================================================================");
    println!("  Concordance (sconc.exe) — 한국어 통시·공시 다층 말뭉치 통합 검색기");
    println!("  [독립 실행형(Standalone Zero-Install) 애플리케이션 모드]");
    println!("================================================================");
    println!("  * 프로젝트 루트 : {}", root_dir.display());
    println!("  * 데이터베이스  : {}", db_path.display());
    println!(
        "  * 메모리 프로파일: {} (총 {:.1}GB / 가용 {:.1}GB)",
        mem_profile.tier_label, mem_profile.total_ram_gb, mem_profile.available_ram_gb
    );
    println!("  * 내장 DB 빌더  : Rust Native Builder (Python 등 외부 설치 불필요)");
    println!("  * 로컬 주소     : {} (공용 네트워크 프록시 우회)", url);
    println!("================================================================");

    let server = match Server::http(&bind_addr) {
        Ok(s) => s,
        Err(_) => {
            println!("  [안내] 포트 {}에서 실행 중인 이전 sconc 프로세스를 종료하고 새 엔진으로 교체합니다...", port);
            #[cfg(target_os = "windows")]
            {
                let my_pid = std::process::id();
                let _ = Command::new("taskkill")
                    .args(["/F", "/FI", &format!("PID ne {}", my_pid), "/IM", "sconc.exe"])
                    .output();
            }
            let mut bound_server = None;
            for _ in 0..15 {
                std::thread::sleep(std::time::Duration::from_millis(150));
                if let Ok(s) = Server::http(&bind_addr) {
                    bound_server = Some(s);
                    break;
                }
            }
            match bound_server {
                Some(s) => {
                    println!("  [완료] 새 검색기 엔진이 포트 {}에 바인딩되었습니다.", port);
                    s
                }
                None => {
                    println!("  [안내] 포트 {}에서 이미 검색기 엔진이 실행 중입니다.", port);
                    if auto_open {
                        println!("  [안내] 독립 실행 창을 엽니다: {}", url);
                        open_browser_with_root(&url, &root_dir);
                    }
                    return;
                }
            }
        }
    };

    if auto_open {
        open_browser_with_root(&url, &root_dir);
    }

    for mut req in server.incoming_requests() {
        let state = Arc::clone(&state);
        std::thread::spawn(move || {
            let url = req.url().to_string();
            let path_part = url.split('?').next().unwrap_or("/");
            let params = parse_query_params(&url);

            let resp = match (req.method(), path_part) {
                (&Method::Get, "/api/overview") => handle_overview(&state),
                (&Method::Get, "/api/search") => handle_search(&state, &params),
                (&Method::Get, "/api/search/stream") | (&Method::Get, "/api/search/stream/") => {
                    handle_search_stream(&state, &params, req);
                    return;
                },
                (&Method::Post, "/api/search/cancel") => handle_search_cancel(&state),
                (&Method::Get, "/api/segment") => handle_segment_detail(&state, &params),
                (&Method::Get, "/api/source-jump") => handle_source_jump(&state, &params),
                (&Method::Get, "/api/open-editor") | (&Method::Post, "/api/open-editor") => handle_open_editor(&params),
                (&Method::Get, "/api/system/installed-browsers") => handle_installed_browsers(&state),
                (&Method::Get, "/api/system/switch-browser") | (&Method::Post, "/api/system/switch-browser") => handle_switch_browser(&state, &params),
                (&Method::Get, "/api/system/pick-folder") | (&Method::Post, "/api/system/pick-folder") => handle_pick_folder(&state),
                (&Method::Get, "/api/document/delete") | (&Method::Post, "/api/document/delete") => handle_document_delete(&state, &params),
                (&Method::Post, "/api/segment/edit") => handle_segment_edit(&state, &mut req),
                (&Method::Post, "/api/build-db") | (&Method::Post, "/api/build_db") => handle_build_db(&state, &mut req),
                (&Method::Get, "/api/build-db/status") | (&Method::Get, "/api/build_db/status") => handle_build_db_status(&state),
                (&Method::Post, "/api/build-db/stop") | (&Method::Post, "/api/build_db/stop") => handle_build_db_stop(&state),
                (&Method::Post, "/api/build-db/cancel") | (&Method::Post, "/api/build_db/cancel") => handle_build_db_cancel(&state),
                (&Method::Get, "/api/export/mapping.tsv") => serve_static_file(
                    &state.root_dir.join("db").join("corpus_db_mapping.tsv"),
                    "text/tab-separated-values; charset=utf-8",
                ),
                (&Method::Get, "/api/export/mapping.json") => serve_static_file(
                    &state.root_dir.join("db").join("corpus_db_mapping.json"),
                    "application/json; charset=utf-8",
                ),
                (&Method::Get, "/fonts/HANBatang.ttf") => {
                    serve_static_file(&state.hanbatang_path, "font/ttf")
                }
                (&Method::Get, "/fonts/KoPubWorldDotumMedium.ttf") => {
                    serve_static_file(&state.kopub_ttf_path, "font/ttf")
                }
                (&Method::Get, "/fonts/KoPubWorldDotumProMedium.otf") => {
                    serve_static_file(&state.kopub_otf_path, "font/otf")
                }
                (&Method::Get, "/") | (&Method::Get, "/index.html") => serve_static_or_embedded(
                    &state.web_dir.join("index.html"),
                    Some(EMBEDDED_INDEX_HTML),
                    "text/html; charset=utf-8",
                ),
                (&Method::Get, "/styles.css") => serve_static_or_embedded(
                    &state.web_dir.join("styles.css"),
                    Some(EMBEDDED_STYLES_CSS),
                    "text/css; charset=utf-8",
                ),
                (&Method::Get, "/hangul_automata.js") => serve_static_or_embedded(
                    &state.web_dir.join("hangul_automata.js"),
                    Some(EMBEDDED_HANGUL_AUTOMATA_JS),
                    "application/javascript; charset=utf-8",
                ),
                (&Method::Get, "/app.js") => serve_static_or_embedded(
                    &state.web_dir.join("app.js"),
                    Some(EMBEDDED_APP_JS),
                    "application/javascript; charset=utf-8",
                ),
                _ => Response::from_string("404 Not Found").with_status_code(StatusCode(404)),
            };
            let _ = req.respond(resp);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::find_target_line_in_source;
    use crate::normalizer::find_bracket_query_match;
    use regex::Regex;
    use crate::search::{
        check_word_boundary, evaluate_segment_match, expand_bracket_syntax_to_regex,
        find_all_char_subslices, has_regex_syntax, is_jongseong_jamo, query_requires_eojeol_start,
        SearchQueryContext,
    };
    #[test]
    fn test_find_target_line_in_source_exact() {
        let lines = vec![
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
            "<TEI>",
            "  <text>",
            "    <s n=\"1\" id=\"BTAA0001-00000001\">첫 번째 문장입니다.</s>",
            "    <s n=\"2\" id=\"BTAA0001-00000002\">두 번째 훈몽자회 구문입니다.</s>",
            "  </text>",
            "</TEI>",
        ];

        // 1. Exact sent_id match
        let idx1 = find_target_line_in_source(&lines, "BTAA0001-00000002", "", "", 2);
        assert_eq!(idx1, Some(4), "Should match line 4 by sent_id");

        // 2. Sent_id suffix / n attribute match
        let idx2 = find_target_line_in_source(&lines, "2", "", "", 2);
        assert_eq!(idx2, Some(4), "Should match line 4 by digit n=\"2\"");

        // 3. Raw text match fallback
        let idx3 = find_target_line_in_source(&lines, "", "첫 번째 문장", "", 1);
        assert_eq!(idx3, Some(3), "Should match line 3 by raw text");
    }

    #[test]
    fn test_jamo_0_1_01_syntax() {
        let normalizer = Normalizer::new(std::path::Path::new("assets/data/pua_table.json"));

        // 1. [ㄱ/ㅏ/0] -> matches '가', but NOT '각'
        let q_ga_0 = normalizer.parse_bracket_query("[ㄱ/ㅏ/0]").expect("parse [ㄱ/ㅏ/0]");
        let u_ga = normalizer.tokenize_text_units("가");
        let u_gak = normalizer.tokenize_text_units("각");
        assert!(find_bracket_query_match(&q_ga_0, &u_ga).is_some(), "[ㄱ/ㅏ/0] must match '가'");
        assert!(find_bracket_query_match(&q_ga_0, &u_gak).is_none(), "[ㄱ/ㅏ/0] must NOT match '각'");

        // 2. [ㄱ/ㅏ/1] -> matches '각', '간', but NOT '가'
        let q_ga_1 = normalizer.parse_bracket_query("[ㄱ/ㅏ/1]").expect("parse [ㄱ/ㅏ/1]");
        let u_gan = normalizer.tokenize_text_units("간");
        assert!(find_bracket_query_match(&q_ga_1, &u_gak).is_some(), "[ㄱ/ㅏ/1] must match '각'");
        assert!(find_bracket_query_match(&q_ga_1, &u_gan).is_some(), "[ㄱ/ㅏ/1] must match '간'");
        assert!(find_bracket_query_match(&q_ga_1, &u_ga).is_none(), "[ㄱ/ㅏ/1] must NOT match '가'");

        // 3. [ㄱ/ㅏ/01] -> matches both '가' and '각'
        let q_ga_01 = normalizer.parse_bracket_query("[ㄱ/ㅏ/01]").expect("parse [ㄱ/ㅏ/01]");
        assert!(find_bracket_query_match(&q_ga_01, &u_ga).is_some(), "[ㄱ/ㅏ/01] must match '가'");
        assert!(find_bracket_query_match(&q_ga_01, &u_gak).is_some(), "[ㄱ/ㅏ/01] must match '각'");

        // 4. [1/ㅏ/0] -> matches '가', '나', '다', but NOT '각', '고'
        let q_any_a_0 = normalizer.parse_bracket_query("[1/ㅏ/0]").expect("parse [1/ㅏ/0]");
        let u_na = normalizer.tokenize_text_units("나");
        let u_da = normalizer.tokenize_text_units("다");
        let u_go = normalizer.tokenize_text_units("고");
        assert!(find_bracket_query_match(&q_any_a_0, &u_ga).is_some(), "[1/ㅏ/0] must match '가'");
        assert!(find_bracket_query_match(&q_any_a_0, &u_na).is_some(), "[1/ㅏ/0] must match '나'");
        assert!(find_bracket_query_match(&q_any_a_0, &u_da).is_some(), "[1/ㅏ/0] must match '다'");
        assert!(find_bracket_query_match(&q_any_a_0, &u_gak).is_none(), "[1/ㅏ/0] must NOT match '각'");
        assert!(find_bracket_query_match(&q_any_a_0, &u_go).is_none(), "[1/ㅏ/0] must NOT match '고'");

        // 5. [1/1/0] (all syllables with no final consonant) vs [1/1/1] (syllables with final consonant)
        let q_no_coda = normalizer.parse_bracket_query("[1/1/0]").expect("parse [1/1/0]");
        let q_has_coda = normalizer.parse_bracket_query("[1/1/1]").expect("parse [1/1/1]");
        let u_ha = normalizer.tokenize_text_units("하");
        let u_hang = normalizer.tokenize_text_units("항");
        assert!(find_bracket_query_match(&q_no_coda, &u_ha).is_some(), "[1/1/0] must match '하'");
        assert!(find_bracket_query_match(&q_no_coda, &u_hang).is_none(), "[1/1/0] must NOT match '항'");
        assert!(find_bracket_query_match(&q_has_coda, &u_hang).is_some(), "[1/1/1] must match '항'");
        assert!(find_bracket_query_match(&q_has_coda, &u_ha).is_none(), "[1/1/1] must NOT match '하'");
    }

    #[test]
    fn test_morph_jamo_search() {
        let normalizer = Normalizer::new(std::path::Path::new("assets/data/pua_table.json"));
        let morph_text = "가/VV + 았/EP + 다/EF";
        let units = normalizer.tokenize_text_units(morph_text);

        let q_tokens = normalizer.parse_bracket_query("[1/ㅏ/0]/VV").expect("parse [1/ㅏ/0]/VV");
        let m = find_bracket_query_match(&q_tokens, &units);
        assert!(m.is_some(), "[1/ㅏ/0]/VV must match '가/VV' in morph_flat");
        let (sc, ec) = m.unwrap();
        let matched_slice: String = morph_text.chars().skip(sc).take(ec - sc).collect();
        assert_eq!(matched_slice, "가/VV");
    }

    #[test]
    fn test_expand_bracket_syntax_to_regex() {
        let r1 = expand_bracket_syntax_to_regex("[ㄱ/ㅏ/0]/VV.*[1/ㅓ/0]");
        assert!(r1.contains("가/VV"), "Should expand [ㄱ/ㅏ/0] to '가'");
        let re = Regex::new(&r1).expect("Regex compile expanded string");
        assert!(re.is_match("가/VV + 서/EC"));
        assert!(!re.is_match("각/NNG + 서/EC"));
    }


    #[test]
    fn test_syllable_vs_jamo_matching() {
        let normalizer = Normalizer::new(std::path::Path::new("assets/data/pua_table.json"));
        let query = "하도";
        let query_nfd = normalizer.to_nfd_norm(query);
        let query_chars: Vec<char> = query_nfd.chars().collect();

        // Case 1: "하동" should NOT match in syllable mode (mode != "jamo")
        let text1 = "하동에 갔다";
        let (norm1, _) = normalizer.to_nfd_norm_with_map(text1);
        let candidates1 = find_all_char_subslices(&norm1, &query_chars);
        assert_eq!(candidates1.len(), 1);
        let (_s1, e1) = candidates1[0];
        // Next char is 'ㅇ' (jongseong), so in syllable mode it fails!
        assert!(e1 < norm1.len() && is_jongseong_jamo(norm1[e1]));

        // Case 2: "하도" should MATCH in syllable mode
        let text2 = "하도 말이 많아서";
        let (norm2, _) = normalizer.to_nfd_norm_with_map(text2);
        let candidates2 = find_all_char_subslices(&norm2, &query_chars);
        assert_eq!(candidates2.len(), 1);
        let (_s2, e2) = candidates2[0];
        // Next char is space (not jongseong), so it passes!
        assert!(e2 == norm2.len() || !is_jongseong_jamo(norm2[e2]));
    }

    #[test]
    fn test_space_literal_character_match_and_boundary() {
        let normalizer = Normalizer::new(std::path::Path::new("assets/data/pua_table.json"));
        let query = "하도 말이";
        let query_nfd = normalizer.to_nfd_norm(query);
        let query_chars: Vec<char> = query_nfd.chars().collect();

        let text = "그는 하도 말이 많아서 탈이다";
        let (norm, char_map) = normalizer.to_nfd_norm_with_map(text);
        let candidates = find_all_char_subslices(&norm, &query_chars);
        assert_eq!(candidates.len(), 1);
        let (s, e) = candidates[0];
        let orig_start = char_map.get(s).copied().unwrap_or(0);
        let orig_end = char_map.get(e.saturating_sub(1)).map(|&idx| idx + 1).unwrap_or(orig_start + 1);
        let matched: String = text.chars().skip(orig_start).take(orig_end - orig_start).collect();
        assert_eq!(matched, "하도 말이");

        // Word boundary test (' 하도', '하도 ')
        let text_chars: Vec<char> = "하도 천하도 하도다.".chars().collect();
        // index 0..2 ("하도") -> word start AND word end
        assert!(check_word_boundary(&text_chars, 0, 2, true, true));
        // index 4..6 ("하도" inside "천하도") -> NOT word start, IS word end
        assert!(!check_word_boundary(&text_chars, 4, 6, true, false));
        assert!(check_word_boundary(&text_chars, 4, 6, false, true));
        // index 7..9 ("하도" inside "하도다.") -> IS word start, NOT word end
        assert!(check_word_boundary(&text_chars, 7, 9, true, false));
        assert!(!check_word_boundary(&text_chars, 7, 9, false, true));
    }

    #[test]
    fn test_native_builder_schema() {
        let conn = Connection::open_in_memory().expect("open in-memory sqlite");
        builder::init_db_schema(&conn).expect("init_db_schema");
        let tbl_cnt: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('documents', 'source_files', 'segments', 'segment_edits')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tbl_cnt, 4);

        let sample_file = std::path::Path::new("../sample/2BA92A01.txt");
        if sample_file.exists() {
            let tmp_db = std::path::Path::new("../db/test_unit_build.corpdb");
            let _ = std::fs::remove_file(tmp_db);
            let normalizer = Normalizer::new(std::path::Path::new("../assets/data/pua_table.json"));
            let mgr = Arc::new(Mutex::new(DbBuildManager::new()));
            {
                let mut m = mgr.lock().unwrap();
                m.progress.is_running = true;
                m.progress.status = "RUNNING".to_string();
            }
            builder::build_folder_into_db_native(sample_file, tmp_db, &normalizer, &mgr)
                .expect("build_folder_into_db_native");
            let m = mgr.lock().unwrap();
            assert_eq!(m.progress.status, "COMPLETED");
            assert!(m.progress.sentences_imported > 0, "Should import >0 sentences from 2BA92A01.txt");
            assert!(m.progress.eojeols_imported > 0, "Should import >0 eojeols from 2BA92A01.txt");
            drop(m);
            let _ = std::fs::remove_file(tmp_db);
        }
    }

    #[test]
    fn test_search_unit_eojeol_sentence_text_and_eojeol_start_matching() {
        let state = AppState {
            db_path: PathBuf::from(":memory:"),
            web_dir: PathBuf::from("../web"),
            root_dir: PathBuf::from(".."),
            hanbatang_path: PathBuf::from(""),
            kopub_ttf_path: PathBuf::from(""),
            kopub_otf_path: PathBuf::from(""),
            normalizer: Normalizer::new(std::path::Path::new("assets/data/pua_table.json")),
            memory_profile: MemoryProfile::detect(),
            latest_search_seq: AtomicU64::new(0),
            cancel_search_seq: AtomicU64::new(0),
            db_builder: Arc::new(Mutex::new(DbBuildManager::new())),
            port: 8787,
        };

        let query = "하[ㄴ/1/01].{0,10}/NNG";
        assert!(has_regex_syntax(query));
        let expanded = expand_bracket_syntax_to_regex(query);
        let re_unanchored = Regex::new(&expanded).unwrap();
        let re_anchored = Regex::new(&format!("^(?:{})", expanded)).unwrap();
        let norm_q_nfd = state.normalizer.to_nfd_norm(query);
        let norm_q_chars: Vec<char> = norm_q_nfd.chars().collect();
        let norm_m_q = state.normalizer.normalize_morph_str(query);
        let b_tokens = state.normalizer.parse_bracket_query(query);

        let make_qctx = |unit: &'static str| SearchQueryContext {
            core_query: query,
            search_unit: unit,
            req_eojeol_start: query_requires_eojeol_start(query),
            has_leading_space: false,
            has_trailing_space: false,
            use_surface: false,
            use_morph: true,
            use_sense: false,
            use_syntax: false,
            use_text: false,
            use_jamo: true,
            use_regex: true,
            norm_query_chars: &norm_q_chars,
            norm_morph_query: &norm_m_q,
            bracket_tokens: b_tokens.as_deref(),
            compiled_regex: Some(&re_unanchored),
            anchored_regex: Some(&re_anchored),
        };

        // 1. '자제하는[자제/NNG + 하/XSV + 는/ETM]' and '건설하는[건설/NNG + 하/XSV + 는/ETM]'
        // MUST NOT match in eojeol, sentence, OR text mode!
        for (disp_false, morph_false) in [
            (
                "자제하는 모습을 보였다.",
                "자제하는[자제/NNG + 하/XSV + 는/ETM] | 모습을[모습/NNG + 을/JKO] | 보였다.[보이/VV + 었/EP + 다/EF + ./SF]",
            ),
            (
                "신규 공장을 건설하는 업체들에 제출토록 했다.",
                "신규[신규/NNG] | 공장을[공장/NNG + 을/JKO] | 건설하는[건설/NNG + 하/XSV + 는/ETM] | 업체들에[업체/NNG + 들/XSN + 에/JKB] | 제출토록[제출/NNG + 하/XSV + 도록/EC] | 했다.[하/VX + 았/EP + 다/EF + ./SF]",
            ),
        ] {
            for mode_u in ["eojeol", "sentence", "text"] {
                assert!(
                    evaluate_segment_match(
                        &state,
                        &make_qctx(mode_u),
                        disp_false,
                        disp_false,
                        disp_false,
                        morph_false,
                        "",
                        ""
                    )
                    .is_none(),
                    "'{}' must NOT match in {} unit mode",
                    disp_false,
                    mode_u
                );
            }
        }

        // 2. '하늘이[하늘/NNG+이/JKS]' MUST match in eojeol mode (eojeol starts with '하')
        let disp_true_ej = "푸른 하늘이 열렸다.";
        let morph_true_ej = "푸른[푸르/VA+ㄴ/ETM] | 하늘이[하늘/NNG+이/JKS] | 열렸다.[열리/VV+었/EP+다/EF+./SF]";
        let m_ej = evaluate_segment_match(
            &state,
            &make_qctx("eojeol"),
            disp_true_ej,
            disp_true_ej,
            disp_true_ej,
            morph_true_ej,
            "",
            "",
        );
        assert!(m_ej.is_some(), "'하늘이[하늘/NNG+이/JKS]' should match in eojeol mode");
        let (sc, ec, _) = m_ej.unwrap();
        let matched_word: String = disp_true_ej.chars().skip(sc).take(ec - sc).collect();
        assert_eq!(matched_word, "하늘이");

        // 3. '하나[하나/NR] | 일을[일/NNG + 을/JKO]'
        // - Does NOT match in eojeol mode (since /NNG is in the next eojeol)
        // - DOES match in sentence mode (eojeol starts with '하나' and crosses eojeol boundary within sentence)
        let disp_cross_ej = "그가 하나 일을 보았다.";
        let morph_cross_ej = "그가[그/NP + 가/JKS] | 하나[하나/NR] | 일을[일/NNG + 을/JKO] | 보았다.[보/VV + 았/EP + 다/EF + ./SF]";
        assert!(
            evaluate_segment_match(
                &state,
                &make_qctx("eojeol"),
                disp_cross_ej,
                disp_cross_ej,
                disp_cross_ej,
                morph_cross_ej,
                "",
                ""
            )
            .is_none(),
            "Cross-eojeol '하나 일을' must NOT match in strict eojeol mode"
        );
        let m_sent = evaluate_segment_match(
            &state,
            &make_qctx("sentence"),
            disp_cross_ej,
            disp_cross_ej,
            disp_cross_ej,
            morph_cross_ej,
            "",
            "",
        );
        assert!(
            m_sent.is_some(),
            "Cross-eojeol '하나 일을' MUST match in sentence mode"
        );
        let (sc_s, ec_s, _) = m_sent.unwrap();
        let matched_sent_span: String = disp_cross_ej.chars().skip(sc_s).take(ec_s - sc_s).collect();
        assert_eq!(matched_sent_span, "하나 일을");

        // 4. In text mode (unbounded), unanchored match starting mid-eojeol ('대한하나님의[대한/NNP + 하나님/NNG + 의/JKG]') is allowed
        let disp_mid = "대한하나님의 은혜를 기렸다.";
        let morph_mid = "대한하나님의[대한/NNP + 하나님/NNG + 의/JKG] | 은혜를[은혜/NNG + 를/JKO] | 기렸다.[기리/VV + 었/EP + 다/EF + ./SF]";
        assert!(
            evaluate_segment_match(
                &state,
                &make_qctx("eojeol"),
                disp_mid,
                disp_mid,
                disp_mid,
                morph_mid,
                "",
                ""
            )
            .is_none(),
            "Mid-eojeol '대한하나님의' must NOT match in eojeol mode"
        );
        let m_text = evaluate_segment_match(
            &state,
            &make_qctx("text"),
            disp_mid,
            disp_mid,
            disp_mid,
            morph_mid,
            "",
            "",
        );
        assert!(
            m_text.is_some(),
            "In text mode (unbounded), mid-eojeol match is allowed"
        );
    }

    #[test]
    fn test_korean_learners_corpus_xml_parsing() {
        let tmp_dir = std::env::temp_dir().join("sconc_test_klc_xml");
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let xml_path = tmp_dir.join("sample_klc.xml");
        let xml_content = r#"<?xml version="1.0" encoding="utf-8"?>
<Korean_Learners_Corpus>
  <Header>
    <Title>한국어 학습자 말뭉치</Title>
    <AssignmentTheme>집 구하기</AssignmentTheme>
    <Nationality>중국</Nationality>
    <DataGrade>1급</DataGrade>
  </Header>
  <BODY>
    <P to="20" from="0">
      <SENTENCE to="20" from="0">
        <s>원문_대체_무시</s>
        <MorphemeAnnotations>
          <word>
            <w>그리고</w>
            <morph from="0" to="3" analyzedType="Normal" pos="MAJ" subsequence="1">그리고</morph>
          </word>
          <word>
            <w>부엌이</w>
            <morph from="4" to="7" analyzedType="Normal" pos="NNG" subsequence="1">부엌</morph>
            <morph from="4" to="7" analyzedType="Normal" pos="JKS" subsequence="2">이</morph>
          </word>
          <word>
            <w>있으면</w>
            <morph from="8" to="11" analyzedType="Normal" pos="VV" subsequence="1">있</morph>
            <morph from="8" to="11" analyzedType="Normal" pos="EC" subsequence="2">으면</morph>
          </word>
          <word>
            <w>좋겠다.</w>
            <morph from="12" to="16" analyzedType="Ambiguity" pos="VA" subsequence="1">좋</morph>
            <morph from="12" to="16" analyzedType="Ambiguity" pos="EP" subsequence="2">겠</morph>
            <morph from="12" to="16" analyzedType="Ambiguity" pos="EF" subsequence="3">다</morph>
            <morph from="12" to="16" analyzedType="Normal" pos="SF" subsequence="4">.</morph>
          </word>
        </MorphemeAnnotations>
      </SENTENCE>
    </P>
  </BODY>
</Korean_Learners_Corpus>"#;
        std::fs::write(&xml_path, xml_content).unwrap();

        let norm = Normalizer::new(std::path::Path::new("nonexistent.json"));
        let (theme, nat, grade, segs, _) =
            builder::parse_learner_xml(&xml_path, "KLC", &norm).unwrap();
        assert_eq!(theme, "집 구하기");
        assert_eq!(nat, "중국");
        assert_eq!(grade, "1급");
        assert_eq!(segs.len(), 1, "Only <BODY> <P> sentence should be parsed");

        let seg = &segs[0];
        assert_eq!(seg.text_raw, "그리고 부엌이 있으면 좋겠다.");
        assert!(
            !seg.text_raw.contains("한국어 학습자 말뭉치") && !seg.text_raw.contains("집 구하기"),
            "Header metadata must not leak into corpus text"
        );
        assert!(
            seg.morph_flat.contains("좋겠다.[좋/VA + 겠/EP + 다/EF + ./SF]"),
            "morph_flat must be in Sejong format, got: {}",
            seg.morph_flat
        );

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
