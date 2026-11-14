use crate::builder;
use crate::models::{
    json_response, ApiResponse, AppState, DbBuildProgress, DocumentInfo, EditHistoryItem, EditSegmentRequest,
    NeighborSentence,
    OverviewResponse, SegmentDetailResponse, SourceFileInfo,
};
use rusqlite::{params, Connection};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

pub(crate) fn handle_overview(state: &AppState) -> ApiResponse {
    let conn = match Connection::open(&state.db_path) {
        Ok(c) => c,
        Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 500),
    };
    state.memory_profile.apply_pragmas(&conn);

    let _ = conn.execute("ALTER TABLE documents ADD COLUMN speaker_info TEXT DEFAULT ''", []);
    let _ = conn.execute("ALTER TABLE documents ADD COLUMN other_meta TEXT DEFAULT ''", []);

    let mut documents = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT doc_id, title, author, pub_year, corpus_category, annotation_layers,
                source_files_list, storage_mode, sentence_count, eojeol_count, created_at,
                COALESCE(speaker_info, ''), COALESCE(other_meta, '')
         FROM documents ORDER BY rowid ASC",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(DocumentInfo {
                doc_id: row.get(0)?,
                title: row.get(1)?,
                author: row.get(2).unwrap_or_default(),
                pub_year: row.get(3).unwrap_or_default(),
                corpus_category: row.get(4)?,
                annotation_layers: row.get(5)?,
                source_files_list: row.get(6)?,
                storage_mode: row.get(7)?,
                sentence_count: row.get(8)?,
                eojeol_count: row.get(9)?,
                created_at: row.get(10)?,
                speaker_info: row.get(11).unwrap_or_default(),
                other_meta: row.get(12).unwrap_or_default(),
            })
        }) {
            for r in rows.flatten() {
                documents.push(r);
            }
        }
    }

    let mut source_files = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT file_id, file_name, rel_path, file_size_bytes, original_encoding,
                text_format, corpus_type, raw_notation_desc, analysis_notation_desc,
                linked_doc_id, db_file_name, imported_sentences, imported_eojeols, COALESCE(note, '')
         FROM source_files ORDER BY file_id ASC",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(SourceFileInfo {
                file_id: row.get(0)?,
                file_name: row.get(1)?,
                rel_path: row.get(2)?,
                file_size_bytes: row.get(3)?,
                original_encoding: row.get(4)?,
                text_format: row.get(5)?,
                corpus_type: row.get(6)?,
                raw_notation_desc: row.get(7)?,
                analysis_notation_desc: row.get(8)?,
                linked_doc_id: row.get(9)?,
                target_db_file: row.get(10)?,
                sentence_count: row.get(11)?,
                eojeol_count: row.get(12)?,
                notes: row.get(13).unwrap_or_default(),
            })
        }) {
            for r in rows.flatten() {
                source_files.push(r);
            }
        }
    }

    let total_sentences: i64 = documents.iter().map(|d| d.sentence_count).sum();
    let total_eojeols: i64 = documents.iter().map(|d| d.eojeol_count).sum();
    let db_size_bytes = fs::metadata(&state.db_path).map(|m| m.len()).unwrap_or(0);

    let resp = OverviewResponse {
        memory_profile: state.memory_profile.clone(),
        db_path: state.db_path.to_string_lossy().into_owned(),
        db_size_bytes,
        total_documents: documents.len(),
        total_source_files: source_files.len(),
        total_sentences,
        total_eojeols,
        documents,
        source_files,
    };
    json_response(&resp, 200)
}


