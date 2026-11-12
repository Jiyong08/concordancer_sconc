use regex::Regex;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::{DbBuildManager, Normalizer};

fn decode_utf16_le(bytes: &[u8]) -> String {
    let u16s: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&u16s)
}

fn decode_utf16_be(bytes: &[u8]) -> String {
    let u16s: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&u16s)
}

#[cfg(target_os = "windows")]
fn decode_cp949(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn MultiByteToWideChar(
            CodePage: u32,
            dwFlags: u32,
            lpMultiByteStr: *const u8,
            cbMultiByte: i32,
            lpWideCharStr: *mut u16,
            cchWideChar: i32,
        ) -> i32;
    }
    unsafe {
        let len = MultiByteToWideChar(
            949,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        );
        if len <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut wide = vec![0u16; len as usize];
        let written = MultiByteToWideChar(
            949,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            wide.as_mut_ptr(),
            len,
        );
        if written <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        wide.truncate(written as usize);
        String::from_utf16_lossy(&wide)
    }
}

#[cfg(not(target_os = "windows"))]
fn decode_cp949(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[derive(Clone, Debug)]
pub struct SegmentRecord {
    pub seq_no: usize,
    pub sent_id_str: String,
    pub part_type: String,
    pub page_info: String,
    pub lang: String,
    pub text_raw: String,
    pub text_unicode: String,
    pub text_nfd_norm: String,
    pub morph_flat: String,
    pub sense_flat: String,
    pub syntax_tree: String,
    pub eojeols_json: String,
    pub eojeol_cnt: usize,
}

#[derive(Clone, Debug)]
pub struct SourceFileRecord {
    pub file_name: String,
    pub rel_path: String,
    pub file_size_bytes: u64,
    pub original_encoding: String,
    pub text_format: String,
    pub corpus_type: String,
    pub raw_notation_desc: String,
    pub analysis_notation_desc: String,
    pub linked_doc_id: String,
    pub db_file_name: String,
    pub imported_sentences: usize,
    pub imported_eojeols: usize,
    pub note: String,
}

#[derive(Clone, Debug)]
pub struct DocumentRecord {
    pub doc_id: String,
    pub title: String,
    pub author: String,
    pub pub_year: String,
    pub corpus_category: String,
    pub annotation_layers: String,
    pub source_files_list: String,
    pub sentence_count: usize,
    pub eojeol_count: usize,
    pub speaker_info: String,
    pub other_meta: String,
}

/// 말뭉치 파일 상단 헤더(<Header>, <teiHeader>, JSON metadata)에서 화자 정보와 기타 메타데이터를 분리 추출
pub fn extract_corpus_metadata_from_file(filepath: &Path) -> (String, String) {
    let Ok(mut f) = fs::File::open(filepath) else {
        return (String::new(), String::new());
    };
    let mut buf = vec![0u8; 65536];
    let n = std::io::Read::read(&mut f, &mut buf).unwrap_or(0);
    if n == 0 {
        return (String::new(), String::new());
    }
    buf.truncate(n);
    let head = decode_bytes_auto(&buf).0;
    extract_corpus_metadata_from_text(&head)
}

pub fn extract_corpus_metadata_from_text(text: &str) -> (String, String) {
    let mut speaker_parts: Vec<String> = Vec::new();
    let mut other_parts: Vec<String> = Vec::new();

    // 1) XML <Header> or <teiHeader>
    let re_hdr = Regex::new(r"(?is)<(?:Header|teiHeader)\b[^>]*>(.*?)</(?:Header|teiHeader)>").unwrap();
    if let Some(cap) = re_hdr.captures(text) {
        let hdr_body = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let re_leaf = Regex::new(r"(?is)<([A-Za-z0-9_:-]+)\b[^>]*>([^<]+)</([A-Za-z0-9_:-]+)>").unwrap();
        for lcap in re_leaf.captures_iter(hdr_body) {
            let tag = lcap.get(1).map(|m| m.as_str()).unwrap_or("");
            let end_tag = lcap.get(3).map(|m| m.as_str()).unwrap_or("");
            if !tag.eq_ignore_ascii_case(end_tag) {
                continue;
            }
            let val = lcap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
            if val.is_empty() {
                continue;
            }
            let low = tag.to_lowercase();
            let (is_speaker, label) = match low.as_str() {
                "agegroup" | "age" => (true, "연령"),
                "nationality" => (true, "국적"),
                "gender" | "sex" => (true, "성별"),
                "motherlanguage" | "nativelanguage" => (true, "모어"),
                "datagrade" | "grade" => (true, "등급"),
                "learnertype" => (true, "화자유형"),
                "learnertypedetail" => (true, "세부유형"),
                "learningpurpose" => (true, "학습목적"),
                "occupation" => (true, "직업"),
                "birthplace" => (true, "출생지"),
                "sampleseq" => (false, "일련번호"),
                "learningenvironment" => (false, "학습환경"),
                "sourcetype" => (false, "매체"),
                "assignmenttype" => (false, "과제유형"),
                "assignmentgenre" => (false, "장르"),
                "assignmenttheme" => (false, "주제"),
                "sentencecount" => (false, "원본문장수"),
                "wordcount" => (false, "원본어절수"),
                "title" => (false, "제목"),
                "author" => (false, "저자"),
                "date" | "pubdate" => (false, "작성일"),
                "publisher" => (false, "발행처"),
                _ => (false, tag),
            };
            let entry = format!("{}: {}", label, val);
            if is_speaker {
                if !speaker_parts.contains(&entry) {
                    speaker_parts.push(entry);
                }
            } else if !other_parts.contains(&entry) {
                other_parts.push(entry);
            }
        }

        // <person age="..." sex="..." ...> 속성도 추출
        let re_person = Regex::new(r#"(?is)<person\b([^>]*)>"#).unwrap();
        let re_attr = Regex::new(r#"([A-Za-z0-9_:-]+)=["']([^"']+)["']"#).unwrap();
        for pcap in re_person.captures_iter(hdr_body) {
            let attrs = pcap.get(1).map(|m| m.as_str()).unwrap_or("");
            for acap in re_attr.captures_iter(attrs) {
                let k = acap.get(1).map(|m| m.as_str()).unwrap_or("");
                let v = acap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                if v.is_empty() || k.eq_ignore_ascii_case("id") || k.eq_ignore_ascii_case("xml:id") {
                    continue;
                }
                let label = match k.to_lowercase().as_str() {
                    "age" => "연령",
                    "sex" | "gender" => "성별",
                    "occupation" => "직업",
                    "role" => "역할",
                    _ => k,
                };
                let entry = format!("{}: {}", label, v);
                if !speaker_parts.contains(&entry) {
                    speaker_parts.push(entry);
                }
            }
        }
    }

    // 2) JSON metadata (모두의 말뭉치 등)
    if speaker_parts.is_empty() && other_parts.is_empty() && text.trim_start().starts_with('{') {
        if let Ok(val) = serde_json::from_str::<Value>(text) {
            if let Some(meta) = val.get("metadata").and_then(|m| m.as_object()) {
                for (k, v) in meta {
                    if k == "speaker" || k == "person" {
                        if let Some(arr) = v.as_array() {
                            for sp in arr {
                                if let Some(obj) = sp.as_object() {
                                    for (sk, sv) in obj {
                                        let s_str = sv.as_str().map(|x| x.to_string()).unwrap_or_else(|| sv.to_string());
                                        let label = match sk.as_str() {
                                            "age" => "연령",
                                            "sex" => "성별",
                                            "occupation" => "직업",
                                            "birthplace" => "출생지",
                                            _ => sk.as_str(),
                                        };
                                        let entry = format!("{}: {}", label, s_str.trim_matches('"'));
                                        if !speaker_parts.contains(&entry) {
                                            speaker_parts.push(entry);
                                        }
                                    }
                                }
                            }
                        }
                    } else if let Some(s_val) = v.as_str() {
                        if !s_val.trim().is_empty() {
                            let label = match k.as_str() {
                                "title" => "제목",
                                "author" => "저자",
                                "publisher" => "발행처",
                                "date" => "작성일",
                                "topic" => "주제",
                                "setting" => "배경",
                                _ => k.as_str(),
                            };
                            other_parts.push(format!("{}: {}", label, s_val.trim()));
                        }
                    }
                }
            }
        }
    }

    (speaker_parts.join(" | "), other_parts.join(" | "))
}

fn decode_bytes_auto(raw: &[u8]) -> (String, String) {
    if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return (String::from_utf8_lossy(&raw[3..]).into_owned(), "UTF-8-BOM".to_string());
    }
    if raw.starts_with(&[0xFF, 0xFE]) {
        let u16s: Vec<u16> = raw[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return (String::from_utf16_lossy(&u16s), "UTF-16LE".to_string());
    }
    if raw.starts_with(&[0xFE, 0xFF]) {
        let u16s: Vec<u16> = raw[2..].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        return (String::from_utf16_lossy(&u16s), "UTF-16BE".to_string());
    }
    if let Ok(s) = std::str::from_utf8(raw) {
        return (s.to_string(), "UTF-8".to_string());
    }
    let s = String::from_utf8_lossy(raw).into_owned();
    (s, "UTF-8-LOSSY".to_string())
}

/// 파일 인코딩 자동 감지 및 디코딩 (UTF-8, UTF-8-SIG, UTF-16LE/BE, CP949/EUC-KR)
pub fn read_file_auto_encoding(filepath: &Path) -> Result<(String, String), String> {
    let raw = fs::read(filepath)
        .map_err(|e| format!("파일 읽기 오류 ({}): {}", filepath.display(), e))?;
    if raw.is_empty() {
        return Ok((String::new(), "UTF-8".to_string()));
    }

    let (decoded, enc_label) = if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
        (
            String::from_utf8_lossy(&raw[3..]).into_owned(),
            "UTF-8-SIG (with BOM)".to_string(),
        )
    } else if raw.starts_with(&[0xFF, 0xFE]) {
        (decode_utf16_le(&raw[2..]), "UTF-16LE (with BOM)".to_string())
    } else if raw.starts_with(&[0xFE, 0xFF]) {
        (decode_utf16_be(&raw[2..]), "UTF-16BE (with BOM)".to_string())
    } else if let Ok(utf8_str) = std::str::from_utf8(&raw) {
        (utf8_str.to_string(), "UTF-8".to_string())
    } else {
        let sample_len = raw.len().min(512);
        let sample = &raw[..sample_len];
        let even_nulls = sample.iter().step_by(2).filter(|&&b| b == 0).count();
        let odd_nulls = sample.iter().skip(1).step_by(2).filter(|&&b| b == 0).count();
        if odd_nulls > sample_len / 6 && even_nulls < 4 {
            (decode_utf16_le(&raw), "UTF-16LE (No BOM)".to_string())
        } else if even_nulls > sample_len / 6 && odd_nulls < 4 {
            (decode_utf16_be(&raw), "UTF-16BE (No BOM)".to_string())
        } else {
            (decode_cp949(&raw), "CP949 / EUC-KR".to_string())
        }
    };

    let clean = decoded.replace("\r\n", "\n").replace('\r', "\n");
    Ok((clean, enc_label))
}

fn contains_old_hangul_jamo(s: &str) -> bool {
    s.chars().any(|ch| {
        let cp = ch as u32;
        (0x1100..=0x11FF).contains(&cp)
            || (0x3130..=0x318F).contains(&cp)
            || (0xA960..=0xA97F).contains(&cp)
            || (0xD7B0..=0xD7FF).contains(&cp)
            || (0x302E..=0x302F).contains(&cp)
            || (0xE000..=0xF8FF).contains(&cp)
    })
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(ch);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn make_segment(
    seq_no: usize,
    sent_id_str: &str,
    part_type: &str,
    page_info: &str,
    lang: &str,
    text_raw: &str,
    morph_flat: &str,
    sense_flat: &str,
    syntax_tree: &str,
    eojeols_val: Value,
    eojeol_count_override: Option<usize>,
    norm: &Normalizer,
) -> SegmentRecord {
    let text_unicode = norm.pua_to_unicode(text_raw);
    let morph_uni = norm.pua_to_unicode(morph_flat);
    let sense_uni = norm.pua_to_unicode(sense_flat);
    let syntax_uni = norm.pua_to_unicode(syntax_tree);
    let text_nfd_norm = norm.to_nfd_norm(&text_unicode);

    let eojeol_cnt = eojeol_count_override
        .unwrap_or_else(|| text_unicode.split_whitespace().count());

    let eojeol_json_raw = serde_json::to_string(&eojeols_val).unwrap_or_else(|_| "[]".to_string());
    let eojeols_json = norm.pua_to_unicode(&eojeol_json_raw);

    SegmentRecord {
        seq_no,
        sent_id_str: sent_id_str.to_string(),
        part_type: if part_type.is_empty() { "main".to_string() } else { part_type.to_string() },
        page_info: page_info.to_string(),
        lang: if lang.is_empty() { "kor".to_string() } else { lang.to_string() },
        text_raw: text_raw.to_string(),
        text_unicode,
        text_nfd_norm,
        morph_flat: morph_uni,
        sense_flat: sense_uni,
        syntax_tree: syntax_uni,
        eojeols_json,
        eojeol_cnt,
    }
}

/// 1. 세종 형태분석 + 형태의미 + 구문분석 3중 연동 파서 (BT + BS + BG)
pub fn parse_sejong_linked_triple(
    bt_path: &Path,
    bs_path: &Path,
    bg_path: &Path,
    code: &str,
    norm: &Normalizer,
) -> Result<(Vec<SegmentRecord>, HashMap<String, String>), String> {
    let (bt_text, bt_enc) = read_file_auto_encoding(bt_path)?;
    let (bs_text, bs_enc) = read_file_auto_encoding(bs_path)?;
    let (bg_text, bg_enc) = read_file_auto_encoding(bg_path)?;

    let mut bg_records: Vec<(String, String)> = Vec::new();
    let mut in_body = false;
    let mut cur_sent: Option<String> = None;
    let mut cur_tree_lines: Vec<String> = Vec::new();
    let re_comment = Regex::new(r"^;\s*([A-Za-z0-9_\-]+)?\s*(.*)$").unwrap();

    for l in bg_text.lines() {
        let s = l.trim();
        let lower = s.to_lowercase();
        if lower.contains("<body>") {
            in_body = true;
            continue;
        }
        if lower.contains("</body>") {
            break;
        }
        if !in_body && !s.starts_with(';') && !s.starts_with('(') {
            continue;
        }
        if s.starts_with(';') {
            if let Some(prev_s) = cur_sent.take() {
                if !cur_tree_lines.is_empty() {
                    bg_records.push((prev_s, cur_tree_lines.join("\n")));
                    cur_tree_lines.clear();
                }
            }
            if let Some(cap) = re_comment.captures(s) {
                let s_txt = cap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                let s_id = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                cur_sent = Some(if !s_txt.is_empty() {
                    s_txt.to_string()
                } else {
                    s_id.to_string()
                });
            } else {
                cur_sent = Some(s.trim_start_matches(';').trim().to_string());
            }
            continue;
        }
        if cur_sent.is_some() && !s.is_empty() && !s.starts_with('<') {
            cur_tree_lines.push(l.trim_end().to_string());
        }
    }
    if let Some(prev_s) = cur_sent.take() {
        if !cur_tree_lines.is_empty() {
            bg_records.push((prev_s, cur_tree_lines.join("\n")));
        }
    }

    let mut bs_map: HashMap<String, String> = HashMap::new();
    for l in bs_text.lines() {
        if l.contains('\t') {
            let p: Vec<&str> = l.split('\t').collect();
            if p.len() >= 3 {
                let id_key = p[0].split('-').nth(1).unwrap_or(p[0].trim()).to_string();
                bs_map.insert(id_key, p[2].trim().to_string());
            }
        }
    }

    struct EjItem {
        bt_id: String,
        word: String,
        morph: String,
        sense: String,
        tag: String,
        page_info: String,
    }

    let mut eojeols: Vec<EjItem> = Vec::new();
    let mut cur_tag = "p".to_string();
    let mut cur_date = String::new();
    let mut cur_page = String::new();

    for l in bt_text.lines() {
        let s = l.trim();
        let lower = s.to_lowercase();
        if lower == "<date>" || lower == "<page>" || lower == "<head>" || lower == "<p>" {
            cur_tag = lower.trim_matches(|c| c == '<' || c == '>').to_string();
            continue;
        }
        if l.contains('\t') && !s.starts_with('<') {
            let p: Vec<&str> = l.split('\t').collect();
            if p.len() >= 3 {
                let bt_id = p[0].trim().to_string();
                let num = bt_id.split('-').nth(1).unwrap_or(&bt_id).to_string();
                let word = p[1].trim().to_string();
                let morph = p[2].trim().to_string();
                if cur_tag == "date" {
                    cur_date = word.clone();
                } else if cur_tag == "page" {
                    cur_page = word.clone();
                }
                let sense = bs_map.get(&num).cloned().unwrap_or_default();
                let page_info = if !cur_date.is_empty() || !cur_page.is_empty() {
                    format!("{} p.{}", cur_date, cur_page).trim().to_string()
                } else {
                    String::new()
                };
                eojeols.push(EjItem {
                    bt_id,
                    word,
                    morph,
                    sense,
                    tag: cur_tag.clone(),
                    page_info,
                });
            }
        }
    }

    let clean_alnum = |s: &str| -> Vec<char> {
        s.chars()
            .filter(|c| !c.is_whitespace() && !matches!(c, '`' | '\'' | '"' | '“' | '”' | '‘' | '’' | '(' | ')' | '.' | ',' | '?' | '!' | '…' | '-'))
            .collect()
    };

    let mut char_stream: Vec<char> = Vec::new();
    let mut char_to_eojeol: Vec<usize> = Vec::new();
    for (idx, e) in eojeols.iter().enumerate() {
        for ch in clean_alnum(&e.word) {
            char_stream.push(ch);
            char_to_eojeol.push(idx);
        }
    }

    let mut c_pos = 0usize;
    let mut segments = Vec::new();

    for (s_idx, (bg_sent, tree_str)) in bg_records.iter().enumerate() {
        let t = clean_alnum(bg_sent);
        let chunk_range = if eojeols.is_empty() {
            0..0
        } else if t.is_empty() {
            let e_start = char_to_eojeol
                .get(c_pos.min(char_to_eojeol.len().saturating_sub(1)))
                .copied()
                .unwrap_or(0);
            e_start..(e_start + 1).min(eojeols.len())
        } else {
            let suf_len = t.len().min(6);
            let suffix = &t[t.len() - suf_len..];
            let expected_end = c_pos + t.len();
            let search_from = (c_pos + 1).max(expected_end.saturating_sub(25)).min(char_stream.len());
            let search_to = (expected_end + 25).min(char_stream.len());

            let mut best_end: Option<usize> = None;
            let mut best_dist = usize::MAX;
            if search_to >= search_from + suf_len {
                for pos in search_from..=(search_to - suf_len) {
                    if &char_stream[pos..pos + suf_len] == suffix {
                        let cand_end = pos + suf_len;
                        let dist = cand_end.abs_diff(expected_end);
                        if dist < best_dist {
                            best_dist = dist;
                            best_end = Some(cand_end);
                        }
                    }
                }
            }
            let end_c = best_end.unwrap_or_else(|| expected_end.min(char_stream.len()));
            let e_start = char_to_eojeol
                .get(c_pos.min(char_to_eojeol.len().saturating_sub(1)))
                .copied()
                .unwrap_or(0);
            let e_end = char_to_eojeol
                .get(end_c.saturating_sub(1).max(c_pos).min(char_to_eojeol.len().saturating_sub(1)))
                .map(|&x| (x + 1).min(eojeols.len()))
                .unwrap_or(0);
            c_pos = end_c;
            e_start..e_end
        };

        let chunk = &eojeols[chunk_range];
        let reconstructed_raw = if !chunk.is_empty() {
            chunk.iter().map(|x| x.word.as_str()).collect::<Vec<_>>().join(" ")
        } else {
            bg_sent.clone()
        };
        let morph_flat = chunk
            .iter()
            .map(|x| format!("{}[{}]", x.word, x.morph))
            .collect::<Vec<_>>()
            .join(" | ");
        let sense_flat = chunk
            .iter()
            .map(|x| format!("{}[{}]", x.word, x.sense))
            .collect::<Vec<_>>()
            .join(" | ");
        let eojeol_arr: Vec<Value> = chunk
            .iter()
            .map(|x| {
                json!({
                    "id": x.bt_id,
                    "ej_id": x.bt_id,
                    "word": x.word,
                    "surface": x.word,
                    "morph": x.morph,
                    "sense": x.sense
                })
            })
            .collect();

        let p_type = chunk.first().map(|x| x.tag.as_str()).unwrap_or("p");
        let p_info = chunk.first().map(|x| x.page_info.as_str()).unwrap_or("");

        segments.push(make_segment(
            s_idx + 1,
            &format!("BR{}-S{:04}", code, s_idx + 1),
            p_type,
            p_info,
            "kor",
            &reconstructed_raw,
            &morph_flat,
            &sense_flat,
            tree_str,
            Value::Array(eojeol_arr),
            Some(chunk.len().max(1)),
            norm,
        ));
    }

    let mut enc_map = HashMap::new();
    enc_map.insert("bt_enc".to_string(), bt_enc);
    enc_map.insert("bs_enc".to_string(), bs_enc);
    enc_map.insert("bg_enc".to_string(), bg_enc);
    Ok((segments, enc_map))
}

/// 2. 세종 구문분석 단독 파일 (BG*.txt 또는 ; 주석 + (S ...) 구문트리) 파서
pub fn parse_sejong_syntax_standalone(
    filepath: &Path,
    norm: &Normalizer,
) -> Result<(Vec<SegmentRecord>, String), String> {
    let (text, enc) = read_file_auto_encoding(filepath)?;
    let fname_stem = filepath
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "BG".to_string());

    let lower = text.to_lowercase();
    let body = if let Some(pos) = lower.find("<body") {
        let after = &text[pos..];
        let start = after.find('>').map(|i| i + 1).unwrap_or(0);
        let sub = &after[start..];
        if let Some(end) = sub.to_lowercase().find("</body>") {
            &sub[..end]
        } else {
            sub
        }
    } else {
        &text
    };

    let mut segments = Vec::new();
    let mut cur_comment = String::new();
    let mut cur_tree: Vec<String> = Vec::new();
    let mut seq_no = 0usize;
    let re_comment = Regex::new(r"^;\s*([A-Za-z0-9_\-]+)?\s*(.*)$").unwrap();

    let mut flush_tree = |comment: &str, tree_lines: &[String]| {
        if tree_lines.is_empty() {
            return;
        }
        seq_no += 1;
        let tree_str = tree_lines.join("\n");
        let raw_sent = if !comment.is_empty() {
            comment.to_string()
        } else {
            let mut words = Vec::new();
            for line in tree_lines {
                if let Some(last_close) = line.rfind(')') {
                    let prefix = &line[..last_close];
                    let trimmed = prefix.trim_end_matches(')');
                    if let Some(tok) = trimmed.split_whitespace().last() {
                        if !tok.starts_with('(') {
                            words.push(tok.to_string());
                        }
                    }
                }
            }
            if words.is_empty() {
                tree_str.clone()
            } else {
                words.join(" ")
            }
        };
        segments.push(make_segment(
            seq_no,
            &format!("{}-S{:05}", fname_stem, seq_no),
            "syntax",
            &fname_stem,
            "kor",
            &raw_sent,
            "",
            "",
            &tree_str,
            json!([]),
            None,
            norm,
        ));
    };

    for raw_line in body.lines() {
        let s = raw_line.trim_end();
        let st = s.trim();
        if st.is_empty() || st.starts_with('<') {
            continue;
        }
        if st.starts_with(';') {
            flush_tree(&cur_comment, &cur_tree);
            cur_tree.clear();
            if let Some(cap) = re_comment.captures(st) {
                let s_txt = cap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                let s_id = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                cur_comment = if !s_txt.is_empty() {
                    s_txt.to_string()
                } else {
                    s_id.to_string()
                };
            } else {
                cur_comment = st.trim_start_matches(';').trim().to_string();
            }
        } else {
            cur_tree.push(s.to_string());
        }
    }
    flush_tree(&cur_comment, &cur_tree);

    Ok((segments, enc))
}

/// 3. 세종 문어 형태분석(BT*) / 형태의미분석(BS*) 단독 파일 파서
pub fn parse_sejong_morph_or_sense_tsv(
    filepath: &Path,
    is_sense: bool,
    norm: &Normalizer,
) -> Result<(Vec<SegmentRecord>, String), String> {
    let (text, enc) = read_file_auto_encoding(filepath)?;
    let fname_stem = filepath
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "BT".to_string());

    let lower = text.to_lowercase();
    let body = if let Some(pos) = lower.find("</teiheader>") {
        &text[pos + 12..]
    } else if let Some(pos) = lower.find("<body") {
        let after = &text[pos..];
        let start = after.find('>').map(|i| i + 1).unwrap_or(0);
        &after[start..]
    } else {
        &text
    };

    let mut segments = Vec::new();
    let mut seq_no = 0usize;
    let mut cur_eojeols: Vec<(String, String, String)> = Vec::new();
    let mut cur_page = String::new();
    let mut cur_tag = "p".to_string();
    let re_page = Regex::new(r#"<page[^>]*n=["']?([^"'>\s]+)"#).ok();

    let mut flush_sent = |page: &str, tag: &str, ejs: &mut Vec<(String, String, String)>| {
        if ejs.is_empty() {
            return;
        }
        seq_no += 1;
        let raw_sent = ejs.iter().map(|x| x.1.as_str()).collect::<Vec<_>>().join(" ");
        let analysis_flat = ejs
            .iter()
            .map(|x| format!("{}[{}]", x.1, x.2))
            .collect::<Vec<_>>()
            .join(" | ");
        let eojeol_json: Vec<Value> = ejs
            .iter()
            .map(|(eid, surf, ana)| {
                if is_sense {
                    json!({ "id": eid, "ej_id": eid, "word": surf, "surface": surf, "morph": "", "sense": ana })
                } else {
                    json!({ "id": eid, "ej_id": eid, "word": surf, "surface": surf, "morph": ana, "sense": "" })
                }
            })
            .collect();

        let (morph_str, sense_str) = if is_sense {
            ("", analysis_flat.as_str())
        } else {
            (analysis_flat.as_str(), "")
        };

        let page_str = if !page.is_empty() {
            page.to_string()
        } else {
            fname_stem.clone()
        };

        segments.push(make_segment(
            seq_no,
            &format!("{}-{:05}", fname_stem, seq_no),
            tag,
            &page_str,
            "kor",
            &raw_sent,
            morph_str,
            sense_str,
            "",
            Value::Array(eojeol_json),
            Some(ejs.len()),
            norm,
        ));
        ejs.clear();
    };

    for raw_line in body.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('<') {
            if let Some(ref re) = re_page {
                if let Some(cap) = re.captures(line) {
                    if let Some(m) = cap.get(1) {
                        cur_page = m.as_str().to_string();
                    }
                }
            }
            let l_tag = line.to_lowercase();
            if l_tag.starts_with("</p")
                || l_tag.starts_with("</s")
                || l_tag.starts_with("</head")
                || l_tag.starts_with("<p")
                || l_tag.starts_with("<s")
                || l_tag.starts_with("<head")
            {
                flush_sent(&cur_page, &cur_tag, &mut cur_eojeols);
                if l_tag.starts_with("<head") {
                    cur_tag = "head".to_string();
                } else if l_tag.starts_with("<p") {
                    cur_tag = "p".to_string();
                } else if l_tag.starts_with("<s") {
                    cur_tag = "s".to_string();
                }
            }
            continue;
        }

        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            let eid = parts[0].trim().to_string();
            let surf = parts[1].trim().to_string();
            let ana = parts[2].trim().to_string();
            let is_end = ana.contains("/SF")
                || surf.ends_with('.')
                || surf.ends_with('?')
                || surf.ends_with('!')
                || cur_eojeols.len() >= 35;
            cur_eojeols.push((eid, surf, ana));
            if is_end {
                flush_sent(&cur_page, &cur_tag, &mut cur_eojeols);
            }
        } else if parts.len() == 2 {
            let surf = parts[0].trim().to_string();
            let ana = parts[1].trim().to_string();
            let eid = format!("{}", cur_eojeols.len() + 1);
            let is_end = ana.contains("/SF")
                || surf.ends_with('.')
                || surf.ends_with('?')
                || surf.ends_with('!')
                || cur_eojeols.len() >= 35;
            cur_eojeols.push((eid, surf, ana));
            if is_end {
                flush_sent(&cur_page, &cur_tag, &mut cur_eojeols);
            }
        }
    }
    flush_sent(&cur_page, &cur_tag, &mut cur_eojeols);

    Ok((segments, enc))
}

