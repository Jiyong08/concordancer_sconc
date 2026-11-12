use crate::models::{json_response, ApiResponse, AppState};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub(crate) struct SourceJumpLine {
    pub(crate) line_no: usize,
    pub(crate) content: String,
    pub(crate) is_target: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct SourceJumpResponse {
    pub(crate) segment_id: i64,
    pub(crate) doc_id: String,
    pub(crate) doc_title: String,
    pub(crate) file_name: String,
    pub(crate) file_path: String,
    pub(crate) line_no: Option<usize>,
    pub(crate) sent_id: String,
    pub(crate) page_ref: String,
    pub(crate) text_raw: String,
    pub(crate) text_unicode: String,
    pub(crate) editor_status: Option<String>,
    pub(crate) lines: Vec<SourceJumpLine>,
}

// [요구사항 2] 원본 파일에서 해당 세그먼트의 정확한 라인 번호를 탐색하는 정밀 함수
pub(crate) fn find_target_line_in_source(
    all_lines: &[&str],
    sent_id: &str,
    text_raw: &str,
    text_unicode: &str,
    _seq_no: i64,
) -> Option<usize> {
    // 1. sent_id 매칭
    if !sent_id.is_empty() {
        for (idx, line) in all_lines.iter().enumerate() {
            if line.contains(sent_id) {
                return Some(idx);
            }
        }
        // 번호 패턴 매칭 (예: BTAA0001-00000003 또는 n="1")
        if let Some(num_str) = sent_id.split('-').last() {
            let digits: String = num_str.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(num) = digits.parse::<usize>() {
                if num > 0 {
                    let pat1 = format!("{:08}", num);
                    let pat2 = format!("n=\"{}\"", num);
                    let pat3 = format!("id=\"{}\"", num);
                    for (idx, line) in all_lines.iter().enumerate() {
                        if line.contains(&pat1) || line.contains(&pat2) || line.contains(&pat3) {
                            return Some(idx);
                        }
                    }
                }
            }
        }
    }

    let clean_raw = text_raw.trim();
    // 2. clean_raw 전체가 line에 포함되어 있는지 검사
    if !clean_raw.is_empty() && clean_raw.len() >= 3 {
        for (idx, line) in all_lines.iter().enumerate() {
            if line.contains(clean_raw) {
                return Some(idx);
            }
        }
    }

    // 3. 사전류(훈몽자회 등) 한자 <g>{한자}</g> 매칭
    let hanja_chars: Vec<char> = clean_raw.chars().filter(|&c| ('\u{4e00}'..='\u{9fff}').contains(&c)).collect();
    if !hanja_chars.is_empty() {
        let h = hanja_chars[0];
        let h_tag = format!("<g>{}</g>", h);
        for (idx, line) in all_lines.iter().enumerate() {
            if line.contains(&h_tag) || line.contains(h) {
                return Some(idx);
            }
        }
    }

    // 4. 토큰 기반 다중 어절 매칭 (Scoring)
    let probe = if !clean_raw.is_empty() { clean_raw } else { text_unicode.trim() };
    let token_re = regex::Regex::new(r"[\uac00-\ud7a3\u1100-\u11ff\ua960-\ua97f\ud7b0-\ud7ff\u4e00-\u9fffa-zA-Z0-9]+").ok();
    if let Some(re) = token_re {
        let tokens: Vec<&str> = re.find_iter(probe)
            .map(|m| m.as_str())
            .filter(|w| w.chars().count() >= 2 && *w != "훈" && *w != "음")
            .collect();

        if !tokens.is_empty() {
            let mut best_score = 0;
            let mut best_idx = None;
            for (idx, line) in all_lines.iter().enumerate() {
                let mut score = 0;
                for t in &tokens {
                    if line.contains(t) {
                        score += 1;
                    }
                }
                if score > best_score {
                    best_score = score;
                    best_idx = Some(idx);
                    if score == tokens.len() {
                        return Some(idx);
                    }
                }
            }
            if best_score >= 1 {
                return best_idx;
            }
        }
    }

    None
}

// [요구사항 2] 각 PC의 기본 텍스트 에디터(VS Code, Notepad++, 메모장 등)로 해당 라인을 여는 함수
pub(crate) fn open_in_external_editor(file_path: &Path, line_no: Option<usize>) -> String {
    let path_str = file_path.to_string_lossy().to_string();
    let line = line_no.unwrap_or(1);

    // 1. VS Code가 있는 경우 (`code -g "path:line"`)
    let vs_cmd = if cfg!(target_os = "windows") { "code.cmd" } else { "code" };
    if let Ok(_) = std::process::Command::new(vs_cmd)
        .arg("-g")
        .arg(format!("{}:{}", path_str, line))
        .spawn()
    {
        return format!("VS Code로 열림 (라인 #{})", line);
    }

    // 2. Notepad++가 있는 경우 (`notepad++ -n{line} "path"`)
    if let Ok(_) = std::process::Command::new("notepad++")
        .arg(format!("-n{}", line))
        .arg(&path_str)
        .spawn()
    {
        return format!("Notepad++로 열림 (라인 #{})", line);
    }

    // 3. Windows 메모장(Notepad) 및 기본 연결 프로그램
    #[cfg(target_os = "windows")]
    {
        if let Ok(_) = std::process::Command::new("notepad.exe").arg(&path_str).spawn() {
            return format!("기본 메모장(Notepad)으로 열림 (권장 위치: 라인 #{})", line);
        }
        let _ = std::process::Command::new("cmd").args(["/c", "start", "", &path_str]).spawn();
        return format!("시스템 기본 텍스트 에디터로 열림 (라인 #{})", line);
    }

    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-t").arg(&path_str).spawn();
        return format!("Mac 기본 텍스트 에디터로 열림 (라인 #{})", line);
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(&path_str).spawn();
        return format!("기본 텍스트 에디터로 열림 (라인 #{})", line);
    }
}