pub(crate) fn handle_segment_detail(
    state: &AppState,
    params: &HashMap<String, String>,
) -> ApiResponse {
    let seg_id: i64 = match params.get("id").and_then(|s| s.parse().ok()) {
        Some(id) => id,
        None => return json_response(&serde_json::json!({ "error": "Missing id parameter" }), 400),
    };

    let conn = match Connection::open(&state.db_path) {
        Ok(c) => c,
        Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 500),
    };
    crate::builder::ensure_error_columns(&conn);

    let row_res = conn.query_row(
        "SELECT s.segment_id, s.doc_id, d.title, d.corpus_category, d.annotation_layers, d.source_files_list,
                s.seq_no, COALESCE(s.sent_id_str, ''), '', COALESCE(s.page_info, ''), COALESCE(s.lang, 'kor'), COALESCE(s.part_type, ''),
                s.text_raw, s.text_unicode, COALESCE(s.morph_flat, ''), COALESCE(s.sense_flat, ''), COALESCE(s.syntax_tree, ''),
                COALESCE(s.eojeols_json, '[]'), '{}',
                COALESCE(s.error_flat, ''), COALESCE(s.errors_json, '[]'), COALESCE(s.corrected_text, '')
         FROM segments s
         JOIN documents d ON s.doc_id = d.doc_id
         WHERE s.segment_id = ?",
        params![seg_id],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7).unwrap_or_default(),
                row.get::<_, String>(8).unwrap_or_default(),
                row.get::<_, String>(9).unwrap_or_default(),
                row.get::<_, String>(10).unwrap_or_default(),
                row.get::<_, String>(11).unwrap_or_default(),
                row.get::<_, String>(12)?,
                row.get::<_, String>(13)?,
                row.get::<_, String>(14).unwrap_or_default(),
                row.get::<_, String>(15).unwrap_or_default(),
                row.get::<_, String>(16).unwrap_or_default(),
                row.get::<_, String>(17).unwrap_or_else(|_| "[]".to_string()),
                row.get::<_, String>(18).unwrap_or_else(|_| "{}".to_string()),
                row.get::<_, String>(19).unwrap_or_default(),
                row.get::<_, String>(20).unwrap_or_else(|_| "[]".to_string()),
                row.get::<_, String>(21).unwrap_or_default(),
            ))
        },
    );

    let (
        segment_id,
        doc_id,
        doc_title,
        corpus_category,
        annotation_layers,
        source_files_list,
        seq_no,
        sent_id,
        speaker,
        page_ref,
        lang,
        sent_type,
        text_raw,
        text_unicode,
        morph_flat,
        sense_flat,
        syntax_tree,
        eojeols_str,
        meta_str,
        error_flat,
        errors_str,
        corrected_text,
    ) = match row_res {
        Ok(r) => r,
        Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 404),
    };

    let text_display = state
        .normalizer
        .compose_modern_hangul_for_display(&text_unicode);
    let eojeols: serde_json::Value =
        serde_json::from_str(&eojeols_str).unwrap_or_else(|_| serde_json::json!([]));
    let meta: serde_json::Value =
        serde_json::from_str(&meta_str).unwrap_or_else(|_| serde_json::json!({}));

    let mut neighbors = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT segment_id, seq_no, COALESCE(sent_id_str, ''), COALESCE(page_info, ''), text_unicode
         FROM segments
         WHERE doc_id = ? AND seq_no BETWEEN ? AND ?
         ORDER BY seq_no ASC",
    ) {
        if let Ok(rows) = stmt.query_map(params![&doc_id, seq_no - 3, seq_no + 3], |r| {
            let tu: String = r.get(4)?;
            Ok(NeighborSentence {
                segment_id: r.get(0)?,
                seq_no: r.get(1)?,
                sent_id: r.get(2).unwrap_or_default(),
                page_ref: r.get(3).unwrap_or_default(),
                text_unicode: state.normalizer.compose_modern_hangul_for_display(&tu),
            })
        }) {
            for n in rows.flatten() {
                neighbors.push(n);
            }
        }
    }

    let mut edits = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT segment_id, '원문/형태 수정', original_text_raw, edited_text_raw, updated_at
         FROM segment_edits
         WHERE segment_id = ?",
    ) {
        if let Ok(rows) = stmt.query_map(params![segment_id], |r| {
            Ok(EditHistoryItem {
                edit_id: r.get(0)?,
                field_name: r.get(1)?,
                old_value: r.get(2)?,
                new_value: r.get(3)?,
                edited_at: r.get(4)?,
            })
        }) {
            for ed in rows.flatten() {
                edits.push(ed);
            }
        }
    }

    let errors_val: serde_json::Value = serde_json::from_str(&errors_str).unwrap_or_else(|_| serde_json::json!([]));
    let resp = SegmentDetailResponse {
        segment_id,
        doc_id,
        doc_title,
        corpus_category,
        annotation_layers,
        source_files_list,
        seq_no,
        sent_id,
        speaker,
        page_ref,
        lang,
        sent_type,
        text_raw,
        text_unicode,
        text_display,
        morph_flat,
        sense_flat,
        syntax_tree,
        error_flat,
        errors: errors_val,
        corrected_text,
        eojeols,
        meta,
        neighbors,
        edits,
    };
    json_response(&resp, 200)
}


