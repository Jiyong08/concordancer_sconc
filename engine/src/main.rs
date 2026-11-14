#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod builder;
mod editor;
mod handlers;
mod jamo_table;
mod models;
mod normalizer;
mod preflight;
mod search;

pub use models::{DbBuildManager, DbBuildProgress};
pub use normalizer::Normalizer;

use editor::{handle_open_editor, handle_source_jump};
use handlers::{
    handle_build_db, handle_build_db_cancel, handle_build_db_status, handle_build_db_stop,
    handle_document_delete, handle_export_save_txt, handle_overview, handle_pick_folder,
    handle_segment_detail, handle_segment_edit,
};
use models::{binary_response, parse_query_params, ApiResponse, AppState, MemoryProfile};
use normalizer::find_bracket_query_match;
use search::{handle_search, handle_search_cancel, handle_search_stream};
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewBuilder;

const EMBEDDED_INDEX_HTML: &[u8] = include_bytes!("../../web/index.html");
const EMBEDDED_STYLES_CSS: &[u8] = include_bytes!("../../web/styles.css");
const EMBEDDED_HANGUL_AUTOMATA_JS: &[u8] = include_bytes!("../../web/hangul_automata.js");
const EMBEDDED_APP_JS: &[u8] = include_bytes!("../../web/app.js");
const EMBEDDED_ICON_RGBA: &[u8] = include_bytes!("../../assets/icon_64.rgba");


use rusqlite::Connection;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
fn serve_static_file(path: &Path, content_type: &str) -> ApiResponse {
    serve_static_or_embedded(path, None, content_type)
}

