use crate::jamo_table::{decompose_l_cp, decompose_t_cp, decompose_v_cp};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const EMBEDDED_PUA_TABLE_JSON: &str = include_str!("../../assets/data/pua_table.json");

#[derive(Clone)]
pub struct Normalizer {
    pua_to_nfd: HashMap<char, String>,
    compat_to_nfd: HashMap<char, char>,
    jong_to_choseong: HashMap<char, char>,
}

#[derive(Debug, Clone)]
pub(crate) enum TextUnitKind {
    Syllable {
        l: Vec<char>,
        v: Vec<char>,
        t: Vec<char>,
    },
    Other(char),
}

#[derive(Debug, Clone)]
pub(crate) struct TextUnit {
    pub(crate) start_char: usize,
    pub(crate) end_char: usize,
    pub(crate) kind: TextUnitKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum GlobOp {
    Zero,      // 0 : 0 atomic jamos (empty/none)
    One,       // 1 : exactly 1 atomic jamo (any)
    ZeroOrOne, // 01: 0 or 1 atomic jamo
    Star,      // * : 0 or more atomic jamos (legacy)
    Plus,      // + : 1 or more atomic jamos (legacy)
    Question,  // ? : 1 atomic jamo (legacy)
    Lit(char),
}

#[derive(Debug, Clone)]
pub(crate) enum BracketQueryToken {
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

    pub(crate) fn decompose_char(&self, ch: char, out: &mut Vec<char>) {
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

    pub(crate) fn push_norm_char(&self, ch: char, out: &mut Vec<char>) {
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

    pub(crate) fn to_nfd_norm_with_map(&self, text: &str) -> (Vec<char>, Vec<usize>) {
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

    pub(crate) fn normalize_morph_str(&self, text: &str) -> String {
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

    pub(crate) fn normalize_morph_for_regex(&self, text: &str) -> String {
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

    pub(crate) fn compose_modern_hangul_for_display(&self, text: &str) -> String {
        // Fast-path: PUA(E000..F8FF)나 첫가끝 NFD 자모(1100..11FF)가 전혀 없으면 변환 없이 즉시 반환
        if !text
            .chars()
            .any(|c| matches!(c as u32, 0x1100..=0x11FF | 0xE000..=0xF8FF))
        {
            return text.to_string();
        }
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
    pub(crate) fn jamo_char_to_atoms(&self, ch: char) -> Vec<char> {
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

    pub(crate) fn parse_slot_glob(&self, raw_slot: &str) -> Vec<GlobOp> {
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
    pub(crate) fn parse_bracket_query(&self, query: &str) -> Option<Vec<BracketQueryToken>> {
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
    pub(crate) fn tokenize_text_units(&self, display_text: &str) -> Vec<TextUnit> {
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

pub(crate) fn is_combining_mark(cp: u32) -> bool {
    cp == 0x302E || cp == 0x302F || (0x0300..=0x030A).contains(&cp)
}

pub(crate) fn match_glob_ops(ops: &[GlobOp], target: &[char]) -> bool {
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

pub(crate) fn match_unit_token(tok: &BracketQueryToken, unit: &TextUnit) -> bool {
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

pub(crate) fn match_tokens_at(tokens: &[BracketQueryToken], units: &[TextUnit]) -> Option<usize> {
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

pub(crate) fn find_bracket_query_match(
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