pub(crate) fn handle_document_delete(
    state: &AppState,
    params: &HashMap<String, String>,
) -> ApiResponse {
    let doc_ids_raw = params.get("doc_ids").or_else(|| params.get("doc_id")).map(|s| s.as_str()).unwrap_or("");
    let doc_ids: Vec<&str> = doc_ids_raw.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if doc_ids.is_empty() {
        return json_response(&serde_json::json!({ "error": "삭제할 doc_id 또는 doc_ids 파라미터가 필요합니다." }), 400);
    }

    let conn = match Connection::open(&state.db_path) {
        Ok(c) => c,
        Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 500),
    };

    let mut deleted_items = Vec::new();
    let mut total_deleted_segs = 0usize;

    for did in &doc_ids {
        let doc_info: Option<(String, String, i64, String)> = conn.query_row(
            "SELECT title, corpus_category, sentence_count, source_files_list FROM documents WHERE doc_id = ?",
            params![did],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).ok();

        if let Some((title, category, sents, sources)) = doc_info {
            let del_segs = conn.execute("DELETE FROM segments WHERE doc_id = ?", params![did]).unwrap_or(0);
            let _ = conn.execute("DELETE FROM source_files WHERE linked_doc_id = ?", params![did]);
            let _ = conn.execute("DELETE FROM documents WHERE doc_id = ?", params![did]);
            total_deleted_segs += del_segs as usize;
            deleted_items.push(serde_json::json!({
                "doc_id": did,
                "title": title,
                "category": category,
                "sentence_count": sents,
                "source_files": sources,
            }));
        }
    }
    let _ = conn.execute("DELETE FROM segment_edits WHERE segment_id NOT IN (SELECT segment_id FROM segments)", []);

    json_response(&serde_json::json!({
        "status": "ok",
        "message": format!("총 {}개 말뭉치 ({}문장) 삭제가 완료되었습니다.", deleted_items.len(), total_deleted_segs),
        "deleted_count": deleted_items.len(),
        "total_deleted_segments": total_deleted_segs,
        "deleted_items": deleted_items,
    }), 200)
}