pub(crate) fn handle_open_editor(params: &HashMap<String, String>) -> ApiResponse {
    let file_path_str = match params.get("file_path") {
        Some(p) if !p.is_empty() => p,
        _ => return json_response(&serde_json::json!({ "error": "file_path required" }), 400),
    };
    let line_no = params.get("line").and_then(|v| v.parse().ok());
    let path = Path::new(file_path_str);
    if !path.exists() {
        return json_response(&serde_json::json!({ "error": format!("파일을 찾을 수 없습니다: {}", file_path_str) }), 404);
    }
    let status = open_in_external_editor(path, line_no);
    json_response(&serde_json::json!({ "ok": true, "status": status, "file_path": file_path_str, "line": line_no }), 200)
}

pub(crate) fn handle_source_jump(
    state: &AppState,
    params: &HashMap<String, String>,
) -> ApiResponse {
    let segment_id: i64 = match params.get("segment_id").and_then(|v| v.parse().ok()) {
        Some(id) => id,
        None => return json_response(&serde_json::json!({ "error": "segment_id required" }), 400),
    };

    let conn = match Connection::open(&state.db_path) {
        Ok(c) => c,
        Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 500),
    };

    let seg_info = conn.query_row(
        "SELECT s.doc_id, d.title, s.seq_no, s.sent_id, s.page_ref, s.text_raw, s.text_unicode, d.source_files_list
         FROM segments s
         JOIN documents d ON s.doc_id = d.doc_id
         WHERE s.segment_id = ?",
        params![segment_id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
            ))
        },
    );

    let (doc_id, doc_title, seq_no, sent_id, page_ref, text_raw, text_unicode, source_files_json) =
        match seg_info {
            Ok(tuple) => tuple,
            Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 404),
        };

    let files: Vec<String> = serde_json::from_str(&source_files_json).unwrap_or_default();
    let file_name = files.first().cloned().unwrap_or_else(|| format!("{}.txt", doc_id));

    // [요구사항 2] 원본 파일 경로 탐색: source_files 테이블의 rel_path 및 다중 후보 경로 종합 검색
    let mut candidate_paths = Vec::new();

    // 1) source_files 테이블에서 실제 등록된 rel_path 확인
    if let Ok(mut stmt) = conn.prepare("SELECT rel_path FROM source_files WHERE file_name = ? OR linked_doc_id = ?") {
        if let Ok(rows) = stmt.query_map(params![&file_name, &doc_id], |r| r.get::<_, String>(0)) {
            for rp in rows.flatten() {
                let p = PathBuf::from(&rp);
                if p.exists() {
                    candidate_paths.push(p);
                } else {
                    candidate_paths.push(state.root_dir.join(&rp));
                }
            }
        }
    }

    // 2) 표준 샘플 및 드라이브 후보 추가
    candidate_paths.push(state.root_dir.join("sample").join(&file_name));
    candidate_paths.push(state.root_dir.join(&file_name));
    candidate_paths.push(PathBuf::from(r"F:\내 드라이브\AG\concordance\sample").join(&file_name));
    candidate_paths.push(PathBuf::from(r"F:\내 드라이브\AG\concordance").join(&file_name));

    // 세종 3중 연동의 경우 documents.source_files_list의 다른 파일들도 후보로 추가
    for f in &files {
        candidate_paths.push(state.root_dir.join("sample").join(f));
        candidate_paths.push(state.root_dir.join(f));
    }

    let mut found_path: Option<PathBuf> = None;
    for p in candidate_paths {
        if p.exists() {
            found_path = Some(p);
            break;
        }
    }

    let mut result_lines = Vec::new();
    let mut target_line_no: Option<usize> = None;

    if let Some(ref p) = found_path {
        if let Ok(bytes) = fs::read(p) {
            let content_str = if bytes.starts_with(&[0xff, 0xfe]) {
                let u16s: Vec<u16> = bytes[2..]
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16_lossy(&u16s)
            } else if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
                String::from_utf8_lossy(&bytes[3..]).to_string()
            } else {
                String::from_utf8_lossy(&bytes).to_string()
            };

            let all_lines: Vec<&str> = content_str.lines().collect();

            // [요구사항 2] 정밀 매칭 알고리즘으로 해당 부분의 정확한 라인 탐색
            let matched_idx = find_target_line_in_source(&all_lines, &sent_id, &text_raw, &text_unicode, seq_no);

            if let Some(m_idx) = matched_idx {
                target_line_no = Some(m_idx + 1);
                let start_idx = m_idx.saturating_sub(15);
                let end_idx = (m_idx + 16).min(all_lines.len());
                for i in start_idx..end_idx {
                    result_lines.push(SourceJumpLine {
                        line_no: i + 1,
                        content: all_lines[i].to_string(),
                        is_target: i == m_idx,
                    });
                }
            } else {
                for (i, line) in all_lines.iter().take(30).enumerate() {
                    result_lines.push(SourceJumpLine {
                        line_no: i + 1,
                        content: line.to_string(),
                        is_target: i == 0,
                    });
                }
            }
        }
    }

    if result_lines.is_empty() {
        if let Ok(mut stmt) = conn.prepare(
            "SELECT seq_no, text_raw, text_unicode, sent_id, page_ref
             FROM segments
             WHERE doc_id = ? AND seq_no BETWEEN ? - 5 AND ? + 5
             ORDER BY seq_no ASC",
        ) {
            if let Ok(rows) = stmt.query_map(params![doc_id, seq_no, seq_no], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            }) {
                for r in rows.flatten() {
                    let is_tgt = r.0 == seq_no;
                    result_lines.push(SourceJumpLine {
                        line_no: r.0 as usize,
                        content: if !r.1.is_empty() { r.1 } else { r.2 },
                        is_target: is_tgt,
                    });
                }
            }
        }
    }

    // [요구사항 2] 점프 실행 시 PC의 기본 텍스트 에디터(VS Code / 메모장 등)로 해당 파일과 라인을 즉시 실행
    let mut editor_status = None;
    let should_open_editor = params.get("open_editor").map(|v| v != "0").unwrap_or(true);

    if should_open_editor {
        if let Some(ref p) = found_path {
            let status = open_in_external_editor(p, target_line_no);
            editor_status = Some(status);
        }
    }

    let resp = SourceJumpResponse {
        segment_id,
        doc_id,
        doc_title,
        file_name: file_name.clone(),
        file_path: found_path
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("(DB 아카이브: {})", file_name)),
        line_no: target_line_no,
        sent_id,
        page_ref,
        text_raw,
        text_unicode,
        editor_status,
        lines: result_lines,
    };

    json_response(&resp, 200)
}

