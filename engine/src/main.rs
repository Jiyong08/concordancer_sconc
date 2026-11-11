mod builder;
mod jamo_table;

const EMBEDDED_PUA_TABLE_JSON: &str = include_str!("../../assets/data/pua_table.json");
const EMBEDDED_INDEX_HTML: &[u8] = include_bytes!("../../web/index.html");
const EMBEDDED_STYLES_CSS: &[u8] = include_bytes!("../../web/styles.css");
const EMBEDDED_HANGUL_AUTOMATA_JS: &[u8] = include_bytes!("../../web/hangul_automata.js");
const EMBEDDED_APP_JS: &[u8] = include_bytes!("../../web/app.js");

use jamo_table::{decompose_l_cp, decompose_t_cp, decompose_v_cp};
use regex::Regex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use sysinfo::System;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

#[derive(Debug, Clone, Serialize)]
struct MemoryProfile {
    total_ram_gb: f64,
    available_ram_gb: f64,
    tier_code: String,
    tier_label: String,
    sqlite_cache_mb: i64,
    sqlite_mmap_mb: i64,
    search_batch_limit: usize,
    worker_threads: usize,
}

impl MemoryProfile {
    fn detect() -> Self {
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

    fn apply_pragmas(&self, conn: &Connection) {
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

#[derive(Clone)]
pub struct Normalizer {
    pua_to_nfd: HashMap<char, String>,
    compat_to_nfd: HashMap<char, char>,
    jong_to_choseong: HashMap<char, char>,
}

#[derive(Debug, Clone)]
enum TextUnitKind {
    Syllable {
        l: Vec<char>,
        v: Vec<char>,
        t: Vec<char>,
    },
    Other(char),
}

#[derive(Debug, Clone)]
struct TextUnit {
    start_char: usize,
    end_char: usize,
    kind: TextUnitKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
enum GlobOp {
    Zero,      // 0 : 0 atomic jamos (empty/none)
    One,       // 1 : exactly 1 atomic jamo (any)
    ZeroOrOne, // 01: 0 or 1 atomic jamo
    Star,      // * : 0 or more atomic jamos (legacy)
    Plus,      // + : 1 or more atomic jamos (legacy)
    Question,  // ? : 1 atomic jamo (legacy)
    Lit(char),
}

#[derive(Debug, Clone)]
enum BracketQueryToken {
    Syllable {
        l_ops: Vec<GlobOp>,
        v_ops: Vec<GlobOp>,
        t_ops: Vec<GlobOp>,
    },
    UnitStar,
    UnitPlus,
    UnitQuestion,
    LiteralChar(char),
}

impl Normalizer {
    pub fn new(pua_json_path: &Path) -> Self {
        let mut pua_to_nfd = HashMap::new();
        let raw_json = fs::read_to_string(pua_json_path)
            .unwrap_or_else(|_| EMBEDDED_PUA_TABLE_JSON.to_string());
        if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&raw_json) {
            for (hex_key, nfd_val) in map {
                if let Ok(cp) = u32::from_str_radix(&hex_key, 16) {
                    if let Some(ch) = char::from_u32(cp) {
                        pua_to_nfd.insert(ch, nfd_val);
                    }
                }
            }
        }

        let compat_pairs: &[(char, char)] = &[
            ('ㄱ', '\u{1100}'), ('ㄲ', '\u{1101}'), ('ㄳ', '\u{11AA}'), ('ㄴ', '\u{1102}'),
            ('ㄵ', '\u{11AC}'), ('ㄶ', '\u{11AD}'), ('ㄷ', '\u{1103}'), ('ㄸ', '\u{1104}'),
            ('ㄹ', '\u{1105}'), ('ㄺ', '\u{11B0}'), ('ㄻ', '\u{11B1}'), ('ㄼ', '\u{11B2}'),
            ('ㄽ', '\u{11B3}'), ('ㄾ', '\u{11B4}'), ('ㄿ', '\u{11B5}'), ('ㅀ', '\u{11B6}'),
            ('ㅁ', '\u{1106}'), ('ㅂ', '\u{1107}'), ('ㅃ', '\u{1108}'), ('ㅄ', '\u{1121}'),
            ('ㅅ', '\u{1109}'), ('ㅆ', '\u{110A}'), ('ㅇ', '\u{110B}'), ('ㅈ', '\u{110C}'),
            ('ㅉ', '\u{110D}'), ('ㅊ', '\u{110E}'), ('ㅋ', '\u{110F}'), ('ㅌ', '\u{1110}'),
            ('ㅍ', '\u{1111}'), ('ㅎ', '\u{1112}'),
            ('ㅥ', '\u{1114}'), ('ㅦ', '\u{1115}'), ('ㅧ', '\u{11D7}'), ('ㅨ', '\u{11D9}'),
            ('ㅩ', '\u{11DC}'), ('ㅪ', '\u{111B}'), ('ㅫ', '\u{111C}'), ('ㅬ', '\u{111D}'),
            ('ㅭ', '\u{111C}'), ('ㅮ', '\u{111E}'), ('ㅯ', '\u{111F}'), ('ㅰ', '\u{1120}'),
            ('ㅱ', '\u{111D}'), ('ㅲ', '\u{1120}'), ('ㅳ', '\u{1121}'), ('ㅴ', '\u{1122}'),
            ('ㅵ', '\u{1123}'), ('ㅶ', '\u{1127}'), ('ㅷ', '\u{1129}'), ('ㅸ', '\u{112B}'),
            ('ㅹ', '\u{112C}'), ('ㅺ', '\u{112D}'), ('ㅼ', '\u{112E}'), ('ㅽ', '\u{112F}'),
            ('ㅾ', '\u{1132}'), ('ㅿ', '\u{1140}'), ('ㆀ', '\u{1147}'), ('ㆁ', '\u{114C}'),
            ('ㆂ', '\u{1152}'), ('ㆃ', '\u{1153}'), ('ㆄ', '\u{1157}'), ('ㆅ', '\u{1158}'),
            ('ㆆ', '\u{1159}'),
            ('ㅏ', '\u{1161}'), ('ㅐ', '\u{1162}'), ('ㅑ', '\u{1163}'), ('ㅒ', '\u{1164}'),
            ('ㅓ', '\u{1165}'), ('ㅔ', '\u{1166}'), ('ㅕ', '\u{1167}'), ('ㅖ', '\u{1168}'),
            ('ㅗ', '\u{1169}'), ('ㅘ', '\u{116A}'), ('ㅙ', '\u{116B}'), ('ㅚ', '\u{116C}'),
            ('ㅛ', '\u{116D}'), ('ㅜ', '\u{116E}'), ('ㅝ', '\u{116F}'), ('ㅞ', '\u{1170}'),
            ('ㅟ', '\u{1171}'), ('ㅠ', '\u{1172}'), ('ㅡ', '\u{1173}'), ('ㅢ', '\u{1174}'),
            ('ㅣ', '\u{1175}'), ('ㆍ', '\u{119E}'), ('ᆢ', '\u{11A2}'), ('ㆎ', '\u{11A1}'),
            ('ㆇ', '\u{1184}'), ('ㆈ', '\u{1185}'), ('ㆉ', '\u{1188}'), ('ㆊ', '\u{1191}'),
            ('ㆋ', '\u{1192}'), ('ㆌ', '\u{1194}'),
        ];
        let mut compat_to_nfd = HashMap::new();
        for &(k, v) in compat_pairs {
            compat_to_nfd.insert(k, v);
        }

        let jong_pairs: &[(char, char)] = &[
            ('\u{11A8}', '\u{1100}'), ('\u{11A9}', '\u{1101}'), ('\u{11AB}', '\u{1102}'),
            ('\u{11AE}', '\u{1103}'), ('\u{11AF}', '\u{1105}'), ('\u{11B7}', '\u{1106}'),
            ('\u{11B8}', '\u{1107}'), ('\u{11BA}', '\u{1109}'), ('\u{11BB}', '\u{110A}'),
            ('\u{11BC}', '\u{110B}'), ('\u{11BD}', '\u{110C}'), ('\u{11BE}', '\u{110E}'),
            ('\u{11BF}', '\u{110F}'), ('\u{11C0}', '\u{1110}'), ('\u{11C1}', '\u{1111}'),
            ('\u{11C2}', '\u{1112}'), ('\u{11E6}', '\u{112B}'), ('\u{11EB}', '\u{1140}'),
            ('\u{11F0}', '\u{114C}'), ('\u{11F9}', '\u{1159}'), ('\u{11E2}', '\u{111D}'),
            ('\u{11F4}', '\u{1157}'),
        ];
        let mut jong_to_choseong = HashMap::new();
        for &(j, c) in jong_pairs {
            jong_to_choseong.insert(j, c);
        }

        Normalizer {
            pua_to_nfd,
            compat_to_nfd,
            jong_to_choseong,
        }
    }

    fn decompose_char(&self, ch: char, out: &mut Vec<char>) {
        if let Some(mapped) = self.pua_to_nfd.get(&ch) {
            for sub in mapped.chars() {
                self.push_norm_char(sub, out);
            }
            return;
        }
        let cp = ch as u32;
        if (0xAC00..=0xD7A3).contains(&cp) {
            let s_idx = cp - 0xAC00;
            let l = 0x1100 + (s_idx / (21 * 28));
            let v = 0x1161 + ((s_idx % (21 * 28)) / 28);
            let t = s_idx % 28;
            if let Some(lc) = char::from_u32(l) {
                out.push(lc);
            }
            if let Some(vc) = char::from_u32(v) {
                out.push(vc);
            }
            if t > 0 {
                if let Some(tc) = char::from_u32(0x11A7 + t) {
                    out.push(tc);
                }
            }
        } else {
            self.push_norm_char(ch, out);
        }
    }

    fn push_norm_char(&self, ch: char, out: &mut Vec<char>) {
        let cp = ch as u32;
        if cp == 0x115F || cp == 0x1160 {
            return;
        }
        if let Some(&mapped) = self.compat_to_nfd.get(&ch) {
            out.push(mapped);
        } else {
            out.push(ch);
        }
    }

    pub fn pua_to_unicode(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for ch in text.chars() {
            if let Some(nfd) = self.pua_to_nfd.get(&ch) {
                out.push_str(nfd);
            } else {
                out.push(ch);
            }
        }
        out
    }

    pub fn to_nfd_norm(&self, text: &str) -> String {
        let mut out = Vec::with_capacity(text.len() * 2);
        for ch in text.chars() {
            self.decompose_char(ch, &mut out);
        }
        out.into_iter().collect()
    }

    fn to_nfd_norm_with_map(&self, text: &str) -> (Vec<char>, Vec<usize>) {
        let mut norm_chars = Vec::with_capacity(text.len() * 2);
        let mut char_map = Vec::with_capacity(text.len() * 2);
        for (orig_idx, ch) in text.chars().enumerate() {
            let before_len = norm_chars.len();
            self.decompose_char(ch, &mut norm_chars);
            let added = norm_chars.len() - before_len;
            for _ in 0..added {
                char_map.push(orig_idx);
            }
        }
        (norm_chars, char_map)
    }

    fn normalize_morph_str(&self, text: &str) -> String {
        let nfd = self.to_nfd_norm(text);
        let mut out = String::with_capacity(nfd.len());
        for ch in nfd.chars() {
            if let Some(&cho) = self.jong_to_choseong.get(&ch) {
                out.push(cho);
            } else {
                out.push(ch);
            }
        }
        out.replace(" + ", "+")
    }

    fn normalize_morph_for_regex(&self, text: &str) -> String {
        let composed = self.compose_modern_hangul_for_display(text);
        const CHO_COMPAT: [char; 19] = [
            'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
        ];
        const JUNG_COMPAT: [char; 21] = [
            'ㅏ', 'ㅐ', 'ㅑ', 'ㅒ', 'ㅓ', 'ㅔ', 'ㅕ', 'ㅖ', 'ㅗ', 'ㅘ', 'ㅙ', 'ㅚ', 'ㅛ', 'ㅜ', 'ㅝ', 'ㅞ', 'ㅟ', 'ㅠ', 'ㅡ', 'ㅢ', 'ㅣ',
        ];
        const JONG_COMPAT: [char; 27] = [
            'ㄱ', 'ㄲ', 'ㄳ', 'ㄴ', 'ㄵ', 'ㄶ', 'ㄷ', 'ㄹ', 'ㄺ', 'ㄻ', 'ㄼ', 'ㄽ', 'ㄾ', 'ㄿ', 'ㅀ', 'ㅁ', 'ㅂ', 'ㅄ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
        ];
        let mut out = String::with_capacity(composed.len());
        for ch in composed.chars() {
            let cp = ch as u32;
            if (0x1100..=0x1112).contains(&cp) {
                out.push(CHO_COMPAT[(cp - 0x1100) as usize]);
            } else if (0x1161..=0x1175).contains(&cp) {
                out.push(JUNG_COMPAT[(cp - 0x1161) as usize]);
            } else if (0x11A8..=0x11C2).contains(&cp) {
                out.push(JONG_COMPAT[(cp - 0x11A8) as usize]);
            } else {
                out.push(ch);
            }
        }
        out
    }

    fn compose_modern_hangul_for_display(&self, text: &str) -> String {
        let mut expanded = String::with_capacity(text.len());
        for ch in text.chars() {
            if let Some(pua_nfd) = self.pua_to_nfd.get(&ch) {
                expanded.push_str(pua_nfd);
            } else {
                expanded.push(ch);
            }
        }
        let chars: Vec<char> = expanded.chars().collect();
        let mut out = String::with_capacity(chars.len());
        let mut i = 0;
        while i < chars.len() {
            let c0 = chars[i] as u32;
            if (0x1100..=0x1112).contains(&c0) && i + 1 < chars.len() {
                let c1 = chars[i + 1] as u32;
                if (0x1161..=0x1175).contains(&c1) {
                    let l_idx = c0 - 0x1100;
                    let v_idx = c1 - 0x1161;
                    let mut t_idx = 0;
                    let mut step = 2;
                    if i + 2 < chars.len() {
                        let c2 = chars[i + 2] as u32;
                        if (0x11A8..=0x11C2).contains(&c2) {
                            t_idx = c2 - 0x11A7;
                            step = 3;
                        } else if (0x1161..=0x11A7).contains(&c2)
                            || (0xD7B0..=0xD7FB).contains(&c2)
                            || (0x11C3..=0x11FF).contains(&c2)
                        {
                            out.push(chars[i]);
                            i += 1;
                            continue;
                        }
                    }
                    if let Some(syl) = char::from_u32(0xAC00 + (l_idx * 21 + v_idx) * 28 + t_idx) {
                        out.push(syl);
                        i += step;
                        continue;
                    }
                }
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    }

    /// Converts a single Jamo character (Compatibility Jamo, Choseong, Jungseong, or Jongseong)
    /// into its canonical atomic Jamo sequence.
    fn jamo_char_to_atoms(&self, ch: char) -> Vec<char> {
        let mapped = self.compat_to_nfd.get(&ch).copied().unwrap_or(ch);
        let cp = mapped as u32;
        if let Some(s) = decompose_l_cp(cp) {
            return s.chars().collect();
        }
        if let Some(s) = decompose_v_cp(cp) {
            return s.chars().collect();
        }
        if let Some(s) = decompose_t_cp(cp) {
            return s.chars().collect();
        }
        vec![ch]
    }

    fn parse_slot_glob(&self, raw_slot: &str) -> Vec<GlobOp> {
        let mut ops = Vec::new();
        let chars: Vec<char> = raw_slot.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i].is_whitespace() || chars[i] == '\u{115F}' || chars[i] == '\u{1160}' || chars[i] == '\u{F87F}' {
                i += 1;
                continue;
            }
            // Check for "01" (0 or 1 jamo) vs "0" (0 jamos)
            if chars[i] == '0' {
                if i + 1 < chars.len() && chars[i + 1] == '1' {
                    ops.push(GlobOp::ZeroOrOne);
                    i += 2;
                    continue;
                } else {
                    ops.push(GlobOp::Zero);
                    i += 1;
                    continue;
                }
            }
            if chars[i] == '1' {
                ops.push(GlobOp::One);
                i += 1;
                continue;
            }
            match chars[i] {
                '*' => {
                    ops.push(GlobOp::Star);
                    i += 1;
                }
                '+' => {
                    ops.push(GlobOp::Plus);
                    i += 1;
                }
                '?' => {
                    ops.push(GlobOp::One);
                    i += 1;
                }
                ch => {
                    for atom in self.jamo_char_to_atoms(ch) {
                        ops.push(GlobOp::Lit(atom));
                    }
                    i += 1;
                }
            }
        }
        ops
    }

    /// Parses a query containing `[초/중/종]` bracket syntax into `Vec<BracketQueryToken>`.
    fn parse_bracket_query(&self, query: &str) -> Option<Vec<BracketQueryToken>> {
        if !query.contains('[') || !query.contains(']') || !query.contains('/') {
            return None;
        }
        let chars: Vec<char> = query.chars().collect();
        let mut tokens = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '[' {
                let mut end_br = i + 1;
                while end_br < chars.len() && chars[end_br] != ']' {
                    end_br += 1;
                }
                if end_br < chars.len() && chars[end_br] == ']' {
                    let inner: String = chars[i + 1..end_br].iter().collect();
                    let parts: Vec<&str> = inner.split('/').collect();
                    if parts.len() >= 2 {
                        let l_ops = self.parse_slot_glob(parts[0]);
                        let v_ops = self.parse_slot_glob(parts[1]);
                        let t_ops = if parts.len() >= 3 {
                            self.parse_slot_glob(parts[2])
                        } else {
                            Vec::new()
                        };
                        tokens.push(BracketQueryToken::Syllable { l_ops, v_ops, t_ops });
                        i = end_br + 1;
                        continue;
                    }
                }
            }
            let ch = chars[i];
            match ch {
                '*' => {
                    tokens.push(BracketQueryToken::UnitStar);
                    i += 1;
                }
                '+' => {
                    tokens.push(BracketQueryToken::UnitPlus);
                    i += 1;
                }
                '?' => {
                    tokens.push(BracketQueryToken::UnitQuestion);
                    i += 1;
                }
                _ => {
                    // Check if `ch` is a precomposed Hangul syllable or NFD syllable in the query
                    let cp = ch as u32;
                    if (0xAC00..=0xD7A3).contains(&cp) {
                        let s_idx = cp - 0xAC00;
                        let l_cp = 0x1100 + (s_idx / (21 * 28));
                        let v_cp = 0x1161 + ((s_idx % (21 * 28)) / 28);
                        let t_rem = s_idx % 28;
                        let l_ops = decompose_l_cp(l_cp)
                            .unwrap_or("")
                            .chars()
                            .map(GlobOp::Lit)
                            .collect();
                        let v_ops = decompose_v_cp(v_cp)
                            .unwrap_or("")
                            .chars()
                            .map(GlobOp::Lit)
                            .collect();
                        let t_ops = if t_rem > 0 {
                            decompose_t_cp(0x11A7 + t_rem)
                                .unwrap_or("")
                                .chars()
                                .map(GlobOp::Lit)
                                .collect()
                        } else {
                            Vec::new()
                        };
                        tokens.push(BracketQueryToken::Syllable { l_ops, v_ops, t_ops });
                        i += 1;
                    } else if let Some(l_str) = decompose_l_cp(cp) {
                        if i + 1 < chars.len() {
                            if let Some(v_str) = decompose_v_cp(chars[i + 1] as u32) {
                                let l_ops = l_str.chars().map(GlobOp::Lit).collect();
                                let v_ops = v_str.chars().map(GlobOp::Lit).collect();
                                let mut step = 2;
                                let mut t_ops = Vec::new();
                                if i + 2 < chars.len() {
                                    if let Some(t_str) = decompose_t_cp(chars[i + 2] as u32) {
                                        t_ops = t_str.chars().map(GlobOp::Lit).collect();
                                        step = 3;
                                    }
                                }
                                tokens.push(BracketQueryToken::Syllable { l_ops, v_ops, t_ops });
                                i += step;
                                continue;
                            }
                        }
                        tokens.push(BracketQueryToken::LiteralChar(ch));
                        i += 1;
                    } else {
                        tokens.push(BracketQueryToken::LiteralChar(ch));
                        i += 1;
                    }
                }
            }
        }
        if tokens.is_empty() {
            None
        } else {
            Some(tokens)
        }
    }

    /// Tokenizes `display_text` into `Vec<TextUnit>` where each Hangul/Old Hangul syllable is decomposed
    /// into its `(L, V, T)` atomic Jamo vectors.
    fn tokenize_text_units(&self, display_text: &str) -> Vec<TextUnit> {
        let chars: Vec<char> = display_text.chars().collect();
        let mut units = Vec::with_capacity(chars.len());
        let mut i = 0;
        while i < chars.len() {
            let ch = chars[i];
            let cp = ch as u32;
            if (0xAC00..=0xD7A3).contains(&cp) {
                let s_idx = cp - 0xAC00;
                let l_cp = 0x1100 + (s_idx / (21 * 28));
                let v_cp = 0x1161 + ((s_idx % (21 * 28)) / 28);
                let t_rem = s_idx % 28;
                let l = decompose_l_cp(l_cp).unwrap_or("").chars().collect();
                let v = decompose_v_cp(v_cp).unwrap_or("").chars().collect();
                let t = if t_rem > 0 {
                    decompose_t_cp(0x11A7 + t_rem).unwrap_or("").chars().collect()
                } else {
                    Vec::new()
                };
                let mut end_i = i + 1;
                while end_i < chars.len() && is_combining_mark(chars[end_i] as u32) {
                    end_i += 1;
                }
                units.push(TextUnit {
                    start_char: i,
                    end_char: end_i,
                    kind: TextUnitKind::Syllable { l, v, t },
                });
                i = end_i;
                continue;
            }
            if let Some(l_str) = decompose_l_cp(cp) {
                if i + 1 < chars.len() {
                    if let Some(v_str) = decompose_v_cp(chars[i + 1] as u32) {
                        let l = l_str.chars().collect();
                        let v = v_str.chars().collect();
                        let mut end_i = i + 2;
                        let mut t = Vec::new();
                        if end_i < chars.len() {
                            if let Some(t_str) = decompose_t_cp(chars[end_i] as u32) {
                                t = t_str.chars().collect();
                                end_i += 1;
                            }
                        }
                        while end_i < chars.len() && is_combining_mark(chars[end_i] as u32) {
                            end_i += 1;
                        }
                        units.push(TextUnit {
                            start_char: i,
                            end_char: end_i,
                            kind: TextUnitKind::Syllable { l, v, t },
                        });
                        i = end_i;
                        continue;
                    }
                }
            }
            units.push(TextUnit {
                start_char: i,
                end_char: i + 1,
                kind: TextUnitKind::Other(ch),
            });
            i += 1;
        }
        units
    }
}

fn is_combining_mark(cp: u32) -> bool {
    cp == 0x302E || cp == 0x302F || (0x0300..=0x030A).contains(&cp)
}

fn match_glob_ops(ops: &[GlobOp], target: &[char]) -> bool {
    if ops.is_empty() {
        return target.is_empty();
    }
    match &ops[0] {
        GlobOp::Zero => {
            // 0 jamos required: target must be empty if Zero is the only op,
            // or consume 0 jamos and check subsequent ops.
            if ops.len() == 1 {
                target.is_empty()
            } else {
                match_glob_ops(&ops[1..], target)
            }
        }
        GlobOp::One => {
            if target.is_empty() {
                return false;
            }
            // If the entire slot is `1`, it matches any non-empty slot jamo (including single or compound)
            if ops.len() == 1 {
                true
            } else {
                match_glob_ops(&ops[1..], &target[1..])
            }
        }
        GlobOp::ZeroOrOne => {
            if ops.len() == 1 {
                true // 0 or 1 slot matches empty or non-empty slot
            } else {
                // Try 0 consumed
                if match_glob_ops(&ops[1..], target) {
                    return true;
                }
                // Try 1 consumed
                if !target.is_empty() && match_glob_ops(&ops[1..], &target[1..]) {
                    return true;
                }
                false
            }
        }
        GlobOp::Star => {
            for skip in 0..=target.len() {
                if match_glob_ops(&ops[1..], &target[skip..]) {
                    return true;
                }
            }
            false
        }
        GlobOp::Plus => {
            if target.is_empty() {
                return false;
            }
            for skip in 1..=target.len() {
                if match_glob_ops(&ops[1..], &target[skip..]) {
                    return true;
                }
            }
            false
        }
        GlobOp::Question => {
            if target.is_empty() {
                return false;
            }
            if ops.len() == 1 {
                return true;
            }
            match_glob_ops(&ops[1..], &target[1..])
        }
        GlobOp::Lit(c) => {
            if target.is_empty() {
                return false;
            }
            let eq = target[0] == *c || (*c == 'ㅇ' && target[0] == 'ㆁ');
            eq && match_glob_ops(&ops[1..], &target[1..])
        }
    }
}

fn match_unit_token(tok: &BracketQueryToken, unit: &TextUnit) -> bool {
    match (tok, &unit.kind) {
        (
            BracketQueryToken::Syllable {
                l_ops,
                v_ops,
                t_ops,
            },
            TextUnitKind::Syllable { l, v, t },
        ) => {
            if !match_glob_ops(l_ops, l) {
                return false;
            }
            if !match_glob_ops(v_ops, v) {
                return false;
            }
            match_glob_ops(t_ops, t)
        }
        (BracketQueryToken::UnitQuestion, _) => true,
        (BracketQueryToken::LiteralChar(c), TextUnitKind::Other(u_c)) => c == u_c,
        _ => false,
    }
}

fn match_tokens_at(tokens: &[BracketQueryToken], units: &[TextUnit]) -> Option<usize> {
    if tokens.is_empty() {
        return Some(0);
    }
    match &tokens[0] {
        BracketQueryToken::UnitStar => {
            for skip in 0..=units.len() {
                if let Some(sub_len) = match_tokens_at(&tokens[1..], &units[skip..]) {
                    return Some(skip + sub_len);
                }
            }
            None
        }
        BracketQueryToken::UnitPlus => {
            if units.is_empty() {
                return None;
            }
            for skip in 1..=units.len() {
                if let Some(sub_len) = match_tokens_at(&tokens[1..], &units[skip..]) {
                    return Some(skip + sub_len);
                }
            }
            None
        }
        tok => {
            if units.is_empty() {
                return None;
            }
            if match_unit_token(tok, &units[0]) {
                if let Some(sub_len) = match_tokens_at(&tokens[1..], &units[1..]) {
                    return Some(1 + sub_len);
                }
            }
            None
        }
    }
}

fn find_bracket_query_match(
    tokens: &[BracketQueryToken],
    units: &[TextUnit],
) -> Option<(usize, usize)> {
    for start_idx in 0..units.len() {
        if let Some(consumed) = match_tokens_at(tokens, &units[start_idx..]) {
            if consumed > 0 {
                let sc = units[start_idx].start_char;
                let ec = units[start_idx + consumed - 1].end_char;
                return Some((sc, ec));
            }
        }
    }
    None
}

#[derive(Debug, Serialize)]
struct DocumentInfo {
    doc_id: String,
    title: String,
    author: String,
    pub_year: String,
    corpus_category: String,
    annotation_layers: String,
    source_files_list: String,
    storage_mode: String,
    sentence_count: i64,
    eojeol_count: i64,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct SourceFileInfo {
    file_id: i64,
    file_name: String,
    rel_path: String,
    file_size_bytes: i64,
    original_encoding: String,
    text_format: String,
    corpus_type: String,
    raw_notation_desc: String,
    analysis_notation_desc: String,
    linked_doc_id: String,
    target_db_file: String,
    sentence_count: i64,
    eojeol_count: i64,
    notes: String,
}

#[derive(Debug, Serialize)]
struct OverviewResponse {
    memory_profile: MemoryProfile,
    db_path: String,
    db_size_bytes: u64,
    total_documents: usize,
    total_source_files: usize,
    total_sentences: i64,
    total_eojeols: i64,
    documents: Vec<DocumentInfo>,
    source_files: Vec<SourceFileInfo>,
}

#[derive(Debug, Serialize)]
struct SearchHit {
    segment_id: i64,
    doc_id: String,
    doc_title: String,
    corpus_category: String,
    annotation_layers: String,
    seq_no: i64,
    sent_id: String,
    page_ref: String,
    speaker: String,
    lang: String,
    sent_type: String,
    left_context: String,
    node: String,
    right_context: String,
    text_raw: String,
    text_unicode: String,
    morph_flat: String,
    sense_flat: String,
    syntax_tree: String,
    has_morph: bool,
    has_sense: bool,
    has_syntax: bool,
    match_layer: String,
}

#[derive(Debug, Serialize)]
struct SearchResponse {
    query: String,
    mode: String,
    normalized_query_nfd: String,
    normalized_query_display: String,
    total_matches: usize,
    returned_count: usize,
    elapsed_ms: f64,
    cancelled: bool,
    memory_tier: String,
    hits: Vec<SearchHit>,
}

#[derive(Debug, Serialize)]
struct NeighborSentence {
    segment_id: i64,
    seq_no: i64,
    sent_id: String,
    page_ref: String,
    text_unicode: String,
}

#[derive(Debug, Serialize)]
struct EditHistoryItem {
    edit_id: i64,
    field_name: String,
    old_value: String,
    new_value: String,
    edited_at: String,
}

#[derive(Debug, Serialize)]
struct SegmentDetailResponse {
    segment_id: i64,
    doc_id: String,
    doc_title: String,
    corpus_category: String,
    annotation_layers: String,
    source_files_list: String,
    seq_no: i64,
    sent_id: String,
    speaker: String,
    page_ref: String,
    lang: String,
    sent_type: String,
    text_raw: String,
    text_unicode: String,
    text_display: String,
    morph_flat: String,
    sense_flat: String,
    syntax_tree: String,
    eojeols: serde_json::Value,
    meta: serde_json::Value,
    neighbors: Vec<NeighborSentence>,
    edits: Vec<EditHistoryItem>,
}

#[derive(Debug, Deserialize)]
struct EditSegmentRequest {
    segment_id: i64,
    text_raw: Option<String>,
    text_unicode: Option<String>,
    morph_flat: Option<String>,
    sense_flat: Option<String>,
    syntax_tree: Option<String>,
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

struct AppState {
    root_dir: PathBuf,
    db_path: PathBuf,
    web_dir: PathBuf,
    hanbatang_path: PathBuf,
    kopub_ttf_path: PathBuf,
    kopub_otf_path: PathBuf,
    normalizer: Normalizer,
    memory_profile: MemoryProfile,
    latest_search_seq: AtomicU64,
    cancel_search_seq: AtomicU64,
    db_builder: Arc<Mutex<DbBuildManager>>,
    port: u16,
}

fn parse_query_params(url: &str) -> HashMap<String, String> {
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

fn json_response<T: Serialize>(data: &T, status: u16) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(data).unwrap_or_else(|_| b"{}".to_vec());
    let header =
        Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..]).unwrap();
    let cors = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap();
    Response::from_data(body)
        .with_status_code(StatusCode(status))
        .with_header(header)
        .with_header(cors)
}

fn handle_overview(state: &AppState) -> Response<std::io::Cursor<Vec<u8>>> {
    let conn = match Connection::open(&state.db_path) {
        Ok(c) => c,
        Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 500),
    };
    state.memory_profile.apply_pragmas(&conn);

    let mut documents = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT doc_id, title, author, pub_year, corpus_category, annotation_layers,
                source_files_list, storage_mode, sentence_count, eojeol_count, created_at
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

/// Extract KWIC Left Context, Node, and Right Context by Eojeol (word) count!
/// Default `context_words` is 5 (좌우 5어절).
fn extract_kwic_by_word_count(
    text: &str,
    start_char: usize,
    end_char: usize,
    context_words: usize,
) -> (String, String, String, usize, usize, bool, bool) {
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    let s = start_char.min(total);
    let e = end_char.min(total).max(s);

    let node: String = chars[s..e].iter().collect();

    if context_words >= 999 {
        let left: String = chars[0..s].iter().collect();
        let right: String = chars[e..total].iter().collect();
        return (left, node, right, 999, 999, false, false);
    }

    // Left Context: keep any attached prefix of the matched word + up to `context_words` preceding eojeols
    let mut left_idx = s;
    // 1) If s is inside a word (not preceded by whitespace), move back to the start of the current word
    while left_idx > 0 && !chars[left_idx - 1].is_whitespace() {
        left_idx -= 1;
    }
    // 2) Now count `context_words` preceding words backwards
    let mut words_counted = 0usize;
    while left_idx > 0 && words_counted < context_words {
        // Skip whitespace before the word
        let mut ws_pos = left_idx;
        while ws_pos > 0 && chars[ws_pos - 1].is_whitespace() {
            ws_pos -= 1;
        }
        if ws_pos == 0 {
            left_idx = 0;
            break;
        }
        // Skip the word characters
        while ws_pos > 0 && !chars[ws_pos - 1].is_whitespace() {
            ws_pos -= 1;
        }
        left_idx = ws_pos;
        words_counted += 1;
    }

    let left: String = chars[left_idx..s].iter().collect();
    let left_truncated = left_idx > 0;

    // Right Context: keep any attached suffix of the matched word + up to `context_words` following eojeols
    let mut right_idx = e;
    while right_idx < total && !chars[right_idx].is_whitespace() {
        right_idx += 1;
    }
    let mut r_words_counted = 0usize;
    while right_idx < total && r_words_counted < context_words {
        let mut ws_pos = right_idx;
        while ws_pos < total && chars[ws_pos].is_whitespace() {
            ws_pos += 1;
        }
        if ws_pos == total {
            right_idx = total;
            break;
        }
        while ws_pos < total && !chars[ws_pos].is_whitespace() {
            ws_pos += 1;
        }
        right_idx = ws_pos;
        r_words_counted += 1;
    }

    let right: String = chars[e..right_idx].iter().collect();
    let right_truncated = right_idx < total;

    (
        left,
        node,
        right,
        words_counted,
        r_words_counted,
        left_truncated,
        right_truncated,
    )
}

fn pad_kwic_context(
    conn: &Connection,
    doc_id: &str,
    seq_no: i64,
    mut left: String,
    node: String,
    mut right: String,
    words_counted: usize,
    r_words_counted: usize,
    context_words: usize,
    left_truncated: bool,
    right_truncated: bool,
) -> (String, String, String) {
    if context_words >= 999 {
        return (left, node, right);
    }

    // 1) Left Context padding across previous segments
    let mut left_has_more = left_truncated;
    if words_counted < context_words {
        let mut needed = context_words - words_counted;
        let mut prev_words = Vec::new();
        if let Ok(mut stmt_prev) = conn.prepare(
            "SELECT text_unicode FROM segments WHERE doc_id = ? AND seq_no < ? ORDER BY seq_no DESC LIMIT 3",
        ) {
            if let Ok(mut rows) = stmt_prev.query(params![doc_id, seq_no]) {
                while let Ok(Some(row)) = rows.next() {
                    let prev_text: String = row.get(0).unwrap_or_default();
                    let words: Vec<&str> = prev_text.split_whitespace().collect();
                    if words.is_empty() {
                        continue;
                    }
                    if words.len() <= needed {
                        needed -= words.len();
                        for w in words.into_iter().rev() {
                            prev_words.push(w.to_string());
                        }
                    } else {
                        let take_start = words.len() - needed;
                        for w in words[take_start..].iter().rev() {
                            prev_words.push((*w).to_string());
                        }
                        left_has_more = true;
                        break;
                    }
                    if needed == 0 {
                        break;
                    }
                }
            }
        }
        if !prev_words.is_empty() {
            prev_words.reverse();
            let prepended = prev_words.join(" ");
            let trimmed = left.trim_start();
            left = if trimmed.is_empty() {
                prepended
            } else {
                format!("{} {}", prepended, trimmed)
            };
        }
    }
    if left_has_more {
        left = format!("…{}", left.trim_start());
    }

    // 2) Right Context padding across succeeding segments
    let mut right_has_more = right_truncated;
    if r_words_counted < context_words {
        let mut needed = context_words - r_words_counted;
        let mut next_words = Vec::new();
        if let Ok(mut stmt_next) = conn.prepare(
            "SELECT text_unicode FROM segments WHERE doc_id = ? AND seq_no > ? ORDER BY seq_no ASC LIMIT 3",
        ) {
            if let Ok(mut rows) = stmt_next.query(params![doc_id, seq_no]) {
                while let Ok(Some(row)) = rows.next() {
                    let next_text: String = row.get(0).unwrap_or_default();
                    let words: Vec<&str> = next_text.split_whitespace().collect();
                    if words.is_empty() {
                        continue;
                    }
                    if words.len() <= needed {
                        needed -= words.len();
                        for w in words {
                            next_words.push(w.to_string());
                        }
                    } else {
                        for w in &words[..needed] {
                            next_words.push((*w).to_string());
                        }
                        right_has_more = true;
                        break;
                    }
                    if needed == 0 {
                        break;
                    }
                }
            }
        }
        if !next_words.is_empty() {
            let appended = next_words.join(" ");
            let trimmed = right.trim_end();
            right = if trimmed.is_empty() {
                appended
            } else {
                format!("{} {}", trimmed, appended)
            };
        }
    }
    if right_has_more {
        right = format!("{}…", right.trim_end());
    }

    (left, node, right)
}

#[allow(dead_code)]
fn find_char_subslice(haystack: &[char], needle: &[char]) -> Option<(usize, usize)> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    let first = needle[0];
    let max_start = haystack.len() - needle.len();
    for i in 0..=max_start {
        if haystack[i] == first && &haystack[i..i + needle.len()] == needle {
            return Some((i, i + needle.len()));
        }
    }
    None
}

fn find_all_char_subslices(haystack: &[char], needle: &[char]) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    if needle.is_empty() || needle.len() > haystack.len() {
        return matches;
    }
    let first = needle[0];
    let max_start = haystack.len() - needle.len();
    for i in 0..=max_start {
        if haystack[i] == first && &haystack[i..i + needle.len()] == needle {
            matches.push((i, i + needle.len()));
        }
    }
    matches
}