pub(crate) fn handle_segment_edit(
    state: &AppState,
    body: &[u8],
) -> ApiResponse {
    let payload: EditSegmentRequest = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 400),
    };

    let conn = match Connection::open(&state.db_path) {
        Ok(c) => c,
        Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 500),
    };

    let _ = builder::init_db_schema(&conn);

    let orig_row: Option<(String, String)> = conn
        .query_row(
            "SELECT text_raw, morph_flat FROM segments WHERE segment_id = ?",
            params![payload.segment_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();

    let (orig_raw, orig_morph) = orig_row.unwrap_or_default();
    let new_raw = payload
        .text_raw
        .as_ref()
        .or(payload.text_unicode.as_ref())
        .cloned()
        .unwrap_or_else(|| orig_raw.clone());
    let new_morph = payload
        .morph_flat
        .clone()
        .unwrap_or_else(|| orig_morph.clone());

    let now_str = {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("{}", secs)
    };

    let _ = conn.execute(
        "INSERT OR REPLACE INTO segment_edits
         (segment_id, original_text_raw, original_morph_flat, edited_text_raw, edited_morph_flat, updated_at, synced_to_source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
        params![
            payload.segment_id,
            orig_raw,
            orig_morph,
            new_raw,
            new_morph,
            now_str
        ],
    );

    if let Some(ref raw_val) = payload.text_raw {
        let uni = state.normalizer.pua_to_unicode(raw_val);
        let nfd = state.normalizer.to_nfd_norm(&uni);
        let _ = conn.execute(
            "UPDATE segments SET text_raw = ?1, text_unicode = ?2, text_nfd_norm = ?3, is_modified = 1 WHERE segment_id = ?4",
            params![raw_val, uni, nfd, payload.segment_id],
        );
    } else if let Some(ref new_val) = payload.text_unicode {
        let uni = state.normalizer.pua_to_unicode(new_val);
        let nfd = state.normalizer.to_nfd_norm(&uni);
        let _ = conn.execute(
            "UPDATE segments SET text_unicode = ?1, text_nfd_norm = ?2, is_modified = 1 WHERE segment_id = ?3",
            params![uni, nfd, payload.segment_id],
        );
    }
    if let Some(ref m) = payload.morph_flat {
        let _ = conn.execute(
            "UPDATE segments SET morph_flat = ?1, is_modified = 1 WHERE segment_id = ?2",
            params![m, payload.segment_id],
        );
    }
    if let Some(ref s) = payload.sense_flat {
        let _ = conn.execute(
            "UPDATE segments SET sense_flat = ?1, is_modified = 1 WHERE segment_id = ?2",
            params![s, payload.segment_id],
        );
    }
    if let Some(ref st) = payload.syntax_tree {
        let _ = conn.execute(
            "UPDATE segments SET syntax_tree = ?1, is_modified = 1 WHERE segment_id = ?2",
            params![st, payload.segment_id],
        );
    }

    json_response(&serde_json::json!({ "ok": true, "success": true }), 200)
}

#[derive(Debug, Deserialize)]
pub(crate) struct BuildDbRequest {
    pub(crate) folder_path: String,
    pub(crate) db_name: Option<String>,
    pub(crate) file_paths: Option<Vec<String>>,
}

pub(crate) fn handle_build_db(
    state: &AppState,
    body: &[u8],
) -> ApiResponse {
    let payload: BuildDbRequest = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 400),
    };

    let mut resolved_paths: Vec<PathBuf> = Vec::new();

    if let Some(ref list) = payload.file_paths {
        for fp in list {
            let clean = fp.trim();
            if clean.is_empty() { continue; }
            let p = PathBuf::from(clean);
            if p.exists() {
                resolved_paths.push(p);
            } else {
                let rel = state.root_dir.join(clean);
                if rel.exists() {
                    resolved_paths.push(rel);
                } else {
                    resolved_paths.push(p);
                }
            }
        }
    }

    if resolved_paths.is_empty() {
        let raw_folder = payload.folder_path.trim().to_string();
        if raw_folder.contains('|') || raw_folder.contains(';') {
            let sep = if raw_folder.contains('|') { '|' } else { ';' };
            for part in raw_folder.split(sep) {
                let clean = part.trim();
                if clean.is_empty() { continue; }
                let p = PathBuf::from(clean);
                if p.exists() {
                    resolved_paths.push(p);
                } else {
                    let rel = state.root_dir.join(clean);
                    if rel.exists() {
                        resolved_paths.push(rel);
                    } else {
                        resolved_paths.push(p);
                    }
                }
            }
        } else if !raw_folder.is_empty() {
            let p = PathBuf::from(&raw_folder);
            if p.exists() {
                resolved_paths.push(p);
            } else {
                let rel = state.root_dir.join(&raw_folder);
                if rel.exists() {
                    resolved_paths.push(rel);
                } else {
                    resolved_paths.push(p);
                }
            }
        }
    }

    if resolved_paths.is_empty() {
        return json_response(
            &serde_json::json!({ "error": "지정한 파일 또는 폴더 경로를 입력하거나 선택해 주세요." }),
            400,
        );
    }

    for p in &resolved_paths {
        if !p.exists() {
            return json_response(
                &serde_json::json!({ "error": format!("지정한 파일/폴더가 존재하지 않습니다: {}", p.display()) }),
                400,
            );
        }
    }

    let target_db = if let Some(name) = payload.db_name {
        let clean = name.trim();
        if clean.is_empty() {
            state.db_path.clone()
        } else if clean.ends_with(".corpdb") {
            state.root_dir.join("db").join(clean)
        } else {
            state.root_dir.join("db").join(format!("{}.corpdb", clean))
        }
    } else {
        state.db_path.clone()
    };

    let mut mgr = state.db_builder.lock().unwrap();
    if mgr.progress.is_running {
        return json_response(
            &serde_json::json!({ "error": "이미 다른 DB 생성 작업이 진행 중입니다." }),
            409,
        );
    }

    let is_new_db = !target_db.exists();
    let desc_str = if resolved_paths.len() == 1 {
        resolved_paths[0].to_string_lossy().into_owned()
    } else {
        format!("{}건의 선택 파일/폴더", resolved_paths.len())
    };
    mgr.progress = DbBuildProgress {
        is_running: true,
        status: "RUNNING".to_string(),
        target_db_path: target_db.to_string_lossy().into_owned(),
        is_new_db_file: is_new_db,
        log_lines: vec![format!("[시작] 내장 Rust 파서로 말뭉치 스캔 시작: {}", desc_str)],
        ..DbBuildProgress::default()
    };

    let initial_prog = mgr.progress.clone();
    drop(mgr);

    let db_builder_arc = Arc::clone(&state.db_builder);
    let normalizer_clone = state.normalizer.clone();

    // 백그라운드 네이티브 Rust DB 빌더 스레드 (외부 Python 불필요)
    std::thread::spawn(move || {
        if let Err(e) = builder::build_paths_into_db_native(
            &resolved_paths,
            &target_db,
            &normalizer_clone,
            &db_builder_arc,
        ) {
            let mut m = db_builder_arc.lock().unwrap();
            m.progress.is_running = false;
            m.progress.status = "ERROR".to_string();
            m.progress.error_message = Some(e);
        }
    });

    json_response(&initial_prog, 200)
}

