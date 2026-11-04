mod jamo_table;

use jamo_table::{decompose_l_cp, decompose_t_cp, decompose_v_cp};
use regex::Regex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
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

struct Normalizer {
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
    fn new(pua_json_path: &Path) -> Self {
        let mut pua_to_nfd = HashMap::new();
        if let Ok(content) = fs::read_to_string(pua_json_path) {
            if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&content) {
                for (hex_key, nfd_val) in map {
                    if let Ok(cp) = u32::from_str_radix(&hex_key, 16) {
                        if let Some(ch) = char::from_u32(cp) {
                            pua_to_nfd.insert(ch, nfd_val);
                        }
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

    fn to_nfd_norm(&self, text: &str) -> String {
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
            if chars[i].is_whitespace() {
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

fn handle_search(
    state: &AppState,
    params: &HashMap<String, String>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let t0 = Instant::now();
    let my_search_seq = state.latest_search_seq.fetch_add(1, Ordering::Relaxed) + 1;

    let raw_query = params.get("q").map(|s| s.as_str()).unwrap_or("");
    let has_leading_space = raw_query.starts_with(' ');
    let has_trailing_space = raw_query.ends_with(' ');
    let core_query = raw_query.trim();

    let targets_param = params.get("targets").map(|s| s.as_str()).unwrap_or("");
    let methods_param = params.get("methods").map(|s| s.as_str()).unwrap_or("");
    let mode = params.get("mode").map(|s| s.as_str()).unwrap_or("all");

    // Target layers: surface, morph, sense, syntax
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
            "all" => {
                use_surface = true;
                use_morph = true;
                use_sense = true;
                use_syntax = true;
            }
            "surface" | "jamo" | "regex" => use_surface = true,
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

    if core_query.is_empty() && selected_doc_ids.is_empty() && layer_filter == "ALL" && cat_filter == "ALL" {
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

    let compiled_regex = if (use_regex || mode == "regex") && !core_query.is_empty() {
        let expanded_regex_str = expand_bracket_syntax_to_regex(core_query);
        match Regex::new(&expanded_regex_str) {
            Ok(r) => Some(r),
            Err(e) => {
                match Regex::new(core_query) {
                    Ok(r2) => Some(r2),
                    Err(_) => {
                        return json_response(
                            &serde_json::json!({ "error": format!("정규표현식 오류: {}", e) }),
                            400,
                        );
                    }
                }
            }
        }
    } else {
        None
    };

    let mut sql = String::from(
        "SELECT s.segment_id, s.doc_id, d.title, d.corpus_category, d.annotation_layers,
                s.seq_no, COALESCE(s.sent_id_str, ''), COALESCE(s.page_info, ''), '', COALESCE(s.lang, 'kor'), COALESCE(s.part_type, ''),
                s.text_raw, s.text_unicode, s.text_nfd_norm,
                COALESCE(s.morph_flat, ''), COALESCE(s.sense_flat, ''), COALESCE(s.syntax_tree, '')
         FROM segments s
         JOIN documents d ON s.doc_id = d.doc_id
         WHERE 1=1",
    );
    let mut sql_params: Vec<String> = Vec::new();

    if !selected_doc_ids.is_empty() {
        let placeholders = vec!["?"; selected_doc_ids.len()].join(", ");
        sql.push_str(&format!(" AND s.doc_id IN ({})", placeholders));
        for did in &selected_doc_ids {
            sql_params.push(did.clone());
        }
    }
    if cat_filter != "ALL" && !cat_filter.is_empty() {
        sql.push_str(" AND d.corpus_category LIKE ?");
        sql_params.push(format!("%{}%", cat_filter));
    }
    if layer_filter == "TRIPLE" {
        sql.push_str(" AND COALESCE(s.morph_flat, '') != '' AND COALESCE(s.sense_flat, '') != '' AND COALESCE(s.syntax_tree, '') != ''");
    } else if layer_filter == "OLD_HANGUL" {
        sql.push_str(" AND d.corpus_category LIKE '%옛한글%'");
    } else if layer_filter == "MORPH" {
        sql.push_str(" AND COALESCE(s.morph_flat, '') != ''");
    } else if layer_filter == "SENSE" {
        sql.push_str(" AND COALESCE(s.sense_flat, '') != ''");
    } else if layer_filter == "SYNTAX" {
        sql.push_str(" AND COALESCE(s.syntax_tree, '') != ''");
    }

    let choseong_only = is_choseong_only_query(&norm_query_chars);
    if !core_query.is_empty() && !use_regex && mode != "regex" && !choseong_only && bracket_tokens.is_none() {
        if use_surface && !use_morph && !use_sense && !use_syntax {
            sql.push_str(
                " AND (s.text_nfd_norm LIKE ? OR s.text_unicode LIKE ? OR s.text_raw LIKE ?)",
            );
            let pat_nfd = format!("%{}%", norm_query_nfd);
            let pat_raw = format!("%{}%", core_query);
            sql_params.push(pat_nfd);
            sql_params.push(pat_raw.clone());
            sql_params.push(pat_raw);
        } else if !use_surface && use_morph && !use_sense && !use_syntax {
            sql.push_str(" AND COALESCE(s.morph_flat, '') != ''");
        } else if !use_surface && !use_morph && use_sense && !use_syntax {
            sql.push_str(" AND COALESCE(s.sense_flat, '') != ''");
        } else if !use_surface && !use_morph && !use_sense && use_syntax {
            sql.push_str(" AND COALESCE(s.syntax_tree, '') != ''");
        }
    }

    sql.push_str(" ORDER BY s.segment_id ASC");

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

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, String>(6).unwrap_or_default(),
            row.get::<_, String>(7).unwrap_or_default(),
            row.get::<_, String>(8).unwrap_or_default(),
            row.get::<_, String>(9).unwrap_or_default(),
            row.get::<_, String>(10).unwrap_or_default(),
            row.get::<_, String>(11)?,
            row.get::<_, String>(12)?,
            row.get::<_, String>(13)?,
            row.get::<_, String>(14).unwrap_or_default(),
            row.get::<_, String>(15).unwrap_or_default(),
            row.get::<_, String>(16).unwrap_or_default(),
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
                doc_title,
                corpus_category,
                annotation_layers,
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
                        match_layer: "문서 열람".to_string(),
                    });
                }
                continue;
            }

            let display_chars: Vec<char> = display_text.chars().collect();
            let d_len = display_chars.len();

            let check_boundary = |orig_start: usize, orig_end: usize| -> bool {
                if has_leading_space {
                    if orig_start > 0 && !display_chars[orig_start - 1].is_whitespace() {
                        return false;
                    }
                }
                if has_trailing_space {
                    if orig_end < d_len && !display_chars[orig_end].is_whitespace() {
                        return false;
                    }
                }
                true
            };

            let mut matched_span: Option<(usize, usize, &str)> = None;

            // 1. Surface matching
            if use_surface && matched_span.is_none() {
                // 1-1) Jamo bracket matching on display_text
                if (use_jamo || bracket_tokens.is_some()) && bracket_tokens.is_some() {
                    let b_tokens = bracket_tokens.as_ref().unwrap();
                    let units = state.normalizer.tokenize_text_units(&display_text);
                    if let Some((sc, ec)) = find_bracket_query_match(b_tokens, &units) {
                        if check_boundary(sc, ec) {
                            matched_span = Some((sc, ec, "원문/자소"));
                        }
                    }
                }

                // 1-2) Regex matching on display_text
                if use_regex && compiled_regex.is_some() && matched_span.is_none() {
                    let re = compiled_regex.as_ref().unwrap();
                    if let Some(m) = re.find(&display_text) {
                        let start_c = display_text[..m.start()].chars().count();
                        let end_c = start_c + display_text[m.start()..m.end()].chars().count();
                        if check_boundary(start_c, end_c) {
                            matched_span = Some((start_c, end_c, "원문/정규식"));
                        }
                    } else if let Some(_m) = re.find(&text_nfd_norm) {
                        if !has_leading_space && !has_trailing_space {
                            matched_span =
                                Some((0, display_text.chars().count().min(8), "원문/정규식(NFD)"));
                        }
                    }
                }

                // 1-3) Plain text syllable/PUA matching
                if use_text && matched_span.is_none() {
                    let (norm_chars, char_map) =
                        state.normalizer.to_nfd_norm_with_map(&display_text);
                    let candidates = find_all_char_subslices(&norm_chars, &norm_query_chars);

                    for (n_start, n_end) in candidates {
                        if !use_jamo {
                            if n_end < norm_chars.len() && is_jongseong_jamo(norm_chars[n_end]) {
                                continue;
                            }
                        }

                        let orig_start = char_map.get(n_start).copied().unwrap_or(0);
                        let orig_end = char_map
                            .get(n_end.saturating_sub(1))
                            .map(|&idx| idx + 1)
                            .unwrap_or(orig_start + 1);

                        if check_boundary(orig_start, orig_end) {
                            matched_span = Some((orig_start, orig_end, "원문/음절"));
                            break;
                        }
                    }

                    if matched_span.is_none() && text_raw.contains(core_query) {
                        if let Some(byte_pos) = display_text.find(core_query) {
                            let sc = display_text[..byte_pos].chars().count();
                            let ec = sc + core_query.chars().count();
                            if check_boundary(sc, ec) {
                                matched_span = Some((sc, ec, "원문(PUA)"));
                            }
                        } else if !has_leading_space && !has_trailing_space {
                            matched_span =
                                Some((0, display_text.chars().count().min(6), "원문(PUA)"));
                        }
                    }
                }
            }

            // 2. Morpheme layer matching
            if use_morph && matched_span.is_none() && !morph_flat.is_empty() {
                let eojeol_analyses: Vec<&str> = morph_flat.split(" | ").collect();
                let words: Vec<&str> = display_text.split_whitespace().collect();

                // 2-1) Jamo bracket matching on morph_flat
                if (use_jamo || bracket_tokens.is_some()) && bracket_tokens.is_some() {
                    let b_tokens = bracket_tokens.as_ref().unwrap();
                    for (idx, ea) in eojeol_analyses.iter().enumerate() {
                        let ea_units = state.normalizer.tokenize_text_units(ea);
                        if find_bracket_query_match(b_tokens, &ea_units).is_some() {
                            if let Some(&w) = words.get(idx) {
                                if let Some(byte_pos) = display_text.find(w) {
                                    let sc = display_text[..byte_pos].chars().count();
                                    let ec = sc + w.chars().count();
                                    if check_boundary(sc, ec) {
                                        matched_span = Some((sc, ec, "형태소/자소"));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                // 2-2) Regex matching on morph_flat
                if use_regex && compiled_regex.is_some() && matched_span.is_none() {
                    let re = compiled_regex.as_ref().unwrap();
                    for (idx, ea) in eojeol_analyses.iter().enumerate() {
                        if re.is_match(ea) {
                            if let Some(&w) = words.get(idx) {
                                if let Some(byte_pos) = display_text.find(w) {
                                    let sc = display_text[..byte_pos].chars().count();
                                    let ec = sc + w.chars().count();
                                    if check_boundary(sc, ec) {
                                        matched_span = Some((sc, ec, "형태소/정규식"));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                // 2-3) Text matching on morph_flat
                if use_text && matched_span.is_none() {
                    let norm_m = state.normalizer.normalize_morph_str(&morph_flat);
                    if norm_m.contains(&norm_morph_query) || morph_flat.contains(core_query) {
                        for (idx, ea) in eojeol_analyses.iter().enumerate() {
                            if state
                                .normalizer
                                .normalize_morph_str(ea)
                                .contains(&norm_morph_query)
                                || ea.contains(core_query)
                            {
                                if let Some(&w) = words.get(idx) {
                                    if let Some(byte_pos) = display_text.find(w) {
                                        let sc = display_text[..byte_pos].chars().count();
                                        let ec = sc + w.chars().count();
                                        if check_boundary(sc, ec) {
                                            matched_span = Some((sc, ec, "형태분석"));
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 3. Sense layer matching
            if use_sense && matched_span.is_none() && !sense_flat.is_empty() {
                let eojeol_analyses: Vec<&str> = sense_flat.split(" | ").collect();
                let words: Vec<&str> = display_text.split_whitespace().collect();

                // 3-1) Jamo bracket matching on sense_flat
                if (use_jamo || bracket_tokens.is_some()) && bracket_tokens.is_some() {
                    let b_tokens = bracket_tokens.as_ref().unwrap();
                    for (idx, ea) in eojeol_analyses.iter().enumerate() {
                        let ea_units = state.normalizer.tokenize_text_units(ea);
                        if find_bracket_query_match(b_tokens, &ea_units).is_some() {
                            if let Some(&w) = words.get(idx) {
                                if let Some(byte_pos) = display_text.find(w) {
                                    let sc = display_text[..byte_pos].chars().count();
                                    let ec = sc + w.chars().count();
                                    if check_boundary(sc, ec) {
                                        matched_span = Some((sc, ec, "형태의미/자소"));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                // 3-2) Regex matching on sense_flat
                if use_regex && compiled_regex.is_some() && matched_span.is_none() {
                    let re = compiled_regex.as_ref().unwrap();
                    for (idx, ea) in eojeol_analyses.iter().enumerate() {
                        if re.is_match(ea) {
                            if let Some(&w) = words.get(idx) {
                                if let Some(byte_pos) = display_text.find(w) {
                                    let sc = display_text[..byte_pos].chars().count();
                                    let ec = sc + w.chars().count();
                                    if check_boundary(sc, ec) {
                                        matched_span = Some((sc, ec, "형태의미/정규식"));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                // 3-3) Text matching on sense_flat
                if use_text && matched_span.is_none() {
                    let norm_s = state.normalizer.normalize_morph_str(&sense_flat);
                    if norm_s.contains(&norm_morph_query) || sense_flat.contains(core_query) {
                        for (idx, ea) in eojeol_analyses.iter().enumerate() {
                            if state
                                .normalizer
                                .normalize_morph_str(ea)
                                .contains(&norm_morph_query)
                                || ea.contains(core_query)
                            {
                                if let Some(&w) = words.get(idx) {
                                    if let Some(byte_pos) = display_text.find(w) {
                                        let sc = display_text[..byte_pos].chars().count();
                                        let ec = sc + w.chars().count();
                                        if check_boundary(sc, ec) {
                                            matched_span = Some((sc, ec, "형태의미"));
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 4. Syntax tree layer matching
            if use_syntax && matched_span.is_none() && !syntax_tree.is_empty() {
                let first_word_len = display_text
                    .split_whitespace()
                    .next()
                    .map(|w| w.chars().count())
                    .unwrap_or(6);

                // 4-1) Jamo bracket matching on syntax_tree
                if (use_jamo || bracket_tokens.is_some()) && bracket_tokens.is_some() {
                    let b_tokens = bracket_tokens.as_ref().unwrap();
                    let syn_units = state.normalizer.tokenize_text_units(&syntax_tree);
                    if find_bracket_query_match(b_tokens, &syn_units).is_some() {
                        if check_boundary(0, first_word_len) {
                            matched_span = Some((0, first_word_len, "구문/자소"));
                        }
                    }
                }

                // 4-2) Regex matching on syntax_tree
                if use_regex && compiled_regex.is_some() && matched_span.is_none() {
                    let re = compiled_regex.as_ref().unwrap();
                    if re.is_match(&syntax_tree) {
                        if check_boundary(0, first_word_len) {
                            matched_span = Some((0, first_word_len, "구문/정규식"));
                        }
                    }
                }

                // 4-3) Text matching on syntax_tree
                if use_text && matched_span.is_none() {
                    let norm_syn = state.normalizer.normalize_morph_str(&syntax_tree);
                    if syntax_tree.contains(core_query) || norm_syn.contains(&norm_morph_query) {
                        if check_boundary(0, first_word_len) {
                            matched_span = Some((0, first_word_len, "구문분석"));
                        }
                    }
                }
            }

            if let Some((start_c, end_c, layer_name)) = matched_span {
                total_matches += 1;
                if hits.len() < limit {
                    let (raw_left, node, raw_right, w_cnt, rw_cnt, l_trunc, r_trunc) =
                        extract_kwic_by_word_count(
                            &display_text,
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

    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS segment_edits (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            segment_id INTEGER,
            original_text_raw TEXT,
            edited_text_raw TEXT,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    );

    let original: Option<String> = conn
        .query_row(
            "SELECT text_unicode FROM segments WHERE segment_id = ?",
            params![payload.segment_id],
            |r| r.get(0),
        )
        .ok();

    if let Some(ref new_val) = payload.text_unicode {
        let _ = conn.execute(
            "INSERT INTO segment_edits (segment_id, original_text_raw, edited_text_raw) VALUES (?, ?, ?)",
            params![payload.segment_id, original, new_val],
        );
        let _ = conn.execute(
            "UPDATE segments SET text_unicode = ?, text_display = ? WHERE segment_id = ?",
            params![new_val, new_val, payload.segment_id],
        );
    }
    if let Some(ref m) = payload.morph_flat {
        let _ = conn.execute(
            "UPDATE segments SET morph_flat = ? WHERE segment_id = ?",
            params![m, payload.segment_id],
        );
    }
    if let Some(ref s) = payload.sense_flat {
        let _ = conn.execute(
            "UPDATE segments SET sense_flat = ? WHERE segment_id = ?",
            params![s, payload.segment_id],
        );
    }
    if let Some(ref st) = payload.syntax_tree {
        let _ = conn.execute(
            "UPDATE segments SET syntax_tree = ? WHERE segment_id = ?",
            params![st, payload.segment_id],
        );
    }

    json_response(&serde_json::json!({ "success": true }), 200)
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

    let folder = payload.folder_path.trim().to_string();
    if folder.is_empty() || !Path::new(&folder).exists() {
        return json_response(
            &serde_json::json!({ "error": format!("지정한 폴더가 존재하지 않습니다: {}", folder) }),
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
    mgr.progress = DbBuildProgress {
        is_running: true,
        status: "RUNNING".to_string(),
        target_db_path: target_db.to_string_lossy().into_owned(),
        is_new_db_file: is_new_db,
        log_lines: vec![format!("[시작] 말뭉치 폴더 스캔 시작: {}", folder)],
        ..DbBuildProgress::default()
    };

    let py_script = state.root_dir.join("corpus_builder.py");
    let target_db_str = target_db.to_string_lossy().into_owned();

    let mut cmd = Command::new("python");
    cmd.env("PYTHONUTF8", "1")
        .args([
            py_script.to_str().unwrap_or("corpus_builder.py"),
            "--folder",
            &folder,
            "--db",
            &target_db_str,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            mgr.progress.is_running = false;
            mgr.progress.status = "ERROR".to_string();
            mgr.progress.error_message = Some(e.to_string());
            return json_response(&serde_json::json!({ "error": format!("Python 프로세스 실행 실패: {}", e) }), 500);
        }
    };

    let stdout = child.stdout.take();
    mgr.child = Some(child);

    let db_builder_arc = Arc::clone(&state.db_builder);

    // 백그라운드 모니터링 스레드
    std::thread::spawn(move || {
        let mut final_json_str = String::new();

        if let Some(out) = stdout {
            let reader = BufReader::new(out);
            for line_res in reader.lines() {
                if let Ok(line) = line_res {
                    if line.starts_with("PROGRESS:") {
                        let json_part = &line["PROGRESS:".len()..];
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_part) {
                            let mut m = db_builder_arc.lock().unwrap();
                            if !m.progress.is_running {
                                break;
                            }
                            if let Some(total) = val.get("total_files").and_then(|v| v.as_u64()) {
                                m.progress.total_files = total as usize;
                            }
                            if let Some(proc) = val.get("processed_files").and_then(|v| v.as_u64()) {
                                m.progress.processed_files = proc as usize;
                            }
                            if let Some(pct) = val.get("percent").and_then(|v| v.as_f64()) {
                                m.progress.percent = pct;
                            }
                            if let Some(cf) = val.get("current_file").and_then(|v| v.as_str()) {
                                m.progress.current_file = cf.to_string();
                            }
                            if let Some(cd) = val.get("current_doc_id").and_then(|v| v.as_str()) {
                                if !cd.is_empty() && !m.progress.newly_created_doc_ids.contains(&cd.to_string()) {
                                    m.progress.newly_created_doc_ids.push(cd.to_string());
                                }
                                m.progress.current_doc_id = cd.to_string();
                            }
                            if let Some(sents) = val.get("sentences_imported").and_then(|v| v.as_u64()) {
                                m.progress.sentences_imported = sents as usize;
                            }
                            if let Some(eojeols) = val.get("eojeols_imported").and_then(|v| v.as_u64()) {
                                m.progress.eojeols_imported = eojeols as usize;
                            }
                            if let Some(sk) = val.get("skipped_files").and_then(|v| v.as_u64()) {
                                m.progress.skipped_files = sk as usize;
                            }
                            if let Some(msg) = val.get("message").and_then(|v| v.as_str()) {
                                if !msg.is_empty() {
                                    if m.progress.log_lines.len() >= 100 {
                                        m.progress.log_lines.remove(0);
                                    }
                                    m.progress.log_lines.push(msg.to_string());
                                }
                            }
                        }
                    } else if line.starts_with('{') || !final_json_str.is_empty() {
                        final_json_str.push_str(&line);
                    } else if !line.trim().is_empty() {
                        let mut m = db_builder_arc.lock().unwrap();
                        if m.progress.log_lines.len() >= 100 {
                            m.progress.log_lines.remove(0);
                        }
                        m.progress.log_lines.push(line);
                    }
                }
            }
        }

        let mut m = db_builder_arc.lock().unwrap();
        if let Some(mut ch) = m.child.take() {
            let status_res = ch.wait();
            if m.progress.status == "RUNNING" {
                match status_res {
                    Ok(st) if st.success() => {
                        m.progress.is_running = false;
                        m.progress.status = "COMPLETED".to_string();
                        m.progress.percent = 100.0;
                        m.progress.log_lines.push("[완료] 데이터베이스 생성이 성공적으로 완료되었습니다!".to_string());
                        if let Ok(summary) = serde_json::from_str::<serde_json::Value>(&final_json_str) {
                            m.progress.result_summary = Some(summary);
                        }
                    }
                    Ok(st) => {
                        m.progress.is_running = false;
                        m.progress.status = "ERROR".to_string();
                        m.progress.error_message = Some(format!("프로세스가 종료 코드 {:?}로 비정상 종료되었습니다.", st.code()));
                    }
                    Err(e) => {
                        m.progress.is_running = false;
                        m.progress.status = "ERROR".to_string();
                        m.progress.error_message = Some(e.to_string());
                    }
                }
            }
        }
    });

    let initial_prog = mgr.progress.clone();
    json_response(&initial_prog, 200)
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
    match fs::read(path) {
        Ok(bytes) => {
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
        Err(_) => Response::from_string("404 Not Found").with_status_code(StatusCode(404)),
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
    if cwd.join("web").join("index.html").exists() {
        return cwd;
    }
    if let Some(parent) = cwd.parent() {
        if parent.join("web").join("index.html").exists() {
            return parent.to_path_buf();
        }
    }
    if let Ok(exe_path) = env::current_exe() {
        let mut anc = exe_path.as_path();
        while let Some(p) = anc.parent() {
            if p.join("web").join("index.html").exists() {
                return p.to_path_buf();
            }
            anc = p;
        }
    }
    cwd
}

fn open_browser(url: &str) {
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

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut port: u16 = 8787;
    let mut auto_open = false;
    for i in 1..args.len() {
        if args[i] == "--port" && i + 1 < args.len() {
            port = args[i + 1].parse().unwrap_or(8787);
        }
        if args[i] == "--open" {
            auto_open = true;
        }
    }

    let root_dir = find_root_dir();
    let _ = fs::create_dir_all(root_dir.join("db"));
    let db_path = root_dir.join("db").join("concordance.corpdb");
    let web_dir = root_dir.join("web");

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
    println!("  Concordance — 한국어 통시·공시 다층 말뭉치 통합 검색기 (Rust Engine)");
    println!("================================================================");
    println!("  * 프로젝트 루트 : {}", root_dir.display());
    println!("  * 데이터베이스  : {}", db_path.display());
    println!(
        "  * 메모리 프로파일: {} (총 {:.1}GB / 가용 {:.1}GB)",
        mem_profile.tier_label, mem_profile.total_ram_gb, mem_profile.available_ram_gb
    );
    println!("  * 기본 내장 폰트: KoPubWorld Dotum Medium.ttf + 함초롬바탕(HANBatang.ttf)");
    println!("  * 접속 주소     : {}", url);
    println!("================================================================");

    let server = match Server::http(&bind_addr) {
        Ok(s) => s,
        Err(_) => {
            println!("  [안내] 포트 {}에서 이미 검색기 서버가 실행 중입니다.", port);
            if auto_open {
                println!("  [안내] 브라우저로 검색기 화면을 엽니다: {}", url);
                open_browser(&url);
            }
            return;
        }
    };

    if auto_open {
        open_browser(&url);
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
                (&Method::Post, "/api/search/cancel") => handle_search_cancel(&state),
                (&Method::Get, "/api/segment") => handle_segment_detail(&state, &params),
                (&Method::Get, "/api/source-jump") => handle_source_jump(&state, &params),
                (&Method::Get, "/api/open-editor") | (&Method::Post, "/api/open-editor") => handle_open_editor(&params),
                (&Method::Post, "/api/segment/edit") => handle_segment_edit(&state, &mut req),
                (&Method::Post, "/api/build-db") => handle_build_db(&state, &mut req),
                (&Method::Get, "/api/build-db/status") => handle_build_db_status(&state),
                (&Method::Post, "/api/build-db/stop") => handle_build_db_stop(&state),
                (&Method::Post, "/api/build-db/cancel") => handle_build_db_cancel(&state),
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
                (&Method::Get, "/") | (&Method::Get, "/index.html") => serve_static_file(
                    &state.web_dir.join("index.html"),
                    "text/html; charset=utf-8",
                ),
                (&Method::Get, "/styles.css") => serve_static_file(
                    &state.web_dir.join("styles.css"),
                    "text/css; charset=utf-8",
                ),
                (&Method::Get, "/hangul_automata.js") => serve_static_file(
                    &state.web_dir.join("hangul_automata.js"),
                    "application/javascript; charset=utf-8",
                ),
                (&Method::Get, "/app.js") => serve_static_file(
                    &state.web_dir.join("app.js"),
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
    fn test_space_word_boundary_rules() {
        let normalizer = Normalizer::new(std::path::Path::new("assets/data/pua_table.json"));
        let query = "하도";
        let query_nfd = normalizer.to_nfd_norm(query);
        let query_chars: Vec<char> = query_nfd.chars().collect();

        // Helper boundary checker
        let check = |text: &str, has_leading: bool, has_trailing: bool| -> bool {
            let (norm, char_map) = normalizer.to_nfd_norm_with_map(text);
            let display_chars: Vec<char> = text.chars().collect();
            let d_len = display_chars.len();
            let candidates = find_all_char_subslices(&norm, &query_chars);

            for (n_start, n_end) in candidates {
                if n_end < norm.len() && is_jongseong_jamo(norm[n_end]) {
                    continue;
                }
                let orig_start = char_map.get(n_start).copied().unwrap_or(0);
                let orig_end = char_map.get(n_end.saturating_sub(1)).map(|&idx| idx + 1).unwrap_or(orig_start + 1);

                let mut ok = true;
                if has_leading {
                    if orig_start > 0 && !display_chars[orig_start - 1].is_whitespace() {
                        ok = false;
                    }
                }
                if has_trailing {
                    if orig_end < d_len && !display_chars[orig_end].is_whitespace() {
                        ok = false;
                    }
                }
                if ok {
                    return true;
                }
            }
            false
        };

        // Query ' 하도 ' (has_leading=true, has_trailing=true)
        // 1. Sentence start: "하도 말이 많다" -> MUST MATCH! (요구사항 3)
        assert!(check("하도 말이 많다", true, true));

        // 2. Mid-sentence isolated adverb: "정말 하도 어이가 없다" -> MUST MATCH! (요구사항 2)
        assert!(check("정말 하도 어이가 없다", true, true));

        // 3. Substring in word: "말하도록 하였다" -> MUST NOT MATCH! (요구사항 2)
        assert!(!check("말하도록 하였다", true, true));

        // 4. Substring at end of word: "일하도 말았다" -> MUST NOT MATCH!
        assert!(!check("일하도 말았다", true, true));

        // 5. Followed by jongseong: "하동으로 갔다" -> MUST NOT MATCH! (요구사항 4)
        assert!(!check("하동으로 갔다", true, true));
    }
}
