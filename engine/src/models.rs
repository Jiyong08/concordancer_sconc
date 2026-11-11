use crate::normalizer::Normalizer;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use sysinfo::System;
use tiny_http::{Header, Response, StatusCode};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MemoryProfile {
    pub(crate) total_ram_gb: f64,
    pub(crate) available_ram_gb: f64,
    pub(crate) tier_code: String,
    pub(crate) tier_label: String,
    pub(crate) sqlite_cache_mb: i64,
    pub(crate) sqlite_mmap_mb: i64,
    pub(crate) search_batch_limit: usize,
    pub(crate) worker_threads: usize,
}

impl MemoryProfile {
    pub(crate) fn detect() -> Self {
        let mut sys = System::new();
        sys.refresh_memory();
        let total_bytes = sys.total_memory();
        let avail_bytes = sys.available_memory();
        let total_gb = (total_bytes as f64) / (1024.0 * 1024.0 * 1024.0);
        let avail_gb = (avail_bytes as f64) / (1024.0 * 1024.0 * 1024.0);
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        if total_gb <= 8.5 || avail_gb < 2.5 {
            MemoryProfile {
                total_ram_gb: (total_gb * 10.0).round() / 10.0,
                available_ram_gb: (avail_gb * 10.0).round() / 10.0,
                tier_code: "TIER_1_8GB".to_string(),
                tier_label: "Tier 1 — 절전/스트리밍 모드 (8GB급 표준)".to_string(),
                sqlite_cache_mb: 64,
                sqlite_mmap_mb: 128,
                search_batch_limit: 500,
                worker_threads: cpus.min(2).max(1),
            }
        } else if total_gb <= 16.5 {
            MemoryProfile {
                total_ram_gb: (total_gb * 10.0).round() / 10.0,
                available_ram_gb: (avail_gb * 10.0).round() / 10.0,
                tier_code: "TIER_2_16GB".to_string(),
                tier_label: "Tier 2 — 균형 가속 모드 (16GB급)".to_string(),
                sqlite_cache_mb: 256,
                sqlite_mmap_mb: 512,
                search_batch_limit: 2000,
                worker_threads: cpus.min(4).max(2),
            }
        } else if total_gb <= 32.5 {
            MemoryProfile {
                total_ram_gb: (total_gb * 10.0).round() / 10.0,
                available_ram_gb: (avail_gb * 10.0).round() / 10.0,
                tier_code: "TIER_3_32GB".to_string(),
                tier_label: "Tier 3 — 고속 인메모리 캐시 모드 (32GB급)".to_string(),
                sqlite_cache_mb: 1024,
                sqlite_mmap_mb: 2048,
                search_batch_limit: 5000,
                worker_threads: cpus.min(8).max(4),
            }
        } else {
            MemoryProfile {
                total_ram_gb: (total_gb * 10.0).round() / 10.0,
                available_ram_gb: (avail_gb * 10.0).round() / 10.0,
                tier_code: "TIER_4_64GB_PLUS".to_string(),
                tier_label: "Tier 4 — 최대 성능 풀-메모리 모드 (64GB+ 워크스테이션)".to_string(),
                sqlite_cache_mb: 2048,
                sqlite_mmap_mb: 4096,
                search_batch_limit: 10000,
                worker_threads: cpus,
            }
        }
    }

    pub(crate) fn apply_pragmas(&self, conn: &Connection) {
        let cache_kib = -(self.sqlite_cache_mb * 1024);
        let mmap_bytes = self.sqlite_mmap_mb * 1024 * 1024;
        let _ = conn.execute_batch(&format!(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = MEMORY;
             PRAGMA cache_size = {};
             PRAGMA mmap_size = {};",
            cache_kib, mmap_bytes
        ));
    }
}