pub(crate) fn handle_pick_folder(state: &AppState) -> ApiResponse {
    #[cfg(target_os = "windows")]
    {
        let init_dir_path = {
            let guard = state.last_pick_folder.lock().unwrap();
            guard.clone().unwrap_or_else(|| PathBuf::from(r"C:\"))
        };
        let init_dir_str = if init_dir_path.exists() {
            init_dir_path.to_string_lossy().replace('\'', "''")
        } else {
            r"C:\".to_string()
        };

        let ps_script = format!(
            "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8;              Add-Type -AssemblyName System.Windows.Forms;              $d = New-Object System.Windows.Forms.OpenFileDialog;              $d.Title = '말뭉치 파일(복수 선택 가능) 또는 파일이 있는 폴더 선택';              $d.Filter = '말뭉치 텍스트 파일 (*.txt;*.xml;*.json;*.sgml;*.tei;*.csv;*.tsv)|*.txt;*.xml;*.json;*.sgml;*.tei;*.csv;*.tsv|모든 파일 (*.*)|*.*';              $d.Multiselect = $true;              $d.CheckFileExists = $false;              $d.InitialDirectory = '{}';              $d.FileName = '폴더전체선택';              if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{                  if ($d.FileNames.Count -eq 1 -and [System.IO.Path]::GetFileName($d.FileName) -eq '폴더전체선택') {{                      [Console]::WriteLine('DIR|' + [System.IO.Path]::GetDirectoryName($d.FileName))                  }} else {{                      foreach ($f in $d.FileNames) {{ [Console]::WriteLine('FILE|' + $f) }}                  }}              }}",
            init_dir_str
        );

        if let Ok(output) = Command::new("powershell")
            .args(["-NoProfile", "-STA", "-Command", &ps_script])
            .output()
        {
            let stdout_str = String::from_utf8_lossy(&output.stdout);
            let mut files = Vec::new();
            let mut folder_res: Option<String> = None;

            for line in stdout_str.lines() {
                let clean = line.trim();
                if clean.starts_with("DIR|") {
                    folder_res = Some(clean[4..].trim().to_string());
                } else if clean.starts_with("FILE|") {
                    files.push(clean[5..].trim().to_string());
                }
            }

            if let Some(dir) = folder_res {
                let p = PathBuf::from(&dir);
                if p.exists() {
                    let mut guard = state.last_pick_folder.lock().unwrap();
                    *guard = Some(p);
                }
                return json_response(&serde_json::json!({
                    "ok": true,
                    "folder_path": dir,
                    "is_folder": true,
                    "file_paths": [dir]
                }), 200);
            } else if !files.is_empty() {
                if let Some(first) = files.first() {
                    if let Some(parent) = Path::new(first).parent() {
                        let mut guard = state.last_pick_folder.lock().unwrap();
                        *guard = Some(parent.to_path_buf());
                    }
                }
                let display_str = if files.len() == 1 {
                    files[0].clone()
                } else {
                    files.join(" | ")
                };
                return json_response(&serde_json::json!({
                    "ok": true,
                    "folder_path": display_str,
                    "is_folder": false,
                    "file_paths": files
                }), 200);
            }
        }
        return json_response(&serde_json::json!({ "ok": false, "cancelled": true }), 200);
    }
    #[cfg(target_os = "macos")]
    {
        let init_dir_path = {
            let guard = state.last_pick_folder.lock().unwrap();
            guard.clone().unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("/"))
            })
        };
        let init_dir_str = if init_dir_path.exists() {
            init_dir_path.to_string_lossy().replace('"', "\\\"")
        } else {
            "/".to_string()
        };

        let apple_script = format!(
            "set initLoc to POSIX file \"{}\" as alias\n\
             set choiceBtn to button returned of (display dialog \"말뭉치 대상을 선택하세요.\" buttons {{\"취소\", \"파일 선택(복수 가능)\", \"폴더 전체 선택\"}} default button \"폴더 전체 선택\" with title \"말뭉치 폴더/파일 선택\")\n\
             if choiceBtn is \"폴더 전체 선택\" then\n\
               set chosenFolder to choose folder with prompt \"말뭉치 폴더를 선택하세요\" default location initLoc\n\
               return \"DIR|\" & POSIX path of chosenFolder\n\
             else if choiceBtn is \"파일 선택(복수 가능)\" then\n\
               set chosenFiles to choose file with prompt \"말뭉치 파일을 선택하세요 (Command 키로 복수 선택 가능)\" default location initLoc with multiple selections allowed\n\
               set outStr to \"\"\n\
               repeat with f in chosenFiles\n\
                 set outStr to outStr & \"FILE|\" & POSIX path of f & linefeed\n\
               end repeat\n\
               return outStr\n\
             end if",
            init_dir_str
        );

        if let Ok(output) = Command::new("osascript").args(["-e", &apple_script]).output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut files: Vec<String> = Vec::new();
                let mut picked_dir: Option<String> = None;

                for line in stdout.lines() {
                    let trimmed = line.trim();
                    if let Some(d) = trimmed.strip_prefix("DIR|") {
                        let d_clean = d.trim_end_matches('/').to_string();
                        if !d_clean.is_empty() {
                            picked_dir = Some(d_clean);
                        }
                    } else if let Some(f) = trimmed.strip_prefix("FILE|") {
                        let f_clean = f.trim().to_string();
                        if !f_clean.is_empty() {
                            files.push(f_clean);
                        }
                    }
                }

                if let Some(dir) = picked_dir {
                    if let Ok(mut guard) = state.last_pick_folder.lock() {
                        *guard = Some(PathBuf::from(&dir));
                    }
                    return json_response(&serde_json::json!({
                        "ok": true,
                        "folder_path": dir,
                        "is_folder": true,
                        "file_paths": []
                    }), 200);
                }

                if !files.is_empty() {
                    if let Some(parent) = PathBuf::from(&files[0]).parent() {
                        if let Ok(mut guard) = state.last_pick_folder.lock() {
                            *guard = Some(parent.to_path_buf());
                        }
                    }
                    let display_str = if files.len() == 1 {
                        files[0].clone()
                    } else {
                        format!("[선택한 {}개 파일] {} 외", files.len(), files[0])
                    };
                    return json_response(&serde_json::json!({
                        "ok": true,
                        "folder_path": display_str,
                        "is_folder": false,
                        "file_paths": files
                    }), 200);
                }
            }
        }
        return json_response(&serde_json::json!({ "ok": false, "cancelled": true }), 200);
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = state;
        json_response(&serde_json::json!({ "ok": false, "error": "Windows/macOS 환경에서만 파일/폴더 선택 대화상자를 지원합니다." }), 400)
    }
}