fn is_jongseong_jamo(c: char) -> bool {
    matches!(c, '\u{11A8}'..='\u{11FF}' | '\u{D7CB}'..='\u{D7FB}')
}

fn is_choseong_only_query(q_norm: &[char]) -> bool {
    !q_norm.is_empty()
        && q_norm.len() >= 2
        && q_norm.iter().all(|&c| {
            let cp = c as u32;
            (0x1100..=0x115E).contains(&cp) || (0xA960..=0xA97C).contains(&cp)
        })
}

fn handle_search_cancel(state: &AppState) -> Response<std::io::Cursor<Vec<u8>>> {
    let cur = state.latest_search_seq.load(Ordering::Relaxed);
    state.cancel_search_seq.store(cur, Ordering::Relaxed);
    json_response(&serde_json::json!({ "ok": true, "cancelled_seq": cur }), 200)
}


/// Expands `[초/중/종]` bracket expressions into standard Regular Expression character classes
/// so that regex and jamo bracket queries can be executed seamlessly together.
fn expand_bracket_syntax_to_regex(query: &str) -> String {
    if !query.contains('[') || !query.contains('/') || !query.contains(']') {
        return query.to_string();
    }
    const CHO_LIST: [char; 19] = [
        'ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ',
    ];
    const JUNG_LIST: [char; 21] = [
        'ㅏ', 'ㅐ', 'ㅑ', 'ㅒ', 'ㅓ', 'ㅔ', 'ㅕ', 'ㅖ', 'ㅗ', 'ㅘ', 'ㅙ', 'ㅚ', 'ㅛ', 'ㅜ', 'ㅝ', 'ㅞ', 'ㅟ', 'ㅠ', 'ㅡ', 'ㅢ', 'ㅣ',
    ];
    const JONG_LIST: [&str; 28] = [
        "", "ㄱ", "ㄲ", "ㄳ", "ㄴ", "ㄵ", "ㄶ", "ㄷ", "ㄹ", "ㄺ", "ㄻ", "ㄼ", "ㄽ", "ㄾ", "ㄿ", "ㅀ", "ㅁ", "ㅂ", "ㅄ", "ㅅ", "ㅆ", "ㅇ", "ㅈ", "ㅊ", "ㅋ", "ㅌ", "ㅍ", "ㅎ",
    ];

    let chars: Vec<char> = query.chars().collect();
    let mut out = String::with_capacity(query.len() * 2);
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            let mut end_br = i + 1;
            while end_br < chars.len() && chars[end_br] != ']' {
                end_br += 1;
            }
            if end_br < chars.len() && chars[end_br] == ']' {
                let inner: String = chars[i + 1..end_br].iter().collect();
                if inner.contains('/') {
                    let parts: Vec<&str> = inner.split('/').collect();
                    let cho_part = parts[0].trim();
                    let jung_part = parts.get(1).map(|s| s.trim()).unwrap_or("");
                    let jong_part = parts.get(2).map(|s| s.trim()).unwrap_or("0");

                    // Determine Cho indices
                    let cho_indices: Vec<usize> = if cho_part == "1" || cho_part == "?" || cho_part == "*" {
                        (0..19).collect()
                    } else {
                        CHO_LIST.iter().enumerate().filter(|(_, &c)| cho_part.contains(c)).map(|(idx, _)| idx).collect()
                    };

                    // Determine Jung indices
                    let jung_indices: Vec<usize> = if jung_part == "1" || jung_part == "?" || jung_part == "*" {
                        (0..21).collect()
                    } else {
                        JUNG_LIST.iter().enumerate().filter(|(_, &c)| jung_part.contains(c)).map(|(idx, _)| idx).collect()
                    };

                    // Determine Jong indices
                    let jong_indices: Vec<usize> = if jong_part == "0" {
                        vec![0]
                    } else if jong_part == "1" || jong_part == "?" {
                        (1..28).collect()
                    } else if jong_part == "01" || jong_part == "*" {
                        (0..28).collect()
                    } else {
                        JONG_LIST.iter().enumerate().filter(|(_, &s)| !s.is_empty() && jong_part.contains(s)).map(|(idx, _)| idx).collect()
                    };

                    let mut syllables = Vec::new();
                    for &l in &cho_indices {
                        for &v in &jung_indices {
                            for &t in &jong_indices {
                                let code = 0xAC00 + (l * 21 + v) * 28 + t;
                                if let Some(ch) = char::from_u32(code as u32) {
                                    syllables.push(ch);
                                }
                            }
                        }
                    }

                    if !syllables.is_empty() {
                        if syllables.len() == 1 {
                            out.push(syllables[0]);
                        } else if syllables.len() > 100 {
                            // If very large set (e.g. [1/1/0]), compress or emit bracket class
                            out.push('[');
                            for ch in syllables {
                                out.push(ch);
                            }
                            out.push(']');
                        } else {
                            out.push('[');
                            for ch in syllables {
                                out.push(ch);
                            }
                            out.push(']');
                        }
                        i = end_br + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn check_word_boundary(
    display_chars: &[char],
    orig_start: usize,
    orig_end: usize,
    has_leading_space: bool,
    has_trailing_space: bool,
) -> bool {
    if has_leading_space && orig_start > 0 {
        if let Some(&prev_c) = display_chars.get(orig_start - 1) {
            if !prev_c.is_whitespace() && !prev_c.is_ascii_punctuation() {
                return false;
            }
        }
    }
    if has_trailing_space && orig_end < display_chars.len() {
        if let Some(&next_c) = display_chars.get(orig_end) {
            if !next_c.is_whitespace() && !next_c.is_ascii_punctuation() {
                return false;
            }
        }
    }
    true
}

/// Checks if the query contains regular expression metacharacters outside `[초/중/종]` Jamo brackets.
fn has_regex_syntax(query: &str) -> bool {
    if query.is_empty() {
        return false;
    }
    // Remove [초/중/종] jamo brackets first
    let chars: Vec<char> = query.chars().collect();
    let mut stripped = String::with_capacity(query.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            let mut end_br = i + 1;
            while end_br < chars.len() && chars[end_br] != ']' {
                end_br += 1;
            }
            if end_br < chars.len() && chars[end_br] == ']' {
                let inner: String = chars[i + 1..end_br].iter().collect();
                if inner.contains('/') {
                    i = end_br + 1;
                    continue;
                }
            }
        }
        stripped.push(chars[i]);
        i += 1;
    }
    stripped.contains(".{")
        || stripped.contains(".*")
        || stripped.contains(".+")
        || stripped.contains(".?")
        || stripped.contains("\\d")
        || stripped.contains("\\s")
        || stripped.contains("\\S")
        || stripped.contains("\\w")
        || stripped.contains("\\b")
        || stripped.contains('^')
        || stripped.contains('$')
        || stripped.contains('|')
        || (stripped.contains('{') && stripped.contains('}'))
        || (stripped.contains('[') && stripped.contains(']'))
        || (stripped.contains('(') && stripped.contains(')'))
}

/// Determines whether a query requires matching at the start of an eojeol (`어절 시작부분 매칭`)
/// when in `eojeol` or `sentence` search unit mode.
fn query_requires_eojeol_start(core_query: &str) -> bool {
    let q = core_query.trim();
    if q.is_empty() {
        return false;
    }
    if q.starts_with('^') {
        return true;
    }
    // Explicit leading wildcards or tag/sense delimiters allow mid-eojeol matching
    if q.starts_with(".*")
        || q.starts_with(".+")
        || q.starts_with(".?")
        || q.starts_with('*')
        || q.starts_with('?')
        || q.starts_with("\\S*")
        || q.starts_with("\\w*")
        || q.starts_with('/')
        || q.starts_with('_')
        || q.starts_with('+')
        || q.starts_with('-')
        || q.starts_with(')')
        || q.starts_with('(')
    {
        return false;
    }
    // Pure POS / syntax category tag (e.g., NNG, VV, ETM, NP_SBJ)
    if q.chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_' || c == '/' || c == '+')
    {
        return false;
    }
    true
}

fn is_leading_quote_or_bracket(c: char) -> bool {
    matches!(
        c,
        '"' | '\'' | '“' | '”' | '‘' | '’' | '(' | '<' | '{' | '「' | '『' | '《' | '〈' | '【' | '〔'
    )
}

fn strip_leading_surface_quotes(s: &str) -> (&str, usize) {
    let mut byte_idx = 0;
    let mut skipped_chars = 0;
    for (b_i, ch) in s.char_indices() {
        if is_leading_quote_or_bracket(ch) {
            byte_idx = b_i + ch.len_utf8();
            skipped_chars += 1;
        } else {
            break;
        }
    }
    (&s[byte_idx..], skipped_chars)
}

fn strip_leading_morph_quotes(s: &str) -> &str {
    let mut cur = s.trim_start();
    loop {
        if let Some(plus_idx) = cur.find('+') {
            let first_m = cur[..plus_idx].trim();
            if first_m.ends_with("/SS") || first_m.ends_with("/SP") || first_m.ends_with("/SO") {
                cur = cur[plus_idx + 1..].trim_start();
                continue;
            }
        }
        break;
    }
    cur
}

fn split_eojeol_word_and_analysis(ea: &str) -> (&str, &str) {
    if let Some(open_idx) = ea.find('[') {
        if ea.ends_with(']') && open_idx + 1 < ea.len() {
            return (&ea[..open_idx], &ea[open_idx + 1..ea.len() - 1]);
        }
    }
    (ea, ea)
}

fn compute_display_word_spans(display_text: &str) -> Vec<(usize, usize, &str)> {
    let mut spans = Vec::new();
    let mut cur_char = 0usize;
    let mut in_word = false;
    let mut word_start_byte = 0usize;
    let mut word_start_char = 0usize;

    for (b_idx, ch) in display_text.char_indices() {
        if ch.is_whitespace() {
            if in_word {
                spans.push((
                    word_start_char,
                    cur_char,
                    &display_text[word_start_byte..b_idx],
                ));
                in_word = false;
            }
        } else if !in_word {
            in_word = true;
            word_start_byte = b_idx;
            word_start_char = cur_char;
        }
        cur_char += 1;
    }
    if in_word {
        spans.push((
            word_start_char,
            cur_char,
            &display_text[word_start_byte..],
        ));
    }
    spans
}

fn find_eojeol_span_in_display(
    display_text: &str,
    word_spans: &[(usize, usize, &str)],
    start_eojeol_idx: usize,
    end_eojeol_idx: usize,
    first_ea_word: &str,
) -> (usize, usize) {
    if !word_spans.is_empty() {
        let s_idx = start_eojeol_idx.min(word_spans.len() - 1);
        let e_idx = end_eojeol_idx.max(s_idx).min(word_spans.len() - 1);
        let (_, _, w_at_s) = word_spans[s_idx];
        if first_ea_word.is_empty()
            || w_at_s == first_ea_word
            || w_at_s.contains(first_ea_word)
            || first_ea_word.contains(w_at_s)
        {
            return (word_spans[s_idx].0, word_spans[e_idx].1);
        }
        // Search nearby word_spans if whitespace splitting shifted indices
        for (idx, &(sc, _, w_str)) in word_spans.iter().enumerate() {
            if !first_ea_word.is_empty()
                && (w_str == first_ea_word
                    || w_str.contains(first_ea_word)
                    || first_ea_word.contains(w_str))
            {
                let span_len = end_eojeol_idx.saturating_sub(start_eojeol_idx);
                let target_e = (idx + span_len).min(word_spans.len() - 1);
                return (sc, word_spans[target_e].1);
            }
        }
        return (word_spans[s_idx].0, word_spans[e_idx].1);
    }
    if !first_ea_word.is_empty() {
        if let Some(byte_pos) = display_text.find(first_ea_word) {
            let sc = display_text[..byte_pos].chars().count();
            let ec = sc + first_ea_word.chars().count();
            return (sc, ec);
        }
    }
    (0, display_text.chars().count().min(6).max(1))
}

struct SearchQueryContext<'a> {
    core_query: &'a str,
    search_unit: &'a str, // "eojeol" | "sentence" | "text"
    req_eojeol_start: bool,
    has_leading_space: bool,
    has_trailing_space: bool,
    use_surface: bool,
    use_morph: bool,
    use_sense: bool,
    use_syntax: bool,
    use_text: bool,
    use_jamo: bool,
    use_regex: bool,
    norm_query_chars: &'a [char],
    norm_morph_query: &'a str,
    bracket_tokens: Option<&'a [BracketQueryToken]>,
    compiled_regex: Option<&'a Regex>,
    anchored_regex: Option<&'a Regex>,
}

fn match_analysis_layer(
    state: &AppState,
    qctx: &SearchQueryContext,
    display_text: &str,
    word_spans: &[(usize, usize, &str)],
    flat_str: &str,
    layer_label: &'static str,
    layer_jamo_label: &'static str,
    layer_regex_label: &'static str,
) -> Option<(usize, usize, &'static str)> {
    if flat_str.is_empty() {
        return None;
    }
    let raw_eas: Vec<&str> = flat_str.split(" | ").collect();
    if raw_eas.is_empty() {
        return None;
    }

    // Precompute normalized eojeol structures: pure morphological/sense analysis per eojeol
    struct EojeolItem {
        raw_word: String,
        mo_clean: String,
        norm_mo: String,
    }

    let mut items: Vec<EojeolItem> = Vec::with_capacity(raw_eas.len());
    for ea in &raw_eas {
        let (w_raw, m_raw) = split_eojeol_word_and_analysis(ea);
        let w_disp = state.normalizer.compose_modern_hangul_for_display(w_raw);
        let mo_compat = state.normalizer.normalize_morph_for_regex(m_raw);
        let mo_clean = strip_leading_morph_quotes(&mo_compat).to_string();
        let norm_mo = state.normalizer.normalize_morph_str(&mo_clean);
        items.push(EojeolItem {
            raw_word: w_disp,
            mo_clean,
            norm_mo,
        });
    }

    let req_start = qctx.req_eojeol_start && qctx.search_unit != "text";

    // Helper closure to test a single eojeol strictly within its own analysis (`mo_clean`)
    let check_single_eojeol = |item: &EojeolItem| -> Option<&'static str> {
        // 1) Regex match (takes priority when query has regex syntax or regex is enabled)
        if qctx.use_regex {
            if req_start {
                if let Some(re_anc) = qctx.anchored_regex {
                    if re_anc.is_match(&item.mo_clean) {
                        return Some(layer_regex_label);
                    }
                }
            } else if let Some(re) = qctx.compiled_regex {
                if re.is_match(&item.mo_clean) {
                    return Some(layer_regex_label);
                }
            }
        }
        // 2) Jamo bracket match (when not a regex-quantifier query)
        if (qctx.use_jamo || qctx.bracket_tokens.is_some()) && !qctx.use_regex {
            if let Some(b_tokens) = qctx.bracket_tokens {
                let mo_units = state.normalizer.tokenize_text_units(&item.mo_clean);
                if let Some((sc, _)) = find_bracket_query_match(b_tokens, &mo_units) {
                    if !req_start || sc == 0 {
                        return Some(layer_jamo_label);
                    }
                }
            }
        }
        // 3) Plain text / normalized morph match
        if qctx.use_text && !qctx.use_regex && qctx.bracket_tokens.is_none() {
            let cq_clean = qctx.core_query.replace(" + ", "+");
            if req_start {
                if item.mo_clean.starts_with(qctx.core_query)
                    || item.mo_clean.replace(" + ", "+").starts_with(&cq_clean)
                    || item.norm_mo.starts_with(qctx.norm_morph_query)
                {
                    return Some(layer_label);
                }
            } else if item.mo_clean.contains(qctx.core_query)
                || item.mo_clean.replace(" + ", "+").contains(&cq_clean)
                || item.norm_mo.contains(qctx.norm_morph_query)
            {
                return Some(layer_label);
            }
        }
        None
    };

    // 1. Check single-eojeol matches first
    for (idx, item) in items.iter().enumerate() {
        if let Some(lbl) = check_single_eojeol(item) {
            let (sc, ec) =
                find_eojeol_span_in_display(display_text, word_spans, idx, idx, &item.raw_word);
            return Some((sc, ec, lbl));
        }
    }

    // If search_unit is strictly "eojeol", do not cross eojeol boundaries!
    if qctx.search_unit == "eojeol" || items.len() < 2 {
        return None;
    }

    // 2. Multi-eojeol matching within the sentence ("sentence" or "text" mode)
    // Build pure morpheme sentence stream `mo_0 | mo_1 | ...` with eojeol index mapping
    for start_idx in 0..items.len() {
        let mut mo_tail = String::new();
        let mut eojeol_end_byte_offsets: Vec<(usize, usize)> = Vec::new();
        for j in start_idx..items.len() {
            if j > start_idx {
                mo_tail.push_str(" | ");
            }
            mo_tail.push_str(&items[j].mo_clean);
            let b_end = mo_tail.len();
            eojeol_end_byte_offsets.push((j, b_end));
        }

        // 2-1) Regex multi-eojeol match
        if qctx.use_regex {
            let re_to_use = if req_start {
                qctx.anchored_regex
            } else {
                qctx.compiled_regex
            };
            if let Some(re) = re_to_use {
                if let Some(m) = re.find(&mo_tail) {
                    if !req_start || m.start() == 0 {
                        let end_ej = eojeol_end_byte_offsets
                            .iter()
                            .find(|&&(_, b_end)| m.end() <= b_end)
                            .map(|&(ej, _)| ej)
                            .unwrap_or(items.len() - 1);
                        let (sc, ec) = find_eojeol_span_in_display(
                            display_text,
                            word_spans,
                            start_idx,
                            end_ej,
                            &items[start_idx].raw_word,
                        );
                        return Some((sc, ec, layer_regex_label));
                    }
                }
            }
        }

        // 2-2) Jamo bracket multi-eojeol match
        if (qctx.use_jamo || qctx.bracket_tokens.is_some()) && !qctx.use_regex {
            if let Some(b_tokens) = qctx.bracket_tokens {
                let tail_units = state.normalizer.tokenize_text_units(&mo_tail);
                if let Some((sc_u, ec_u)) = find_bracket_query_match(b_tokens, &tail_units) {
                    if !req_start || sc_u == 0 {
                        let matched_byte_len: usize =
                            mo_tail.chars().take(ec_u).map(|c| c.len_utf8()).sum();
                        let end_ej = eojeol_end_byte_offsets
                            .iter()
                            .find(|&&(_, b_end)| matched_byte_len <= b_end)
                            .map(|&(ej, _)| ej)
                            .unwrap_or(items.len() - 1);
                        let (sc, ec) = find_eojeol_span_in_display(
                            display_text,
                            word_spans,
                            start_idx,
                            end_ej,
                            &items[start_idx].raw_word,
                        );
                        return Some((sc, ec, layer_jamo_label));
                    }
                }
            }
        }

        // 2-3) Plain text multi-eojeol match
        if qctx.use_text && !qctx.use_regex && qctx.bracket_tokens.is_none() {
            if req_start {
                if mo_tail.starts_with(qctx.core_query) {
                    let (sc, ec) = find_eojeol_span_in_display(
                        display_text,
                        word_spans,
                        start_idx,
                        (start_idx + 1).min(items.len() - 1),
                        &items[start_idx].raw_word,
                    );
                    return Some((sc, ec, layer_label));
                }
            } else if mo_tail.contains(qctx.core_query) {
                let (sc, ec) = find_eojeol_span_in_display(
                    display_text,
                    word_spans,
                    start_idx,
                    (start_idx + 1).min(items.len() - 1),
                    &items[start_idx].raw_word,
                );
                return Some((sc, ec, layer_label));
            }
        }

        // In "text" mode (where req_start is false), start_idx == 0 already scanned the entire mo_tail unanchored
        if !req_start {
            break;
        }
    }

    None
}

fn evaluate_segment_match(
    state: &AppState,
    qctx: &SearchQueryContext,
    display_text: &str,
    text_raw: &str,
    text_nfd_norm: &str,
    morph_flat: &str,
    sense_flat: &str,
    syntax_tree: &str,
) -> Option<(usize, usize, &'static str)> {
    let word_spans = compute_display_word_spans(display_text);
    let req_start = qctx.req_eojeol_start && qctx.search_unit != "text";

    // 1. Surface layer matching
    if qctx.use_surface {
        if qctx.search_unit == "eojeol" {
            for &(w_sc, w_ec, w_str) in &word_spans {
                let (w_clean, skipped_c) = strip_leading_surface_quotes(w_str);
                if w_clean.is_empty() {
                    continue;
                }
                // 1-1) Regex in eojeol mode
                if qctx.use_regex {
                    if req_start {
                        if let Some(re_anc) = qctx.anchored_regex {
                            if let Some(m) = re_anc.find(w_clean) {
                                let m_chars = w_clean[..m.end()].chars().count();
                                return Some((
                                    w_sc + skipped_c,
                                    (w_sc + skipped_c + m_chars).max(w_ec),
                                    "원문/정규식",
                                ));
                            }
                        }
                    } else if let Some(re) = qctx.compiled_regex {
                        if let Some(m) = re.find(w_clean) {
                            let sc = w_clean[..m.start()].chars().count();
                            let ec = sc + w_clean[m.start()..m.end()].chars().count();
                            return Some((w_sc + skipped_c + sc, w_sc + skipped_c + ec, "원문/정규식"));
                        }
                    }
                }
                // 1-2) Jamo bracket in eojeol mode
                if (qctx.use_jamo || qctx.bracket_tokens.is_some()) && !qctx.use_regex {
                    if let Some(b_tokens) = qctx.bracket_tokens {
                        let units = state.normalizer.tokenize_text_units(w_clean);
                        if let Some((sc, ec)) = find_bracket_query_match(b_tokens, &units) {
                            if !req_start || sc == 0 {
                                return Some((
                                    w_sc + skipped_c + sc,
                                    w_sc + skipped_c + ec,
                                    "원문/자소",
                                ));
                            }
                        }
                    }
                }
                // 1-3) Text / NFD in eojeol mode
                if qctx.use_text && !qctx.use_regex && qctx.bracket_tokens.is_none() {
                    let (norm_chars, char_map) = state.normalizer.to_nfd_norm_with_map(w_clean);
                    let candidates = find_all_char_subslices(&norm_chars, qctx.norm_query_chars);
                    for (n_start, n_end) in candidates {
                        if !qctx.use_jamo && n_end < norm_chars.len() && is_jongseong_jamo(norm_chars[n_end]) {
                            continue;
                        }
                        let orig_start = char_map.get(n_start).copied().unwrap_or(0);
                        let orig_end = char_map
                            .get(n_end.saturating_sub(1))
                            .map(|&idx| idx + 1)
                            .unwrap_or(orig_start + 1);
                        if req_start && orig_start != 0 {
                            continue;
                        }
                        if qctx.has_trailing_space && orig_end < w_clean.chars().count() {
                            continue;
                        }
                        return Some((
                            w_sc + skipped_c + orig_start,
                            w_sc + skipped_c + orig_end,
                            "원문/음절",
                        ));
                    }
                }
            }
        } else {
            // Sentence or Text unit mode on Surface
            let display_chars: Vec<char> = display_text.chars().collect();
            let is_valid_start = |char_idx: usize| -> bool {
                if !req_start || char_idx == 0 {
                    return true;
                }
                let mut k = char_idx;
                while k > 0 && is_leading_quote_or_bracket(display_chars[k - 1]) {
                    k -= 1;
                }
                k == 0 || display_chars[k - 1].is_whitespace()
            };

            // 1-1) Regex on display_text
            if qctx.use_regex {
                if let Some(re) = qctx.compiled_regex {
                    for m in re.find_iter(display_text) {
                        let start_c = display_text[..m.start()].chars().count();
                        if !is_valid_start(start_c) {
                            continue;
                        }
                        let end_c = start_c + display_text[m.start()..m.end()].chars().count();
                        return Some((start_c, end_c, "원문/정규식"));
                    }
                    if !req_start {
                        if let Some(_m) = re.find(text_nfd_norm) {
                            return Some((0, display_chars.len().min(8), "원문/정규식(NFD)"));
                        }
                    }
                }
            }

            // 1-2) Jamo bracket on display_text
            if (qctx.use_jamo || qctx.bracket_tokens.is_some()) && !qctx.use_regex {
                if let Some(b_tokens) = qctx.bracket_tokens {
                    let units = state.normalizer.tokenize_text_units(display_text);
                    for start_u in 0..units.len() {
                        if !is_valid_start(units[start_u].start_char) {
                            continue;
                        }
                        if let Some(consumed) = match_tokens_at(b_tokens, &units[start_u..]) {
                            if consumed > 0 {
                                let sc = units[start_u].start_char;
                                let ec = units[start_u + consumed - 1].end_char;
                                return Some((sc, ec, "원문/자소"));
                            }
                        }
                    }
                }
            }

            // 1-3) Text / NFD on display_text
            if qctx.use_text && !qctx.use_regex && qctx.bracket_tokens.is_none() {
                let (norm_chars, char_map) = state.normalizer.to_nfd_norm_with_map(display_text);
                let candidates = find_all_char_subslices(&norm_chars, qctx.norm_query_chars);
                for (n_start, n_end) in candidates {
                    if !qctx.use_jamo && n_end < norm_chars.len() && is_jongseong_jamo(norm_chars[n_end]) {
                        continue;
                    }
                    let orig_start = char_map.get(n_start).copied().unwrap_or(0);
                    let orig_end = char_map
                        .get(n_end.saturating_sub(1))
                        .map(|&idx| idx + 1)
                        .unwrap_or(orig_start + 1);
                    if !is_valid_start(orig_start) {
                        continue;
                    }
                    if !check_word_boundary(
                        &display_chars,
                        orig_start,
                        orig_end,
                        qctx.has_leading_space,
                        qctx.has_trailing_space,
                    ) {
                        continue;
                    }
                    return Some((orig_start, orig_end, "원문/음절"));
                }
                if text_raw.contains(qctx.core_query) {
                    if let Some(byte_pos) = display_text.find(qctx.core_query) {
                        let sc = display_text[..byte_pos].chars().count();
                        let ec = sc + qctx.core_query.chars().count();
                        if is_valid_start(sc)
                            && check_word_boundary(
                                &display_chars,
                                sc,
                                ec,
                                qctx.has_leading_space,
                                qctx.has_trailing_space,
                            )
                        {
                            return Some((sc, ec, "원문(PUA)"));
                        }
                    } else if !req_start && !qctx.has_leading_space && !qctx.has_trailing_space {
                        return Some((0, display_chars.len().min(6), "원문(PUA)"));
                    }
                }
            }
        }
    }

    // 2. Morpheme layer matching
    if qctx.use_morph {
        if let Some(res) = match_analysis_layer(
            state,
            qctx,
            display_text,
            &word_spans,
            morph_flat,
            "형태분석",
            "형태소/자소",
            "형태소/정규식",
        ) {
            return Some(res);
        }
    }

    // 3. Sense layer matching
    if qctx.use_sense {
        if let Some(res) = match_analysis_layer(
            state,
            qctx,
            display_text,
            &word_spans,
            sense_flat,
            "형태의미",
            "형태의미/자소",
            "형태의미/정규식",
        ) {
            return Some(res);
        }
    }

    // 4. Syntax tree layer matching
    if qctx.use_syntax && !syntax_tree.is_empty() {
        let first_word_len = word_spans.first().map(|w| w.1 - w.0).unwrap_or(6);
        let syn_compat = state.normalizer.normalize_morph_for_regex(syntax_tree);
        if qctx.use_regex {
            if let Some(re) = qctx.compiled_regex {
                if re.is_match(&syn_compat) || re.is_match(syntax_tree) {
                    return Some((0, first_word_len, "구문/정규식"));
                }
            }
        }
        if (qctx.use_jamo || qctx.bracket_tokens.is_some()) && !qctx.use_regex {
            if let Some(b_tokens) = qctx.bracket_tokens {
                let syn_units = state.normalizer.tokenize_text_units(&syn_compat);
                if find_bracket_query_match(b_tokens, &syn_units).is_some() {
                    return Some((0, first_word_len, "구문/자소"));
                }
            }
        }
        if qctx.use_text && !qctx.use_regex && qctx.bracket_tokens.is_none() {
            let norm_syn = state.normalizer.normalize_morph_str(syntax_tree);
            if syntax_tree.contains(qctx.core_query)
                || syn_compat.contains(qctx.core_query)
                || norm_syn.contains(qctx.norm_morph_query)
            {
                return Some((0, first_word_len, "구문분석"));
            }
        }
    }

    None
}

fn evaluate_cross_sentence_match(
    state: &AppState,
    conn: &Connection,
    qctx: &SearchQueryContext,
    doc_id: &str,
    seq_no: i64,
    display_text: &str,
    text_raw: &str,
    text_nfd_norm: &str,
    morph_flat: &str,
    sense_flat: &str,
    syntax_tree: &str,
) -> Option<(String, usize, usize, &'static str)> {
    let mut stmt_next = conn
        .prepare_cached(
            "SELECT text_raw, text_unicode, text_nfd_norm, COALESCE(morph_flat, ''), COALESCE(sense_flat, ''), COALESCE(syntax_tree, '')
             FROM segments WHERE doc_id = ?1 AND seq_no = ?2",
        )
        .ok()?;
    let (next_raw, next_unicode, next_nfd, next_morph, next_sense, next_syntax): (
        String,
        String,
        String,
        String,
        String,
        String,
    ) = stmt_next
        .query_row(params![doc_id, seq_no + 1], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3).unwrap_or_default(),
                r.get(4).unwrap_or_default(),
                r.get(5).unwrap_or_default(),
            ))
        })
        .ok()?;

    let next_display = state
        .normalizer
        .compose_modern_hangul_for_display(&next_unicode);
    let cur_char_len = display_text.chars().count();
    if cur_char_len == 0 || next_display.is_empty() {
        return None;
    }

    let comb_display = format!("{} {}", display_text, next_display);
    let comb_raw = format!("{} {}", text_raw, next_raw);
    let comb_nfd = format!("{} {}", text_nfd_norm, next_nfd);
    let comb_morph = if !morph_flat.is_empty() && !next_morph.is_empty() {
        format!("{} | {}", morph_flat, next_morph)
    } else {
        String::new()
    };
    let comb_sense = if !sense_flat.is_empty() && !next_sense.is_empty() {
        format!("{} | {}", sense_flat, next_sense)
    } else {
        String::new()
    };
    let comb_syntax = if !syntax_tree.is_empty() && !next_syntax.is_empty() {
        format!("{} {}", syntax_tree, next_syntax)
    } else {
        String::new()
    };

    if let Some((sc, ec, lbl)) = evaluate_segment_match(
        state,
        qctx,
        &comb_display,
        &comb_raw,
        &comb_nfd,
        &comb_morph,
        &comb_sense,
        &comb_syntax,
    ) {
        if sc < cur_char_len && ec > cur_char_len {
            return Some((comb_display, sc, ec, lbl));
        }
    }
    None
}