/// 4. 세종 문어 원시 말뭉치 (2BA_*.txt 등 TEI/SGML 문서) — 닫히지 않은 <p>/<s> 태그도 완벽 지원
pub fn parse_sejong_tei_written(
    filepath: &Path,
    norm: &Normalizer,
) -> Result<(Vec<SegmentRecord>, String, bool), String> {
    let (text, enc) = read_file_auto_encoding(filepath)?;
    let fname_stem = filepath
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "DOC".to_string());

    let lower = text.to_lowercase();
    let body = if let Some(pos) = lower.find("</teiheader>") {
        &text[pos + 12..]
    } else if let Some(pos) = lower.find("<body") {
        let after_body = &text[pos..];
        let start_idx = after_body.find('>').map(|i| i + 1).unwrap_or(0);
        let sub = &after_body[start_idx..];
        if let Some(end) = sub.to_lowercase().find("</body>") {
            &sub[..end]
        } else {
            sub
        }
    } else {
        &text
    };

    // 만약 본문 내부가 탭으로 구분된 형태분석/의미분석 TSV 구조라면 전용 TSV 파서로 위임!
    let non_empty_lines: Vec<&str> = body
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('<'))
        .take(20)
        .collect();
    let tsv_count = non_empty_lines.iter().filter(|l| l.contains('\t')).count();
    if !non_empty_lines.is_empty() && tsv_count * 2 >= non_empty_lines.len() {
        let is_sense = non_empty_lines.iter().any(|l| l.contains("__"));
        let (segs, enc_out) = parse_sejong_morph_or_sense_tsv(filepath, is_sense, norm)?;
        return Ok((segs, enc_out, true));
    }

    let mut segments = Vec::new();
    let mut seq_no = 0usize;
    let mut cur_date = String::new();
    let mut cur_page = String::new();

    // 1차 시도: 닫힌 태그 (<date>...</date>, <page>...</page>, <head>...</head>, <p>...</p>, <s>...</s>)
    let re_date = Regex::new(r"(?is)<date\b[^>]*>(.*?)</date>").unwrap();
    let re_page = Regex::new(r"(?is)<page\b[^>]*>(.*?)</page>").unwrap();
    if let Some(cap) = re_date.captures(body) {
        cur_date = strip_tags(cap.get(1).map(|m| m.as_str()).unwrap_or(""));
    }
    if let Some(cap) = re_page.captures(body) {
        cur_page = strip_tags(cap.get(1).map(|m| m.as_str()).unwrap_or(""));
    }

    let re_blocks = Regex::new(r"(?is)<(head|p|s)\b[^>]*>(.*?)</(?:head|p|s)>").unwrap();
    for cap in re_blocks.captures_iter(body) {
        let tag = cap.get(1).map(|m| m.as_str().to_lowercase()).unwrap_or_else(|| "p".to_string());
        let inner = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let clean_raw = strip_tags(inner);
        if clean_raw.is_empty() {
            continue;
        }
        seq_no += 1;
        let page_str = if !cur_date.is_empty() || !cur_page.is_empty() {
            format!("{} p.{}", cur_date, cur_page).trim().to_string()
        } else {
            fname_stem.clone()
        };
        segments.push(make_segment(
            seq_no,
            &format!("{}-{:05}", fname_stem, seq_no),
            &tag,
            &page_str,
            "kor",
            &clean_raw,
            "",
            "",
            "",
            json!([]),
            None,
            norm,
        ));
    }

    // 2차 폴백: SGML 특성상 </p>나 </s> 닫는 태그가 생략된 경우 줄 단위 추출
    if segments.is_empty() {
        for raw_line in body.lines() {
            let trimmed = raw_line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let clean_raw = strip_tags(trimmed);
            if clean_raw.is_empty() {
                continue;
            }
            seq_no += 1;
            segments.push(make_segment(
                seq_no,
                &format!("{}-{:05}", fname_stem, seq_no),
                "p",
                &fname_stem,
                "kor",
                &clean_raw,
                "",
                "",
                "",
                json!([]),
                None,
                norm,
            ));
        }
    }

    Ok((segments, enc, false))
}