#[derive(Debug, Serialize)]
pub(crate) struct DocumentInfo {
    pub(crate) doc_id: String,
    pub(crate) title: String,
    pub(crate) author: String,
    pub(crate) pub_year: String,
    pub(crate) corpus_category: String,
    pub(crate) annotation_layers: String,
    pub(crate) source_files_list: String,
    pub(crate) storage_mode: String,
    pub(crate) sentence_count: i64,
    pub(crate) eojeol_count: i64,
    pub(crate) created_at: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SourceFileInfo {
    pub(crate) file_id: i64,
    pub(crate) file_name: String,
    pub(crate) rel_path: String,
    pub(crate) file_size_bytes: i64,
    pub(crate) original_encoding: String,
    pub(crate) text_format: String,
    pub(crate) corpus_type: String,
    pub(crate) raw_notation_desc: String,
    pub(crate) analysis_notation_desc: String,
    pub(crate) linked_doc_id: String,
    pub(crate) target_db_file: String,
    pub(crate) sentence_count: i64,
    pub(crate) eojeol_count: i64,
    pub(crate) notes: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct OverviewResponse {
    pub(crate) memory_profile: MemoryProfile,
    pub(crate) db_path: String,
    pub(crate) db_size_bytes: u64,
    pub(crate) total_documents: usize,
    pub(crate) total_source_files: usize,
    pub(crate) total_sentences: i64,
    pub(crate) total_eojeols: i64,
    pub(crate) documents: Vec<DocumentInfo>,
    pub(crate) source_files: Vec<SourceFileInfo>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SearchHit {
    pub(crate) segment_id: i64,
    pub(crate) doc_id: String,
    pub(crate) doc_title: String,
    pub(crate) corpus_category: String,
    pub(crate) annotation_layers: String,
    pub(crate) seq_no: i64,
    pub(crate) sent_id: String,
    pub(crate) page_ref: String,
    pub(crate) speaker: String,
    pub(crate) lang: String,
    pub(crate) sent_type: String,
    pub(crate) left_context: String,
    pub(crate) node: String,
    pub(crate) right_context: String,
    pub(crate) text_raw: String,
    pub(crate) text_unicode: String,
    pub(crate) morph_flat: String,
    pub(crate) sense_flat: String,
    pub(crate) syntax_tree: String,
    pub(crate) has_morph: bool,
    pub(crate) has_sense: bool,
    pub(crate) has_syntax: bool,
    pub(crate) match_layer: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SearchResponse {
    pub(crate) query: String,
    pub(crate) mode: String,
    pub(crate) normalized_query_nfd: String,
    pub(crate) normalized_query_display: String,
    pub(crate) total_matches: usize,
    pub(crate) returned_count: usize,
    pub(crate) elapsed_ms: f64,
    pub(crate) cancelled: bool,
    pub(crate) memory_tier: String,
    pub(crate) hits: Vec<SearchHit>,
}

#[derive(Debug, Serialize)]
pub(crate) struct NeighborSentence {
    pub(crate) segment_id: i64,
    pub(crate) seq_no: i64,
    pub(crate) sent_id: String,
    pub(crate) page_ref: String,
    pub(crate) text_unicode: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct EditHistoryItem {
    pub(crate) edit_id: i64,
    pub(crate) field_name: String,
    pub(crate) old_value: String,
    pub(crate) new_value: String,
    pub(crate) edited_at: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SegmentDetailResponse {
    pub(crate) segment_id: i64,
    pub(crate) doc_id: String,
    pub(crate) doc_title: String,
    pub(crate) corpus_category: String,
    pub(crate) annotation_layers: String,
    pub(crate) source_files_list: String,
    pub(crate) seq_no: i64,
    pub(crate) sent_id: String,
    pub(crate) speaker: String,
    pub(crate) page_ref: String,
    pub(crate) lang: String,
    pub(crate) sent_type: String,
    pub(crate) text_raw: String,
    pub(crate) text_unicode: String,
    pub(crate) text_display: String,
    pub(crate) morph_flat: String,
    pub(crate) sense_flat: String,
    pub(crate) syntax_tree: String,
    pub(crate) eojeols: serde_json::Value,
    pub(crate) meta: serde_json::Value,
    pub(crate) neighbors: Vec<NeighborSentence>,
    pub(crate) edits: Vec<EditHistoryItem>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EditSegmentRequest {
    pub(crate) segment_id: i64,
    pub(crate) text_raw: Option<String>,
    pub(crate) text_unicode: Option<String>,
    pub(crate) morph_flat: Option<String>,
    pub(crate) sense_flat: Option<String>,
    pub(crate) syntax_tree: Option<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbBuildProgress {
    pub is_running: bool,
    pub status: String,
    pub current_file: String,
    pub total_files: usize,
    pub processed_files: usize,
    pub percent: f64,
    pub current_doc_id: String,
    pub sentences_imported: usize,
    pub eojeols_imported: usize,
    pub skipped_files: usize,
    pub log_lines: Vec<String>,
    pub target_db_path: String,
    pub is_new_db_file: bool,
    pub newly_created_doc_ids: Vec<String>,
    pub error_message: Option<String>,
    pub result_summary: Option<serde_json::Value>,
}

impl Default for DbBuildProgress {
    fn default() -> Self {
        Self {
            is_running: false,
            status: "IDLE".to_string(),
            current_file: String::new(),
            total_files: 0,
            processed_files: 0,
            percent: 0.0,
            current_doc_id: String::new(),
            sentences_imported: 0,
            eojeols_imported: 0,
            skipped_files: 0,
            log_lines: Vec::new(),
            target_db_path: String::new(),
            is_new_db_file: false,
            newly_created_doc_ids: Vec::new(),
            error_message: None,
            result_summary: None,
        }
    }
}

pub struct DbBuildManager {
    pub progress: DbBuildProgress,
    pub child: Option<Child>,
}

impl DbBuildManager {
    pub fn new() -> Self {
        Self {
            progress: DbBuildProgress::default(),
            child: None,
        }
    }
}

pub(crate) struct AppState {
    pub(crate) root_dir: PathBuf,
    pub(crate) db_path: PathBuf,
    pub(crate) web_dir: PathBuf,
    pub(crate) hanbatang_path: PathBuf,
    pub(crate) kopub_ttf_path: PathBuf,
    pub(crate) kopub_otf_path: PathBuf,
    pub(crate) normalizer: Normalizer,
    pub(crate) memory_profile: MemoryProfile,
    pub(crate) latest_search_seq: AtomicU64,
    pub(crate) cancel_search_seq: AtomicU64,
    pub(crate) db_builder: Arc<Mutex<DbBuildManager>>,
    pub(crate) port: u16,
}

pub(crate) fn parse_query_params(url: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(idx) = url.find('?') {
        let qs = &url[idx + 1..];
        for pair in qs.split('&') {
            if pair.is_empty() {
                continue;
            }
            let mut it = pair.splitn(2, '=');
            let k = it.next().unwrap_or("");
            let v = it.next().unwrap_or("");
            let k_dec = urlencoding::decode(k).unwrap_or_else(|_| k.into()).into_owned();
            let v_dec = urlencoding::decode(&v.replace('+', " "))
                .unwrap_or_else(|_| v.into())
                .into_owned();
            params.insert(k_dec, v_dec);
        }
    }
    params
}

pub(crate) fn json_response<T: Serialize>(data: &T, status: u16) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(data).unwrap_or_else(|_| b"{}".to_vec());
    let header =
        Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..]).unwrap();
    let cors = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap();
    Response::from_data(body)
        .with_status_code(StatusCode(status))
        .with_header(header)
        .with_header(cors)
}