fn handle_search(
    state: &AppState,
    params: &HashMap<String, String>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let t0 = Instant::now();
    let my_search_seq = state.latest_search_seq.fetch_add(1, Ordering::Relaxed) + 1;

    let raw_query = params.get("q").map(|s| s.as_str()).unwrap_or("");
    let has_leading_space = raw_query.starts_with(' ') && !raw_query.trim().is_empty();
    let has_trailing_space = raw_query.ends_with(' ') && !raw_query.trim().is_empty();
    let core_query = if has_leading_space || has_trailing_space {
        raw_query.trim()
    } else {
        raw_query
    };

    let search_unit = match params
        .get("search_unit")
        .map(|s| s.trim())
        .unwrap_or("eojeol")
    {
        "sentence" => "sentence",
        "text" => "text",
        _ => "eojeol",
    };

    let targets_param = params.get("targets").map(|s| s.as_str()).unwrap_or("");
    let methods_param = params.get("methods").map(|s| s.as_str()).unwrap_or("");
    let mode = params.get("mode").map(|s| s.as_str()).unwrap_or("surface");

    // Target layers: single layer by default (surface, morph, sense, syntax)
    let mut use_surface = false;
    let mut use_morph = false;
    let mut use_sense = false;
    let mut use_syntax = false;

    if !targets_param.is_empty() {
        for t in targets_param.split(',') {
            match t.trim() {
                "surface" => use_surface = true,
                "morph" => use_morph = true,
                "sense" => use_sense = true,
                "syntax" => use_syntax = true,
                "all" => {
                    use_surface = true;
                    use_morph = true;
                    use_sense = true;
                    use_syntax = true;
                }
                _ => {}
            }
        }
    } else {
        match mode {
            "morph" => use_morph = true,
            "sense" => use_sense = true,
            "syntax" => use_syntax = true,
            _ => use_surface = true,
        }
    }

    // Methods: text, jamo, regex
    let mut use_text = false;
    let mut use_jamo = false;
    let mut use_regex = false;

    if !methods_param.is_empty() {
        for m in methods_param.split(',') {
            match m.trim() {
                "text" => use_text = true,
                "jamo" => use_jamo = true,
                "regex" => use_regex = true,
                _ => {}
            }
        }
    } else {
        match mode {
            "regex" => use_regex = true,
            "jamo" => use_jamo = true,
            _ => {
                use_text = true;
                if core_query.contains('[') && core_query.contains('/') && core_query.contains(']') {
                    use_jamo = true;
                }
            }
        }
    }

    if has_regex_syntax(core_query) {
        use_regex = true;
    }

    if !use_surface && !use_morph && !use_sense && !use_syntax {
        use_surface = true;
    }
    if !use_text && !use_jamo && !use_regex {
        use_text = true;
    }

    let doc_filter = params.get("doc_id").map(|s| s.as_str()).unwrap_or("ALL");
    let doc_ids_raw = params.get("doc_ids").map(|s| s.as_str()).unwrap_or("");
    let selected_doc_ids: Vec<String> = if !doc_ids_raw.is_empty() {
        doc_ids_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "ALL")
            .collect()
    } else if doc_filter != "ALL" && !doc_filter.is_empty() {
        vec![doc_filter.to_string()]
    } else {
        Vec::new()
    };
    let cat_filter = params.get("category").map(|s| s.as_str()).unwrap_or("ALL");
    let layer_filter = params
        .get("layer_filter")
        .map(|s| s.as_str())
        .unwrap_or("ALL");
    let context_words: usize = params
        .get("context_words")
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    let raw_limit: usize = params
        .get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);
    let limit: usize = if raw_limit == 0 {
        usize::MAX
    } else {
        raw_limit
    };

    let norm_query_nfd = state.normalizer.to_nfd_norm(core_query);
    let norm_query_display = state
        .normalizer
        .compose_modern_hangul_for_display(&norm_query_nfd);
    let norm_query_chars: Vec<char> = norm_query_nfd.chars().collect();
    let norm_morph_query = state.normalizer.normalize_morph_str(core_query);

    let bracket_tokens = state.normalizer.parse_bracket_query(core_query);

    if core_query.is_empty()
        && selected_doc_ids.is_empty()
        && layer_filter == "ALL"
        && cat_filter == "ALL"
        && use_surface
    {
        return json_response(
            &SearchResponse {
                query: raw_query.to_string(),
                mode: mode.to_string(),
                normalized_query_nfd: norm_query_nfd,
                normalized_query_display: norm_query_display,
                total_matches: 0,
                returned_count: 0,
                elapsed_ms: 0.0,
                cancelled: false,
                memory_tier: state.memory_profile.tier_label.clone(),
                hits: Vec::new(),
            },
            200,
        );
    }

    let conn = match Connection::open(&state.db_path) {
        Ok(c) => c,
        Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 500),
    };
    state.memory_profile.apply_pragmas(&conn);

    let (compiled_regex, anchored_regex) = if (use_regex || mode == "regex") && !core_query.is_empty() {
        let expanded_regex_str = expand_bracket_syntax_to_regex(core_query);
        let base_pat = match Regex::new(&expanded_regex_str) {
            Ok(_) => expanded_regex_str,
            Err(e) => match Regex::new(core_query) {
                Ok(_) => core_query.to_string(),
                Err(_) => {
                    return json_response(
                        &serde_json::json!({ "error": format!("정규표현식 오류: {}", e) }),
                        400,
                    );
                }
            },
        };
        let r_unanchored = Regex::new(&base_pat).ok();
        let anchored_pat = if base_pat.starts_with('^') {
            base_pat.clone()
        } else {
            format!("^(?:{})", base_pat)
        };
        let r_anchored = Regex::new(&anchored_pat).ok();
        (r_unanchored, r_anchored)
    } else {
        (None, None)
    };

    // 문서 메타데이터 인메모리 캐시 조회
    struct DocMeta {
        title: String,
        category: String,
        layers: String,
    }
    let mut doc_meta_map: HashMap<String, DocMeta> = HashMap::new();
    if let Ok(mut stmt_docs) =
        conn.prepare("SELECT doc_id, title, corpus_category, annotation_layers FROM documents")
    {
        if let Ok(doc_iter) = stmt_docs.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        }) {
            for d in doc_iter.flatten() {
                doc_meta_map.insert(
                    d.0,
                    DocMeta {
                        title: d.1,
                        category: d.2,
                        layers: d.3,
                    },
                );
            }
        }
    }

    let mut effective_doc_ids = selected_doc_ids.clone();
    if cat_filter != "ALL" && !cat_filter.is_empty() {
        let cat_matched_ids: Vec<String> = doc_meta_map
            .iter()
            .filter(|(_, meta)| meta.category.contains(cat_filter))
            .map(|(id, _)| id.clone())
            .collect();
        if effective_doc_ids.is_empty() {
            effective_doc_ids = if cat_matched_ids.is_empty() {
                vec!["__NO_MATCHING_DOC__".to_string()]
            } else {
                cat_matched_ids
            };
        } else {
            effective_doc_ids.retain(|id| cat_matched_ids.contains(id));
            if effective_doc_ids.is_empty() {
                effective_doc_ids.push("__NO_MATCHING_DOC__".to_string());
            }
        }
    }

    // 검색 대상 층위가 '형태소/품사', '형태의미', '구문표지'인 경우 해당 분석 말뭉치 문서들로만 엄격 제한
    if !use_surface {
        let layer_matched_ids: Vec<String> = doc_meta_map
            .iter()
            .filter(|(_, meta)| {
                (use_morph && (meta.category.contains("형태") || meta.layers.contains("형태")))
                    || (use_sense && (meta.category.contains("의미") || meta.layers.contains("의미")))
                    || (use_syntax && (meta.category.contains("구문") || meta.layers.contains("구문")))
            })
            .map(|(id, _)| id.clone())
            .collect();
        if effective_doc_ids.is_empty() {
            effective_doc_ids = if layer_matched_ids.is_empty() {
                vec!["__NO_MATCHING_DOC__".to_string()]
            } else {
                layer_matched_ids
            };
        } else {
            effective_doc_ids.retain(|id| layer_matched_ids.contains(id));
            if effective_doc_ids.is_empty() {
                effective_doc_ids.push("__NO_MATCHING_DOC__".to_string());
            }
        }
    }

    let mut sql = String::from(
        "SELECT s.segment_id, s.doc_id, s.seq_no,
                COALESCE(s.sent_id_str, ''), COALESCE(s.page_info, ''), '', COALESCE(s.lang, 'kor'), COALESCE(s.part_type, ''),
                s.text_raw, s.text_unicode, s.text_nfd_norm,
                COALESCE(s.morph_flat, ''), COALESCE(s.sense_flat, ''), COALESCE(s.syntax_tree, '')
         FROM segments s
         WHERE 1=1",
    );
    let mut sql_params: Vec<String> = Vec::new();

    if !effective_doc_ids.is_empty() {
        let placeholders = vec!["?"; effective_doc_ids.len()].join(", ");
        sql.push_str(&format!(" AND s.doc_id IN ({})", placeholders));
        for did in &effective_doc_ids {
            sql_params.push(did.clone());
        }
    }

    // 사용자가 선택한 검색 대상 층위에 따른 자동 말뭉치 필터 적용
    if !use_surface {
        let mut target_filters = Vec::new();
        if use_morph {
            target_filters.push("COALESCE(s.morph_flat, '') != ''");
        }
        if use_sense {
            target_filters.push("COALESCE(s.sense_flat, '') != ''");
        }
        if use_syntax {
            target_filters.push("COALESCE(s.syntax_tree, '') != ''");
        }
        if !target_filters.is_empty() {
            sql.push_str(&format!(" AND ({})", target_filters.join(" OR ")));
        }
    }

    // 명시적 레거시 layer_filter 호환
    if layer_filter == "TRIPLE" {
        sql.push_str(" AND COALESCE(s.morph_flat, '') != '' AND COALESCE(s.sense_flat, '') != '' AND COALESCE(s.syntax_tree, '') != ''");
    } else if layer_filter == "OLD_HANGUL" {
        let old_ids: Vec<String> = doc_meta_map
            .iter()
            .filter(|(_, meta)| meta.category.contains("옛한글"))
            .map(|(id, _)| id.clone())
            .collect();
        if !old_ids.is_empty() {
            let ph = vec!["?"; old_ids.len()].join(", ");
            sql.push_str(&format!(" AND s.doc_id IN ({})", ph));
            for oid in old_ids {
                sql_params.push(oid);
            }
        }
    } else if layer_filter == "MORPH" && use_surface {
        sql.push_str(" AND COALESCE(s.morph_flat, '') != ''");
    } else if layer_filter == "SENSE" && use_surface {
        sql.push_str(" AND COALESCE(s.sense_flat, '') != ''");
    } else if layer_filter == "SYNTAX" && use_surface {
        sql.push_str(" AND COALESCE(s.syntax_tree, '') != ''");
    }

    let choseong_only = is_choseong_only_query(&norm_query_chars);
    if !core_query.is_empty()
        && !use_regex
        && mode != "regex"
        && !choseong_only
        && bracket_tokens.is_none()
        && use_surface
        && !use_morph
        && !use_sense
        && !use_syntax
    {
        let like_tok = if search_unit == "text" && core_query.contains(' ') {
            core_query.split_whitespace().next().unwrap_or(core_query)
        } else {
            core_query
        };
        let like_nfd = state.normalizer.to_nfd_norm(like_tok);
        sql.push_str(" AND (s.text_nfd_norm LIKE ? OR s.text_unicode LIKE ? OR s.text_raw LIKE ?)");
        sql_params.push(format!("%{}%", like_nfd));
        sql_params.push(format!("%{}%", like_tok));
        sql_params.push(format!("%{}%", like_tok));
    }

    sql.push_str(" ORDER BY s.doc_id ASC, s.seq_no ASC");

    if core_query.is_empty() && limit != usize::MAX {
        sql.push_str(&format!(" LIMIT {}", limit));
    }

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 500),
    };

    let param_refs: Vec<&dyn rusqlite::ToSql> =
        sql_params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

    let mut hits = Vec::new();
    let mut total_matches = 0usize;
    let mut cancelled = false;
    let mut scanned_rows = 0usize;

    let qctx = SearchQueryContext {
        core_query,
        search_unit,
        req_eojeol_start: query_requires_eojeol_start(core_query),
        has_leading_space,
        has_trailing_space,
        use_surface,
        use_morph,
        use_sense,
        use_syntax,
        use_text,
        use_jamo,
        use_regex,
        norm_query_chars: &norm_query_chars,
        norm_morph_query: &norm_morph_query,
        bracket_tokens: bracket_tokens.as_deref(),
        compiled_regex: compiled_regex.as_ref(),
        anchored_regex: anchored_regex.as_ref(),
    };

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3).unwrap_or_default(),
            row.get::<_, String>(4).unwrap_or_default(),
            row.get::<_, String>(5).unwrap_or_default(),
            row.get::<_, String>(6).unwrap_or_default(),
            row.get::<_, String>(7).unwrap_or_default(),
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, String>(10)?,
            row.get::<_, String>(11).unwrap_or_default(),
            row.get::<_, String>(12).unwrap_or_default(),
            row.get::<_, String>(13).unwrap_or_default(),
        ))
    });

    if let Ok(iter) = rows {
        for item in iter.flatten() {
            scanned_rows += 1;
            if scanned_rows & 127 == 0 {
                if state.cancel_search_seq.load(Ordering::Relaxed) >= my_search_seq {
                    cancelled = true;
                    break;
                }
            }

            let (
                segment_id,
                doc_id,
                seq_no,
                sent_id,
                page_ref,
                speaker,
                lang,
                sent_type,
                text_raw,
                text_unicode,
                text_nfd_norm,
                morph_flat,
                sense_flat,
                syntax_tree,
            ) = item;

            let (doc_title, corpus_category, annotation_layers) = match doc_meta_map.get(&doc_id) {
                Some(m) => (m.title.clone(), m.category.clone(), m.layers.clone()),
                None => (doc_id.clone(), "원시 말뭉치".to_string(), "원문".to_string()),
            };

            let display_text = state
                .normalizer
                .compose_modern_hangul_for_display(&text_unicode);

            // Browse mode if query is empty
            if core_query.is_empty() {
                total_matches += 1;
                if hits.len() < limit {
                    let first_word_len = display_text
                        .split_whitespace()
                        .next()
                        .map(|w| w.chars().count())
                        .unwrap_or(1);
                    let (raw_left, node, raw_right, w_cnt, rw_cnt, l_trunc, r_trunc) =
                        extract_kwic_by_word_count(
                            &display_text,
                            0,
                            first_word_len,
                            context_words,
                        );
                    let (left, node, right) = pad_kwic_context(
                        &conn,
                        &doc_id,
                        seq_no,
                        raw_left,
                        node,
                        raw_right,
                        w_cnt,
                        rw_cnt,
                        context_words,
                        l_trunc,
                        r_trunc,
                    );
                    let browse_layer = if !use_surface && use_morph {
                        "형태분석 열람"
                    } else if !use_surface && use_sense {
                        "형태의미 열람"
                    } else if !use_surface && use_syntax {
                        "구문분석 열람"
                    } else {
                        "문서 열람"
                    };
                    hits.push(SearchHit {
                        segment_id,
                        doc_id,
                        doc_title,
                        corpus_category,
                        annotation_layers,
                        seq_no,
                        sent_id,
                        page_ref,
                        speaker,
                        lang,
                        sent_type,
                        left_context: left,
                        node,
                        right_context: right,
                        text_raw,
                        text_unicode: display_text,
                        has_morph: !morph_flat.is_empty(),
                        has_sense: !sense_flat.is_empty(),
                        has_syntax: !syntax_tree.is_empty(),
                        morph_flat,
                        sense_flat,
                        syntax_tree,
                        match_layer: browse_layer.to_string(),
                    });
                }
                continue;
            }

            let mut matched_span = evaluate_segment_match(
                state,
                &qctx,
                &display_text,
                &text_raw,
                &text_nfd_norm,
                &morph_flat,
                &sense_flat,
                &syntax_tree,
            )
            .map(|(sc, ec, lbl)| (display_text.clone(), sc, ec, lbl));

            if matched_span.is_none() && search_unit == "text" {
                matched_span = evaluate_cross_sentence_match(
                    state,
                    &conn,
                    &qctx,
                    &doc_id,
                    seq_no,
                    &display_text,
                    &text_raw,
                    &text_nfd_norm,
                    &morph_flat,
                    &sense_flat,
                    &syntax_tree,
                );
            }

            if let Some((kwic_base_text, start_c, end_c, layer_name)) = matched_span {
                total_matches += 1;
                if hits.len() < limit {
                    let (raw_left, node, raw_right, w_cnt, rw_cnt, l_trunc, r_trunc) =
                        extract_kwic_by_word_count(
                            &kwic_base_text,
                            start_c,
                            end_c,
                            context_words,
                        );
                    let (left, node, right) = pad_kwic_context(
                        &conn,
                        &doc_id,
                        seq_no,
                        raw_left,
                        node,
                        raw_right,
                        w_cnt,
                        rw_cnt,
                        context_words,
                        l_trunc,
                        r_trunc,
                    );
                    hits.push(SearchHit {
                        segment_id,
                        doc_id,
                        doc_title,
                        corpus_category,
                        annotation_layers,
                        seq_no,
                        sent_id,
                        page_ref,
                        speaker,
                        lang,
                        sent_type,
                        left_context: left,
                        node,
                        right_context: right,
                        text_raw,
                        text_unicode: display_text,
                        has_morph: !morph_flat.is_empty(),
                        has_sense: !sense_flat.is_empty(),
                        has_syntax: !syntax_tree.is_empty(),
                        morph_flat,
                        sense_flat,
                        syntax_tree,
                        match_layer: layer_name.to_string(),
                    });
                }
            }
        }
    }

    let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
    let returned = hits.len();

    json_response(
        &SearchResponse {
            query: raw_query.to_string(),
            mode: mode.to_string(),
            normalized_query_nfd: norm_query_nfd,
            normalized_query_display: norm_query_display,
            total_matches,
            returned_count: returned,
            elapsed_ms: elapsed,
            cancelled,
            memory_tier: state.memory_profile.tier_label.clone(),
            hits,
        },
        200,
    )
}