fn serve_static_or_embedded(
    path: &Path,
    embedded: Option<&'static [u8]>,
    content_type: &str,
) -> ApiResponse {
    let bytes_opt = fs::read(path)
        .ok()
        .or_else(|| embedded.map(|b| b.to_vec()));
    match bytes_opt {
        Some(bytes) => http::Response::builder()
            .status(200)
            .header("Content-Type", content_type)
            .header("Cache-Control", "no-cache, no-store, must-revalidate")
            .header("Pragma", "no-cache")
            .body(bytes)
            .unwrap_or_else(|_| http::Response::new(Vec::new())),
        None => binary_response(404, "text/plain; charset=utf-8", b"404 Not Found".to_vec()),
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
            // macOS .app 번들(S-conc.app/Contents/MacOS/sconc)에서 실행되는 경우,
            // 읽기 전용 번들 내부가 아닌 쓰기 가능한 ~/Documents/S-conc 를 작업 폴더로 사용
            let exe_str = exe_dir.to_string_lossy();
            if exe_str.contains(".app/Contents/MacOS") || exe_str.contains(".app\\Contents\\MacOS") {
                if let Some(home) = env::var_os("HOME") {
                    let doc_dir = PathBuf::from(home).join("Documents").join("S-conc");
                    let _ = fs::create_dir_all(&doc_dir);
                    return doc_dir;
                }
            }
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
        last_pick_folder: Arc::new(Mutex::new(None)),
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

    let _ = auto_open;
    // GUI 실행 전 필수 런타임/라이브러리 자동 탐지 및 사용자 승인 설치
    preflight::ensure_runtime_dependencies();

    let event_loop = EventLoop::new();
    let window_icon = tao::window::Icon::from_rgba(EMBEDDED_ICON_RGBA.to_vec(), 64, 64).ok();
    let window = WindowBuilder::new()
        .with_title("한국어 말뭉치 통합 검색기 S-conc v0.2")
        .with_inner_size(LogicalSize::new(1480.0, 920.0))
        .with_window_icon(window_icon)
        .build(&event_loop)
        .expect("Failed to create native window");

    let state_proto = Arc::clone(&state);
    #[cfg(any(target_os = "windows", target_os = "android"))]
    let app_url = "http://sconc.localhost/";
    #[cfg(not(any(target_os = "windows", target_os = "android")))]
    let app_url = "sconc://localhost/";

    let _webview = WebViewBuilder::new()
        .with_asynchronous_custom_protocol("sconc".into(), move |_webview_id, request, responder| {
            let state_req = Arc::clone(&state_proto);
            std::thread::spawn(move || {
                let method = request.method().clone();
                let uri = request.uri();
                let path_part = uri.path().to_string();
                let query_url = uri
                    .query()
                    .map(|q| format!("?{}", q))
                    .unwrap_or_default();
                let body = request.body().to_vec();
                let resp = dispatch_request(&state_req, &method, &path_part, &query_url, &body);
                responder.respond(resp);
            });
        })
        .with_url(app_url)
        .build(&window)
        .expect("Failed to create WebView");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}

fn dispatch_request(
    state: &Arc<AppState>,
    method: &http::Method,
    path_part: &str,
    query_url: &str,
    body: &[u8],
) -> ApiResponse {
    let params = parse_query_params(query_url);
    match (method, path_part) {
        (&http::Method::GET, "/api/overview") => handle_overview(state),
        (&http::Method::GET, "/api/search") => handle_search(state, &params),
        (&http::Method::GET, "/api/search/stream") | (&http::Method::GET, "/api/search/stream/") => {
            handle_search_stream(state, &params)
        }
        (&http::Method::POST, "/api/search/cancel") => handle_search_cancel(state),
        (&http::Method::GET, "/api/segment") => handle_segment_detail(state, &params),
        (&http::Method::GET, "/api/source-jump") => handle_source_jump(state, &params),
        (&http::Method::GET, "/api/open-editor") | (&http::Method::POST, "/api/open-editor") => {
            handle_open_editor(&params)
        }
        (&http::Method::GET, "/api/system/pick-folder")
        | (&http::Method::POST, "/api/system/pick-folder") => handle_pick_folder(state),
        (&http::Method::GET, "/api/document/delete")
        | (&http::Method::POST, "/api/document/delete") => handle_document_delete(state, &params),
        (&http::Method::POST, "/api/segment/edit") => handle_segment_edit(state, body),
        (&http::Method::POST, "/api/build-db") | (&http::Method::POST, "/api/build_db") => {
            handle_build_db(state, body)
        }
        (&http::Method::GET, "/api/build-db/status")
        | (&http::Method::GET, "/api/build_db/status") => handle_build_db_status(state),
        (&http::Method::POST, "/api/build-db/stop")
        | (&http::Method::POST, "/api/build_db/stop") => handle_build_db_stop(state),
        (&http::Method::POST, "/api/build-db/cancel")
        | (&http::Method::POST, "/api/build_db/cancel") => handle_build_db_cancel(state),
        (&http::Method::POST, "/api/export/save-txt") => handle_export_save_txt(state, body),
        (&http::Method::GET, "/api/export/mapping.tsv") => serve_static_file(
            &state.root_dir.join("db").join("corpus_db_mapping.tsv"),
            "text/tab-separated-values; charset=utf-8",
        ),
        (&http::Method::GET, "/api/export/mapping.json") => serve_static_file(
            &state.root_dir.join("db").join("corpus_db_mapping.json"),
            "application/json; charset=utf-8",
        ),
        (&http::Method::GET, "/fonts/HANBatang.ttf") => {
            serve_static_file(&state.hanbatang_path, "font/ttf")
        }
        (&http::Method::GET, "/fonts/KoPubWorldDotumMedium.ttf") => {
            serve_static_file(&state.kopub_ttf_path, "font/ttf")
        }
        (&http::Method::GET, "/fonts/KoPubWorldDotumProMedium.otf") => {
            serve_static_file(&state.kopub_otf_path, "font/otf")
        }
        (&http::Method::GET, "/") | (&http::Method::GET, "/index.html") => serve_static_or_embedded(
            &state.web_dir.join("index.html"),
            Some(EMBEDDED_INDEX_HTML),
            "text/html; charset=utf-8",
        ),
        (&http::Method::GET, "/styles.css") => serve_static_or_embedded(
            &state.web_dir.join("styles.css"),
            Some(EMBEDDED_STYLES_CSS),
            "text/css; charset=utf-8",
        ),
        (&http::Method::GET, "/hangul_automata.js") => serve_static_or_embedded(
            &state.web_dir.join("hangul_automata.js"),
            Some(EMBEDDED_HANGUL_AUTOMATA_JS),
            "application/javascript; charset=utf-8",
        ),
        (&http::Method::GET, "/app.js") => serve_static_or_embedded(
            &state.web_dir.join("app.js"),
            Some(EMBEDDED_APP_JS),
            "application/javascript; charset=utf-8",
        ),
        _ => binary_response(404, "text/plain; charset=utf-8", b"404 Not Found".to_vec()),
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
            last_pick_folder: Arc::new(Mutex::new(None)),
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
            use_error: false,
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

        let (speaker_info, other_meta) = builder::extract_corpus_metadata_from_file(&xml_path);
        assert!(
            speaker_info.contains("국적: 중국") && speaker_info.contains("등급: 1급"),
            "speaker_info should contain nationality/grade, got: {}",
            speaker_info
        );
        assert!(
            other_meta.contains("주제: 집 구하기") && other_meta.contains("제목: 한국어 학습자 말뭉치"),
            "other_meta should contain theme/title, got: {}",
            other_meta
        );

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_eojeol_unit_kwic_keyword_full_eojeol() {
        use crate::search::extract_kwic_by_word_count;
        let text = "그리고 부엌이 있으면 좋겠다.";
        // '부엌이' 어절 내부의 한 음절 '이'(index 6..7)가 매칭되었을 때
        let (left, node, right, _, _, _, _) = extract_kwic_by_word_count(text, 6, 7, 5);
        assert_eq!(node, "부엌이", "핵심어(node)는 '이' 한 음절이 아니라 전체 어절 '부엌이'여야 함");
        assert_eq!(left, "그리고 ");
        assert_eq!(right, " 있으면 좋겠다.");
        assert_eq!(format!("{}{}{}", left, node, right), text);

        // 첫 어절의 한 음절 '그'(index 0..1)가 매칭되었을 때
        let (left2, node2, right2, _, _, _, _) = extract_kwic_by_word_count(text, 0, 1, 5);
        assert_eq!(node2, "그리고", "핵심어는 첫 어절 전체 '그리고'여야 함");
        assert_eq!(left2, "");
        assert_eq!(right2, " 부엌이 있으면 좋겠다.");
        assert_eq!(format!("{}{}{}", left2, node2, right2), text);

        // 마지막 어절의 한 음절 '좋'(index 12..13)이 매칭되었을 때
        let (left3, node3, right3, _, _, _, _) = extract_kwic_by_word_count(text, 12, 13, 5);
        assert_eq!(node3, "좋겠다.", "핵심어는 마지막 어절 전체 '좋겠다.'여야 함");
        assert_eq!(left3, "그리고 부엌이 있으면 ");
        assert_eq!(right3, "");
        assert_eq!(format!("{}{}{}", left3, node3, right3), text);
    }

    #[test]
    fn test_unknown_format_builds_raw_only_db() {
        use crate::builder::build_paths_into_db_native;
        use crate::normalizer::Normalizer;
        use crate::DbBuildManager;
        use rusqlite::Connection;
        use std::sync::{Arc, Mutex};

        let tmp_dir = std::env::temp_dir().join(format!(
            "sconc_raw_only_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let txt_path = tmp_dir.join("BT_plain_note.txt");
        std::fs::write(&txt_path, "이것은 일반 평문 테스트 문장입니다.\n두 번째 문장도 있습니다.\n").unwrap();

        let xml_path = tmp_dir.join("custom_corpus.xml");
        std::fs::write(
            &xml_path,
            "<root><entry>샘플 형식과 다른 XML 본문입니다.</entry></root>",
        )
        .unwrap();

        let db_path = tmp_dir.join("test_raw_only.corpdb");
        let norm = Normalizer::new(&tmp_dir.join("nonexistent_pua.json"));
        let mgr = Arc::new(Mutex::new(DbBuildManager::new()));
        mgr.lock().unwrap().progress.is_running = true;

        let summary = build_paths_into_db_native(&[txt_path, xml_path], &db_path, &norm, &mgr)
            .expect("raw-only build should succeed");

        assert_eq!(summary["documents_created"].as_u64(), Some(2));
        assert_eq!(summary["raw_only_count"].as_u64(), Some(2));
        let raw_files = summary["raw_only_files"].as_array().unwrap();
        assert_eq!(raw_files.len(), 2);

        let conn = Connection::open(&db_path).unwrap();
        let non_raw_cnt: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM segments WHERE TRIM(morph_flat) != '' OR TRIM(sense_flat) != '' OR TRIM(syntax_tree) != ''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(non_raw_cnt, 0, "미지원 형식 파일에서 형태·의미·구문 분석 데이터가 생성되면 안 됨");

        let layer_cnt: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM documents WHERE annotation_layers != '평문'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(layer_cnt, 0, "모든 문서는 '평문' 층위로만 등록되어야 함");

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_morph_only_excluded_from_plain_search() {
        use crate::builder::build_paths_into_db_native;
        use crate::normalizer::Normalizer;
        use crate::search::handle_search;
        use crate::DbBuildManager;
        use rusqlite::Connection;
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let tmp_dir = std::env::temp_dir().join(format!(
            "sconc_morph_only_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let xml_path = tmp_dir.join("learner_morph_only.xml");
        std::fs::write(
            &xml_path,
            r#"<?xml version="1.0" encoding="utf-8"?>
<LearnerCorpus>
  <SENTENCE>
    <s>사과가 맛있다.</s>
    <word><w>사과가</w><morph pos="NNG">사과</morph><morph pos="JKS">가</morph></word>
    <word><w>맛있다.</w><morph pos="VA">맛있</morph><morph pos="EF">다</morph></word>
  </SENTENCE>
</LearnerCorpus>"#,
        )
        .unwrap();

        let db_path = tmp_dir.join("test_morph_only.corpdb");
        let norm = Normalizer::new(&tmp_dir.join("nonexistent_pua.json"));
        let mgr = Arc::new(Mutex::new(DbBuildManager::new()));
        mgr.lock().unwrap().progress.is_running = true;

        build_paths_into_db_native(&[xml_path], &db_path, &norm, &mgr).unwrap();

        let conn = Connection::open(&db_path).unwrap();
        let layers: String = conn
            .query_row("SELECT annotation_layers FROM documents LIMIT 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(layers, "형태", "원본 평문 없는 형태분석 파일은 '형태' 표지만 가져야 함");

        let state = AppState {
            db_path: db_path.clone(),
            web_dir: PathBuf::from("../web"),
            root_dir: tmp_dir.clone(),
            hanbatang_path: PathBuf::from(""),
            kopub_ttf_path: PathBuf::from(""),
            kopub_otf_path: PathBuf::from(""),
            normalizer: norm,
            memory_profile: MemoryProfile::detect(),
            latest_search_seq: AtomicU64::new(0),
            cancel_search_seq: AtomicU64::new(0),
            db_builder: mgr,
            last_pick_folder: Arc::new(Mutex::new(None)),
            port: 8787,
        };

        // 1. 평문 검색 시 제외되어야 함
        let mut p_plain = HashMap::new();
        p_plain.insert("q".to_string(), "사과".to_string());
        p_plain.insert("targets".to_string(), "surface".to_string());
        let res_plain = handle_search(&state, &p_plain);
        let json_plain: serde_json::Value = serde_json::from_slice(res_plain.body()).unwrap();
        assert_eq!(
            json_plain["hits"].as_array().map(|a| a.len()),
            Some(0),
            "형태 전용 DB는 평문 검색 대상에서 제외되어야 함 (got: {})",
            json_plain
        );

        // 2. 형태 검색 시 검색되어야 함
        let mut p_morph = HashMap::new();
        p_morph.insert("q".to_string(), "사과/NNG".to_string());
        p_morph.insert("targets".to_string(), "morph".to_string());
        let res_morph = handle_search(&state, &p_morph);
        let json_morph: serde_json::Value = serde_json::from_slice(res_morph.body()).unwrap();
        assert_eq!(
            json_morph["hits"].as_array().map(|a| a.len()),
            Some(1),
            "형태 전용 DB는 형태 검색 시 검색되어야 함 (got: {})",
            json_morph
        );

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_error_annotated_corpus_build_and_search() {
        use crate::builder::build_paths_into_db_native;
        use crate::normalizer::Normalizer;
        use crate::search::handle_search;
        use crate::DbBuildManager;
        use rusqlite::Connection;
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let tmp_dir = std::env::temp_dir().join(format!(
            "sconc_error_corpus_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let xml_path = tmp_dir.join("sample_error_643.xml");
        std::fs::write(
            &xml_path,
            r#"<?xml version="1.0" encoding="utf-8"?>
<Korean_Learners_Corpus>
  <Header>
    <AssignmentTheme>주장</AssignmentTheme>
    <DataGrade>4급</DataGrade>
    <Nationality>중국</Nationality>
  </Header>
  <BODY>
    <P>
      <SENTENCE from="0" to="26">
        <s>최저임금제 설정에 대한 내 주장은 찬성이다.</s>
        <MorphemeAnnotations>
          <word><w>최저임금제</w><morph pos="NNG">최저</morph><morph pos="NNG">임금</morph><morph pos="XSN">제</morph></word>
          <word><w>설정에</w><morph pos="NNG">설정</morph><morph pos="JKB">에</morph></word>
          <word><w>대한</w><morph pos="VV">대하</morph><morph pos="ETM">ㄴ</morph></word>
          <word><w>내</w><morph pos="NP">나</morph><morph pos="JKG">의</morph></word>
          <word><w>주장은</w><morph pos="NNG">주장</morph><morph pos="JX">은</morph></word>
          <word><w>찬성이다.</w><morph pos="NNG">찬성</morph><morph pos="VCP">이</morph><morph pos="EF">다</morph><morph pos="SF">.</morph></word>
        </MorphemeAnnotations>
        <LearnerErrorAnnotations>
          <word>
            <w>주장은</w>
            <morph from="15" to="18" subsequence="1" wordStart="Start">
              <Proofread pos="NNG">입장</Proofread>
              <ErrorArea type="CNNG" />
              <ErrorPattern type="REP" />
            </morph>
            <morph from="15" to="18" subsequence="2" wordStart="None">
              <Preserved>은</Preserved>
            </morph>
          </word>
        </LearnerErrorAnnotations>
      </SENTENCE>
    </P>
  </BODY>
</Korean_Learners_Corpus>"#,
        )
        .unwrap();

        let db_path = tmp_dir.join("test_error.corpdb");
        let norm = Normalizer::new(&tmp_dir.join("nonexistent_pua.json"));
        let mgr = Arc::new(Mutex::new(DbBuildManager::new()));
        mgr.lock().unwrap().progress.is_running = true;

        build_paths_into_db_native(&[xml_path], &db_path, &norm, &mgr).unwrap();

        let conn = Connection::open(&db_path).unwrap();
        let (layers, text_u, error_flat, corrected): (String, String, String, String) = conn
            .query_row(
                "SELECT d.annotation_layers, s.text_unicode, s.error_flat, s.corrected_text
                 FROM segments s JOIN documents d ON s.doc_id = d.doc_id LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(layers, "형태 + 오류");
        assert_eq!(text_u, "최저임금제 설정에 대한 내 주장은 찬성이다.");
        assert!(error_flat.contains("주장은->입장/NNG+은[CNNG|REP]"), "got: {}", error_flat);
        assert_eq!(corrected, "최저임금제 설정에 대한 내 입장은 찬성이다.");

        let state = AppState {
            db_path: db_path.clone(),
            web_dir: PathBuf::from("../web"),
            root_dir: tmp_dir.clone(),
            hanbatang_path: PathBuf::from(""),
            kopub_ttf_path: PathBuf::from(""),
            kopub_otf_path: PathBuf::from(""),
            normalizer: norm,
            memory_profile: MemoryProfile::detect(),
            latest_search_seq: AtomicU64::new(0),
            cancel_search_seq: AtomicU64::new(0),
            db_builder: mgr,
            last_pick_folder: Arc::new(Mutex::new(None)),
            port: 8787,
        };

        // 1. 교정 쌍('->') 검색
        let mut p_pair = HashMap::new();
        p_pair.insert("q".to_string(), "주장->입장".to_string());
        p_pair.insert("targets".to_string(), "error".to_string());
        let res_pair = handle_search(&state, &p_pair);
        let json_pair: serde_json::Value = serde_json::from_slice(res_pair.body()).unwrap();
        assert_eq!(json_pair["hits"].as_array().map(|a| a.len()), Some(1));
        assert_eq!(json_pair["hits"][0]["node"].as_str(), Some("주장은"));

        // 2. 오류 코드 필터 + 빈 검색어 열람
        let mut p_filter = HashMap::new();
        p_filter.insert("q".to_string(), "".to_string());
        p_filter.insert("targets".to_string(), "error".to_string());
        p_filter.insert("error_area".to_string(), "CNNG".to_string());
        p_filter.insert("error_pattern".to_string(), "REP".to_string());
        let res_filter = handle_search(&state, &p_filter);
        let json_filter: serde_json::Value = serde_json::from_slice(res_filter.body()).unwrap();
        assert_eq!(json_filter["hits"].as_array().map(|a| a.len()), Some(1));

        // 3. 평문 검색에서는 제외되어야 함
        let mut p_plain = HashMap::new();
        p_plain.insert("q".to_string(), "찬성".to_string());
        p_plain.insert("targets".to_string(), "surface".to_string());
        let res_plain = handle_search(&state, &p_plain);
        let json_plain: serde_json::Value = serde_json::from_slice(res_plain.body()).unwrap();
        assert_eq!(json_plain["hits"].as_array().map(|a| a.len()), Some(0));

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