pub(crate) fn handle_build_db_status(state: &AppState) -> ApiResponse {
    let mgr = state.db_builder.lock().unwrap();
    json_response(&mgr.progress, 200)
}

pub(crate) fn handle_build_db_stop(state: &AppState) -> ApiResponse {
    let mut mgr = state.db_builder.lock().unwrap();
    if !mgr.progress.is_running {
        return json_response(&serde_json::json!({ "success": false, "message": "현재 실행 중인 DB 생성 작업이 없습니다." }), 200);
    }

    if let Some(mut ch) = mgr.child.take() {
        let _ = ch.kill();
    }
    mgr.progress.is_running = false;
    mgr.progress.status = "STOPPED".to_string();
    mgr.progress.log_lines.push("⏸ [중단] 사용자에 의해 DB 생성이 중단되었습니다. (현재까지 생성된 DB는 보존됨)".to_string());
    json_response(&serde_json::json!({ "success": true, "status": "STOPPED", "message": "DB 생성이 중단되었습니다." }), 200)
}

pub(crate) fn handle_build_db_cancel(state: &AppState) -> ApiResponse {
    let mut mgr = state.db_builder.lock().unwrap();
    if let Some(mut ch) = mgr.child.take() {
        let _ = ch.kill();
    }
    mgr.progress.is_running = false;
    mgr.progress.status = "CANCELLED".to_string();

    let target_path_str = mgr.progress.target_db_path.clone();
    let is_new_db = mgr.progress.is_new_db_file;
    let new_doc_ids = mgr.progress.newly_created_doc_ids.clone();

    if is_new_db {
        let p = Path::new(&target_path_str);
        if p.exists() {
            for _ in 0..6 {
                std::thread::sleep(std::time::Duration::from_millis(150));
                let _ = fs::remove_file(format!("{}-wal", target_path_str));
                let _ = fs::remove_file(format!("{}-shm", target_path_str));
                if fs::remove_file(p).is_ok() || !p.exists() {
                    break;
                }
            }
            if !p.exists() {
                mgr.progress.log_lines.push(format!("🗑️ [취소] 생성 중이던 신규 DB 파일({})을 삭제했습니다.", target_path_str));
            }
        }
    } else {
        if !new_doc_ids.is_empty() {
            if let Ok(conn) = Connection::open(&target_path_str) {
                for doc_id in &new_doc_ids {
                    let _ = conn.execute("DELETE FROM segments WHERE doc_id = ?", params![doc_id]);
                    let _ = conn.execute("DELETE FROM documents WHERE doc_id = ?", params![doc_id]);
                    let _ = conn.execute("DELETE FROM source_files WHERE linked_doc_id = ?", params![doc_id]);
                }
                mgr.progress.log_lines.push(format!("🗑️ [취소] 이번 작업으로 추가되었던 {}개 문서를 기존 DB에서 삭제/롤백했습니다.", new_doc_ids.len()));
            }
        }
    }

    mgr.progress.log_lines.push("❌ [취소 완료] DB 생성이 취소되었으며 관련 데이터가 모두 정리되었습니다.".to_string());
    json_response(&serde_json::json!({
        "success": true,
        "status": "CANCELLED",
        "message": "DB 생성이 취소되었으며, 생성된 데이터가 삭제되었습니다."
    }), 200)
}