struct ChannelReader {
    rx: std::sync::mpsc::Receiver<Vec<u8>>,
    current: std::io::Cursor<Vec<u8>>,
}

impl std::io::Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let n = self.current.read(buf)?;
            if n > 0 {
                return Ok(n);
            }
            match self.rx.recv() {
                Ok(chunk) => {
                    if chunk.is_empty() {
                        return Ok(0);
                    }
                    self.current = std::io::Cursor::new(chunk);
                }
                Err(_) => {
                    return Ok(0);
                }
            }
        }
    }
}


fn handle_search_stream(
    state: &Arc<AppState>,
    params: &HashMap<String, String>,
    req: tiny_http::Request,
) {
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(128);
    let state_worker = Arc::clone(state);
    let params_worker = params.clone();

    std::thread::spawn(move || {
        let t0 = Instant::now();
        let my_search_seq = state_worker.latest_search_seq.fetch_add(1, Ordering::Relaxed) + 1;

        let raw_query = params_worker.get("q").map(|s| s.as_str()).unwrap_or("");
        let has_leading_space = raw_query.starts_with(' ') && !raw_query.trim().is_empty();
        let has_trailing_space = raw_query.ends_with(' ') && !raw_query.trim().is_empty();
        let core_query = if has_leading_space || has_trailing_space {
            raw_query.trim()
        } else {
            raw_query
        };

        let search_unit = match params_worker
            .get("search_unit")
            .map(|s| s.trim())
            .unwrap_or("eojeol")
        {
            "sentence" => "sentence",
            "text" => "text",
            _ => "eojeol",
        };

        let targets_param = params_worker.get("targets").map(|s| s.as_str()).unwrap_or("");
        let methods_param = params_worker.get("methods").map(|s| s.as_str()).unwrap_or("");
        let mode = params_worker.get("mode").map(|s| s.as_str()).unwrap_or("surface");

        let mut use_surface = false;
        let mut use_morph = false;
        let mut use_sense = false;
        let mut use_syntax = false;

        if !targets_param.is_empty() {
            for t in targets_param.split(',') {
                match t.trim() {
                    "surface" => use_surface = true,
                    "morph" => use_morph = true,
                    "sense" => use_sense = true,
                    "syntax" => use_syntax = true,
                    "all" => {
                        use_surface = true;
                        use_morph = true;
                        use_sense = true;
                        use_syntax = true;
                    }
                    _ => {}
                }
            }
        } else {
            match mode {
                "morph" => use_morph = true,
                "sense" => use_sense = true,
                "syntax" => use_syntax = true,
                _ => use_surface = true,
            }
        }

        let mut use_text = false;
        let mut use_jamo = false;
        let mut use_regex = false;

        if !methods_param.is_empty() {
            for m in methods_param.split(',') {
                match m.trim() {
                    "text" => use_text = true,
                    "jamo" => use_jamo = true,
                    "regex" => use_regex = true,
                    _ => {}
                }
            }
        } else {
            match mode {
                "regex" => use_regex = true,
                "jamo" => use_jamo = true,
                _ => {
                    use_text = true;
                    if core_query.contains('[') && core_query.contains('/') && core_query.contains(']') {
                        use_jamo = true;
                    }
                }
            }
        }

        if has_regex_syntax(core_query) {
            use_regex = true;
        }

        if !use_surface && !use_morph && !use_sense && !use_syntax {
            use_surface = true;
        }
        if !use_text && !use_jamo && !use_regex {
            use_text = true;
        }

        let doc_filter = params_worker.get("doc_id").map(|s| s.as_str()).unwrap_or("ALL");
        let doc_ids_raw = params_worker.get("doc_ids").map(|s| s.as_str()).unwrap_or("");
        let selected_doc_ids: Vec<String> = if !doc_ids_raw.is_empty() {
            doc_ids_raw
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s != "ALL")
                .collect()
        } else if doc_filter != "ALL" && !doc_filter.is_empty() {
            vec![doc_filter.to_string()]
        } else {
            Vec::new()
        };
        let cat_filter = params_worker.get("category").map(|s| s.as_str()).unwrap_or("ALL");
        let layer_filter = params_worker.get("layer_filter").map(|s| s.as_str()).unwrap_or("ALL");
        let context_words: usize = params_worker.get("context_words").and_then(|s| s.parse().ok()).unwrap_or(5);

        let raw_limit: usize = params_worker.get("limit").and_then(|s| s.parse().ok()).unwrap_or(500);
        let limit: usize = if raw_limit == 0 { usize::MAX } else { raw_limit };

        let norm_query_nfd = state_worker.normalizer.to_nfd_norm(core_query);
        let norm_query_display = state_worker.normalizer.compose_modern_hangul_for_display(&norm_query_nfd);
        let norm_query_chars: Vec<char> = norm_query_nfd.chars().collect();
        let norm_morph_query = state_worker.normalizer.normalize_morph_str(core_query);
        let bracket_tokens = state_worker.normalizer.parse_bracket_query(core_query);

        // Send start message
        let start_msg = serde_json::json!({
            "type": "start",
            "query": raw_query,
            "mode": mode,
            "search_unit": search_unit,
            "normalized_query_display": norm_query_display,
            "limit": limit,
        });
        if tx.send(format!("{}\n", start_msg).into_bytes()).is_err() {
            return;
        }

        let conn = match Connection::open(&state_worker.db_path) {
            Ok(c) => c,
            Err(e) => {
                let err_msg = serde_json::json!({ "type": "error", "error": e.to_string() });
                let _ = tx.send(format!("{}\n", err_msg).into_bytes());
                return;
            }
        };
        state_worker.memory_profile.apply_pragmas(&conn);

        let (compiled_regex, anchored_regex) = if (use_regex || mode == "regex") && !core_query.is_empty() {
            let expanded_regex_str = expand_bracket_syntax_to_regex(core_query);
            let base_pat = match Regex::new(&expanded_regex_str) {
                Ok(_) => expanded_regex_str,
                Err(e) => match Regex::new(core_query) {
                    Ok(_) => core_query.to_string(),
                    Err(_) => {
                        let err_msg = serde_json::json!({ "type": "error", "error": format!("정규표현식 오류: {}", e) });
                        let _ = tx.send(format!("{}\n", err_msg).into_bytes());
                        return;
                    }
                },
            };
            let r_unanchored = Regex::new(&base_pat).ok();
            let anchored_pat = if base_pat.starts_with('^') {
                base_pat.clone()
            } else {
                format!("^(?:{})", base_pat)
            };
            let r_anchored = Regex::new(&anchored_pat).ok();
            (r_unanchored, r_anchored)
        } else {
            (None, None)
        };

        struct DocMeta {
            title: String,
            category: String,
            layers: String,
        }
        let mut doc_meta_map: HashMap<String, DocMeta> = HashMap::new();
        if let Ok(mut stmt_docs) = conn.prepare("SELECT doc_id, title, corpus_category, annotation_layers FROM documents") {
            if let Ok(doc_iter) = stmt_docs.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            }) {
                for d in doc_iter.flatten() {
                    doc_meta_map.insert(d.0, DocMeta { title: d.1, category: d.2, layers: d.3 });
                }
            }
        }

        let mut effective_doc_ids = selected_doc_ids.clone();
        if cat_filter != "ALL" && !cat_filter.is_empty() {
            let cat_matched_ids: Vec<String> = doc_meta_map.iter()
                .filter(|(_, meta)| meta.category.contains(cat_filter))
                .map(|(id, _)| id.clone())
                .collect();
            if effective_doc_ids.is_empty() {
                effective_doc_ids = if cat_matched_ids.is_empty() {
                    vec!["__NO_MATCHING_DOC__".to_string()]
                } else {
                    cat_matched_ids
                };
            } else {
                effective_doc_ids.retain(|id| cat_matched_ids.contains(id));
                if effective_doc_ids.is_empty() {
                    effective_doc_ids.push("__NO_MATCHING_DOC__".to_string());
                }
            }
        }

        // 검색 대상 층위가 '형태소/품사', '형태의미', '구문표지'인 경우 해당 분석 말뭉치 문서들로만 엄격 제한
        if !use_surface {
            let layer_matched_ids: Vec<String> = doc_meta_map
                .iter()
                .filter(|(_, meta)| {
                    (use_morph && (meta.category.contains("형태") || meta.layers.contains("형태")))
                        || (use_sense && (meta.category.contains("의미") || meta.layers.contains("의미")))
                        || (use_syntax && (meta.category.contains("구문") || meta.layers.contains("구문")))
                })
                .map(|(id, _)| id.clone())
                .collect();
            if effective_doc_ids.is_empty() {
                effective_doc_ids = if layer_matched_ids.is_empty() {
                    vec!["__NO_MATCHING_DOC__".to_string()]
                } else {
                    layer_matched_ids
                };
            } else {
                effective_doc_ids.retain(|id| layer_matched_ids.contains(id));
                if effective_doc_ids.is_empty() {
                    effective_doc_ids.push("__NO_MATCHING_DOC__".to_string());
                }
            }
        }

        let mut sql = String::from(
            "SELECT s.segment_id, s.doc_id, s.seq_no,
                    COALESCE(s.sent_id_str, ''), COALESCE(s.page_info, ''), '', COALESCE(s.lang, 'kor'), COALESCE(s.part_type, ''),
                    s.text_raw, s.text_unicode, s.text_nfd_norm,
                    COALESCE(s.morph_flat, ''), COALESCE(s.sense_flat, ''), COALESCE(s.syntax_tree, '')
             FROM segments s
             WHERE 1=1",
        );
        let mut sql_params: Vec<String> = Vec::new();

        if !effective_doc_ids.is_empty() {
            let placeholders = vec!["?"; effective_doc_ids.len()].join(", ");
            sql.push_str(&format!(" AND s.doc_id IN ({})", placeholders));
            for did in &effective_doc_ids {
                sql_params.push(did.clone());
            }
        }

        if !use_surface {
            let mut target_filters = Vec::new();
            if use_morph {
                target_filters.push("COALESCE(s.morph_flat, '') != ''");
            }
            if use_sense {
                target_filters.push("COALESCE(s.sense_flat, '') != ''");
            }
            if use_syntax {
                target_filters.push("COALESCE(s.syntax_tree, '') != ''");
            }
            if !target_filters.is_empty() {
                sql.push_str(&format!(" AND ({})", target_filters.join(" OR ")));
            }
        }

        if layer_filter == "TRIPLE" {
            sql.push_str(" AND COALESCE(s.morph_flat, '') != '' AND COALESCE(s.sense_flat, '') != '' AND COALESCE(s.syntax_tree, '') != ''");
        } else if layer_filter == "OLD_HANGUL" {
            let old_ids: Vec<String> = doc_meta_map.iter()
                .filter(|(_, meta)| meta.category.contains("옛한글"))
                .map(|(id, _)| id.clone())
                .collect();
            if !old_ids.is_empty() {
                let ph = vec!["?"; old_ids.len()].join(", ");
                sql.push_str(&format!(" AND s.doc_id IN ({})", ph));
                for oid in old_ids {
                    sql_params.push(oid);
                }
            }
        } else if layer_filter == "MORPH" && use_surface {
            sql.push_str(" AND COALESCE(s.morph_flat, '') != ''");
        } else if layer_filter == "SENSE" && use_surface {
            sql.push_str(" AND COALESCE(s.sense_flat, '') != ''");
        } else if layer_filter == "SYNTAX" && use_surface {
            sql.push_str(" AND COALESCE(s.syntax_tree, '') != ''");
        }

        let choseong_only = is_choseong_only_query(&norm_query_chars);
        if !core_query.is_empty()
            && !use_regex
            && mode != "regex"
            && !choseong_only
            && bracket_tokens.is_none()
            && use_surface
            && !use_morph
            && !use_sense
            && !use_syntax
        {
            let like_tok = if search_unit == "text" && core_query.contains(' ') {
                core_query.split_whitespace().next().unwrap_or(core_query)
            } else {
                core_query
            };
            let like_nfd = state_worker.normalizer.to_nfd_norm(like_tok);
            sql.push_str(" AND (s.text_nfd_norm LIKE ? OR s.text_unicode LIKE ? OR s.text_raw LIKE ?)");
            sql_params.push(format!("%{}%", like_nfd));
            sql_params.push(format!("%{}%", like_tok));
            sql_params.push(format!("%{}%", like_tok));
        }

        sql.push_str(" ORDER BY s.doc_id ASC, s.seq_no ASC");

        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(e) => {
                let err_msg = serde_json::json!({ "type": "error", "error": e.to_string() });
                let _ = tx.send(format!("{}\n", err_msg).into_bytes());
                return;
            }
        };

        let param_refs: Vec<&dyn rusqlite::ToSql> =
            sql_params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

        let mut returned_count = 0usize;
        let mut total_matches = 0usize;
        let mut cancelled = false;
        let mut scanned_rows = 0usize;

        let qctx = SearchQueryContext {
            core_query,
            search_unit,
            req_eojeol_start: query_requires_eojeol_start(core_query),
            has_leading_space,
            has_trailing_space,
            use_surface,
            use_morph,
            use_sense,
            use_syntax,
            use_text,
            use_jamo,
            use_regex,
            norm_query_chars: &norm_query_chars,
            norm_morph_query: &norm_morph_query,
            bracket_tokens: bracket_tokens.as_deref(),
            compiled_regex: compiled_regex.as_ref(),
            anchored_regex: anchored_regex.as_ref(),
        };

        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3).unwrap_or_default(),
                row.get::<_, String>(4).unwrap_or_default(),
                row.get::<_, String>(5).unwrap_or_default(),
                row.get::<_, String>(6).unwrap_or_default(),
                row.get::<_, String>(7).unwrap_or_default(),
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11).unwrap_or_default(),
                row.get::<_, String>(12).unwrap_or_default(),
                row.get::<_, String>(13).unwrap_or_default(),
            ))
        });

        if let Ok(iter) = rows {
            for item in iter.flatten() {
                scanned_rows += 1;
                if scanned_rows & 63 == 0 {
                    if state_worker.cancel_search_seq.load(Ordering::Relaxed) >= my_search_seq {
                        cancelled = true;
                        break;
                    }
                }

                let (
                    segment_id,
                    doc_id,
                    seq_no,
                    sent_id,
                    page_ref,
                    speaker,
                    lang,
                    sent_type,
                    text_raw,
                    text_unicode,
                    text_nfd_norm,
                    morph_flat,
                    sense_flat,
                    syntax_tree,
                ) = item;

                let (doc_title, corpus_category, annotation_layers) = match doc_meta_map.get(&doc_id) {
                    Some(m) => (m.title.clone(), m.category.clone(), m.layers.clone()),
                    None => (doc_id.clone(), "원시 말뭉치".to_string(), "원문".to_string()),
                };

                let display_text = state_worker
                    .normalizer
                    .compose_modern_hangul_for_display(&text_unicode);

                // Browse mode if query is empty
                if core_query.is_empty() {
                    total_matches += 1;
                    returned_count += 1;

                    let first_word_len = display_text
                        .split_whitespace()
                        .next()
                        .map(|w| w.chars().count())
                        .unwrap_or(1);
                    let (raw_left, node, raw_right, w_cnt, rw_cnt, l_trunc, r_trunc) =
                        extract_kwic_by_word_count(
                            &display_text,
                            0,
                            first_word_len,
                            context_words,
                        );
                    let (left, node, right) = pad_kwic_context(
                        &conn,
                        &doc_id,
                        seq_no,
                        raw_left,
                        node,
                        raw_right,
                        w_cnt,
                        rw_cnt,
                        context_words,
                        l_trunc,
                        r_trunc,
                    );
                    let browse_layer = if !use_surface && use_morph {
                        "형태분석 열람"
                    } else if !use_surface && use_sense {
                        "형태의미 열람"
                    } else if !use_surface && use_syntax {
                        "구문분석 열람"
                    } else {
                        "문서 열람"
                    };
                    let hit = SearchHit {
                        segment_id,
                        doc_id,
                        doc_title,
                        corpus_category,
                        annotation_layers,
                        seq_no,
                        sent_id,
                        page_ref,
                        speaker,
                        lang,
                        sent_type,
                        left_context: left,
                        node,
                        right_context: right,
                        text_raw,
                        text_unicode: display_text,
                        has_morph: !morph_flat.is_empty(),
                        has_sense: !sense_flat.is_empty(),
                        has_syntax: !syntax_tree.is_empty(),
                        morph_flat,
                        sense_flat,
                        syntax_tree,
                        match_layer: browse_layer.to_string(),
                    };

                    let hit_msg = serde_json::json!({
                        "type": "hit",
                        "hit": hit,
                        "matched_count": total_matches,
                    });
                    if tx.send(format!("{}\n", hit_msg).into_bytes()).is_err() {
                        return; // client closed stream
                    }

                    if returned_count >= limit {
                        break;
                    }
                    continue;
                }

                let mut matched_span = evaluate_segment_match(
                    &state_worker,
                    &qctx,
                    &display_text,
                    &text_raw,
                    &text_nfd_norm,
                    &morph_flat,
                    &sense_flat,
                    &syntax_tree,
                )
                .map(|(sc, ec, lbl)| (display_text.clone(), sc, ec, lbl));

                if matched_span.is_none() && search_unit == "text" {
                    matched_span = evaluate_cross_sentence_match(
                        &state_worker,
                        &conn,
                        &qctx,
                        &doc_id,
                        seq_no,
                        &display_text,
                        &text_raw,
                        &text_nfd_norm,
                        &morph_flat,
                        &sense_flat,
                        &syntax_tree,
                    );
                }

                // Hit found!
                if let Some((kwic_base_text, match_start, match_end, layer_name)) = matched_span {
                    total_matches += 1;
                    returned_count += 1;

                    let (raw_left, node, raw_right, w_cnt, rw_cnt, l_trunc, r_trunc) =
                        extract_kwic_by_word_count(
                            &kwic_base_text,
                            match_start,
                            match_end,
                            context_words,
                        );
                    let (left, node, right) = pad_kwic_context(
                        &conn,
                        &doc_id,
                        seq_no,
                        raw_left,
                        node,
                        raw_right,
                        w_cnt,
                        rw_cnt,
                        context_words,
                        l_trunc,
                        r_trunc,
                    );
                    let hit = SearchHit {
                        segment_id,
                        doc_id,
                        doc_title,
                        corpus_category,
                        annotation_layers,
                        seq_no,
                        sent_id,
                        page_ref,
                        speaker,
                        lang,
                        sent_type,
                        left_context: left,
                        node,
                        right_context: right,
                        text_raw,
                        text_unicode: display_text,
                        has_morph: !morph_flat.is_empty(),
                        has_sense: !sense_flat.is_empty(),
                        has_syntax: !syntax_tree.is_empty(),
                        morph_flat,
                        sense_flat,
                        syntax_tree,
                        match_layer: layer_name.to_string(),
                    };

                    // [핵심] 검색된 즉시 실시간 스트리밍 전송!
                    let hit_msg = serde_json::json!({
                        "type": "hit",
                        "hit": hit,
                        "matched_count": total_matches,
                    });
                    if tx.send(format!("{}\n", hit_msg).into_bytes()).is_err() {
                        return; // client closed connection
                    }

                    if returned_count >= limit {
                        break;
                    }
                }
            }
        }

        let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
        let done_msg = serde_json::json!({
            "type": "done",
            "total_matches": total_matches,
            "returned_count": returned_count,
            "elapsed_ms": elapsed,
            "cancelled": cancelled,
        });
        let _ = tx.send(format!("{}\n", done_msg).into_bytes());
    });

    let reader = ChannelReader {
        rx,
        current: std::io::Cursor::new(Vec::new()),
    };
    let headers = vec![
        Header::from_bytes(&b"Content-Type"[..], &b"application/x-ndjson; charset=utf-8"[..]).unwrap(),
        Header::from_bytes(&b"Cache-Control"[..], &b"no-cache, no-transform"[..]).unwrap(),
        Header::from_bytes(&b"Connection"[..], &b"keep-alive"[..]).unwrap(),
        Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap(),
    ];
    let resp = Response::new(StatusCode(200), headers, reader, None, None);
    let _ = req.respond(resp);
}