/// 5. 세종 구어 원시 말뭉치 (4CM_*.txt)
pub fn parse_sejong_spoken_raw(
    filepath: &Path,
    norm: &Normalizer,
) -> Result<(Vec<SegmentRecord>, String), String> {
    let (text, enc) = read_file_auto_encoding(filepath)?;
    let fname_stem = filepath
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "SPOKEN".to_string());

    let mut segments = Vec::new();
    let mut cur_who = String::new();
    let mut in_text = false;
    let re_u = Regex::new(r#"<u\s+who=["']?([^"'>\s]+)["']?"#).unwrap();
    let re_s = Regex::new(r#"<s\s+n=["']?([^"'>\s]+)["']?[^>]*>(.*?)</s>"#).unwrap();
    let re_phon = Regex::new(r"<phon>(.*?)</phon>").unwrap();

    for l in text.lines() {
        if l.contains("<text>") || l.contains("<body>") {
            in_text = true;
            continue;
        }
        if !in_text && !l.contains("<u ") && !l.contains("<s ") {
            continue;
        }
        if let Some(cap) = re_u.captures(l) {
            cur_who = cap.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
        }
        for cap in re_s.captures_iter(l) {
            let s_num = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let raw_inner = cap.get(2).map(|m| m.as_str()).unwrap_or("");
            let with_phon = re_phon.replace_all(raw_inner, "($1)");
            let clean_text = strip_tags(&with_phon);
            if !clean_text.is_empty() {
                let seq = segments.len() + 1;
                segments.push(make_segment(
                    seq,
                    &format!("{}-S{}", fname_stem, s_num),
                    &format!("발화({})", cur_who),
                    &format!("화자 {}", cur_who),
                    "kor",
                    &clean_text,
                    "",
                    "",
                    &format!(r#"<u who="{}"><s n="{}">{}</s></u>"#, cur_who, s_num, raw_inner),
                    json!([]),
                    None,
                    norm,
                ));
            }
        }
    }

    if segments.is_empty() {
        let (segs, enc_out, _) = parse_sejong_tei_written(filepath, norm)?;
        return Ok((segs, enc_out));
    }

    Ok((segments, enc))
}

/// 6. 세종 구어 형태분석 말뭉치 (5CT_*.txt)
pub fn parse_sejong_spoken_morph(
    filepath: &Path,
    norm: &Normalizer,
) -> Result<(Vec<SegmentRecord>, String), String> {
    let (text, enc) = read_file_auto_encoding(filepath)?;
    let fname_stem = filepath
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "5CT".to_string());

    let mut segments = Vec::new();
    let mut cur_who = String::new();
    let mut cur_sn = String::new();
    let mut cur_eojeols: Vec<(String, String, String)> = Vec::new();
    let mut in_text = false;
    let re_who = Regex::new(r#"who=["']?([^"'>\s]+)"#).unwrap();
    let re_sn = Regex::new(r#"n=["']?([^"'>\s]+)"#).unwrap();

    for l in text.lines() {
        if l.contains("<text>") || l.contains("<body>") {
            in_text = true;
            continue;
        }
        if !in_text && !l.contains('\t') {
            continue;
        }
        if l.contains('\t') {
            let parts: Vec<&str> = l.split('\t').collect();
            let line_id = parts[0].trim();
            let col1 = parts.get(1).map(|s| s.trim()).unwrap_or("");
            if col1.starts_with("<u ") {
                if let Some(cap) = re_who.captures(col1) {
                    cur_who = cap.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
                }
            } else if col1.starts_with("<s ") {
                if let Some(cap) = re_sn.captures(col1) {
                    cur_sn = cap.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
                }
                cur_eojeols.clear();
            } else if col1 == "</s>" {
                if !cur_eojeols.is_empty() {
                    let seq = segments.len() + 1;
                    let raw_s = cur_eojeols.iter().map(|x| x.1.as_str()).collect::<Vec<_>>().join(" ");
                    let m_flat = cur_eojeols
                        .iter()
                        .map(|x| format!("{}[{}]", x.1, x.2))
                        .collect::<Vec<_>>()
                        .join(" | ");
                    let ej_arr: Vec<Value> = cur_eojeols
                        .iter()
                        .map(|(id, w, m)| {
                            json!({
                                "id": id,
                                "ej_id": id,
                                "word": w,
                                "surface": w,
                                "morph": m,
                                "sense": ""
                            })
                        })
                        .collect();
                    segments.push(make_segment(
                        seq,
                        &format!("{}-S{}", fname_stem, cur_sn),
                        &format!("발화({})", cur_who),
                        &format!("화자 {}", cur_who),
                        "kor",
                        &raw_s,
                        &m_flat,
                        "",
                        "",
                        Value::Array(ej_arr),
                        Some(cur_eojeols.len()),
                        norm,
                    ));
                    cur_eojeols.clear();
                }
            } else if parts.len() >= 3 && !col1.starts_with('<') {
                cur_eojeols.push((
                    line_id.to_string(),
                    col1.to_string(),
                    parts[2].trim().to_string(),
                ));
            }
        }
    }

    if segments.is_empty() {
        return parse_sejong_morph_or_sense_tsv(filepath, false, norm);
    }

    Ok((segments, enc))
}

/// 7. 국립국어원 모두의 말뭉치 JSON 파서
pub fn parse_nikl_json(
    filepath: &Path,
    norm: &Normalizer,
) -> Result<(Vec<SegmentRecord>, String, bool), String> {
    let (text, enc) = read_file_auto_encoding(filepath)?;
    let data: Value = serde_json::from_str(&text)
        .map_err(|e| format!("JSON 파싱 오류 ({}): {}", filepath.display(), e))?;

    let mut segments = Vec::new();
    let mut has_morph = false;

    if let Some(docs) = data.get("document").and_then(|d| d.as_array()) {
        for doc in docs {
            let doc_id = doc.get("id").and_then(|v| v.as_str()).unwrap_or("");

            // 1) sentence 배열 (형태소/어휘 분석 JSON)
            if let Some(sents) = doc.get("sentence").and_then(|s| s.as_array()) {
                if !sents.is_empty() {
                    has_morph = true;
                    for sent in sents {
                        let form = sent.get("form").and_then(|v| v.as_str()).unwrap_or("").trim();
                        if form.is_empty() {
                            continue;
                        }
                        let mut words_map: BTreeMap<i64, (String, Vec<String>)> = BTreeMap::new();
                        if let Some(words) = sent.get("word").and_then(|w| w.as_array()) {
                            for w in words {
                                if let Some(wid) = w.get("id").and_then(|v| v.as_i64()) {
                                    let wf = w.get("form").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    words_map.insert(wid, (wf, Vec::new()));
                                }
                            }
                        }
                        if let Some(morphs) = sent.get("morpheme").and_then(|m| m.as_array()) {
                            for m in morphs {
                                let wid = m.get("word_id").and_then(|v| v.as_i64()).unwrap_or(0);
                                let mf = m.get("form").and_then(|v| v.as_str()).unwrap_or("");
                                let ml = m.get("label").and_then(|v| v.as_str()).unwrap_or("");
                                if let Some(entry) = words_map.get_mut(&wid) {
                                    entry.1.push(format!("{}/{}", mf, ml));
                                }
                            }
                        }

                        let mut eojeol_list = Vec::new();
                        let mut morph_flat_parts = Vec::new();
                        for (wid, (w_word, m_vec)) in &words_map {
                            let m_str = m_vec.join(" + ");
                            let wid_str = wid.to_string();
                            eojeol_list.push(json!({
                                "id": wid_str,
                                "ej_id": wid_str,
                                "word": w_word,
                                "surface": w_word,
                                "morph": m_str,
                                "sense": ""
                            }));
                            morph_flat_parts.push(format!("{}[{}]", w_word, m_str));
                        }

                        let seq = segments.len() + 1;
                        let sid = sent
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("{}-{}", doc_id, seq));

                        segments.push(make_segment(
                            seq,
                            &sid,
                            "sentence",
                            doc_id,
                            "kor",
                            form,
                            &morph_flat_parts.join(" | "),
                            "",
                            "",
                            Value::Array(eojeol_list.clone()),
                            Some(eojeol_list.len().max(1)),
                            norm,
                        ));
                    }
                    continue;
                }
            }

            // 2) paragraph 배열 (원시 문단 JSON)
            if let Some(paras) = doc.get("paragraph").and_then(|p| p.as_array()) {
                let d_meta = doc.get("metadata");
                let d_date = d_meta
                    .and_then(|m| m.get("date"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let d_topic = d_meta
                    .and_then(|m| m.get("topic"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("paragraph");
                let page_info = if !d_date.is_empty() { d_date } else { doc_id };

                for para in paras {
                    let form = para.get("form").and_then(|v| v.as_str()).unwrap_or("").trim();
                    if form.is_empty() {
                        continue;
                    }
                    let seq = segments.len() + 1;
                    let pid = para
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("{}-P{}", doc_id, seq));
                    segments.push(make_segment(
                        seq,
                        &pid,
                        d_topic,
                        page_info,
                        "kor",
                        form,
                        "",
                        "",
                        "",
                        json!([]),
                        None,
                        norm,
                    ));
                }
            }

            // 3) utterance 배열 (구어/메신저 JSON)
            if let Some(utts) = doc.get("utterance").and_then(|u| u.as_array()) {
                for utt in utts {
                    let form = utt
                        .get("form")
                        .and_then(|v| v.as_str())
                        .or_else(|| utt.get("original_form").and_then(|v| v.as_str()))
                        .unwrap_or("")
                        .trim();
                    if form.is_empty() {
                        continue;
                    }
                    let seq = segments.len() + 1;
                    let uid = utt
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("{}-U{}", doc_id, seq));
                    segments.push(make_segment(
                        seq,
                        &uid,
                        "utterance",
                        doc_id,
                        "kor",
                        form,
                        "",
                        "",
                        "",
                        json!([]),
                        None,
                        norm,
                    ));
                }
            }
        }
    }

    Ok((segments, enc, has_morph))
}

/// 8. 한국어 학습자 말뭉치 XML 파서 (<SENTENCE><s>...<word><w>...<morph pos="..."> 및 <s n=><w><m><error> 모두 지원)
pub fn parse_learner_xml(
    xml_path: &Path,
    doc_id_prefix: &str,
    norm: &Normalizer,
) -> Result<(String, String, String, Vec<SegmentRecord>, String), String> {
    let (xml_text, enc) = read_file_auto_encoding(xml_path)?;
    let mut segments = Vec::new();

    let extract_tag = |tag: &str, text: &str| -> String {
        Regex::new(&format!(r"(?is)<{0}\b[^>]*>(.*?)</{0}>", tag))
            .ok()
            .and_then(|re| re.captures(text))
            .and_then(|c| c.get(1))
            .map(|m| strip_tags(m.as_str()))
            .unwrap_or_default()
    };

    let theme = extract_tag("AssignmentTheme", &xml_text);
    let nat = extract_tag("Nationality", &xml_text);
    let grade = extract_tag("DataGrade", &xml_text);
    let page_info = if !nat.is_empty() || !grade.is_empty() {
        format!("{}/{}", nat, grade)
    } else {
        String::new()
    };

    // <Header> 등 메타데이터를 말뭉치 본문에서 제외하기 위해 <BODY> 영역 우선 추출
    let re_body = Regex::new(r"(?is)<BODY\b[^>]*>(.*?)</BODY>").unwrap();
    let body_text = re_body
        .captures(&xml_text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .unwrap_or(&xml_text);

    // 포맷 1: <P> 또는 <SENTENCE> ... <word><w>어절</w><morph pos="품사">형태소</morph></word>
    let re_p = Regex::new(r"(?is)<P\b[^>]*>(.*?)</P>").unwrap();
    let re_sentence = Regex::new(r"(?is)<SENTENCE\b[^>]*>(.*?)</SENTENCE>").unwrap();
    let re_word = Regex::new(r"(?is)<word\b[^>]*>(.*?)</word>").unwrap();
    let re_morph = Regex::new(r#"(?is)<morph\b[^>]*pos=["']([^"']*)["'][^>]*>(.*?)</morph>"#).unwrap();

    let sent_blocks: Vec<&str> = if re_p.is_match(body_text) {
        re_p.captures_iter(body_text)
            .filter_map(|c| c.get(1).map(|m| m.as_str()))
            .collect()
    } else {
        re_sentence
            .captures_iter(body_text)
            .filter_map(|c| c.get(1).map(|m| m.as_str()))
            .collect()
    };

    for (s_idx, s_block) in sent_blocks.into_iter().enumerate() {
        let mut words_data = Vec::new();
        let mut morph_flat_parts = Vec::new();
        let mut raw_words = Vec::new();

        for (w_idx, w_cap) in re_word.captures_iter(s_block).enumerate() {
            let w_block = w_cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let w_form = extract_tag("w", w_block);
            if w_form.is_empty() {
                continue;
            }
            raw_words.push(w_form.clone());

            let mut m_items = Vec::new();
            for m_cap in re_morph.captures_iter(w_block) {
                let m_pos = m_cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                let m_form = strip_tags(m_cap.get(2).map(|m| m.as_str()).unwrap_or(""));
                if !m_form.is_empty() {
                    m_items.push(format!("{}/{}", m_form, m_pos));
                }
            }
            let m_str = m_items.join(" + ");
            morph_flat_parts.push(format!("{}[{}]", w_form, m_str));
            let wid = format!("W{}", w_idx + 1);
            words_data.push(json!({
                "id": wid,
                "ej_id": wid,
                "word": w_form,
                "surface": w_form,
                "morph": m_str,
                "sense": ""
            }));
        }

        let raw_s = if !raw_words.is_empty() {
            raw_words.join(" ")
        } else {
            extract_tag("s", s_block)
        };
        if raw_s.is_empty() {
            continue;
        }

        let seq = s_idx + 1;
        segments.push(make_segment(
            seq,
            &format!("{}-S{:03}", doc_id_prefix, seq),
            "main",
            &page_info,
            "kor",
            &raw_s,
            &morph_flat_parts.join(" | "),
            "",
            "",
            Value::Array(words_data.clone()),
            Some(words_data.len().max(1)),
            norm,
        ));
    }

    Ok((theme, nat, grade, segments, enc))
}

/// 9. 옛한글 사전/자석류 XML 파서 (<c><g><m><s> 또는 훈몽자회 <item><entry><gloss>)
pub fn parse_dictionary_xml(
    filepath: &Path,
    norm: &Normalizer,
) -> Result<(Vec<SegmentRecord>, String), String> {
    let (text, enc) = read_file_auto_encoding(filepath)?;
    let fname_stem = filepath
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "DICT".to_string());

    let mut segments = Vec::new();
    let mut cur_vol = "권차".to_string();
    let mut cur_page = "1a".to_string();
    let mut c_idx = 0usize;

    let re_head = Regex::new(r#"<head\s+n=["']([^"']+)["']"#).unwrap();
    let re_page = Regex::new(r#"<page\s+n=["']([^"']+)["']"#).unwrap();
    let re_c = Regex::new(r"(?is)<c><g>(.*?)</g><m>(.*?)</m><s>(.*?)</s></c>").unwrap();

    for l in text.lines() {
        if let Some(hm) = re_head.captures(l) {
            cur_vol = hm.get(1).map(|m| m.as_str()).unwrap_or("권차").to_string();
            continue;
        }
        if let Some(pm) = re_page.captures(l) {
            cur_page = pm.get(1).map(|m| m.as_str()).unwrap_or("1a").to_string();
            continue;
        }
        if let Some(cm) = re_c.captures(l) {
            c_idx += 1;
            let hanja = cm.get(1).map(|m| m.as_str()).unwrap_or("");
            let hun_raw = cm.get(2).map(|m| m.as_str()).unwrap_or("");
            let eum_raw = cm.get(3).map(|m| m.as_str()).unwrap_or("");
            let hun_uni = norm.pua_to_unicode(hun_raw);
            let eum_uni = norm.pua_to_unicode(eum_raw);
            let raw_line = format!("{} : [훈] {} / [음] {}", hanja, hun_raw, eum_raw);
            let morph_flat = format!(
                "한자(g): {} | 훈/새김(m): {} | 동국정운/한자음(s): {}",
                hanja, hun_uni, eum_uni
            );

            segments.push(make_segment(
                c_idx,
                &format!("{}-{}:{}-{:04}", fname_stem, cur_vol, cur_page, c_idx),
                "gloss",
                &format!("{} {}", cur_vol, cur_page),
                "kor+chi",
                &raw_line,
                &morph_flat,
                "",
                l.trim(),
                json!([]),
                Some(3),
                norm,
            ));
        }
    }

    Ok((segments, enc))
}

/// 10. 옛한글 일반 문헌/언해/학습서 (<sent ...> ... </sent>)
pub fn parse_sent_xml(
    filepath: &Path,
    norm: &Normalizer,
) -> Result<(Vec<SegmentRecord>, String), String> {
    let (content, enc) = read_file_auto_encoding(filepath)?;
    let fname_stem = filepath
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "SENT".to_string());

    let mut segments = Vec::new();
    let re_sent = Regex::new(r"(?is)<sent\s+([^>]+)>(.*?)</sent>").unwrap();
    let re_type = Regex::new(r#"type=["']([^"']+)["']"#).unwrap();
    let re_lang = Regex::new(r#"lang=["']([^"']+)["']"#).unwrap();
    let re_page = Regex::new(r#"page=["']([^"']+)["']"#).unwrap();
    let re_n = Regex::new(r#"n=["']([^"']+)["']"#).unwrap();

    for (idx, cap) in re_sent.captures_iter(&content).enumerate() {
        let seq = idx + 1;
        let attrs_str = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let body = cap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
        if body.is_empty() {
            continue;
        }
        let stype_v = re_type
            .captures(attrs_str)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .unwrap_or("main");
        let slang_v = re_lang
            .captures(attrs_str)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .unwrap_or("kor");
        let spage_v = re_page
            .captures(attrs_str)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .unwrap_or("");
        let sn_str = seq.to_string();
        let sn_v = re_n
            .captures(attrs_str)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .unwrap_or(&sn_str);

        let syntax_meta = format!(
            "[문헌 메타] 유형: {} | 언어: {} | 장차: {} | 문장번호: {}",
            stype_v, slang_v, spage_v, sn_v
        );
        segments.push(make_segment(
            seq,
            &format!("{}-{}-{}", fname_stem, spage_v, sn_v),
            stype_v,
            format!("{} #{}", spage_v, sn_v).trim(),
            slang_v,
            body,
            "",
            "",
            &syntax_meta,
            json!([]),
            None,
            norm,
        ));
    }

    Ok((segments, enc))
}

/// 11. 일반 평문 및 대본
pub fn parse_plain_or_script(
    filepath: &Path,
    norm: &Normalizer,
) -> Result<(Vec<SegmentRecord>, String), String> {
    let (text, enc) = read_file_auto_encoding(filepath)?;
    let fname_stem = filepath
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "TXT".to_string());

    let mut segments = Vec::new();
    let mut cur_scene = "본문".to_string();
    let mut idx = 0usize;

    for l in text.lines() {
        let s = l.trim();
        if s.is_empty() {
            continue;
        }
        if s.starts_with('#') {
            cur_scene = s.chars().take(30).collect();
            continue;
        }
        idx += 1;
        segments.push(make_segment(
            idx,
            &format!("{}-{:05}", fname_stem, idx),
            "text",
            &cur_scene,
            "kor",
            s,
            "",
            "",
            "",
            json!([]),
            None,
            norm,
        ));
    }

    Ok((segments, enc))
}

/// SQLite 코퍼스 DB 스키마 초기화 (corpus_builder.py 및 engine/src/main.rs 와 100% 동일한 스키마)
pub fn init_db_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;

        CREATE TABLE IF NOT EXISTS documents (
            doc_id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            author TEXT,
            pub_year TEXT,
            corpus_category TEXT NOT NULL,
            annotation_layers TEXT NOT NULL,
            source_files_list TEXT NOT NULL,
            storage_mode TEXT NOT NULL DEFAULT 'ARCHIVED',
            sentence_count INTEGER NOT NULL DEFAULT 0,
            eojeol_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            speaker_info TEXT DEFAULT '',
            other_meta TEXT DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS source_files (
            file_id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_name TEXT NOT NULL,
            rel_path TEXT NOT NULL,
            file_size_bytes INTEGER NOT NULL,
            original_encoding TEXT NOT NULL,
            text_format TEXT NOT NULL,
            corpus_type TEXT NOT NULL,
            raw_notation_desc TEXT NOT NULL,
            analysis_notation_desc TEXT NOT NULL,
            linked_doc_id TEXT NOT NULL,
            db_file_name TEXT NOT NULL,
            imported_sentences INTEGER NOT NULL,
            imported_eojeols INTEGER NOT NULL,
            file_hash TEXT,
            note TEXT,
            FOREIGN KEY (linked_doc_id) REFERENCES documents(doc_id)
        );

        CREATE TABLE IF NOT EXISTS segments (
            segment_id INTEGER PRIMARY KEY AUTOINCREMENT,
            doc_id TEXT NOT NULL,
            seq_no INTEGER NOT NULL,
            sent_id_str TEXT NOT NULL,
            part_type TEXT,
            lang TEXT DEFAULT 'kor',
            page_info TEXT,
            text_raw TEXT NOT NULL,
            text_unicode TEXT NOT NULL,
            text_nfd_norm TEXT NOT NULL,
            morph_flat TEXT,
            sense_flat TEXT,
            syntax_tree TEXT,
            eojeols_json TEXT,
            is_modified INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (doc_id) REFERENCES documents(doc_id)
        );

        CREATE TABLE IF NOT EXISTS segment_edits (
            segment_id INTEGER PRIMARY KEY,
            original_text_raw TEXT NOT NULL,
            original_morph_flat TEXT,
            edited_text_raw TEXT NOT NULL,
            edited_morph_flat TEXT,
            updated_at TEXT NOT NULL,
            synced_to_source INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (segment_id) REFERENCES segments(segment_id)
        );

        CREATE INDEX IF NOT EXISTS idx_segments_doc ON segments(doc_id, seq_no);
        CREATE INDEX IF NOT EXISTS idx_segments_lang ON segments(lang);
        ",
    )
    .map_err(|e| format!("DB 스키마 초기화 실패: {}", e))?;
    let _ = conn.execute("ALTER TABLE documents ADD COLUMN speaker_info TEXT DEFAULT ''", []);
    let _ = conn.execute("ALTER TABLE documents ADD COLUMN other_meta TEXT DEFAULT ''", []);

    Ok(())
}

fn collect_corpus_files(folder: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let valid_exts: HashSet<&str> = ["txt", "xml", "json", "sgml", "tei", "csv", "tsv"]
        .into_iter()
        .collect();

    fn walk(dir: &Path, valid_exts: &HashSet<&str>, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|x| x.path())).collect();
            paths.sort();
            for p in paths {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if name.starts_with('.') {
                    continue;
                }
                if p.is_dir() {
                    walk(&p, valid_exts, out);
                } else if p.is_file() {
                    let ext = p
                        .extension()
                        .map(|e| e.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    if valid_exts.contains(ext.as_str()) {
                        out.push(p);
                    }
                }
            }
        }
    }

    if folder.is_file() {
        out.push(folder.to_path_buf());
    } else if folder.is_dir() {
        walk(folder, &valid_exts, &mut out);
    }
    out
}

fn push_log(db_builder: &Arc<Mutex<DbBuildManager>>, msg: String) {
    if let Ok(mut m) = db_builder.lock() {
        if m.progress.log_lines.len() >= 120 {
            m.progress.log_lines.remove(0);
        }
        m.progress.log_lines.push(msg);
    }
}

fn is_build_stopped(db_builder: &Arc<Mutex<DbBuildManager>>) -> bool {
    if let Ok(m) = db_builder.lock() {
        !m.progress.is_running
    } else {
        false
    }
}

/// 폴더(또는 단일 파일)를 스캔하여 SQLite 코퍼스 데이터베이스에 적재 (100% Native Rust — Python 불필요!)
pub fn build_folder_into_db_native(
    folder_path: &Path,
    db_path: &Path,
    norm: &Normalizer,
    db_builder: &Arc<Mutex<DbBuildManager>>,
) -> Result<Value, String> {
    let t0 = Instant::now();
    if !folder_path.exists() {
        return Err(format!(
            "지정한 경로가 존재하지 않습니다: {}",
            folder_path.display()
        ));
    }

    if let Some(parent) = db_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let all_files = collect_corpus_files(folder_path);
    if all_files.is_empty() {
        return Err(format!(
            "폴더 내에 처리 가능한 말뭉치 파일(.txt, .xml, .json, .sgml, .tei)이 없습니다: {}",
            folder_path.display()
        ));
    }

    let total_files_count = all_files.len();
    if let Ok(mut m) = db_builder.lock() {
        m.progress.total_files = total_files_count;
        m.progress.processed_files = 0;
        m.progress.percent = 1.0;
        m.progress.current_file = "폴더 내 말뭉치 파일 분류 중...".to_string();
    }
    push_log(
        db_builder,
        format!(
            "📂 스캔 완료: 총 {}개 말뭉치 파일 발견 ({})",
            total_files_count,
            folder_path.display()
        ),
    );

    let mut conn = Connection::open(db_path)
        .map_err(|e| format!("DB 파일 열기 실패 ({}): {}", db_path.display(), e))?;
    init_db_schema(&conn)?;

    let db_filename = db_path
        .file_name()
        .map(|n| format!("db/{}", n.to_string_lossy()))
        .unwrap_or_else(|| "db/concordance.corpdb".to_string());

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let now_str = format!("2026-11-11 (ts:{})", now_secs);

    let mut used_paths: HashSet<PathBuf> = HashSet::new();
    let mut processed_files_count = 0usize;
    let mut total_sents_added = 0usize;
    let mut total_eojeols_added = 0usize;
    let mut newly_created_doc_ids = Vec::new();
    let mut summary_items = Vec::new();

    // 문서 1건 및 세그먼트를 즉시 트랜잭션 커밋하는 클로저
    let save_doc_to_db = |conn: &mut Connection,
                          doc: &DocumentRecord,
                          sfiles: &[SourceFileRecord],
                          segs: &[SegmentRecord]|
     -> Result<(), String> {
        if segs.is_empty() {
            return Err(format!("문장이 추출되지 않았습니다: {}", doc.doc_id));
        }
        let tx = conn
            .transaction()
            .map_err(|e| format!("트랜잭션 시작 오류: {}", e))?;

        tx.execute("DELETE FROM segments WHERE doc_id = ?1", params![doc.doc_id])
            .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM source_files WHERE linked_doc_id = ?1",
            params![doc.doc_id],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM documents WHERE doc_id = ?1", params![doc.doc_id])
            .map_err(|e| e.to_string())?;

        tx.execute(
            "INSERT INTO documents (
                doc_id, title, author, pub_year, corpus_category, annotation_layers,
                source_files_list, storage_mode, sentence_count, eojeol_count, created_at,
                speaker_info, other_meta
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'ARCHIVED', ?8, ?9, ?10, ?11, ?12)",
            params![
                doc.doc_id,
                doc.title,
                doc.author,
                doc.pub_year,
                doc.corpus_category,
                doc.annotation_layers,
                doc.source_files_list,
                doc.sentence_count as i64,
                doc.eojeol_count as i64,
                now_str,
                doc.speaker_info,
                doc.other_meta
            ],
        )
        .map_err(|e| e.to_string())?;

        for sf in sfiles {
            tx.execute(
                "INSERT INTO source_files (
                    file_name, rel_path, file_size_bytes, original_encoding,
                    text_format, corpus_type, raw_notation_desc, analysis_notation_desc,
                    linked_doc_id, db_file_name, imported_sentences, imported_eojeols, file_hash, note
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, '', ?13)",
                params![
                    sf.file_name,
                    sf.rel_path,
                    sf.file_size_bytes as i64,
                    sf.original_encoding,
                    sf.text_format,
                    sf.corpus_type,
                    sf.raw_notation_desc,
                    sf.analysis_notation_desc,
                    sf.linked_doc_id,
                    sf.db_file_name,
                    sf.imported_sentences as i64,
                    sf.imported_eojeols as i64,
                    sf.note
                ],
            )
            .map_err(|e| e.to_string())?;
        }

        {
            let mut stmt_seg = tx
                .prepare(
                    "INSERT INTO segments (
                        doc_id, seq_no, sent_id_str, part_type, lang, page_info,
                        text_raw, text_unicode, text_nfd_norm,
                        morph_flat, sense_flat, syntax_tree, eojeols_json
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                )
                .map_err(|e| e.to_string())?;

            for s in segs {
                stmt_seg
                    .execute(params![
                        doc.doc_id,
                        s.seq_no as i64,
                        s.sent_id_str,
                        s.part_type,
                        s.lang,
                        s.page_info,
                        s.text_raw,
                        s.text_unicode,
                        s.text_nfd_norm,
                        s.morph_flat,
                        s.sense_flat,
                        s.syntax_tree,
                        s.eojeols_json
                    ])
                    .map_err(|e| e.to_string())?;
            }
        }

        tx.commit().map_err(|e| format!("DB 커밋 오류: {}", e))?;
        Ok(())
    };

    // 1. 세종 3중 연동 (BT* + BS* + BG*) 그룹 탐색
    let mut bt_map: HashMap<(PathBuf, String), PathBuf> = HashMap::new();
    let mut bs_map: HashMap<(PathBuf, String), PathBuf> = HashMap::new();
    let mut bg_map: HashMap<(PathBuf, String), PathBuf> = HashMap::new();

    for f in &all_files {
        let ext = f
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if ext != "txt" {
            continue;
        }
        let stem = f
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let upper = stem.to_uppercase();
        let parent = f.parent().unwrap_or(folder_path).to_path_buf();
        if upper.starts_with("BT") && stem.len() > 2 {
            bt_map.insert((parent, upper[2..].to_string()), f.clone());
        } else if upper.starts_with("BS") && stem.len() > 2 {
            bs_map.insert((parent, upper[2..].to_string()), f.clone());
        } else if upper.starts_with("BG") && stem.len() > 2 {
            bg_map.insert((parent, upper[2..].to_string()), f.clone());
        }
    }

    let mut common_keys: Vec<(PathBuf, String)> = bt_map
        .keys()
        .filter(|k| bs_map.contains_key(*k) && bg_map.contains_key(*k))
        .cloned()
        .collect();
    common_keys.sort();

    for key in common_keys {
        if is_build_stopped(db_builder) {
            break;
        }
        let bt_f = bt_map[&key].clone();
        let bs_f = bs_map[&key].clone();
        let bg_f = bg_map[&key].clone();
        used_paths.insert(bt_f.clone());
        used_paths.insert(bs_f.clone());
        used_paths.insert(bg_f.clone());

        let code = &key.1;
        let doc_id = format!("BR{}_LINKED", code);
        let bt_name = bt_f.file_name().unwrap_or_default().to_string_lossy().to_string();
        let bs_name = bs_f.file_name().unwrap_or_default().to_string_lossy().to_string();
        let bg_name = bg_f.file_name().unwrap_or_default().to_string_lossy().to_string();

        if let Ok(mut m) = db_builder.lock() {
            m.progress.current_file = format!("{} + {} + {}", bt_name, bs_name, bg_name);
            m.progress.current_doc_id = doc_id.clone();
        }

        if let Ok((segs, enc_map)) = parse_sejong_linked_triple(&bt_f, &bs_f, &bg_f, code, norm) {
            let ej_sum: usize = segs.iter().map(|s| s.eojeol_cnt).sum();
            let sfiles = vec![
                SourceFileRecord {
                    file_name: bt_name.clone(),
                    rel_path: bt_f.to_string_lossy().to_string(),
                    file_size_bytes: fs::metadata(&bt_f).map(|m| m.len()).unwrap_or(0),
                    original_encoding: enc_map.get("bt_enc").cloned().unwrap_or_default(),
                    text_format: "TEI.2 SGML/XML + TSV (3열)".to_string(),
                    corpus_type: "형태 분석 말뭉치 (세종 문어)".to_string(),
                    raw_notation_desc: "<p>, <head> 내 '어절ID \\t 어절원문 \\t 형태소분석' 표기".to_string(),
                    analysis_notation_desc: "3번째 열에 '형태소/품사 + ...' 표기".to_string(),
                    linked_doc_id: doc_id.clone(),
                    db_file_name: db_filename.clone(),
                    imported_sentences: segs.len(),
                    imported_eojeols: ej_sum,
                    note: "동일 원문 기반 3중 통합 연동".to_string(),
                },
                SourceFileRecord {
                    file_name: bs_name.clone(),
                    rel_path: bs_f.to_string_lossy().to_string(),
                    file_size_bytes: fs::metadata(&bs_f).map(|m| m.len()).unwrap_or(0),
                    original_encoding: enc_map.get("bs_enc").cloned().unwrap_or_default(),
                    text_format: "TEI.2 SGML/XML + TSV (3열)".to_string(),
                    corpus_type: "형태의미 분석 말뭉치 (세종 문어)".to_string(),
                    raw_notation_desc: "<p>, <head> 내 '어절ID \\t 어절원문 \\t 의미분석' 표기".to_string(),
                    analysis_notation_desc: "표준국어대사전 어깨번호 '__02/NNG' 표기".to_string(),
                    linked_doc_id: doc_id.clone(),
                    db_file_name: db_filename.clone(),
                    imported_sentences: segs.len(),
                    imported_eojeols: ej_sum,
                    note: "동일 원문 기반 3중 통합 연동".to_string(),
                },
                SourceFileRecord {
                    file_name: bg_name.clone(),
                    rel_path: bg_f.to_string_lossy().to_string(),
                    file_size_bytes: fs::metadata(&bg_f).map(|m| m.len()).unwrap_or(0),
                    original_encoding: enc_map.get("bg_enc").cloned().unwrap_or_default(),
                    text_format: "TEI.2 SGML/XML + S-식 구문 트리".to_string(),
                    corpus_type: "구문 분석 말뭉치 (세종 트리뱅크)".to_string(),
                    raw_notation_desc: "세미콜론('; ')으로 시작하는 주석 행에 전체 문장 원문 표기".to_string(),
                    analysis_notation_desc: "소괄호 중첩 S-식 트리 구문 표지".to_string(),
                    linked_doc_id: doc_id.clone(),
                    db_file_name: db_filename.clone(),
                    imported_sentences: segs.len(),
                    imported_eojeols: ej_sum,
                    note: "동일 원문 기반 3중 통합 연동".to_string(),
                },
            ];

            let doc = DocumentRecord {
                doc_id: doc_id.clone(),
                title: format!("세종 3중 연동 말뭉치 (BR{}) [형태·형태의미·구문]", code),
                author: "국립국어원".to_string(),
                pub_year: "1993".to_string(),
                corpus_category: "다층 통합 말뭉치 (형태+의미+구문)".to_string(),
                annotation_layers: format!(
                    "원문 + 형태소분석(BT{}) + 어휘의미분석(BS{}) + 구문분석(BG{})",
                    code, code, code
                ),
                source_files_list: format!("{}, {}, {}", bt_name, bs_name, bg_name),
                sentence_count: segs.len(),
                eojeol_count: ej_sum,
                speaker_info: String::new(),
                other_meta: "기관: 국립국어원 | 연도: 1993 | 유형: 21세기 세종계획 3중 연동".to_string(),
            };

            if save_doc_to_db(&mut conn, &doc, &sfiles, &segs).is_ok() {
                processed_files_count += 3;
                total_sents_added += segs.len();
                total_eojeols_added += ej_sum;
                newly_created_doc_ids.push(doc_id.clone());
                summary_items.push(json!({
                    "type": "세종 3중 연동",
                    "doc_id": doc_id,
                    "files": [bt_name, bs_name, bg_name],
                    "sentences": segs.len()
                }));

                let pct = (processed_files_count as f64 / total_files_count as f64) * 100.0;
                if let Ok(mut m) = db_builder.lock() {
                    m.progress.processed_files = processed_files_count;
                    m.progress.percent = pct.min(99.9);
                    m.progress.sentences_imported = total_sents_added;
                    m.progress.eojeols_imported = total_eojeols_added;
                    m.progress.newly_created_doc_ids = newly_created_doc_ids.clone();
                }
                push_log(
                    db_builder,
                    format!(
                        "✓ [세종 3중 연동] {} 완료 ({}문장, {}어절)",
                        doc.title,
                        segs.len(),
                        ej_sum
                    ),
                );
            }
        }
    }

    // 2. 학습자 말뭉치 연동 (동일 stem 의 .txt + .xml)
    let mut txt_stem_map: HashMap<(PathBuf, String), PathBuf> = HashMap::new();
    let mut xml_stem_map: HashMap<(PathBuf, String), PathBuf> = HashMap::new();
    for f in &all_files {
        if used_paths.contains(f) {
            continue;
        }
        let ext = f
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let stem = f
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let parent = f.parent().unwrap_or(folder_path).to_path_buf();
        if ext == "txt" {
            txt_stem_map.insert((parent, stem), f.clone());
        } else if ext == "xml" {
            xml_stem_map.insert((parent, stem), f.clone());
        }
    }

    let mut paired_stems: Vec<(PathBuf, String)> = txt_stem_map
        .keys()
        .filter(|k| xml_stem_map.contains_key(*k))
        .cloned()
        .collect();
    paired_stems.sort();

    let re_nonword = Regex::new(r"[^A-Za-z0-9가-힣_]+").unwrap();

    for key in paired_stems {
        if is_build_stopped(db_builder) {
            break;
        }
        let tf = txt_stem_map[&key].clone();
        let xf = xml_stem_map[&key].clone();
        let (x_head, _) = read_file_auto_encoding(&xf).unwrap_or_default();
        if !x_head.contains("<LearnerCorpus") && !x_head.contains("<SENTENCE>") {
            continue;
        }

        used_paths.insert(tf.clone());
        used_paths.insert(xf.clone());

        let base = &key.1;
        let doc_id = format!("{}_LINKED", re_nonword.replace_all(base, "_").to_uppercase());
        let tf_name = tf.file_name().unwrap_or_default().to_string_lossy().to_string();
        let xf_name = xf.file_name().unwrap_or_default().to_string_lossy().to_string();

        if let Ok(mut m) = db_builder.lock() {
            m.progress.current_file = format!("{} + {}", tf_name, xf_name);
            m.progress.current_doc_id = doc_id.clone();
        }

        if let Ok((theme, nat, grade, segs, x_enc)) = parse_learner_xml(&xf, &doc_id, norm) {
            let (_, t_enc) = read_file_auto_encoding(&tf).unwrap_or_default();
            let ej_sum: usize = segs.iter().map(|s| s.eojeol_cnt).sum();
            let sfiles = vec![
                SourceFileRecord {
                    file_name: tf_name.clone(),
                    rel_path: tf.to_string_lossy().to_string(),
                    file_size_bytes: fs::metadata(&tf).map(|m| m.len()).unwrap_or(0),
                    original_encoding: t_enc,
                    text_format: "Plain Text".to_string(),
                    corpus_type: "원시 말뭉치 (한국어 학습자)".to_string(),
                    raw_notation_desc: "문단/문장 줄바꿈 평문 텍스트".to_string(),
                    analysis_notation_desc: "없음 (연동 XML에 형태 주석 수록)".to_string(),
                    linked_doc_id: doc_id.clone(),
                    db_file_name: db_filename.clone(),
                    imported_sentences: segs.len(),
                    imported_eojeols: ej_sum,
                    note: "원문+XML 자동 통합 연동".to_string(),
                },
                SourceFileRecord {
                    file_name: xf_name.clone(),
                    rel_path: xf.to_string_lossy().to_string(),
                    file_size_bytes: fs::metadata(&xf).map(|m| m.len()).unwrap_or(0),
                    original_encoding: x_enc,
                    text_format: "LearnerCorpus XML".to_string(),
                    corpus_type: "형태 분석 말뭉치 (한국어 학습자)".to_string(),
                    raw_notation_desc: "<SENTENCE><s> 태그 내 전체 문장 및 <word><w> 어절 표기".to_string(),
                    analysis_notation_desc: "<morph pos=\"품사\"> 태그로 형태소 분석 표기".to_string(),
                    linked_doc_id: doc_id.clone(),
                    db_file_name: db_filename.clone(),
                    imported_sentences: segs.len(),
                    imported_eojeols: ej_sum,
                    note: "원문+XML 자동 통합 연동".to_string(),
                },
            ];

            let doc = DocumentRecord {
                doc_id: doc_id.clone(),
                title: format!(
                    "한국어 학습자 말뭉치 ({}: {} - {}/{})",
                    base, theme, nat, grade
                ),
                author: format!("{} 학습자 ({})", nat, grade),
                pub_year: "2020".to_string(),
                corpus_category: "학습자 말뭉치 (원문+형태 통합)".to_string(),
                annotation_layers: format!("원문({}) + 형태소분석({})", tf_name, xf_name),
                source_files_list: format!("{}, {}", tf_name, xf_name),
                sentence_count: segs.len(),
                eojeol_count: ej_sum,
                speaker_info: extract_corpus_metadata_from_file(&xf).0,
                other_meta: extract_corpus_metadata_from_file(&xf).1,
            };

            if save_doc_to_db(&mut conn, &doc, &sfiles, &segs).is_ok() {
                processed_files_count += 2;
                total_sents_added += segs.len();
                total_eojeols_added += ej_sum;
                newly_created_doc_ids.push(doc_id.clone());
                summary_items.push(json!({
                    "type": "학습자 2중 연동",
                    "doc_id": doc_id,
                    "files": [tf_name, xf_name],
                    "sentences": segs.len()
                }));

                let pct = (processed_files_count as f64 / total_files_count as f64) * 100.0;
                if let Ok(mut m) = db_builder.lock() {
                    m.progress.processed_files = processed_files_count;
                    m.progress.percent = pct.min(99.9);
                    m.progress.sentences_imported = total_sents_added;
                    m.progress.eojeols_imported = total_eojeols_added;
                    m.progress.newly_created_doc_ids = newly_created_doc_ids.clone();
                }
                push_log(
                    db_builder,
                    format!(
                        "✓ [학습자 연동] {} 완료 ({}문장, {}어절)",
                        doc.title,
                        segs.len(),
                        ej_sum
                    ),
                );
            }
        }
    }

    // 3. 개별 단일 파일 자동 분류 및 DB 적재
    for f in &all_files {
        if is_build_stopped(db_builder) {
            break;
        }
        if used_paths.contains(f) {
            continue;
        }

        let fname = f
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let stem = f
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let upper_fname = fname.to_uppercase();
        let ext = f
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        let mut doc_id = re_nonword.replace_all(&stem, "_").trim_matches('_').to_uppercase();
        if doc_id.is_empty() {
            doc_id = format!("DOC_{}", processed_files_count + 1);
        }

        if let Ok(mut m) = db_builder.lock() {
            m.progress.current_file = fname.clone();
            m.progress.current_doc_id = doc_id.clone();
        }

        let (head_text, enc_label) = match read_file_auto_encoding(f) {
            Ok(res) => res,
            Err(err) => {
                processed_files_count += 1;
                push_log(db_builder, format!("⚠️ [읽기 오류] {}: {}", fname, err));
                continue;
            }
        };
        let head_slice: String = head_text.chars().take(6000).collect();
        let lower_head = head_slice.to_lowercase();

        let mut segs: Vec<SegmentRecord> = Vec::new();
        let mut ctype = String::new();
        let mut tformat = String::new();
        let mut raw_desc = String::new();
        let mut ana_desc = String::new();
        let mut layers = String::new();
        let mut title = stem.clone();

        if head_slice.contains("<c><g>") && head_slice.contains("<m>") && head_slice.contains("<s>") {
            if let Ok((s, _)) = parse_dictionary_xml(f, norm) {
                segs = s;
                ctype = "어휘/자석 말뭉치 (옛한글)".to_string();
                tformat = "Custom XML (<c><g><m><s>)".to_string();
                raw_desc = "<g>한자, <m>훈(한양PUA/유니코드), <s>음 분리 표기".to_string();
                ana_desc = "자석(훈/음) 구조화 태그 및 한양 PUA→유니코드 변환".to_string();
                layers = "자석(한자·훈·음) + 옛한글 유니코드 변환".to_string();
                title = format!("{} (옛한글 사전/자석)", stem);
            }
        } else if head_slice.contains("<sent ") && head_slice.contains("</sent>") {
            if let Ok((s, _)) = parse_sent_xml(f, norm) {
                segs = s;
                ctype = "원시/대역 말뭉치 (옛한글)".to_string();
                tformat = "Custom XML (<doc><sent>)".to_string();
                raw_desc = "<sent type=\"...\" lang=\"kor|chi|jap\" page=\"...\"> 내 원문 표기".to_string();
                ana_desc = "장차(page), 언어(lang) 메타 속성 및 옛한글 변환".to_string();
                layers = "원문(한양PUA→유니코드 변환) + 장차/언어 메타".to_string();
                title = format!("{} (옛한글 문헌)", stem);
            }
        } else if head_slice.contains("<Korean_Learners_Corpus")
            || head_slice.contains("<LearnerCorpus")
            || head_slice.contains("<MorphemeAnnotations")
            || head_slice.contains("<SENTENCE>")
            || head_slice.contains("<SENTENCE ")
        {
            if let Ok((theme, nat, grade, s, _)) = parse_learner_xml(f, &doc_id, norm) {
                segs = s;
                ctype = "형태 분석 말뭉치 (학습자)".to_string();
                tformat = "LearnerCorpus XML".to_string();
                raw_desc = "<SENTENCE><s> 및 <word><w> 태그".to_string();
                ana_desc = "<morph pos=\"...\"> 형태소 주석".to_string();
                layers = "원문 + 형태소분석".to_string();
                title = format!("{} ({} - {}/{})", stem, theme, nat, grade);
            }
        } else if ext == "json" && head_slice.contains("\"document\"") {
            if let Ok((s, _, has_m)) = parse_nikl_json(f, norm) {
                segs = s;
                if has_m {
                    ctype = "형태 분석 말뭉치 (모두의 말뭉치 JSON)".to_string();
                    tformat = "NIKL JSON (sentence/word/morpheme)".to_string();
                    raw_desc = "sentence[].form에 원문 표기".to_string();
                    ana_desc = "sentence[].morpheme[] 배열에 형태소/품사 표기".to_string();
                    layers = "원문 + 형태소분석(JSON)".to_string();
                    title = format!("{} (형태분석 JSON)", stem);
                } else {
                    ctype = "원시 말뭉치 (모두의 말뭉치 JSON)".to_string();
                    tformat = "NIKL JSON (paragraph/utterance)".to_string();
                    raw_desc = "paragraph[].form에 문단 원문 표기".to_string();
                    ana_desc = "없음 (문서 메타데이터 포함)".to_string();
                    layers = "원문 + 문서메타".to_string();
                    title = format!("{} (원시 JSON)", stem);
                }
            }
        } else if upper_fname.starts_with("5CT_")
            || (head_slice.contains("<s n=") && head_slice.contains('\t') && head_slice.contains("<u "))
        {
            if let Ok((s, _)) = parse_sejong_spoken_morph(f, norm) {
                segs = s;
                ctype = "구어 형태 분석 말뭉치 (세종 구어)".to_string();
                tformat = "TEI.2 SGML/XML + TSV (3열)".to_string();
                raw_desc = "어절ID \\t 어절원문 \\t 형태소분석 표기".to_string();
                ana_desc = "3번째 열에 '형태소/품사 + ...' 표기".to_string();
                layers = "구어전사 원문 + 형태소분석 + 화자정보".to_string();
                title = format!("{} (세종 구어 형태분석)", stem);
            }
        } else if upper_fname.starts_with("4CM_")
            || (head_slice.contains("<u who=") && head_slice.contains("<s n="))
        {
            if let Ok((s, _)) = parse_sejong_spoken_raw(f, norm) {
                segs = s;
                ctype = "구어 전사 원시 말뭉치 (세종 구어)".to_string();
                tformat = "TEI.2 SGML/XML (<u who><s n>)".to_string();
                raw_desc = "<s n=\"번호\"> 내 구어 발화문 및 <phon> 억양 표기".to_string();
                ana_desc = "화자(who) 및 발화 번호 태그".to_string();
                layers = "구어전사 원문 + 화자/억양 표지".to_string();
                title = format!("{} (세종 구어 원시)", stem);
            }
        } else if upper_fname.starts_with("BG")
            || (head_slice.contains("(S") && head_slice.contains("\n;"))
            || (head_slice.contains("(NP") && head_slice.contains("(VP"))
        {
            // 세종 구문분석 단독 파일 (BG*.txt 등)
            if let Ok((s, _)) = parse_sejong_syntax_standalone(f, norm) {
                segs = s;
                ctype = "구문 분석 말뭉치 (세종 트리뱅크)".to_string();
                tformat = "TEI.2 SGML/XML + S-식 구문 트리".to_string();
                raw_desc = "세미콜론('; ')으로 시작하는 주석 행에 문장 원문 표기".to_string();
                ana_desc = "소괄호 중첩 S-식 트리 구문 표지".to_string();
                layers = "원문 + 구문분석(S-Expression)".to_string();
                title = format!("{} (세종 구문분석)", stem);
            }
        } else if upper_fname.starts_with("BS")
            || (head_slice.contains('\t')
                && Regex::new(r"__\d+/").map(|r| r.is_match(&head_slice)).unwrap_or(false))
        {
            // 세종 형태의미분석 단독 파일 (BS*.txt 등)
            if let Ok((s, _)) = parse_sejong_morph_or_sense_tsv(f, true, norm) {
                segs = s;
                ctype = "형태의미 분석 말뭉치 (세종 문어)".to_string();
                tformat = "TEI.2 SGML/XML + TSV (3열)".to_string();
                raw_desc = "어절ID \\t 어절원문 \\t 의미분석 표기".to_string();
                ana_desc = "표준국어대사전 어깨번호 '__01/NNG' 표기".to_string();
                layers = "원문 + 형태의미분석(어깨번호)".to_string();
                title = format!("{} (세종 형태의미분석)", stem);
            }
        } else if upper_fname.starts_with("BT")
            || (head_slice.contains('\t')
                && Regex::new(r"/[NVMAJSEUX][A-Z]{1,3}").map(|r| r.is_match(&head_slice)).unwrap_or(false))
        {
            // 세종 문어 형태분석 단독 파일 (BT*.txt 등)
            if let Ok((s, _)) = parse_sejong_morph_or_sense_tsv(f, false, norm) {
                segs = s;
                ctype = "형태 분석 말뭉치 (세종 문어)".to_string();
                tformat = "TEI.2 SGML/XML + TSV (3열)".to_string();
                raw_desc = "어절ID \\t 어절원문 \\t 형태소분석 표기".to_string();
                ana_desc = "3번째 열에 '형태소/품사 + ...' 표기".to_string();
                layers = "원문 + 형태소분석".to_string();
                title = format!("{} (세종 문어 형태분석)", stem);
            }
        } else if lower_head.contains("<tei.2>") || lower_head.contains("<body") {
            if let Ok((s, _, has_m)) = parse_sejong_tei_written(f, norm) {
                segs = s;
                if has_m {
                    ctype = "형태 분석 말뭉치 (세종 문어)".to_string();
                    tformat = "TEI.2 SGML/XML + TSV".to_string();
                    raw_desc = "<p>, <head> 내 어절원문 표기".to_string();
                    ana_desc = "형태소/품사 표기".to_string();
                    layers = "원문 + 형태소분석".to_string();
                    title = format!("{} (세종 문어 형태분석)", stem);
                } else {
                    ctype = "문어 원시 말뭉치 (세종 문어)".to_string();
                    tformat = "TEI.2 SGML/XML (<head><p><page>)".to_string();
                    raw_desc = "<p>, <head> 태그 내 문단/문장 원문 표기".to_string();
                    ana_desc = "출전 지면(<page>), 날짜(<date>) 구조 태그".to_string();
                    layers = "원문 + 지면/제목 구조태그".to_string();
                    title = format!("{} (세종 문어 원시)", stem);
                }
            }
        } else if let Ok((s, _)) = parse_plain_or_script(f, norm) {
            segs = s;
            let has_old = segs.iter().any(|x| contains_old_hangul_jamo(&x.text_unicode));
            ctype = if has_old {
                "일반 텍스트 말뭉치 (옛한글)".to_string()
            } else {
                "원시 말뭉치 (일반 평문/대본)".to_string()
            };
            tformat = "Plain Text".to_string();
            raw_desc = "줄바꿈 단위 대사/문장 평문 표기".to_string();
            ana_desc = "없음 (원시 텍스트)".to_string();
            layers = "원문".to_string();
            title = format!("{} (일반 텍스트)", stem);
        }

        if segs.is_empty() {
            processed_files_count += 1;
            push_log(
                db_builder,
                format!("⚠️ [문장 없음 건너뜀] {}: 추출된 문장이 없습니다.", fname),
            );
            continue;
        }

        let (meta_sp, meta_ot) = extract_corpus_metadata_from_file(f);
        let ej_sum: usize = segs.iter().map(|s| s.eojeol_cnt).sum();
        let sf = SourceFileRecord {
            file_name: fname.clone(),
            rel_path: f.to_string_lossy().to_string(),
            file_size_bytes: fs::metadata(f).map(|m| m.len()).unwrap_or(0),
            original_encoding: enc_label,
            text_format: tformat,
            corpus_type: ctype.clone(),
            raw_notation_desc: raw_desc,
            analysis_notation_desc: ana_desc,
            linked_doc_id: doc_id.clone(),
            db_file_name: db_filename.clone(),
            imported_sentences: segs.len(),
            imported_eojeols: ej_sum,
            note: "폴더 스캔 자동 등록 (Native Rust)".to_string(),
        };

        let doc = DocumentRecord {
            doc_id: doc_id.clone(),
            title,
            author: "미상".to_string(),
            pub_year: "".to_string(),
            corpus_category: ctype.clone(),
            annotation_layers: layers,
            source_files_list: fname.clone(),
            sentence_count: segs.len(),
            eojeol_count: ej_sum,
            speaker_info: meta_sp,
            other_meta: meta_ot,
        };

        match save_doc_to_db(&mut conn, &doc, &[sf], &segs) {
            Ok(()) => {
                processed_files_count += 1;
                total_sents_added += segs.len();
                total_eojeols_added += ej_sum;
                newly_created_doc_ids.push(doc_id.clone());
                summary_items.push(json!({
                    "type": ctype,
                    "doc_id": doc_id,
                    "files": [fname],
                    "sentences": segs.len()
                }));

                let pct = (processed_files_count as f64 / total_files_count as f64) * 100.0;
                if let Ok(mut m) = db_builder.lock() {
                    m.progress.processed_files = processed_files_count;
                    m.progress.percent = pct.min(99.9);
                    m.progress.sentences_imported = total_sents_added;
                    m.progress.eojeols_imported = total_eojeols_added;
                    m.progress.newly_created_doc_ids = newly_created_doc_ids.clone();
                }
                push_log(
                    db_builder,
                    format!(
                        "✓ [{}] {} → {}문장 ({}어절)",
                        ctype,
                        fname,
                        segs.len(),
                        ej_sum
                    ),
                );
            }
            Err(e) => {
                processed_files_count += 1;
                push_log(db_builder, format!("⚠️ [저장 오류] {}: {}", fname, e));
            }
        }
    }

    let elapsed_sec = (t0.elapsed().as_secs_f64() * 100.0).round() / 100.0;
    let summary = json!({
        "ok": true,
        "folder": folder_path.to_string_lossy(),
        "db_path": db_path.to_string_lossy(),
        "total_files_scanned": total_files_count,
        "documents_created": newly_created_doc_ids.len(),
        "sentences_imported": total_sents_added,
        "eojeols_imported": total_eojeols_added,
        "elapsed_sec": elapsed_sec,
        "items": summary_items
    });

    if let Ok(mut m) = db_builder.lock() {
        if m.progress.is_running {
            m.progress.is_running = false;
            m.progress.status = "COMPLETED".to_string();
            m.progress.percent = 100.0;
            m.progress.current_file = "완료".to_string();
            m.progress.result_summary = Some(summary.clone());
            m.progress.log_lines.push(format!(
                "[완료] 데이터베이스 생성이 성공적으로 완료되었습니다! (총 {}개 말뭉치, {}문장, {}어절, {}초)",
                newly_created_doc_ids.len(),
                total_sents_added,
                total_eojeols_added,
                elapsed_sec
            ));
        }
    }

    Ok(summary)
}