#[derive(Debug, Deserialize)]
pub(crate) struct SaveTxtRequest {
    pub(crate) default_name: String,
    pub(crate) content: String,
}

pub(crate) fn handle_export_save_txt(state: &AppState, body: &[u8]) -> ApiResponse {
    let payload: SaveTxtRequest = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(e) => return json_response(&serde_json::json!({ "ok": false, "error": e.to_string() }), 400),
    };

    #[cfg(target_os = "windows")]
    {
        let initial_dir = state.root_dir.to_string_lossy().replace('\'', "''");
        let safe_name = payload.default_name.replace('\'', "''");
        let ps_script = format!(
            "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
             Add-Type -AssemblyName System.Windows.Forms; \
             $d = New-Object System.Windows.Forms.SaveFileDialog; \
             $d.Title = '검색 결과 TXT 저장 위치 및 파일명 지정'; \
             $d.Filter = '텍스트 파일 (*.txt)|*.txt|TSV 파일 (*.tsv)|*.tsv|모든 파일 (*.*)|*.*'; \
             $d.DefaultExt = 'txt'; \
             $d.InitialDirectory = '{}'; \
             $d.FileName = '{}'; \
             if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ Write-Output $d.FileName }}",
            initial_dir, safe_name
        );

        let output = match Command::new("powershell")
            .args(["-NoProfile", "-STA", "-Command", &ps_script])
            .output()
        {
            Ok(o) => o,
            Err(e) => return json_response(&serde_json::json!({ "ok": false, "error": format!("저장 대화상자 실행 오류: {}", e) }), 500),
        };

        let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if selected.is_empty() {
            return json_response(&serde_json::json!({ "ok": false, "cancelled": true }), 200);
        }

        if let Err(e) = fs::write(&selected, payload.content.as_bytes()) {
            return json_response(&serde_json::json!({ "ok": false, "error": format!("파일 쓰기 실패: {}", e) }), 500);
        }

        return json_response(&serde_json::json!({ "ok": true, "saved_path": selected }), 200);
    }

    #[cfg(target_os = "macos")]
    {
        let default_name = payload.default_name.replace('"', "\\\"");
        let apple_script = format!(
            "set chosenFile to choose file name with prompt \"검색 결과 TXT 저장\" default name \"{}\"\nreturn POSIX path of chosenFile",
            default_name
        );
        let output = match Command::new("osascript").args(["-e", &apple_script]).output() {
            Ok(o) => o,
            Err(e) => {
                return json_response(&serde_json::json!({ "ok": false, "error": format!("macOS 저장 대화상자 실행 오류: {}", e) }), 500);
            }
        };
        if !output.status.success() {
            return json_response(&serde_json::json!({ "ok": false, "cancelled": true }), 200);
        }
        let mut selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if selected.is_empty() {
            return json_response(&serde_json::json!({ "ok": false, "cancelled": true }), 200);
        }
        if !selected.to_lowercase().ends_with(".txt") {
            selected.push_str(".txt");
        }
        if let Err(e) = fs::write(&selected, payload.content.as_bytes()) {
            return json_response(&serde_json::json!({ "ok": false, "error": format!("파일 쓰기 실패: {}", e) }), 500);
        }
        return json_response(&serde_json::json!({ "ok": true, "saved_path": selected }), 200);
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let out_path = state.root_dir.join(&payload.default_name);
        if let Err(e) = fs::write(&out_path, payload.content.as_bytes()) {
            return json_response(&serde_json::json!({ "ok": false, "error": e.to_string() }), 500);
        }
        json_response(&serde_json::json!({ "ok": true, "saved_path": out_path.to_string_lossy() }), 200)
    }
}