fn handle_segment_detail(
    state: &AppState,
    params: &HashMap<String, String>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let seg_id: i64 = match params.get("id").and_then(|s| s.parse().ok()) {
        Some(id) => id,
        None => return json_response(&serde_json::json!({ "error": "Missing id parameter" }), 400),
    };

    let conn = match Connection::open(&state.db_path) {
        Ok(c) => c,
        Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 500),
    };

    let row_res = conn.query_row(
        "SELECT s.segment_id, s.doc_id, d.title, d.corpus_category, d.annotation_layers, d.source_files_list,
                s.seq_no, COALESCE(s.sent_id_str, ''), '', COALESCE(s.page_info, ''), COALESCE(s.lang, 'kor'), COALESCE(s.part_type, ''),
                s.text_raw, s.text_unicode, COALESCE(s.morph_flat, ''), COALESCE(s.sense_flat, ''), COALESCE(s.syntax_tree, ''),
                COALESCE(s.eojeols_json, '[]'), '{}'
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
        eojeols,
        meta,
        neighbors,
        edits,
    };
    json_response(&resp, 200)
}


fn handle_document_delete(
    state: &AppState,
    params: &HashMap<String, String>,
) -> Response<std::io::Cursor<Vec<u8>>> {
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

fn handle_segment_edit(
    state: &AppState,
    req: &mut Request,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut body = String::new();
    if let Err(e) = req.as_reader().read_to_string(&mut body) {
        return json_response(&serde_json::json!({ "error": e.to_string() }), 400);
    }
    let payload: EditSegmentRequest = match serde_json::from_str(&body) {
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
struct BuildDbRequest {
    folder_path: String,
    db_name: Option<String>,
}

fn handle_build_db(
    state: &AppState,
    req: &mut Request,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut body = String::new();
    if let Err(e) = req.as_reader().read_to_string(&mut body) {
        return json_response(&serde_json::json!({ "error": e.to_string() }), 400);
    }
    let payload: BuildDbRequest = match serde_json::from_str(&body) {
        Ok(p) => p,
        Err(e) => return json_response(&serde_json::json!({ "error": e.to_string() }), 400),
    };

    let raw_folder = payload.folder_path.trim().to_string();
    let resolved_folder = {
        let p = PathBuf::from(&raw_folder);
        if p.exists() {
            p
        } else {
            let rel = state.root_dir.join(&raw_folder);
            if rel.exists() {
                rel
            } else {
                p
            }
        }
    };

    if raw_folder.is_empty() || !resolved_folder.exists() {
        return json_response(
            &serde_json::json!({ "error": format!("지정한 폴더가 존재하지 않습니다: {}", raw_folder) }),
            400,
        );
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
    let folder_str = resolved_folder.to_string_lossy().into_owned();
    mgr.progress = DbBuildProgress {
        is_running: true,
        status: "RUNNING".to_string(),
        target_db_path: target_db.to_string_lossy().into_owned(),
        is_new_db_file: is_new_db,
        log_lines: vec![format!("[시작] 내장 Rust 파서로 말뭉치 폴더 스캔 시작: {}", folder_str)],
        ..DbBuildProgress::default()
    };

    let initial_prog = mgr.progress.clone();
    drop(mgr);

    let db_builder_arc = Arc::clone(&state.db_builder);
    let normalizer_clone = state.normalizer.clone();

    // 백그라운드 네이티브 Rust DB 빌더 스레드 (외부 Python 불필요)
    std::thread::spawn(move || {
        if let Err(e) = builder::build_folder_into_db_native(
            &resolved_folder,
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

fn handle_pick_folder(state: &AppState) -> Response<std::io::Cursor<Vec<u8>>> {
    #[cfg(target_os = "windows")]
    {
        let initial_dir = state.root_dir.to_string_lossy().replace('\'', "''");
        let ps_script = format!(
            "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
             Add-Type -AssemblyName System.Windows.Forms; \
             $f = New-Object System.Windows.Forms.FolderBrowserDialog; \
             $f.Description = '말뭉치 텍스트/XML 파일이 들어 있는 폴더를 선택하세요'; \
             $f.SelectedPath = '{}'; \
             $f.ShowNewFolderButton = $false; \
             if ($f.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ Write-Output $f.SelectedPath }}",
            initial_dir
        );
        if let Ok(output) = Command::new("powershell")
            .args(["-NoProfile", "-STA", "-Command", &ps_script])
            .output()
        {
            let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !selected.is_empty() {
                return json_response(&serde_json::json!({ "ok": true, "folder_path": selected }), 200);
            }
        }
        return json_response(&serde_json::json!({ "ok": false, "cancelled": true }), 200);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = state;
        json_response(&serde_json::json!({ "ok": false, "error": "Windows 환경에서만 폴더 선택 대화상자를 지원합니다." }), 400)
    }
}

fn handle_build_db_status(state: &AppState) -> Response<std::io::Cursor<Vec<u8>>> {
    let mgr = state.db_builder.lock().unwrap();
    json_response(&mgr.progress, 200)
}

fn handle_build_db_stop(state: &AppState) -> Response<std::io::Cursor<Vec<u8>>> {
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

fn handle_build_db_cancel(state: &AppState) -> Response<std::io::Cursor<Vec<u8>>> {
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

#[derive(Debug, Serialize)]
struct SourceJumpLine {
    line_no: usize,
    content: String,
    is_target: bool,
}

#[derive(Debug, Serialize)]
struct SourceJumpResponse {
    segment_id: i64,
    doc_id: String,
    doc_title: String,
    file_name: String,
    file_path: String,
    line_no: Option<usize>,
    sent_id: String,
    page_ref: String,
    text_raw: String,
    text_unicode: String,
    editor_status: Option<String>,
    lines: Vec<SourceJumpLine>,
}

// [요구사항 2] 원본 파일에서 해당 세그먼트의 정확한 라인 번호를 탐색하는 정밀 함수
fn find_target_line_in_source(
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
fn open_in_external_editor(file_path: &Path, line_no: Option<usize>) -> String {
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

fn handle_open_editor(params: &HashMap<String, String>) -> Response<std::io::Cursor<Vec<u8>>> {
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

fn handle_source_jump(
    state: &AppState,
    params: &HashMap<String, String>,
) -> Response<std::io::Cursor<Vec<u8>>> {
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

    use super::*;

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
}
